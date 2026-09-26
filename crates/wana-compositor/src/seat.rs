//! Seat state (Phase 10 step 4): pointer position and focus, keyboard
//! focus, the cursor image. Kept free of libwayland so the rules are
//! unit-tested directly; `input.rs` turns them into protocol events.
//!
//! Focus rules:
//! - pointer focus is the topmost window under the cursor; while a button
//!   is held it stays on the surface the press went to (implicit grab), so
//!   a drag that leaves the window keeps reporting to it;
//! - keyboard focus goes to a window when it is mapped, and to the window a
//!   button press lands on (click to focus, which also raises it); when the
//!   focused window goes away it moves to the topmost remaining window.

use std::collections::HashMap;
use wana_wayland::server::{ClientId, Resource};

/// wl_seat.capability bits.
pub const CAP_POINTER: u32 = 1;
pub const CAP_KEYBOARD: u32 = 2;

/// What the cursor shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cursor {
    /// The compositor's arrow.
    Default,
    /// `set_cursor(NULL)`: the focused client hid it.
    Hidden,
    /// A client surface with the cursor role, drawn at the pointer minus
    /// the hotspot.
    Surface { surface: Resource, hx: i32, hy: i32 },
}

/// A window as the hit test sees it: surface, position and size.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rect {
    pub surface: Resource,
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

/// The topmost window containing (`px`, `py`) (`rects` bottom first), with
/// the surface-local position.
pub fn hit(rects: &[Rect], px: f64, py: f64) -> Option<(Resource, f64, f64)> {
    rects.iter().rev().find_map(|r| {
        let (sx, sy) = (px - f64::from(r.x), py - f64::from(r.y));
        (sx >= 0.0 && sy >= 0.0 && sx < f64::from(r.w) && sy < f64::from(r.h))
            .then_some((r.surface, sx, sy))
    })
}

/// Pointer focus: the surface and the serial of the enter that gave it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Focus {
    pub surface: Resource,
    pub client: ClientId,
    pub serial: u32,
}

/// Input state of seat0.
#[derive(Debug)]
pub struct Seat {
    /// Bound wl_seat objects (capability updates go to all of them).
    pub seats: Vec<Resource>,
    pub pointers: Vec<Resource>,
    pub keyboards: Vec<Resource>,
    /// Current capability bits, from the devices present.
    pub caps: u32,
    /// Capabilities ever present (get_pointer/get_keyboard are protocol
    /// violations only for a capability the seat never had).
    pub ever: u32,
    /// Devices by event node, with their capability bits.
    pub devices: HashMap<String, u32>,
    /// Cursor position in output pixels.
    pub pos: (f64, f64),
    /// A pointing device was used: the cursor is shown and focus tracked.
    pub active: bool,
    /// Buttons held (evdev codes).
    pub buttons: Vec<u32>,
    pub pointer_focus: Option<Focus>,
    pub keyboard_focus: Option<Resource>,
    /// A window that should get keyboard focus (just mapped).
    pub focus_request: Option<Resource>,
    /// Keys held (evdev codes), for wl_keyboard.enter.
    pub keys: Vec<u32>,
    pub cursor: Cursor,
}

impl Seat {
    pub fn new(width: i32, height: i32) -> Seat {
        Seat {
            seats: Vec::new(),
            pointers: Vec::new(),
            keyboards: Vec::new(),
            caps: 0,
            ever: 0,
            devices: HashMap::new(),
            pos: (f64::from(width) / 2.0, f64::from(height) / 2.0),
            active: false,
            buttons: Vec::new(),
            pointer_focus: None,
            keyboard_focus: None,
            focus_request: None,
            keys: Vec::new(),
            cursor: Cursor::Default,
        }
    }

    /// Records a device (`caps` = 0 removes it); returns true if the seat's
    /// capabilities changed.
    pub fn set_device(&mut self, sysname: &str, caps: u32) -> bool {
        if caps == 0 {
            self.devices.remove(sysname);
        } else {
            self.devices.insert(sysname.to_owned(), caps);
        }
        let now = self.devices.values().fold(0, |a, c| a | c);
        self.ever |= now;
        std::mem::replace(&mut self.caps, now) != now
    }

    /// Moves the cursor by a relative motion, clamped to the output.
    pub fn move_by(&mut self, dx: f64, dy: f64, width: i32, height: i32) {
        self.pos = clamp((self.pos.0 + dx, self.pos.1 + dy), width, height);
        self.active = true;
    }

    /// Places the cursor from an absolute device (fractions 0..1).
    pub fn move_to(&mut self, fx: f64, fy: f64, width: i32, height: i32) {
        self.pos = clamp(
            (fx * f64::from(width), fy * f64::from(height)),
            width,
            height,
        );
        self.active = true;
    }

    /// Records a button; returns true for the first press of a grab.
    pub fn button(&mut self, code: u32, pressed: bool) -> bool {
        self.active = true;
        if pressed {
            let first = self.buttons.is_empty();
            if !self.buttons.contains(&code) {
                self.buttons.push(code);
            }
            first
        } else {
            self.buttons.retain(|b| *b != code);
            false
        }
    }

    /// Records a key; returns false for a repeated press or a release of a
    /// key that was not down (which are not forwarded).
    pub fn key(&mut self, code: u32, pressed: bool) -> bool {
        let down = self.keys.contains(&code);
        match (pressed, down) {
            (true, false) => {
                self.keys.push(code);
                true
            }
            (false, true) => {
                self.keys.retain(|k| *k != code);
                true
            }
            _ => false,
        }
    }

    /// The keys held, as wl_keyboard.enter's array (u32, native endian).
    pub fn keys_array(&self) -> Vec<u8> {
        self.keys.iter().flat_map(|k| k.to_ne_bytes()).collect()
    }

    /// Forgets a destroyed object. Returns true if it was the pointer focus
    /// surface (the cursor it set goes too).
    pub fn forget(&mut self, res: Resource) -> bool {
        self.seats.retain(|r| *r != res);
        self.pointers.retain(|r| *r != res);
        self.keyboards.retain(|r| *r != res);
        if self.keyboard_focus == Some(res) {
            self.keyboard_focus = None;
        }
        if self.focus_request == Some(res) {
            self.focus_request = None;
        }
        if matches!(self.cursor, Cursor::Surface { surface, .. } if surface == res) {
            self.cursor = Cursor::Default;
        }
        if self.pointer_focus.is_some_and(|f| f.surface == res) {
            self.pointer_focus = None;
            self.cursor = Cursor::Default;
            return true;
        }
        false
    }
}

fn clamp((x, y): (f64, f64), width: i32, height: i32) -> (f64, f64) {
    (
        x.clamp(0.0, f64::from(width - 1).max(0.0)),
        y.clamp(0.0, f64::from(height - 1).max(0.0)),
    )
}

/// Width, height and hotspot of the default arrow.
pub const ARROW_SIZE: (u32, u32) = (13, 20);
pub const ARROW_HOTSPOT: (i32, i32) = (0, 0);

/// The default arrow cursor as premultiplied BGRA pixels: white with a
/// black outline, transparent around it.
pub fn arrow() -> Vec<u8> {
    // Outline polygon, clockwise from the tip, in pixel units.
    const POLY: [(f64, f64); 7] = [
        (1.0, 0.0),
        (12.0, 11.0),
        (7.5, 11.0),
        (10.5, 18.0),
        (8.0, 19.0),
        (5.0, 12.0),
        (1.0, 16.0),
    ];
    let (w, h) = (ARROW_SIZE.0 as i32, ARROW_SIZE.1 as i32);
    let inside = |x: i32, y: i32| -> bool {
        if x < 0 || y < 0 || x >= w || y >= h {
            return false;
        }
        // Even-odd rule at the pixel center.
        let (px, py) = (f64::from(x) + 0.5, f64::from(y) + 0.5);
        let mut c = false;
        for i in 0..POLY.len() {
            let (x1, y1) = POLY[i];
            let (x2, y2) = POLY[(i + 1) % POLY.len()];
            if (y1 > py) != (y2 > py) && px < x1 + (py - y1) * (x2 - x1) / (y2 - y1) {
                c = !c;
            }
        }
        c
    };
    let mut px = Vec::with_capacity((w * h * 4) as usize);
    for y in 0..h {
        for x in 0..w {
            let (on, edge) = if inside(x, y) {
                (true, false)
            } else {
                // Outline: outside pixels touching the shape (8-neighbors).
                let near = (-1..=1).any(|dy| (-1..=1).any(|dx| inside(x + dx, y + dy)));
                (near, near)
            };
            let bgra: [u8; 4] = match (on, edge) {
                (true, false) => [0xFF, 0xFF, 0xFF, 0xFF],
                (true, true) => [0, 0, 0, 0xFF],
                _ => [0, 0, 0, 0],
            };
            px.extend_from_slice(&bgra);
        }
    }
    px
}

#[cfg(test)]
mod tests {
    use super::*;

    fn res(n: usize) -> Resource {
        Resource::from_raw_for_tests(0x1000 + n * 0x10)
    }

    #[test]
    fn hit_test_takes_the_topmost_window() {
        let (a, b) = (res(1), res(2));
        let rects = [
            Rect {
                surface: a,
                x: 0,
                y: 0,
                w: 100,
                h: 100,
            },
            Rect {
                surface: b,
                x: 50,
                y: 50,
                w: 100,
                h: 100,
            },
        ];
        assert_eq!(hit(&rects, 10.0, 10.0), Some((a, 10.0, 10.0)));
        assert_eq!(
            hit(&rects, 60.0, 70.0),
            Some((b, 10.0, 20.0)),
            "b is on top"
        );
        assert_eq!(hit(&rects, 149.5, 149.5), Some((b, 99.5, 99.5)));
        assert_eq!(hit(&rects, 150.0, 60.0), None, "right edge is outside");
        assert_eq!(hit(&[], 1.0, 1.0), None);
    }

    #[test]
    fn cursor_stays_on_the_output() {
        let mut s = Seat::new(1280, 800);
        assert_eq!(s.pos, (640.0, 400.0), "starts at the center");
        assert!(!s.active, "hidden until a pointing device is used");
        s.move_by(-5000.0, 10.0, 1280, 800);
        assert_eq!(s.pos, (0.0, 410.0));
        assert!(s.active);
        s.move_to(1.0, 1.0, 1280, 800);
        assert_eq!(s.pos, (1279.0, 799.0));
        s.move_to(0.25, 0.5, 1280, 800);
        assert_eq!(s.pos, (320.0, 400.0));
    }

    #[test]
    fn buttons_form_an_implicit_grab() {
        let mut s = Seat::new(10, 10);
        assert!(s.button(272, true), "first press starts the grab");
        assert!(!s.button(273, true), "second button joins it");
        assert!(!s.button(272, false));
        assert_eq!(s.buttons, vec![273], "grab held until all released");
        s.button(273, false);
        assert!(s.buttons.is_empty());
    }

    #[test]
    fn keys_are_tracked_for_enter() {
        let mut s = Seat::new(10, 10);
        assert!(s.key(30, true));
        assert!(!s.key(30, true), "repeat from the device is not forwarded");
        assert!(s.key(42, true));
        assert_eq!(
            s.keys_array(),
            [30u32.to_ne_bytes(), 42u32.to_ne_bytes()].concat()
        );
        assert!(s.key(30, false));
        assert!(!s.key(30, false), "release without press");
        assert_eq!(s.keys, vec![42]);
    }

    #[test]
    fn capabilities_follow_devices() {
        let mut s = Seat::new(10, 10);
        assert!(s.set_device("event1", CAP_KEYBOARD));
        assert!(!s.set_device("event0", CAP_KEYBOARD), "no change");
        assert!(s.set_device("event2", CAP_POINTER));
        assert_eq!(s.caps, CAP_POINTER | CAP_KEYBOARD);
        assert!(s.set_device("event2", 0), "pointer unplugged");
        assert_eq!(s.caps, CAP_KEYBOARD);
        assert_eq!(s.ever, CAP_POINTER | CAP_KEYBOARD, "history kept");
    }

    #[test]
    fn destroyed_focus_is_forgotten() {
        let mut s = Seat::new(10, 10);
        let (surf, cur) = (res(1), res(2));
        s.pointer_focus = Some(Focus {
            surface: surf,
            client: ClientId::from_raw_for_tests(1),
            serial: 7,
        });
        s.keyboard_focus = Some(surf);
        s.cursor = Cursor::Surface {
            surface: cur,
            hx: 0,
            hy: 0,
        };
        assert!(!s.forget(cur));
        assert_eq!(s.cursor, Cursor::Default, "cursor surface gone");
        s.cursor = Cursor::Hidden;
        assert!(s.forget(surf));
        assert_eq!((s.pointer_focus, s.keyboard_focus), (None, None));
        assert_eq!(s.cursor, Cursor::Default, "left the client that hid it");
    }

    #[test]
    fn arrow_has_fill_outline_and_transparency() {
        let px = arrow();
        let (w, h) = ARROW_SIZE;
        assert_eq!(px.len(), (w * h * 4) as usize);
        let at = |x: u32, y: u32| {
            let i = ((y * w + x) * 4) as usize;
            [px[i], px[i + 1], px[i + 2], px[i + 3]]
        };
        assert_eq!(at(0, 0), [0, 0, 0, 0xFF], "outline at the tip");
        assert_eq!(at(2, 6), [0xFF; 4], "white inside");
        assert_eq!(at(12, 19), [0; 4], "transparent corner");
        assert_eq!(at(12, 0), [0; 4]);
        // Premultiplied: every pixel is fully opaque or fully transparent.
        assert!(px.chunks(4).all(|p| p[3] == 0xFF || p == [0, 0, 0, 0]));
    }
}
