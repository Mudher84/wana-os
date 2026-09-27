//! Window-management state and geometry rules (Phase 12).
//!
//! Kept free of libwayland so maximize/fullscreen/minimize/restore can be
//! unit-tested independently from the protocol plumbing.

use crate::layer::Rect;

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

    pub fn minimize(&mut self) {
        self.mode = Mode::Minimized;
    }

    pub fn unminimize(&mut self) {
        if self.mode == Mode::Minimized {
            self.mode = Mode::Normal;
        }
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

    const NORMAL: Rect = Rect { x: 100, y: 80, w: 640, h: 480 };
    const USABLE: Rect = Rect { x: 0, y: 40, w: 1280, h: 760 };
    const OUTPUT: Rect = Rect { x: 0, y: 0, w: 1280, h: 800 };

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
    fn configure_states_have_protocol_values() {
        let v = xdg_states(Mode::Maximized, true, false);
        let words: Vec<u32> = v
            .chunks_exact(4)
            .map(|x| u32::from_ne_bytes(x.try_into().unwrap()))
            .collect();
        assert_eq!(words, [1, 4]);
    }
}
