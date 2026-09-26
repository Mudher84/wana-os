//! Layer surfaces (wlr-layer-shell, decision 0003): state, validation and
//! arrangement, free of libwayland so the rules are unit-tested directly.
//!
//! Arrangement on one output, as the protocol describes it:
//! 1. surfaces with a positive exclusive zone (anchored to one edge, or one
//!    edge and both perpendicular ones) are placed first, overlay down to
//!    background, each against the area still usable, and each takes its
//!    zone (margin included) off that edge of the usable area;
//! 2. the other surfaces are placed in the usable area (zone 0) or on the
//!    whole output (zone -1).
//!
//! Placement: a size of 0 stretches between the anchored edges minus the
//! margins; an unanchored axis is centered.
//!
//! Stacking: background, bottom, windows, top, overlay.

/// `zwlr_layer_shell_v1.layer`.
pub const BACKGROUND: u32 = 0;
pub const BOTTOM: u32 = 1;
pub const TOP: u32 = 2;
pub const OVERLAY: u32 = 3;

/// `zwlr_layer_surface_v1.anchor` bits.
pub const ANCHOR_TOP: u32 = 1;
pub const ANCHOR_BOTTOM: u32 = 2;
pub const ANCHOR_LEFT: u32 = 4;
pub const ANCHOR_RIGHT: u32 = 8;

/// `zwlr_layer_surface_v1.keyboard_interactivity`.
pub const KEYBOARD_NONE: u32 = 0;
pub const KEYBOARD_EXCLUSIVE: u32 = 1;
pub const KEYBOARD_ON_DEMAND: u32 = 2;

/// Protocol error codes.
pub mod err {
    /// zwlr_layer_shell_v1
    pub const SHELL_ROLE: u32 = 0;
    pub const SHELL_INVALID_LAYER: u32 = 1;
    pub const SHELL_ALREADY_CONSTRUCTED: u32 = 2;
    /// zwlr_layer_surface_v1
    pub const INVALID_SURFACE_STATE: u32 = 0;
    pub const INVALID_SIZE: u32 = 1;
    pub const INVALID_ANCHOR: u32 = 2;
    pub const INVALID_KEYBOARD_INTERACTIVITY: u32 = 3;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

/// Double-buffered layer surface state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct State {
    pub layer: u32,
    pub anchor: u32,
    pub width: u32,
    pub height: u32,
    /// top, right, bottom, left
    pub margin: [i32; 4],
    pub zone: i32,
    pub keyboard: u32,
}

impl State {
    pub fn new(layer: u32) -> State {
        State {
            layer,
            anchor: 0,
            width: 0,
            height: 0,
            margin: [0; 4],
            zone: 0,
            keyboard: KEYBOARD_NONE,
        }
    }

    /// Checked at commit: a size of 0 needs both opposite anchors.
    pub fn validate(&self) -> Result<(), (u32, String)> {
        let lr = self.anchor & (ANCHOR_LEFT | ANCHOR_RIGHT) == ANCHOR_LEFT | ANCHOR_RIGHT;
        let tb = self.anchor & (ANCHOR_TOP | ANCHOR_BOTTOM) == ANCHOR_TOP | ANCHOR_BOTTOM;
        if self.width == 0 && !lr {
            return Err((
                err::INVALID_SIZE,
                "width 0 needs anchors to both the left and the right edge".into(),
            ));
        }
        if self.height == 0 && !tb {
            return Err((
                err::INVALID_SIZE,
                "height 0 needs anchors to both the top and the bottom edge".into(),
            ));
        }
        Ok(())
    }

    /// The edge a positive exclusive zone applies to, if any.
    pub fn exclusive_edge(&self) -> Option<u32> {
        if self.zone <= 0 {
            return None;
        }
        let horizontal = ANCHOR_LEFT | ANCHOR_RIGHT;
        let vertical = ANCHOR_TOP | ANCHOR_BOTTOM;
        match self.anchor {
            a if a == ANCHOR_TOP || a == ANCHOR_TOP | horizontal => Some(ANCHOR_TOP),
            a if a == ANCHOR_BOTTOM || a == ANCHOR_BOTTOM | horizontal => Some(ANCHOR_BOTTOM),
            a if a == ANCHOR_LEFT || a == ANCHOR_LEFT | vertical => Some(ANCHOR_LEFT),
            a if a == ANCHOR_RIGHT || a == ANCHOR_RIGHT | vertical => Some(ANCHOR_RIGHT),
            _ => None,
        }
    }
}

/// Valid anchor bitfield (4 bits).
pub fn valid_anchor(anchor: u32) -> bool {
    anchor <= 0xF
}

pub fn valid_layer(layer: u32) -> bool {
    layer <= OVERLAY
}

pub fn valid_keyboard(k: u32) -> bool {
    k <= KEYBOARD_ON_DEMAND
}

/// Places a surface with `s` inside `bounds`.
pub fn place(s: &State, bounds: Rect) -> Rect {
    let [mt, mr, mb, ml] = s.margin;
    let (l, r) = (s.anchor & ANCHOR_LEFT != 0, s.anchor & ANCHOR_RIGHT != 0);
    let (t, b) = (s.anchor & ANCHOR_TOP != 0, s.anchor & ANCHOR_BOTTOM != 0);
    let w = if s.width == 0 {
        (bounds.w - ml - mr).max(0)
    } else {
        s.width as i32
    };
    let h = if s.height == 0 {
        (bounds.h - mt - mb).max(0)
    } else {
        s.height as i32
    };
    let x = match (l, r) {
        (true, false) => bounds.x + ml,
        (false, true) => bounds.x + bounds.w - mr - w,
        (true, true) => bounds.x + ml + (bounds.w - ml - mr - w) / 2,
        (false, false) => bounds.x + (bounds.w - w) / 2,
    };
    let y = match (t, b) {
        (true, false) => bounds.y + mt,
        (false, true) => bounds.y + bounds.h - mb - h,
        (true, true) => bounds.y + mt + (bounds.h - mt - mb - h) / 2,
        (false, false) => bounds.y + (bounds.h - h) / 2,
    };
    Rect { x, y, w, h }
}

/// Arranges `surfaces` (state, in creation order) on `output`: returns one
/// rectangle per surface and the area left for windows.
pub fn arrange(output: Rect, surfaces: &[State]) -> (Vec<Rect>, Rect) {
    let mut usable = output;
    let mut rects = vec![
        Rect {
            x: 0,
            y: 0,
            w: 0,
            h: 0
        };
        surfaces.len()
    ];
    let mut order: Vec<usize> = (0..surfaces.len()).collect();
    // Overlay first, background last; creation order within a layer.
    order.sort_by_key(|&i| std::cmp::Reverse(surfaces[i].layer));
    for &i in &order {
        let s = &surfaces[i];
        let Some(edge) = s.exclusive_edge() else {
            continue;
        };
        let r = place(s, usable);
        rects[i] = r;
        let [mt, mr, mb, ml] = s.margin;
        match edge {
            ANCHOR_TOP => {
                let d = (s.zone + mt).min(usable.h);
                usable.y += d;
                usable.h -= d;
            }
            ANCHOR_BOTTOM => usable.h -= (s.zone + mb).min(usable.h),
            ANCHOR_LEFT => {
                let d = (s.zone + ml).min(usable.w);
                usable.x += d;
                usable.w -= d;
            }
            _ => usable.w -= (s.zone + mr).min(usable.w),
        }
    }
    for &i in &order {
        let s = &surfaces[i];
        if s.exclusive_edge().is_some() {
            continue;
        }
        rects[i] = place(s, if s.zone < 0 { output } else { usable });
    }
    (rects, usable)
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

    fn st(layer: u32, anchor: u32, w: u32, h: u32, zone: i32) -> State {
        State {
            anchor,
            width: w,
            height: h,
            zone,
            ..State::new(layer)
        }
    }

    const ALL: u32 = ANCHOR_TOP | ANCHOR_BOTTOM | ANCHOR_LEFT | ANCHOR_RIGHT;
    const TOP_BAR: u32 = ANCHOR_TOP | ANCHOR_LEFT | ANCHOR_RIGHT;

    #[test]
    fn size_zero_needs_opposite_anchors() {
        assert!(st(TOP, TOP_BAR, 0, 40, 40).validate().is_ok());
        let e = st(TOP, ANCHOR_TOP, 0, 40, 0).validate().unwrap_err();
        assert_eq!(e.0, err::INVALID_SIZE);
        assert!(st(TOP, ANCHOR_LEFT | ANCHOR_RIGHT, 100, 0, 0)
            .validate()
            .is_err());
        assert!(st(BACKGROUND, ALL, 0, 0, -1).validate().is_ok());
        assert!(valid_anchor(15) && !valid_anchor(16));
        assert!(valid_layer(3) && !valid_layer(4));
        assert!(valid_keyboard(2) && !valid_keyboard(3));
    }

    #[test]
    fn exclusive_edge_follows_the_protocol() {
        assert_eq!(
            st(TOP, TOP_BAR, 0, 40, 40).exclusive_edge(),
            Some(ANCHOR_TOP)
        );
        assert_eq!(
            st(TOP, ANCHOR_TOP, 100, 40, 40).exclusive_edge(),
            Some(ANCHOR_TOP)
        );
        assert_eq!(
            st(TOP, ANCHOR_LEFT | ANCHOR_TOP | ANCHOR_BOTTOM, 60, 0, 60).exclusive_edge(),
            Some(ANCHOR_LEFT)
        );
        // Corner, parallel edges, all edges, or no positive zone: none.
        assert_eq!(
            st(TOP, ANCHOR_TOP | ANCHOR_LEFT, 10, 10, 5).exclusive_edge(),
            None
        );
        assert_eq!(
            st(TOP, ANCHOR_LEFT | ANCHOR_RIGHT, 0, 10, 5).exclusive_edge(),
            None
        );
        assert_eq!(st(TOP, ALL, 0, 0, 5).exclusive_edge(), None);
        assert_eq!(st(TOP, TOP_BAR, 0, 40, 0).exclusive_edge(), None);
    }

    #[test]
    fn bar_reserves_its_zone_and_background_fills_the_output() {
        let bg = st(BACKGROUND, ALL, 0, 0, -1);
        let bar = st(TOP, TOP_BAR, 0, 40, 40);
        let (r, usable) = arrange(OUT, &[bg, bar]);
        assert_eq!(r[0], OUT, "zone -1: the whole output");
        assert_eq!(
            r[1],
            Rect {
                x: 0,
                y: 0,
                w: 1280,
                h: 40
            }
        );
        assert_eq!(
            usable,
            Rect {
                x: 0,
                y: 40,
                w: 1280,
                h: 760
            }
        );
    }

    #[test]
    fn zones_stack_and_include_margins() {
        let mut top = st(TOP, TOP_BAR, 0, 30, 30);
        top.margin = [4, 0, 0, 0];
        let dock = st(BOTTOM, ANCHOR_BOTTOM, 600, 64, 64);
        let side = st(TOP, ANCHOR_LEFT | ANCHOR_TOP | ANCHOR_BOTTOM, 50, 0, 50);
        let (r, usable) = arrange(OUT, &[top, dock, side]);
        assert_eq!(r[0].y, 4, "margin from the edge");
        // The side panel is placed after the top bar (same layer, later),
        // so it starts below the bar's zone (30 + 4).
        assert_eq!(
            r[2],
            Rect {
                x: 0,
                y: 34,
                w: 50,
                h: 766
            }
        );
        // The dock (bottom layer) comes last, against what is left.
        assert_eq!(
            r[1],
            Rect {
                x: 50 + (1230 - 600) / 2,
                y: 800 - 64,
                w: 600,
                h: 64
            }
        );
        assert_eq!(
            usable,
            Rect {
                x: 50,
                y: 34,
                w: 1230,
                h: 800 - 34 - 64
            }
        );
    }

    #[test]
    fn zone_zero_moves_out_of_reserved_areas() {
        let bar = st(TOP, TOP_BAR, 0, 40, 40);
        // A notification anchored top-right with zone 0: below the bar.
        let mut note = st(OVERLAY, ANCHOR_TOP | ANCHOR_RIGHT, 300, 80, 0);
        note.margin = [8, 8, 0, 0];
        let (r, _) = arrange(OUT, &[bar, note]);
        assert_eq!(
            r[1],
            Rect {
                x: 1280 - 8 - 300,
                y: 40 + 8,
                w: 300,
                h: 80
            }
        );
    }

    #[test]
    fn unanchored_surfaces_are_centered() {
        let launcher = st(OVERLAY, 0, 400, 300, 0);
        let (r, _) = arrange(OUT, &[launcher]);
        assert_eq!(
            r[0],
            Rect {
                x: 440,
                y: 250,
                w: 400,
                h: 300
            }
        );
    }
}
