//! Input routing (Phase 10 step 4): libinput events become wl_pointer and
//! wl_keyboard events for the focused client (focus rules: `seat.rs`).
//!
//! Every event goes only to objects of the client that owns the focused
//! surface. Pointer events are grouped by wl_pointer.frame (v5+). Keys are
//! sent as evdev codes with the compositor's xkb keymap, which each
//! wl_keyboard receives once as a sealed memfd; after a key that changes
//! the xkb state, wl_keyboard.modifiers carries the new state.
//!
//! Timestamps are CLOCK_MONOTONIC milliseconds, like the frame callbacks
//! (DRM flip times).

use crate::globals::Compositor;
use crate::seat::{hit, Cursor, Focus, Rect, CAP_KEYBOARD, CAP_POINTER};
use std::io::{self, Write};
use std::os::unix::io::{AsFd, FromRawFd, OwnedFd};
use wana_input::keyboard::Keyboard;
use wana_input::libinput::{Capability, Event, EventKind};
use wana_log::{debug, info, Subsystem};
use wana_wayland::protocols::wayland::{wl_keyboard, wl_pointer, wl_seat};
use wana_wayland::server::{fixed_from_f64, Arg, ClientId, Ctx, ReqArg, Resource};

const INPUT: Subsystem = Subsystem::Input;
const COMPOSITOR: Subsystem = Subsystem::Compositor;

/// wl_keyboard.repeat_info: 25 keys/s after 600 ms (clients repeat).
const REPEAT_RATE: i32 = 25;
const REPEAT_DELAY: i32 = 600;
/// wl_pointer.axis distance of one wheel detent (the usual 15 "pixels").
const WHEEL_STEP: f64 = 15.0;
const KEYMAP_FORMAT_XKB_V1: u32 = 1;
const AXIS_VERTICAL: u32 = 0;
const AXIS_HORIZONTAL: u32 = 1;
const AXIS_SOURCE_WHEEL: u32 = 0;

mod err {
    pub const POINTER_ROLE: u32 = 0;
    pub const SEAT_MISSING_CAPABILITY: u32 = 0;
}

/// The keymap every wl_keyboard gets: xkb text + NUL in a sealed memfd.
#[derive(Debug)]
pub struct Keymap {
    pub fd: OwnedFd,
    /// Bytes including the terminating NUL.
    pub size: u32,
}

const MFD_CLOEXEC: u32 = 1;
const MFD_ALLOW_SEALING: u32 = 2;
const F_ADD_SEALS: i32 = 1033;
const F_SEAL_SEAL: i32 = 1;
const F_SEAL_SHRINK: i32 = 2;
const F_SEAL_GROW: i32 = 4;
const F_SEAL_WRITE: i32 = 8;
const CLOCK_MONOTONIC: i32 = 1;

#[repr(C)]
struct Timespec {
    sec: i64,
    nsec: i64,
}

extern "C" {
    fn memfd_create(name: *const std::os::raw::c_char, flags: u32) -> i32;
    fn fcntl(fd: i32, cmd: i32, ...) -> i32;
    fn clock_gettime(clock: i32, ts: *mut Timespec) -> i32;
}

/// Writes `text` + NUL into a memfd and seals it: one file shared by every
/// client, which none of them can modify, grow or shrink.
pub fn sealed_keymap(text: &str) -> io::Result<Keymap> {
    // SAFETY: NUL-terminated name.
    let fd = unsafe { memfd_create(c"wana-keymap".as_ptr(), MFD_CLOEXEC | MFD_ALLOW_SEALING) };
    if fd < 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: fresh fd, owned from here on.
    let fd = unsafe { OwnedFd::from_raw_fd(fd) };
    let mut file = std::fs::File::from(fd);
    file.write_all(text.as_bytes())?;
    file.write_all(&[0])?;
    // The offset is shared by every copy of the fd; clients mmap from 0,
    // but leave it at the start for any that read.
    std::io::Seek::rewind(&mut file)?;
    let seals = F_SEAL_SHRINK | F_SEAL_GROW | F_SEAL_WRITE | F_SEAL_SEAL;
    // SAFETY: valid fd; F_ADD_SEALS takes an int argument.
    if unsafe {
        fcntl(
            std::os::unix::io::AsRawFd::as_raw_fd(&file),
            F_ADD_SEALS,
            seals,
        )
    } < 0
    {
        return Err(io::Error::last_os_error());
    }
    let size = u32::try_from(text.len() + 1).map_err(|_| io::Error::other("keymap too large"))?;
    Ok(Keymap {
        fd: file.into(),
        size,
    })
}

/// CLOCK_MONOTONIC in milliseconds (wraps, as the protocol's u32 does).
pub fn now_ms() -> u32 {
    let mut ts = Timespec { sec: 0, nsec: 0 };
    // SAFETY: valid out pointer.
    unsafe { clock_gettime(CLOCK_MONOTONIC, &mut ts) };
    (ts.sec as u64 * 1000 + ts.nsec as u64 / 1_000_000) as u32
}

fn caps_list(caps: u32) -> String {
    let mut v = Vec::new();
    if caps & CAP_POINTER != 0 {
        v.push("pointer");
    }
    if caps & CAP_KEYBOARD != 0 {
        v.push("keyboard");
    }
    if v.is_empty() {
        "none".into()
    } else {
        v.join(", ")
    }
}

impl Compositor {
    /// Installs the keyboard layout (xkb state) and its shared keymap file.
    pub fn set_keyboard(&mut self, kb: Keyboard) -> Result<(), String> {
        let text = kb.keymap_string()?;
        let keymap = sealed_keymap(&text).map_err(|e| format!("keymap memfd: {e}"))?;
        info!(
            INPUT,
            "keymap {} for clients: {} bytes, sealed memfd",
            kb.layout_name(),
            keymap.size
        );
        self.keymap = Some(keymap);
        self.xkb = Some(kb);
        Ok(())
    }

    /// Handles one libinput event.
    pub fn input_event(&mut self, ctx: &Ctx, ev: Event) {
        let (w, h) = (self.output.width, self.output.height);
        match ev.kind {
            EventKind::DeviceAdded(d) => {
                let mut caps = 0;
                if d.has(Capability::Pointer) {
                    caps |= CAP_POINTER;
                }
                if d.has(Capability::Keyboard) && self.keymap.is_some() {
                    caps |= CAP_KEYBOARD;
                }
                info!(
                    INPUT,
                    "device added: {} ({}) [{}]",
                    d.name,
                    d.sysname,
                    d.caps_list()
                );
                self.device_caps(ctx, &d.sysname, caps);
            }
            EventKind::DeviceRemoved(d) => {
                info!(INPUT, "device removed: {} ({})", d.name, d.sysname);
                self.device_caps(ctx, &d.sysname, 0);
            }
            EventKind::Motion { dx, dy } => {
                self.seat.move_by(dx, dy, w, h);
                self.pointer_moved(ctx);
            }
            EventKind::MotionAbsolute { x, y } => {
                self.seat.move_to(x, y, w, h);
                self.pointer_moved(ctx);
            }
            EventKind::Button { code, pressed } => self.pointer_button(ctx, code, pressed),
            EventKind::Scroll {
                vertical,
                horizontal,
            } => self.pointer_scroll(ctx, vertical, horizontal),
            EventKind::Key { code, pressed } => self.key(ctx, code, pressed),
            EventKind::Other(_) => {}
        }
    }

    fn device_caps(&mut self, ctx: &Ctx, sysname: &str, caps: u32) {
        if !self.seat.set_device(sysname, caps) {
            return;
        }
        info!(INPUT, "seat0 capabilities: {}", caps_list(self.seat.caps));
        for s in self.seat.seats.clone() {
            let _ = ctx.post(
                s,
                wl_seat::event::CAPABILITIES,
                &[Arg::Uint(self.seat.caps)],
            );
        }
    }

    /// Mapped windows as hit-test rectangles, bottom first.
    fn rects(&self) -> Vec<Rect> {
        self.windows
            .iter()
            .filter_map(|win| {
                let (w, h) = self.surfaces.get(&win.surface)?.content?;
                Some(Rect {
                    surface: win.surface,
                    x: win.x,
                    y: win.y,
                    w,
                    h,
                })
            })
            .collect()
    }

    /// Surface-local position of the cursor on `surface`, if it is a window.
    fn local(&self, surface: Resource) -> Option<(f64, f64)> {
        let win = self.windows.iter().find(|w| w.surface == surface)?;
        Some((
            self.seat.pos.0 - f64::from(win.x),
            self.seat.pos.1 - f64::from(win.y),
        ))
    }

    fn pointers_of(&self, ctx: &Ctx, client: ClientId) -> Vec<Resource> {
        self.seat
            .pointers
            .iter()
            .copied()
            .filter(|p| ctx.client(*p) == Some(client))
            .collect()
    }

    fn keyboards_of(&self, ctx: &Ctx, client: ClientId) -> Vec<Resource> {
        self.seat
            .keyboards
            .iter()
            .copied()
            .filter(|k| ctx.client(*k) == Some(client))
            .collect()
    }

    /// Re-evaluates pointer focus (not during a grab). Sends leave/enter
    /// when the surface under the cursor changed; returns true if it did.
    fn update_pointer_focus(&mut self, ctx: &Ctx) -> bool {
        if !self.seat.active || !self.seat.buttons.is_empty() {
            return false;
        }
        let target = hit(&self.rects(), self.seat.pos.0, self.seat.pos.1);
        let current = self.seat.pointer_focus.map(|f| f.surface);
        if target.map(|t| t.0) == current {
            return false;
        }
        if let Some(old) = self.seat.pointer_focus.take() {
            let serial = ctx.next_serial();
            for p in self.pointers_of(ctx, old.client) {
                let _ = ctx.post(
                    p,
                    wl_pointer::event::LEAVE,
                    &[Arg::Uint(serial), Arg::Object(Some(old.surface))],
                );
                frame(ctx, p);
            }
            // The cursor image belonged to that client.
            self.seat.cursor = Cursor::Default;
            self.needs_redraw = true;
        }
        if let Some((surface, sx, sy)) = target {
            let Some(client) = ctx.client(surface) else {
                return true;
            };
            let focus = Focus {
                surface,
                client,
                serial: ctx.next_serial(),
            };
            self.seat.pointer_focus = Some(focus);
            for p in self.pointers_of(ctx, client) {
                pointer_enter(ctx, p, &focus, sx, sy);
            }
            debug!(
                COMPOSITOR,
                "pointer focus: {} at {sx:.1},{sy:.1}",
                self.title_of(surface)
            );
        }
        true
    }

    fn pointer_moved(&mut self, ctx: &Ctx) {
        if self.seat.cursor != Cursor::Hidden {
            self.needs_redraw = true;
        }
        if self.update_pointer_focus(ctx) {
            return; // enter carries the position
        }
        let Some(focus) = self.seat.pointer_focus else {
            return;
        };
        let Some((sx, sy)) = self.local(focus.surface) else {
            return;
        };
        let time = now_ms();
        for p in self.pointers_of(ctx, focus.client) {
            let _ = ctx.post(
                p,
                wl_pointer::event::MOTION,
                &[
                    Arg::Uint(time),
                    Arg::Fixed(fixed_from_f64(sx)),
                    Arg::Fixed(fixed_from_f64(sy)),
                ],
            );
            frame(ctx, p);
        }
    }

    fn pointer_button(&mut self, ctx: &Ctx, code: u32, pressed: bool) {
        let was_active = self.seat.active;
        // Focus as of before the press (the grab starts on it).
        self.update_pointer_focus(ctx);
        let first = self.seat.button(code, pressed);
        if !was_active {
            self.needs_redraw = true; // the cursor appears
        }
        let Some(focus) = self.seat.pointer_focus else {
            return;
        };
        if first {
            // Click to focus: raise the window and give it the keyboard.
            self.raise(focus.surface);
            if self.seat.keyboard_focus != Some(focus.surface) {
                self.set_keyboard_focus(ctx, Some(focus.surface));
            }
        }
        let (serial, time) = (ctx.next_serial(), now_ms());
        for p in self.pointers_of(ctx, focus.client) {
            let _ = ctx.post(
                p,
                wl_pointer::event::BUTTON,
                &[
                    Arg::Uint(serial),
                    Arg::Uint(time),
                    Arg::Uint(code),
                    Arg::Uint(u32::from(pressed)),
                ],
            );
            frame(ctx, p);
        }
        debug!(
            COMPOSITOR,
            "button {code} {} -> {}",
            if pressed { "pressed" } else { "released" },
            self.title_of(focus.surface)
        );
        if !pressed && self.seat.buttons.is_empty() {
            // Grab over: the surface under the cursor may differ now.
            self.update_pointer_focus(ctx);
        }
    }

    fn pointer_scroll(&mut self, ctx: &Ctx, vertical: f64, horizontal: f64) {
        let Some(focus) = self.seat.pointer_focus else {
            return;
        };
        let time = now_ms();
        for p in self.pointers_of(ctx, focus.client) {
            let v5 = ctx.version(p) >= 5;
            if v5 {
                let _ = ctx.post(
                    p,
                    wl_pointer::event::AXIS_SOURCE,
                    &[Arg::Uint(AXIS_SOURCE_WHEEL)],
                );
            }
            for (axis, v120) in [(AXIS_VERTICAL, vertical), (AXIS_HORIZONTAL, horizontal)] {
                if v120 == 0.0 {
                    continue;
                }
                if v5 {
                    // Deprecated in v8 (axis_value120); this seat is v7.
                    let _ = ctx.post(
                        p,
                        wl_pointer::event::AXIS_DISCRETE,
                        &[Arg::Uint(axis), Arg::Int((v120 / 120.0).round() as i32)],
                    );
                }
                let _ = ctx.post(
                    p,
                    wl_pointer::event::AXIS,
                    &[
                        Arg::Uint(time),
                        Arg::Uint(axis),
                        Arg::Fixed(fixed_from_f64(v120 / 120.0 * WHEEL_STEP)),
                    ],
                );
            }
            frame(ctx, p);
        }
    }

    fn key(&mut self, ctx: &Ctx, code: u32, pressed: bool) {
        if !self.seat.key(code, pressed) {
            return;
        }
        let Some(kb) = self.xkb.as_mut() else { return };
        let info = kb.key(code, pressed);
        let mods = kb.modifiers();
        let Some(surface) = self.seat.keyboard_focus else {
            return;
        };
        let Some(client) = ctx.client(surface) else {
            return;
        };
        let time = now_ms();
        let key_serial = ctx.next_serial();
        let mods_serial = if info.mods_changed {
            ctx.next_serial()
        } else {
            0
        };
        for k in self.keyboards_of(ctx, client) {
            let _ = ctx.post(
                k,
                wl_keyboard::event::KEY,
                &[
                    Arg::Uint(key_serial),
                    Arg::Uint(time),
                    Arg::Uint(code),
                    Arg::Uint(u32::from(pressed)),
                ],
            );
            if info.mods_changed {
                send_modifiers(ctx, k, mods_serial, &mods);
            }
        }
        debug!(
            COMPOSITOR,
            "key {code} {} ({}) -> {}",
            if pressed { "pressed" } else { "released" },
            info.keysym,
            self.title_of(surface)
        );
    }

    /// Moves keyboard focus, with leave to the old client's keyboards and
    /// enter (+ modifiers) to the new one's.
    fn set_keyboard_focus(&mut self, ctx: &Ctx, surface: Option<Resource>) {
        let old = std::mem::replace(&mut self.seat.keyboard_focus, surface);
        if old == surface {
            return;
        }
        if let Some(old) = old.filter(|o| ctx.is_alive(*o)) {
            if let Some(client) = ctx.client(old) {
                let serial = ctx.next_serial();
                for k in self.keyboards_of(ctx, client) {
                    let _ = ctx.post(
                        k,
                        wl_keyboard::event::LEAVE,
                        &[Arg::Uint(serial), Arg::Object(Some(old))],
                    );
                }
            }
        }
        match surface {
            Some(s) => {
                if let Some(client) = ctx.client(s) {
                    for k in self.keyboards_of(ctx, client) {
                        self.keyboard_enter(ctx, k, s);
                    }
                }
                info!(COMPOSITOR, "keyboard focus: {}", self.title_of(s));
            }
            None => info!(COMPOSITOR, "keyboard focus: none"),
        }
    }

    fn keyboard_enter(&self, ctx: &Ctx, k: Resource, surface: Resource) {
        let serial = ctx.next_serial();
        let keys = self.seat.keys_array();
        let _ = ctx.post(
            k,
            wl_keyboard::event::ENTER,
            &[
                Arg::Uint(serial),
                Arg::Object(Some(surface)),
                Arg::Array(&keys),
            ],
        );
        if let Some(kb) = &self.xkb {
            send_modifiers(ctx, k, ctx.next_serial(), &kb.modifiers());
        }
    }

    /// Puts `surface`'s window on top.
    fn raise(&mut self, surface: Resource) {
        if let Some(i) = self.windows.iter().position(|w| w.surface == surface) {
            if i + 1 != self.windows.len() {
                let w = self.windows.remove(i);
                self.windows.push(w);
                self.needs_redraw = true;
            }
        }
    }

    /// Brings focus in line with the windows after protocol requests: a
    /// window mapped or unmapped under the cursor, a new window that should
    /// get the keyboard, a focused window that went away.
    pub fn sync_focus(&mut self, ctx: &Ctx) {
        if let Some(f) = self.seat.pointer_focus {
            let mapped = self.windows.iter().any(|w| w.surface == f.surface);
            if !mapped && ctx.is_alive(f.surface) {
                // Unmapped while focused (even during a grab): leave.
                let serial = ctx.next_serial();
                for p in self.pointers_of(ctx, f.client) {
                    let _ = ctx.post(
                        p,
                        wl_pointer::event::LEAVE,
                        &[Arg::Uint(serial), Arg::Object(Some(f.surface))],
                    );
                    frame(ctx, p);
                }
                self.seat.pointer_focus = None;
                self.seat.cursor = Cursor::Default;
                self.seat.buttons.clear();
                self.needs_redraw = true;
            }
        }
        self.update_pointer_focus(ctx);

        if let Some(s) = self.seat.focus_request.take() {
            if self.windows.iter().any(|w| w.surface == s) {
                self.set_keyboard_focus(ctx, Some(s));
            }
        }
        let focused_mapped = self
            .seat
            .keyboard_focus
            .is_some_and(|s| self.windows.iter().any(|w| w.surface == s));
        if !focused_mapped {
            let top = self.windows.last().map(|w| w.surface);
            if top != self.seat.keyboard_focus {
                self.set_keyboard_focus(ctx, top);
            }
        }
    }

    /// The cursor to draw on top of the scene: texture and top-left corner.
    pub fn cursor_image(&self) -> Option<(&wana_render::compose::Texture, i32, i32)> {
        if !self.seat.active {
            return None;
        }
        let (x, y) = (self.seat.pos.0 as i32, self.seat.pos.1 as i32);
        match self.seat.cursor {
            Cursor::Hidden => None,
            Cursor::Surface { surface, hx, hy } => match self.content.get(&surface) {
                Some(crate::globals::Content::Texture(t)) => Some((t, x - hx, y - hy)),
                _ => None,
            },
            Cursor::Default => {
                let (hx, hy) = crate::seat::ARROW_HOTSPOT;
                self.arrow.as_ref().map(|t| (t, x - hx, y - hy))
            }
        }
    }

    // --- requests -----------------------------------------------------------
    pub(crate) fn seat_request(&mut self, ctx: &Ctx, res: Resource, opcode: u32, args: &[ReqArg]) {
        let id = match args.first() {
            Some(ReqArg::NewId(id)) => *id,
            _ => return, // release: destructor, applied by the protocol layer
        };
        match opcode {
            wl_seat::request::GET_POINTER => {
                if self.seat.ever & CAP_POINTER == 0 {
                    ctx.post_error(
                        res,
                        err::SEAT_MISSING_CAPABILITY,
                        "get_pointer: seat0 never had a pointer",
                    );
                    return;
                }
                let Some(p) = self.create(
                    ctx,
                    res,
                    &wana_wayland::protocols::wayland::WL_POINTER_INTERFACE,
                    id,
                ) else {
                    return;
                };
                self.seat.pointers.push(p);
                if let Some(f) = self
                    .seat
                    .pointer_focus
                    .filter(|f| ctx.client(p) == Some(f.client))
                {
                    if let Some((sx, sy)) = self.local(f.surface) {
                        pointer_enter(ctx, p, &f, sx, sy);
                    }
                }
            }
            wl_seat::request::GET_KEYBOARD => {
                if self.seat.ever & CAP_KEYBOARD == 0 {
                    ctx.post_error(
                        res,
                        err::SEAT_MISSING_CAPABILITY,
                        "get_keyboard: seat0 never had a keyboard",
                    );
                    return;
                }
                let Some(k) = self.create(
                    ctx,
                    res,
                    &wana_wayland::protocols::wayland::WL_KEYBOARD_INTERFACE,
                    id,
                ) else {
                    return;
                };
                self.seat.keyboards.push(k);
                if let Some(km) = &self.keymap {
                    let _ = ctx.post(
                        k,
                        wl_keyboard::event::KEYMAP,
                        &[
                            Arg::Uint(KEYMAP_FORMAT_XKB_V1),
                            Arg::Fd(km.fd.as_fd()),
                            Arg::Uint(km.size),
                        ],
                    );
                }
                if ctx.version(k) >= 4 {
                    let _ = ctx.post(
                        k,
                        wl_keyboard::event::REPEAT_INFO,
                        &[Arg::Int(REPEAT_RATE), Arg::Int(REPEAT_DELAY)],
                    );
                }
                if let Some(s) = self.seat.keyboard_focus {
                    if ctx.client(s) == ctx.client(k) {
                        self.keyboard_enter(ctx, k, s);
                    }
                }
            }
            wl_seat::request::GET_TOUCH => ctx.post_error(
                res,
                err::SEAT_MISSING_CAPABILITY,
                "get_touch: seat0 has no touch device",
            ),
            _ => {}
        }
    }

    pub(crate) fn pointer_request(
        &mut self,
        ctx: &Ctx,
        res: Resource,
        opcode: u32,
        args: &[ReqArg],
    ) {
        if opcode != wl_pointer::request::SET_CURSOR {
            return; // release: destructor
        }
        let (
            Some(ReqArg::Uint(serial)),
            Some(ReqArg::Object(surface)),
            Some(ReqArg::Int(hx)),
            Some(ReqArg::Int(hy)),
        ) = (args.first(), args.get(1), args.get(2), args.get(3))
        else {
            return;
        };
        // Only the client with pointer focus, answering its latest enter.
        let Some(focus) = self.seat.pointer_focus else {
            return;
        };
        if ctx.client(res) != Some(focus.client) || *serial != focus.serial {
            debug!(COMPOSITOR, "set_cursor with stale serial {serial} ignored");
            return;
        }
        let cursor = match surface {
            None => Cursor::Hidden,
            Some(s) => {
                let Some(surf) = self.surfaces.get_mut(s) else {
                    return;
                };
                match surf.role {
                    crate::surface::Role::None | crate::surface::Role::Cursor => {
                        surf.role = crate::surface::Role::Cursor;
                    }
                    _ => {
                        ctx.post_error(
                            res,
                            err::POINTER_ROLE,
                            "set_cursor: the surface has another role",
                        );
                        return;
                    }
                }
                Cursor::Surface {
                    surface: *s,
                    hx: *hx,
                    hy: *hy,
                }
            }
        };
        if cursor != self.seat.cursor {
            self.seat.cursor = cursor;
            self.needs_redraw = true;
        }
    }
}

fn pointer_enter(ctx: &Ctx, p: Resource, f: &Focus, sx: f64, sy: f64) {
    let _ = ctx.post(
        p,
        wl_pointer::event::ENTER,
        &[
            Arg::Uint(f.serial),
            Arg::Object(Some(f.surface)),
            Arg::Fixed(fixed_from_f64(sx)),
            Arg::Fixed(fixed_from_f64(sy)),
        ],
    );
    frame(ctx, p);
}

/// Ends a group of pointer events (wl_pointer v5+).
fn frame(ctx: &Ctx, p: Resource) {
    if ctx.version(p) >= 5 {
        let _ = ctx.post(p, wl_pointer::event::FRAME, &[]);
    }
}

fn send_modifiers(ctx: &Ctx, k: Resource, serial: u32, m: &wana_input::keyboard::Modifiers) {
    let _ = ctx.post(
        k,
        wl_keyboard::event::MODIFIERS,
        &[
            Arg::Uint(serial),
            Arg::Uint(m.depressed),
            Arg::Uint(m.latched),
            Arg::Uint(m.locked),
            Arg::Uint(m.group),
        ],
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Seek};

    #[test]
    fn keymap_file_is_sealed() {
        let km = sealed_keymap("xkb_keymap { };").unwrap();
        assert_eq!(km.size, 16, "text + NUL");
        let mut f = std::fs::File::from(km.fd);
        let mut back = Vec::new();
        f.read_to_end(&mut back).unwrap();
        assert_eq!(back, b"xkb_keymap { };\0");
        f.rewind().unwrap();
        assert!(f.write_all(b"evil").is_err(), "write sealed");
        assert!(f.set_len(0).is_err(), "shrink sealed");
        assert!(f.set_len(100).is_err(), "grow sealed");
    }

    #[test]
    fn monotonic_clock_advances() {
        let a = now_ms();
        std::thread::sleep(std::time::Duration::from_millis(5));
        assert!(now_ms().wrapping_sub(a) >= 5);
    }

    #[test]
    fn capability_names() {
        assert_eq!(caps_list(0), "none");
        assert_eq!(caps_list(CAP_POINTER | CAP_KEYBOARD), "pointer, keyboard");
    }
}
