//! Deterministic Wana OS motion primitives.
//!
//! Progress is fixed-point (0..=1024), so animations produce identical
//! intermediate values on the host and in the Buildroot target.

pub const SCALE: u32 = 1024;

pub mod duration {
    pub const QUICK_MS: u32 = 120;
    pub const STANDARD_MS: u32 = 180;
    pub const EMPHASIZED_MS: u32 = 240;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Curve {
    Linear,
    EaseOutCubic,
    EaseInOutCubic,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Motion {
    pub duration_ms: u32,
    pub frames: u32,
    pub curve: Curve,
}

impl Motion {
    pub const fn new(duration_ms: u32, frames: u32, curve: Curve) -> Self {
        Self {
            duration_ms,
            frames,
            curve,
        }
    }

    pub fn progress(self, frame: u32) -> u32 {
        ease(self.curve, frame.min(self.frames), self.frames)
    }

    pub fn frame_ms(self) -> u32 {
        if self.frames == 0 {
            self.duration_ms
        } else {
            (self.duration_ms / self.frames).max(1)
        }
    }
}

pub fn ease(curve: Curve, step: u32, steps: u32) -> u32 {
    if steps == 0 {
        return SCALE;
    }
    let x = u64::from(step.min(steps)) * u64::from(SCALE) / u64::from(steps);
    match curve {
        Curve::Linear => x as u32,
        Curve::EaseOutCubic => {
            let inv = u64::from(SCALE) - x;
            (u64::from(SCALE) - inv * inv * inv / u64::from(SCALE * SCALE)) as u32
        }
        Curve::EaseInOutCubic => {
            if x * 2 <= u64::from(SCALE) {
                (4 * x * x * x / u64::from(SCALE * SCALE)) as u32
            } else {
                let inv = u64::from(SCALE) - x;
                (u64::from(SCALE) - 4 * inv * inv * inv / u64::from(SCALE * SCALE)) as u32
            }
        }
    }
}

pub fn lerp_i32(from: i32, to: i32, progress: u32) -> i32 {
    let p = i64::from(progress.min(SCALE));
    let scale = i64::from(SCALE);
    let from = i64::from(from);
    let to = i64::from(to);
    (from + (to - from) * p / scale) as i32
}

pub fn mix_rgb(from: u32, to: u32, progress: u32) -> u32 {
    let p = progress.min(SCALE);
    let channel = |shift: u32| {
        let a = ((from >> shift) & 0xff) as i32;
        let b = ((to >> shift) & 0xff) as i32;
        (lerp_i32(a, b, p) as u32) << shift
    };
    channel(16) | channel(8) | channel(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn curves_are_bounded_monotonic_and_hit_endpoints() {
        for curve in [Curve::Linear, Curve::EaseOutCubic, Curve::EaseInOutCubic] {
            let values: Vec<u32> = (0..=12).map(|step| ease(curve, step, 12)).collect();
            assert_eq!(values[0], 0);
            assert_eq!(*values.last().unwrap(), SCALE);
            assert!(values.windows(2).all(|pair| pair[0] <= pair[1]));
            assert!(values.iter().all(|value| *value <= SCALE));
        }
    }

    #[test]
    fn ease_out_moves_faster_than_linear_early() {
        assert!(ease(Curve::EaseOutCubic, 1, 4) > ease(Curve::Linear, 1, 4));
        assert_eq!(ease(Curve::EaseOutCubic, 4, 4), SCALE);
    }

    #[test]
    fn interpolation_is_deterministic() {
        assert_eq!(lerp_i32(0, 100, SCALE / 2), 50);
        assert_eq!(lerp_i32(100, 0, SCALE / 2), 50);
        assert_eq!(mix_rgb(0x000000, 0xffffff, SCALE / 2), 0x7f7f7f);
        assert_eq!(mix_rgb(0x112233, 0xaabbcc, 0), 0x112233);
        assert_eq!(mix_rgb(0x112233, 0xaabbcc, SCALE), 0xaabbcc);
    }

    #[test]
    fn zero_frame_motion_finishes_immediately() {
        let motion = Motion::new(duration::QUICK_MS, 0, Curve::EaseOutCubic);
        assert_eq!(motion.progress(0), SCALE);
        assert_eq!(motion.frame_ms(), duration::QUICK_MS);
    }
}
