//! Human-readable names for log lines, and small pure helpers that are
//! unit-tested without devices.

/// evdev button codes (linux/input-event-codes.h).
pub const BTN_LEFT: u32 = 0x110;
pub const BTN_RIGHT: u32 = 0x111;
pub const BTN_MIDDLE: u32 = 0x112;
pub const BTN_SIDE: u32 = 0x113;
pub const BTN_EXTRA: u32 = 0x114;

/// Name of a pointer button code, e.g. `BTN_LEFT`, or its hex value.
pub fn button_name(code: u32) -> String {
    match code {
        BTN_LEFT => "BTN_LEFT".into(),
        BTN_RIGHT => "BTN_RIGHT".into(),
        BTN_MIDDLE => "BTN_MIDDLE".into(),
        BTN_SIDE => "BTN_SIDE".into(),
        BTN_EXTRA => "BTN_EXTRA".into(),
        other => format!("button 0x{other:x}"),
    }
}

/// Text typed so far, with the target text a test waits for.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Typed {
    text: String,
}

impl Typed {
    /// Appends the text produced by one key press (may be empty, e.g. Shift).
    pub fn push(&mut self, s: &str) {
        self.text.push_str(s);
    }

    pub fn as_str(&self) -> &str {
        &self.text
    }

    /// True once `expected` has been typed (as the latest input).
    pub fn ends_with(&self, expected: &str) -> bool {
        !expected.is_empty() && self.text.ends_with(expected)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn buttons_have_names() {
        assert_eq!(button_name(0x110), "BTN_LEFT");
        assert_eq!(button_name(0x111), "BTN_RIGHT");
        assert_eq!(button_name(0x2ff), "button 0x2ff");
    }

    #[test]
    fn typed_text_matches_the_latest_input() {
        let mut t = Typed::default();
        assert!(!t.ends_with("wana"));
        for s in ["x", "w", "", "a", "n", "a"] {
            t.push(s);
        }
        assert_eq!(t.as_str(), "xwana");
        assert!(t.ends_with("wana"));
        assert!(!t.ends_with(""), "an empty target never matches");
        t.push(" ");
        assert!(!t.ends_with("wana"));
    }
}
