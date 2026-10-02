//! Shared Wana OS visual design tokens.
//!
//! Keeping the palette here prevents native applications from drifting into
//! slightly different versions of the same desktop identity.

pub mod color {
    pub const BG_DARK: u32 = 0x111827;
    pub const BG_LIGHT: u32 = 0xF4F6FA;
    pub const CARD_DARK: u32 = 0x1E293B;
    pub const CARD_LIGHT: u32 = 0xFFFFFF;
    pub const TEXT_DARK: u32 = 0xF1F5F9;
    pub const TEXT_LIGHT: u32 = 0x172033;
    pub const DIM_DARK: u32 = 0xA8B3C7;
    pub const DIM_LIGHT: u32 = 0x5C667A;
    pub const ACCENT_BLUE: u32 = 0x4F8CFF;
    pub const ACCENT_TEAL: u32 = 0x2DD4BF;
    pub const ACCENT_VIOLET: u32 = 0x8B5CF6;
    pub const INFO_BLUE: u32 = 0x60A5FA;
    pub const DANGER: u32 = 0xF87171;
    pub const DANGER_STRONG: u32 = 0xB94A55;

    pub const DESKTOP_TOP: u32 = 0x16213E;
    pub const DESKTOP_BOTTOM: u32 = 0x1B3A5C;
    pub const SHELL_BAR: u32 = 0x0B0F1A;
    pub const SHELL_BAR_TEXT: u32 = 0xE8ECF4;
    pub const SHELL_PANEL: u32 = 0x1E2638;
    pub const SHELL_BORDER: u32 = 0x3A4560;
    pub const SHELL_DIM: u32 = 0x9AA4B8;
    pub const DOCK_BG: u32 = 0x121827;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Palette {
    pub bg: u32,
    pub card: u32,
    pub text: u32,
    pub dim: u32,
    pub accent: u32,
    pub info: u32,
    pub danger: u32,
    pub danger_strong: u32,
    pub desktop_top: u32,
    pub desktop_bottom: u32,
    pub shell_bar: u32,
    pub shell_bar_text: u32,
    pub shell_panel: u32,
    pub shell_border: u32,
    pub shell_dim: u32,
    pub dock_bg: u32,
}

pub fn palette(theme: &str, accent: &str) -> Palette {
    let accent = match accent {
        "teal" => color::ACCENT_TEAL,
        "violet" => color::ACCENT_VIOLET,
        _ => color::ACCENT_BLUE,
    };
    if theme == "light" {
        Palette {
            bg: color::BG_LIGHT,
            card: color::CARD_LIGHT,
            text: color::TEXT_LIGHT,
            dim: color::DIM_LIGHT,
            accent,
            info: color::INFO_BLUE,
            danger: color::DANGER,
            danger_strong: color::DANGER_STRONG,
            desktop_top: 0xE7EEF9,
            desktop_bottom: 0xDCE8F8,
            shell_bar: 0xFFFFFF,
            shell_bar_text: color::TEXT_LIGHT,
            shell_panel: 0xFFFFFF,
            shell_border: 0xCBD5E1,
            shell_dim: 0x64748B,
            dock_bg: 0xEEF2F7,
        }
    } else {
        Palette {
            bg: color::BG_DARK,
            card: color::CARD_DARK,
            text: color::TEXT_DARK,
            dim: color::DIM_DARK,
            accent,
            info: color::INFO_BLUE,
            danger: color::DANGER,
            danger_strong: color::DANGER_STRONG,
            desktop_top: color::DESKTOP_TOP,
            desktop_bottom: color::DESKTOP_BOTTOM,
            shell_bar: color::SHELL_BAR,
            shell_bar_text: color::SHELL_BAR_TEXT,
            shell_panel: color::SHELL_PANEL,
            shell_border: color::SHELL_BORDER,
            shell_dim: color::SHELL_DIM,
            dock_bg: color::DOCK_BG,
        }
    }
}

pub fn current() -> Palette {
    let theme = std::env::var("WANA_THEME").unwrap_or_else(|_| "dark".into());
    let accent = std::env::var("WANA_ACCENT").unwrap_or_else(|_| "blue".into());
    palette(&theme, &accent)
}

pub mod spacing {
    pub const COMPACT: u32 = 8;
    pub const SMALL: u32 = 16;
    pub const CONTENT: u32 = 28;
    pub const LARGE: u32 = 32;
}

#[cfg(test)]
mod tests {
    use super::color;

    #[test]
    fn dark_and_light_tokens_are_distinct() {
        assert_ne!(color::BG_DARK, color::BG_LIGHT);
        assert_ne!(color::TEXT_DARK, color::TEXT_LIGHT);
        assert_ne!(color::CARD_DARK, color::CARD_LIGHT);
        assert_ne!(super::palette("dark", "blue"), super::palette("light", "blue"));
        assert_ne!(
            super::palette("dark", "blue").accent,
            super::palette("dark", "violet").accent
        );
    }
}
