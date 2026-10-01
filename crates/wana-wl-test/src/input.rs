//! The seat as a client sees it: pointer and keyboard objects, the keymap
//! the compositor sends, and which window has which focus.

use crate::client::{Connection, Event, Proxy, Req, Val};
use crate::wait_for;
use crate::LOG;
use std::fs::File;
use std::os::unix::io::{AsRawFd, FromRawFd};
use wana_input::keyboard::{Keyboard, Modifiers};
use wana_log::{debug, info};
use wana_wayland::protocols::wayland;

extern "C" {
    fn mmap(addr: *mut u8, len: usize, prot: i32, flags: i32, fd: i32, off: i64) -> *mut u8;
    fn munmap(addr: *mut u8, len: usize) -> i32;
}
const PROT_READ: i32 = 1;
const MAP_PRIVATE: i32 = 2;
const MAP_FAILED: *mut u8 = !0usize as *mut u8;
const BTN_LEFT: u32 = 0x110;
const CAP_POINTER: u32 = 1;
const CAP_KEYBOARD: u32 = 2;

/// What the input test waits for: the pointer on window `pointer`, a left
/// click on window `click`, and `text` typed while `focus` had the keyboard.
#[derive(Debug, Clone)]
pub struct Goal {
    pub pointer: &'static str,
    pub click: &'static str,
    pub focus: &'static str,
    pub text: String,
}

/// The seat's pointer and keyboard, and what arrived through them.
#[derive(Debug)]
pub struct Devices {
    pointer: Proxy,
    keyboard: Proxy,
    xkb: Option<Keyboard>,
    /// Known windows: surface and log name.
    windows: Vec<(Proxy, &'static str)>,
    keyboard_focus: Option<&'static str>,
    pointer_focus: Option<&'static str>,
    motions: u32,
    /// Window a left press went to, until its release.
    left_down: Option<&'static str>,
    clicks: Vec<&'static str>,
    /// Last pointer position (surface-local).
    at: (f64, f64),
    /// Text typed since the keyboard focus last changed.
    typed: String,
    /// A cursor surface to set on every pointer enter (hotspot 0,0).
    cursor: Option<Proxy>,
    /// Serial of a pointer enter that still needs its set_cursor.
    cursor_due: Option<u32>,
}

impl Devices {
    /// Waits for pointer + keyboard capabilities, then creates both objects
    /// (before any window maps, so the keyboard enter on map reaches them).
    pub fn new(conn: &Connection, seat: Proxy) -> Result<Devices, String> {
        let mut caps = 0;
        for _ in 0..1000 {
            conn.roundtrip()?;
            while let Some(ev) = conn.next_event() {
                if ev.target == seat && ev.opcode == wayland::wl_seat::event::CAPABILITIES {
                    if let Some(Val::Uint(c)) = ev.args.first() {
                        caps = *c;
                    }
                }
            }
            if caps & (CAP_POINTER | CAP_KEYBOARD) == CAP_POINTER | CAP_KEYBOARD {
                break;
            }
            conn.dispatch()?;
        }
        info!(
            LOG,
            "client: seat capabilities {caps:#x} (pointer + keyboard)"
        );
        let pointer = conn
            .request(
                seat,
                wayland::wl_seat::request::GET_POINTER,
                Some((&wayland::WL_POINTER_INTERFACE, 7)),
                &[Req::NewId],
            )?
            .expect("pointer");
        let keyboard = conn
            .request(
                seat,
                wayland::wl_seat::request::GET_KEYBOARD,
                Some((&wayland::WL_KEYBOARD_INTERFACE, 7)),
                &[Req::NewId],
            )?
            .expect("keyboard");
        Ok(Devices {
            pointer,
            keyboard,
            xkb: None,
            windows: Vec::new(),
            keyboard_focus: None,
            pointer_focus: None,
            motions: 0,
            left_down: None,
            clicks: Vec::new(),
            at: (0.0, 0.0),
            typed: String::new(),
            cursor: None,
            cursor_due: None,
        })
    }

    /// Sets `surface` (with a committed buffer) as the cursor image on
    /// every pointer enter.
    pub fn use_cursor(&mut self, surface: Proxy) {
        self.cursor = Some(surface);
    }

    /// Answers a pending pointer enter with set_cursor.
    fn apply_cursor(&mut self, conn: &Connection) -> Result<(), String> {
        if let (Some(serial), Some(surface)) = (self.cursor_due.take(), self.cursor) {
            conn.request(
                self.pointer,
                wayland::wl_pointer::request::SET_CURSOR,
                None,
                &[
                    Req::Uint(serial),
                    Req::Object(Some(surface)),
                    Req::Int(0),
                    Req::Int(0),
                ],
            )?;
            debug!(LOG, "client: set_cursor (serial {serial})");
        }
        Ok(())
    }

    /// Names `surface` in the log lines.
    pub fn add_window(&mut self, surface: Proxy, name: &'static str) {
        self.windows.push((surface, name));
    }

    fn name(&self, v: &Val) -> Result<&'static str, String> {
        match v {
            Val::Object(Some(p)) => self
                .windows
                .iter()
                .find(|(s, _)| s == p)
                .map(|(_, n)| *n)
                .ok_or_else(|| "focus event for a surface that is not ours".to_string()),
            _ => Err("focus event without a surface".into()),
        }
    }

    fn done(&self, goal: &Goal) -> bool {
        self.pointer_focus == Some(goal.pointer)
            && self.clicks.contains(&goal.click)
            && self.keyboard_focus == Some(goal.focus)
            && self.typed.ends_with(&goal.text)
    }

    /// Dispatches until window `first_focus` has keyboard focus and the
    /// keymap is compiled, reports readiness, then waits for `goal`.
    pub fn wait_for_input(
        &mut self,
        conn: &Connection,
        wm_base: Proxy,
        first_focus: &'static str,
        goal: &Goal,
    ) -> Result<(), String> {
        let ready = |d: &Devices| d.keyboard_focus == Some(first_focus) && d.xkb.is_some();
        if !ready(self) {
            wait_for(conn, wm_base, None, |ev| {
                if let Err(e) = self.event(ev).and_then(|()| self.apply_cursor(conn)) {
                    return Some(Err(e));
                }
                ready(self).then_some(Ok(()))
            })??;
        }
        info!(
            LOG,
            "client: ready for input (keyboard focus on window {first_focus:?}, keymap compiled)"
        );
        if !self.done(goal) {
            wait_for(conn, wm_base, None, |ev| {
                if let Err(e) = self.event(ev).and_then(|()| self.apply_cursor(conn)) {
                    return Some(Err(e));
                }
                self.done(goal).then_some(Ok(()))
            })??;
        }
        info!(
            LOG,
            "client: input received: typed {:?} on window {:?}, left click on window {:?}, {} motion event(s)",
            self.typed,
            goal.focus,
            goal.click,
            self.motions
        );
        Ok(())
    }

    /// Handles one event (anything not for the pointer or keyboard is
    /// ignored).
    pub fn event(&mut self, ev: &Event) -> Result<(), String> {
        use wayland::{wl_keyboard::event as kev, wl_pointer::event as pev};
        let fixed = |v: &Val| match v {
            Val::Int(f) => f64::from(*f) / 256.0,
            _ => 0.0,
        };
        if ev.target == self.keyboard {
            match (ev.opcode, &ev.args[..]) {
                (kev::KEYMAP, [Val::Uint(format), Val::Int(fd), Val::Uint(size)]) => {
                    self.xkb = Some(load_keymap(*format, *fd, *size)?);
                }
                (kev::ENTER, [_, s, Val::Array(keys)]) => {
                    let name = self.name(s)?;
                    self.keyboard_focus = Some(name);
                    self.typed.clear();
                    info!(
                        LOG,
                        "client: keyboard focus on window {name:?} ({} key(s) held)",
                        keys.len() / 4
                    );
                }
                (kev::LEAVE, [_, s]) => {
                    let name = self.name(s)?;
                    if self.keyboard_focus != Some(name) {
                        return Err(format!("keyboard leave from {name:?}, which had no focus"));
                    }
                    self.keyboard_focus = None;
                    info!(LOG, "client: keyboard focus left window {name:?}");
                }
                (kev::KEY, [_, _, Val::Uint(key), Val::Uint(state)]) => {
                    let xkb = self.xkb.as_ref().ok_or("key before keymap")?;
                    let Some(focus) = self.keyboard_focus else {
                        return Err(format!("key {key} without keyboard focus"));
                    };
                    if *state == 1 {
                        let k = xkb.lookup(*key);
                        info!(
                            LOG,
                            "client: key {key} pressed on window {focus:?}: {} text {:?}",
                            k.keysym,
                            k.text
                        );
                        self.typed.push_str(&k.text);
                    }
                }
                (
                    kev::MODIFIERS,
                    [_, Val::Uint(depressed), Val::Uint(latched), Val::Uint(locked), Val::Uint(group)],
                ) => {
                    let m = Modifiers {
                        depressed: *depressed,
                        latched: *latched,
                        locked: *locked,
                        group: *group,
                    };
                    debug!(LOG, "client: modifiers {m:?}");
                    if let Some(xkb) = self.xkb.as_mut() {
                        xkb.set_modifiers(&m);
                    }
                }
                _ => {}
            }
        } else if ev.target == self.pointer {
            match (ev.opcode, &ev.args[..]) {
                (pev::ENTER, [serial, s, x, y]) => {
                    let name = self.name(s)?;
                    if let Val::Uint(serial) = serial {
                        self.cursor_due = Some(*serial);
                    }
                    self.pointer_focus = Some(name);
                    self.at = (fixed(x), fixed(y));
                    info!(
                        LOG,
                        "client: pointer entered window {name:?} at {:.1},{:.1}",
                        self.at.0,
                        self.at.1
                    );
                }
                (pev::LEAVE, [_, s]) => {
                    let name = self.name(s)?;
                    if self.pointer_focus == Some(name) {
                        self.pointer_focus = None;
                    }
                    info!(LOG, "client: pointer left window {name:?}");
                }
                (pev::MOTION, [_, x, y]) => {
                    self.motions += 1;
                    self.at = (fixed(x), fixed(y));
                }
                (pev::BUTTON, [_, _, Val::Uint(button), Val::Uint(state)]) => {
                    let Some(on) = self.pointer_focus else {
                        return Err(format!("button {button} without pointer focus"));
                    };
                    if *button == BTN_LEFT {
                        if *state == 1 {
                            self.left_down = Some(on);
                        } else if self.left_down.take() == Some(on) {
                            self.clicks.push(on);
                            info!(
                                LOG,
                                "client: left click on window {on:?} at {:.1},{:.1}",
                                self.at.0,
                                self.at.1
                            );
                        }
                    }
                }
                _ => {}
            }
        }
        Ok(())
    }
}

/// Maps the keymap file the compositor sent (MAP_PRIVATE, as wl_keyboard
/// v7 requires) and compiles it.
fn load_keymap(format: u32, fd: i32, size: u32) -> Result<Keyboard, String> {
    // SAFETY: the fd was received with the event and is owned from here.
    let file = unsafe { File::from_raw_fd(fd) };
    if format != 1 {
        return Err(format!("keymap format {format}, expected 1 (xkb v1)"));
    }
    let len = size as usize;
    // SAFETY: read-only private mapping of `len` bytes of the file.
    let p = unsafe {
        mmap(
            std::ptr::null_mut(),
            len,
            PROT_READ,
            MAP_PRIVATE,
            file.as_raw_fd(),
            0,
        )
    };
    if p == MAP_FAILED {
        return Err(format!("keymap mmap: {}", std::io::Error::last_os_error()));
    }
    // SAFETY: the mapping is `len` bytes; unmapped right after copying.
    let bytes = unsafe { std::slice::from_raw_parts(p, len) }.to_vec();
    // SAFETY: mapping from above.
    unsafe { munmap(p, len) };
    let text = std::ffi::CStr::from_bytes_until_nul(&bytes)
        .map_err(|_| "keymap is not NUL-terminated".to_string())?
        .to_str()
        .map_err(|e| format!("keymap: {e}"))?;
    let kb = Keyboard::from_string(text)?;
    info!(
        LOG,
        "client: keymap received: {} bytes, layout {}",
        size,
        kb.layout_name()
    );
    Ok(kb)
}
