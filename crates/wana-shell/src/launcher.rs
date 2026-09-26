//! The launcher's keyboard handling, as a pure state machine (the keys are
//! evdev codes, as wl_keyboard sends them: navigation does not depend on
//! the layout).

pub const KEY_ESC: u32 = 1;
pub const KEY_ENTER: u32 = 28;
pub const KEY_KPENTER: u32 = 96;
pub const KEY_HOME: u32 = 102;
pub const KEY_UP: u32 = 103;
pub const KEY_END: u32 = 107;
pub const KEY_DOWN: u32 = 108;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    /// Nothing changes.
    None,
    /// The selection moved: redraw.
    Moved,
    /// Start this app and close.
    Launch(usize),
    Close,
}

/// Which of `len` rows is selected.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Menu {
    pub len: usize,
    pub selected: usize,
}

impl Menu {
    pub fn new(len: usize) -> Menu {
        Menu { len, selected: 0 }
    }

    /// A key press (no wrap-around at the ends).
    pub fn key(&mut self, code: u32) -> Action {
        let last = self.len.saturating_sub(1);
        let to = match code {
            KEY_ESC => return Action::Close,
            KEY_ENTER | KEY_KPENTER if self.len > 0 => return Action::Launch(self.selected),
            KEY_UP => self.selected.saturating_sub(1),
            KEY_DOWN => (self.selected + 1).min(last),
            KEY_HOME => 0,
            KEY_END => last,
            _ => return Action::None,
        };
        if to == self.selected {
            return Action::None;
        }
        self.selected = to;
        Action::Moved
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arrows_move_within_the_list_and_enter_launches() {
        let mut m = Menu::new(3);
        assert_eq!(m.key(KEY_UP), Action::None, "already at the top");
        assert_eq!(m.key(KEY_DOWN), Action::Moved);
        assert_eq!(m.key(KEY_END), Action::Moved);
        assert_eq!(m.selected, 2);
        assert_eq!(m.key(KEY_DOWN), Action::None, "already at the bottom");
        assert_eq!(m.key(KEY_ENTER), Action::Launch(2));
        assert_eq!(m.key(KEY_HOME), Action::Moved);
        assert_eq!(m.key(KEY_KPENTER), Action::Launch(0));
        assert_eq!(m.key(30), Action::None, "letters do nothing yet");
        assert_eq!(m.key(KEY_ESC), Action::Close);
    }

    #[test]
    fn an_empty_list_only_closes() {
        let mut m = Menu::new(0);
        assert_eq!(m.key(KEY_ENTER), Action::None);
        assert_eq!(m.key(KEY_DOWN), Action::None);
        assert_eq!(m.key(KEY_ESC), Action::Close);
    }
}
