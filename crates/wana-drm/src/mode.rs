//! Display modes and mode selection.

use crate::sys::{DrmModeModeInfo, DRM_MODE_TYPE_PREFERRED};

/// A display mode as reported by a connector.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Mode {
    pub raw: DrmModeModeInfo,
}

impl Mode {
    pub fn width(&self) -> u32 {
        self.raw.hdisplay.into()
    }

    pub fn height(&self) -> u32 {
        self.raw.vdisplay.into()
    }

    pub fn is_preferred(&self) -> bool {
        self.raw.type_ & DRM_MODE_TYPE_PREFERRED != 0
    }

    /// Refresh rate in millihertz, computed from the pixel clock so that
    /// 59.94 Hz and 60 Hz modes are distinguished. Falls back to the
    /// integer `vrefresh` if the timings are incomplete.
    pub fn refresh_mhz(&self) -> u32 {
        let total = u64::from(self.raw.htotal) * u64::from(self.raw.vtotal);
        if total == 0 || self.raw.clock == 0 {
            return self.raw.vrefresh * 1000;
        }
        // clock is in kHz: Hz = clock*1000 / total, mHz = clock*1e6 / total.
        ((u64::from(self.raw.clock) * 1_000_000 + total / 2) / total) as u32
    }

    pub fn name(&self) -> String {
        let end = self
            .raw
            .name
            .iter()
            .position(|&b| b == 0)
            .unwrap_or(self.raw.name.len());
        String::from_utf8_lossy(&self.raw.name[..end]).into_owned()
    }
}

impl std::fmt::Display for Mode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mhz = self.refresh_mhz();
        write!(
            f,
            "{}x{}@{}.{:02}",
            self.width(),
            self.height(),
            mhz / 1000,
            (mhz % 1000) / 10
        )?;
        if self.is_preferred() {
            f.write_str(" (preferred)")?;
        }
        Ok(())
    }
}

/// Picks the mode to use: the connector's preferred mode, and among modes
/// with the preferred resolution the highest refresh rate (so a 120 Hz
/// panel is driven at 120 Hz). Without a preferred mode: the largest
/// resolution, then the highest refresh rate.
pub fn select(modes: &[Mode]) -> Option<Mode> {
    let preferred = modes.iter().find(|m| m.is_preferred());
    let (w, h) = match preferred {
        Some(p) => (p.width(), p.height()),
        None => {
            let best = modes
                .iter()
                .max_by_key(|m| u64::from(m.width()) * u64::from(m.height()))?;
            (best.width(), best.height())
        }
    };
    modes
        .iter()
        .filter(|m| m.width() == w && m.height() == h)
        .max_by_key(|m| (m.refresh_mhz(), m.is_preferred()))
        .copied()
}

#[cfg(test)]
pub(crate) fn test_mode(w: u16, h: u16, hz: u32, preferred: bool) -> Mode {
    let mut raw = DrmModeModeInfo {
        hdisplay: w,
        vdisplay: h,
        htotal: w + 160,
        vtotal: h + 40,
        vrefresh: hz,
        ..Default::default()
    };
    raw.clock = ((u64::from(raw.htotal) * u64::from(raw.vtotal) * u64::from(hz)) / 1000) as u32;
    if preferred {
        raw.type_ |= DRM_MODE_TYPE_PREFERRED;
    }
    let name = format!("{w}x{h}");
    raw.name[..name.len()].copy_from_slice(name.as_bytes());
    Mode { raw }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prefers_preferred_resolution_at_highest_refresh() {
        let modes = [
            test_mode(3840, 2160, 30, false),
            test_mode(1920, 1080, 60, true),
            test_mode(1920, 1080, 120, false),
            test_mode(1280, 720, 144, false),
        ];
        let m = select(&modes).unwrap();
        assert_eq!(
            (m.width(), m.height(), m.refresh_mhz() / 1000),
            (1920, 1080, 120)
        );
    }

    #[test]
    fn without_preferred_takes_largest_then_fastest() {
        let modes = [
            test_mode(1280, 720, 60, false),
            test_mode(2560, 1440, 60, false),
            test_mode(2560, 1440, 75, false),
        ];
        let m = select(&modes).unwrap();
        assert_eq!(
            (m.width(), m.height(), m.refresh_mhz() / 1000),
            (2560, 1440, 75)
        );
    }

    #[test]
    fn empty_list_selects_nothing() {
        assert!(select(&[]).is_none());
    }

    #[test]
    fn refresh_distinguishes_59_94_from_60() {
        // CEA 1080p59.94: clock 148352 kHz, 2200 x 1125 total.
        let mut raw = DrmModeModeInfo {
            clock: 148_352,
            htotal: 2200,
            vtotal: 1125,
            vrefresh: 60,
            ..Default::default()
        };
        assert_eq!(Mode { raw }.refresh_mhz(), 59_940);
        raw.clock = 148_500;
        assert_eq!(Mode { raw }.refresh_mhz(), 60_000);
    }

    #[test]
    fn display_and_name() {
        let m = test_mode(1280, 800, 60, true);
        assert_eq!(m.name(), "1280x800");
        assert_eq!(m.to_string(), "1280x800@60.00 (preferred)");
    }
}
