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
    }
}
