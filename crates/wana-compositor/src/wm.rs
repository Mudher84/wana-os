//! Phase 12 window-management policy.
//!
//! This module is intentionally free of libwayland. It owns geometry and
//! state transitions; the protocol layer translates xdg_toplevel requests
//! into these transitions and sends the resulting configure events.

use crate::layer::Rect;
use wana_wayland::server::Resource;

pub const STATE_MAXIMIZED: u32 = 1;
pub const STATE_FULLSCREEN: u32 = 2;
pub const STATE_RESIZING: u32 = 3;
pub const STATE_ACTIVATED: u32 = 4;

pub const EDGE_TOP: u32 = 1;
pub const EDGE_BOTTOM: u32 = 2;
pub const EDGE_LEFT: u32 = 4;
pub const EDGE_TOP_LEFT: u32 = 5;
pub const EDGE_BOTTOM_LEFT: u32 = 6;
pub const EDGE_RIGHT: u32 = 8;
pub const EDGE_TOP_RIGHT: u32 = 9;
pub const EDGE_BOTTOM_RIGHT: u32 = 10;

pub fn valid_resize_edge(edge: u32) -> bool {
    matches!(
        edge,
        EDGE_TOP
            | EDGE_BOTTOM
            | EDGE_LEFT
            | EDGE_TOP_LEFT
            | EDGE_BOTTOM_LEFT
            | EDGE_RIGHT
            | EDGE_TOP_RIGHT
            | EDGE_BOTTOM_RIGHT
    )
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum GrabKind {
    Move,
    Resize(u32),
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Grab {
    pub surface: Resource,
    pub pointer: (f64, f64),
    pub rect: Rect,
    pub kind: GrabKind,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Mode {
    #[default]
    Normal,
    Maximized,
    Fullscreen,
    Minimized,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    pub min_w: i32,
    pub min_h: i32,
    /// Zero means unconstrained, as xdg_toplevel specifies.
    pub max_w: i32,
    pub max_h: i32,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            min_w: 1,
            min_h: 1,
            max_w: 0,
            max_h: 0,
        }
    }
}

impl Limits {
    pub fn clamp(self, w: i32, h: i32) -> (i32, i32) {
        let min_w = self.min_w.max(1);
        let min_h = self.min_h.max(1);
        let max_w = if self.max_w <= 0 {
            i32::MAX
        } else {
            self.max_w.max(min_w)
        };
        let max_h = if self.max_h <= 0 {
            i32::MAX
        } else {
            self.max_h.max(min_h)
        };
        (w.clamp(min_w, max_w), h.clamp(min_h, max_h))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WindowState {
    pub mode: Mode,
    pub rect: Rect,
    pub restore: Rect,
    pub limits: Limits,
    pub resizing: bool,
    pub activated: bool,
}

impl WindowState {
    pub fn new(rect: Rect) -> Self {
        Self {
            mode: Mode::Normal,
            rect,
            restore: rect,
            limits: Limits::default(),
            resizing: false,
            activated: false,
        }
    }

    pub fn normal(&mut self) {
        if self.mode != Mode::Normal {
            self.rect = self.restore;
            self.mode = Mode::Normal;
        }
        self.resizing = false;
    }

    pub fn maximize(&mut self, usable: Rect) {
        if self.mode == Mode::Normal {
            self.restore = self.rect;
        }
        self.rect = usable;
        self.mode = Mode::Maximized;
        self.resizing = false;
    }

    pub fn fullscreen(&mut self, output: Rect) {
        if self.mode == Mode::Normal {
            self.restore = self.rect;
        }
        self.rect = output;
        self.mode = Mode::Fullscreen;
        self.resizing = false;
    }

    pub fn minimize(&mut self) {
        if self.mode == Mode::Normal {
            self.restore = self.rect;
        }
        self.mode = Mode::Minimized;
        self.resizing = false;
        self.activated = false;
    }

    pub fn set_min_size(&mut self, w: i32, h: i32) {
        self.limits.min_w = w.max(1);
        self.limits.min_h = h.max(1);
        let (rw, rh) = self.limits.clamp(self.rect.w, self.rect.h);
        self.rect.w = rw;
        self.rect.h = rh;
    }

    pub fn set_max_size(&mut self, w: i32, h: i32) {
        self.limits.max_w = w.max(0);
        self.limits.max_h = h.max(0);
        let (rw, rh) = self.limits.clamp(self.rect.w, self.rect.h);
        self.rect.w = rw;
        self.rect.h = rh;
    }

    pub fn begin_resize(&mut self) {
        if self.mode == Mode::Normal {
            self.restore = self.rect;
            self.resizing = true;
        }
    }

    pub fn end_resize(&mut self) {
        self.resizing = false;
    }

    pub fn move_to(&mut self, x: i32, y: i32, bounds: Rect) {
        if self.mode != Mode::Normal {
            return;
        }
        let max_x = bounds.x + (bounds.w - self.rect.w).max(0);
        let max_y = bounds.y + (bounds.h - self.rect.h).max(0);
        self.rect.x = x.clamp(bounds.x, max_x);
        self.rect.y = y.clamp(bounds.y, max_y);
        self.restore = self.rect;
    }

    pub fn resize_from(&mut self, start: Rect, edge: u32, dx: i32, dy: i32, bounds: Rect) {
        if self.mode != Mode::Normal {
            return;
        }
        let mut x = start.x;
        let mut y = start.y;
        let mut w = start.w;
        let mut h = start.h;

        if edge & EDGE_LEFT != 0 {
            x = start.x + dx;
            w = start.w - dx;
        } else if edge & EDGE_RIGHT != 0 {
            w = start.w + dx;
        }
        if edge & EDGE_TOP != 0 {
            y = start.y + dy;
            h = start.h - dy;
        } else if edge & EDGE_BOTTOM != 0 {
            h = start.h + dy;
        }

        let (nw, nh) = self.limits.clamp(w, h);
        if edge & EDGE_LEFT != 0 {
            x = start.x + start.w - nw;
        }
        if edge & EDGE_TOP != 0 {
            y = start.y + start.h - nh;
        }
        w = nw;
        h = nh;

        let max_x = bounds.x + bounds.w;
        let max_y = bounds.y + bounds.h;
        x = x.clamp(bounds.x, max_x.saturating_sub(1));
        y = y.clamp(bounds.y, max_y.saturating_sub(1));
        w = w.min((max_x - x).max(1));
        h = h.min((max_y - y).max(1));

        self.rect = Rect { x, y, w, h };
        self.restore = self.rect;
        self.resizing = true;
    }

    pub fn configure_states(self) -> Vec<u8> {
        let mut states = Vec::<u32>::new();
        match self.mode {
            Mode::Maximized => states.push(STATE_MAXIMIZED),
            Mode::Fullscreen => states.push(STATE_FULLSCREEN),
            Mode::Normal | Mode::Minimized => {}
        }
        if self.resizing {
            states.push(STATE_RESIZING);
        }
        if self.activated {
            states.push(STATE_ACTIVATED);
        }
        states.into_iter().flat_map(u32::to_ne_bytes).collect()
    }

    pub fn visible(self) -> bool {
        self.mode != Mode::Minimized
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const OUT: Rect = Rect {
        x: 0,
        y: 0,
        w: 1280,
        h: 800,
    };
    const USABLE: Rect = Rect {
        x: 0,
        y: 40,
        w: 1280,
        h: 760,
    };
    const NORMAL: Rect = Rect {
        x: 400,
        y: 260,
        w: 480,
        h: 320,
    };

    #[test]
    fn maximize_and_fullscreen_restore_normal_geometry() {
        let mut w = WindowState::new(NORMAL);
        w.maximize(USABLE);
        assert_eq!(w.mode, Mode::Maximized);
        assert_eq!(w.rect, USABLE);
        w.normal();
        assert_eq!(w.rect, NORMAL);

        w.fullscreen(OUT);
        assert_eq!(w.rect, OUT);
        w.normal();
        assert_eq!(w.rect, NORMAL);
    }

    #[test]
    fn minimized_windows_are_hidden_but_restore() {
        let mut w = WindowState::new(NORMAL);
        w.minimize();
        assert!(!w.visible());
        w.normal();
        assert!(w.visible());
        assert_eq!(w.rect, NORMAL);
    }

    #[test]
    fn move_is_clamped_to_usable_area() {
        let mut w = WindowState::new(NORMAL);
        w.move_to(-100, 900, USABLE);
        assert_eq!(w.rect.x, 0);
        assert_eq!(w.rect.y, 480);
    }

    #[test]
    fn resize_obeys_edges_limits_and_bounds() {
        let mut w = WindowState::new(NORMAL);
        w.set_min_size(300, 200);
        w.set_max_size(700, 600);
        w.resize_from(NORMAL, EDGE_BOTTOM_RIGHT, 400, 500, USABLE);
        assert_eq!((w.rect.w, w.rect.h), (700, 540));

        w.resize_from(NORMAL, EDGE_TOP_LEFT, 400, 300, USABLE);
        assert_eq!((w.rect.w, w.rect.h), (300, 200));
        assert_eq!((w.rect.x, w.rect.y), (580, 380));
    }

    #[test]
    fn resize_edges_accept_only_protocol_edges() {
        for edge in [
            EDGE_TOP,
            EDGE_BOTTOM,
            EDGE_LEFT,
            EDGE_TOP_LEFT,
            EDGE_BOTTOM_LEFT,
            EDGE_RIGHT,
            EDGE_TOP_RIGHT,
            EDGE_BOTTOM_RIGHT,
        ] {
            assert!(valid_resize_edge(edge));
        }
        assert!(!valid_resize_edge(0));
        assert!(!valid_resize_edge(3));
        assert!(!valid_resize_edge(11));
    }

    #[test]
    fn configure_states_are_native_u32_values() {
        let mut w = WindowState::new(NORMAL);
        w.maximize(USABLE);
        w.activated = true;
        w.begin_resize(); // ignored while maximized
        let vals: Vec<u32> = w
            .configure_states()
            .chunks_exact(4)
            .map(|b| u32::from_ne_bytes(b.try_into().unwrap()))
            .collect();
        assert_eq!(vals, [STATE_MAXIMIZED, STATE_ACTIVATED]);
    }
}
