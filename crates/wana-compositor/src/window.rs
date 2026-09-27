//! Window-management state and geometry rules (Phase 12).
//!
//! Kept free of libwayland so maximize/fullscreen/minimize/restore can be
//! unit-tested independently from the protocol plumbing.

use crate::layer::Rect;
use wana_wayland::server::Resource;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Mode {
    #[default]
    Normal,
    Maximized,
    Fullscreen,
    Minimized,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct State {
    pub mode: Mode,
    /// Position/size to restore after maximize/fullscreen.
    pub restore: Option<Rect>,
    pub min: (i32, i32),
    /// 0 means no maximum, as xdg-shell specifies.
    pub max: (i32, i32),
}

impl Default for State {
    fn default() -> Self {
        State {
            mode: Mode::Normal,
            restore: None,
            min: (0, 0),
            max: (0, 0),
        }
    }
}

impl State {
    pub fn maximize(&mut self, current: Rect, usable: Rect) -> Rect {
        if self.mode == Mode::Normal {
            self.restore = Some(current);
        }
        self.mode = Mode::Maximized;
        usable
    }

    pub fn fullscreen(&mut self, current: Rect, output: Rect) -> Rect {
        if self.mode == Mode::Normal {
            self.restore = Some(current);
        }
        self.mode = Mode::Fullscreen;
        output
    }

    pub fn restore(&mut self, fallback: Rect) -> Rect {
        let out = self.restore.take().unwrap_or(fallback);
        self.mode = Mode::Normal;
        out
    }

    pub fn minimize(&mut self, current: Rect) {
        if self.mode == Mode::Normal {
            self.restore = Some(current);
        }
        self.mode = Mode::Minimized;
    }

    pub fn unminimize(&mut self, fallback: Rect) -> Rect {
        let out = self.restore.take().unwrap_or(fallback);
        self.mode = Mode::Normal;
        out
    }

    pub fn constrain(&self, mut w: i32, mut h: i32) -> (i32, i32) {
        w = w.max(self.min.0.max(1));
        h = h.max(self.min.1.max(1));
        if self.max.0 > 0 {
            w = w.min(self.max.0);
        }
        if self.max.1 > 0 {
            h = h.min(self.max.1);
        }
        (w, h)
    }
}

/// xdg_toplevel.configure states encoded as a Wayland uint array.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GrabKind {
    Move,
    Resize(ResizeEdge),
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Grab {
    pub surface: Resource,
    pub toplevel: Resource,
    pub kind: GrabKind,
    pub start: (f64, f64),
    pub initial: Rect,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResizeEdge {
    Top,
    Bottom,
    Left,
    TopLeft,
    BottomLeft,
    Right,
    TopRight,
    BottomRight,
}

impl ResizeEdge {
    pub fn from_xdg(v: u32) -> Option<Self> {
        match v {
            1 => Some(Self::Top),
            2 => Some(Self::Bottom),
            4 => Some(Self::Left),
            5 => Some(Self::TopLeft),
            6 => Some(Self::BottomLeft),
            8 => Some(Self::Right),
            9 => Some(Self::TopRight),
            10 => Some(Self::BottomRight),
            _ => None,
        }
    }

    fn left(self) -> bool {
        matches!(self, Self::Left | Self::TopLeft | Self::BottomLeft)
    }

    fn right(self) -> bool {
        matches!(self, Self::Right | Self::TopRight | Self::BottomRight)
    }

    fn top(self) -> bool {
        matches!(self, Self::Top | Self::TopLeft | Self::TopRight)
    }

    fn bottom(self) -> bool {
        matches!(self, Self::Bottom | Self::BottomLeft | Self::BottomRight)
    }
}

pub fn moved(initial: Rect, dx: f64, dy: f64) -> Rect {
    Rect {
        x: initial.x.saturating_add(dx.round() as i32),
        y: initial.y.saturating_add(dy.round() as i32),
        ..initial
    }
}

pub fn resized(state: &State, initial: Rect, edge: ResizeEdge, dx: f64, dy: f64) -> Rect {
    let dx = dx.round() as i32;
    let dy = dy.round() as i32;
    let mut x = initial.x;
    let mut y = initial.y;
    let mut w = initial.w;
    let mut h = initial.h;

    if edge.left() {
        x = x.saturating_add(dx);
        w = w.saturating_sub(dx);
    }
    if edge.right() {
        w = w.saturating_add(dx);
    }
    if edge.top() {
        y = y.saturating_add(dy);
        h = h.saturating_sub(dy);
    }
    if edge.bottom() {
        h = h.saturating_add(dy);
    }

    let old_right = initial.x.saturating_add(initial.w);
    let old_bottom = initial.y.saturating_add(initial.h);
    let (cw, ch) = state.constrain(w, h);
    if edge.left() {
        x = old_right.saturating_sub(cw);
    }
    if edge.top() {
        y = old_bottom.saturating_sub(ch);
    }
    Rect { x, y, w: cw, h: ch }
}

/// xdg_toplevel.configure states encoded as a Wayland uint array.
pub fn xdg_states(mode: Mode, activated: bool, resizing: bool) -> Vec<u8> {
    let mut states = Vec::new();
    if mode == Mode::Maximized {
        states.extend_from_slice(&1u32.to_ne_bytes());
    }
    if mode == Mode::Fullscreen {
        states.extend_from_slice(&2u32.to_ne_bytes());
    }
    if resizing {
        states.extend_from_slice(&3u32.to_ne_bytes());
    }
    if activated {
        states.extend_from_slice(&4u32.to_ne_bytes());
    }
    states
}

#[cfg(test)]
mod tests {
    use super::*;

    const NORMAL: Rect = Rect {
        x: 100,
        y: 80,
        w: 640,
        h: 480,
    };
    const USABLE: Rect = Rect {
        x: 0,
        y: 40,
        w: 1280,
        h: 760,
    };
    const OUTPUT: Rect = Rect {
        x: 0,
        y: 0,
        w: 1280,
        h: 800,
    };

    #[test]
    fn maximize_and_fullscreen_restore_the_original_geometry() {
        let mut s = State::default();
        assert_eq!(s.maximize(NORMAL, USABLE), USABLE);
        assert_eq!(s.mode, Mode::Maximized);
        assert_eq!(s.restore(NORMAL), NORMAL);

        assert_eq!(s.fullscreen(NORMAL, OUTPUT), OUTPUT);
        assert_eq!(s.mode, Mode::Fullscreen);
        assert_eq!(s.restore(NORMAL), NORMAL);
    }

    #[test]
    fn constraints_follow_xdg_zero_means_unbounded_rule() {
        let mut s = State::default();
        s.min = (320, 200);
        s.max = (900, 700);
        assert_eq!(s.constrain(100, 100), (320, 200));
        assert_eq!(s.constrain(1200, 900), (900, 700));
        s.max = (0, 0);
        assert_eq!(s.constrain(1200, 900), (1200, 900));
    }

    #[test]
    fn moving_keeps_size_and_changes_origin() {
        assert_eq!(
            moved(NORMAL, 20.4, -10.6),
            Rect {
                x: 120,
                y: 69,
                ..NORMAL
            }
        );
    }

    #[test]
    fn resizing_respects_edges_and_constraints() {
        let mut s = State::default();
        s.min = (320, 200);
        let r = resized(&s, NORMAL, ResizeEdge::TopLeft, 500.0, 500.0);
        assert_eq!(r.w, 320);
        assert_eq!(r.h, 200);
        assert_eq!(r.x + r.w, NORMAL.x + NORMAL.w);
        assert_eq!(r.y + r.h, NORMAL.y + NORMAL.h);
        assert_eq!(ResizeEdge::from_xdg(10), Some(ResizeEdge::BottomRight));
        assert_eq!(ResizeEdge::from_xdg(3), None);
    }

    #[test]
    fn configure_states_have_protocol_values() {
        let v = xdg_states(Mode::Maximized, true, false);
        let words: Vec<u32> = v
            .chunks_exact(4)
            .map(|x| u32::from_ne_bytes(x.try_into().unwrap()))
            .collect();
        assert_eq!(words, [1, 4]);
    }
}
