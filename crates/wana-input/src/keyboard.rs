//! Keyboard layouts with xkbcommon: evdev key codes from libinput become
//! keysyms and text for the active layout.
//!
//! The keymap is compiled from the xkeyboard-config data
//! (`/usr/share/X11/xkb`) with the RMLVO names `evdev` / `pc105` / layout.

use crate::ffi;
use std::ffi::{CStr, CString};
use std::os::raw::c_char;
use std::ptr::NonNull;

/// xkb key codes are evdev codes + 8 (the X11 legacy offset).
pub const EVDEV_OFFSET: u32 = 8;

/// A compiled keymap and its modifier/layout state.
#[derive(Debug)]
pub struct Keyboard {
    ctx: NonNull<ffi::xkb_context>,
    keymap: NonNull<ffi::xkb_keymap>,
    state: NonNull<ffi::xkb_state>,
}

/// What one key event means under the current layout and modifiers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyInfo {
    /// Keysym name, e.g. `a`, `A`, `Shift_L`, `Return`.
    pub keysym: String,
    /// Text the key produces (empty for modifiers and function keys).
    pub text: String,
}

impl Keyboard {
    /// Compiles the keymap for `layout` (e.g. `us`, `de`, `ara`).
    pub fn new(layout: &str) -> Result<Keyboard, String> {
        let layout_c = CString::new(layout).map_err(|e| format!("layout {layout:?}: {e}"))?;
        // SAFETY: no arguments besides a flags value.
        let ctx = NonNull::new(unsafe { ffi::xkb_context_new(ffi::XKB_CONTEXT_NO_FLAGS) }).ok_or(
            "xkb_context_new failed (is xkeyboard-config installed in /usr/share/X11/xkb?)",
        )?;
        let names = ffi::xkb_rule_names {
            rules: c"evdev".as_ptr(),
            model: c"pc105".as_ptr(),
            layout: layout_c.as_ptr(),
            variant: std::ptr::null(),
            options: std::ptr::null(),
        };
        // SAFETY: ctx is valid; `names` and the strings it points to outlive
        // the call (xkbcommon copies what it keeps).
        let keymap = unsafe {
            ffi::xkb_keymap_new_from_names(ctx.as_ptr(), &names, ffi::XKB_KEYMAP_COMPILE_NO_FLAGS)
        };
        let Some(keymap) = NonNull::new(keymap) else {
            // SAFETY: ctx is valid and not used afterwards.
            unsafe { ffi::xkb_context_unref(ctx.as_ptr()) };
            return Err(format!(
                "cannot compile keymap (rules evdev, model pc105, layout {layout}); \
                 is xkeyboard-config installed in /usr/share/X11/xkb?"
            ));
        };
        // SAFETY: keymap is valid.
        let Some(state) = NonNull::new(unsafe { ffi::xkb_state_new(keymap.as_ptr()) }) else {
            // SAFETY: both valid and not used afterwards.
            unsafe {
                ffi::xkb_keymap_unref(keymap.as_ptr());
                ffi::xkb_context_unref(ctx.as_ptr());
            }
            return Err("xkb_state_new failed".into());
        };
        Ok(Keyboard { ctx, keymap, state })
    }

    /// Descriptive name of the first layout, e.g. `English (US)`.
    pub fn layout_name(&self) -> String {
        // SAFETY: keymap is valid; the returned string (or NULL) is owned by it.
        unsafe {
            cstr_or(
                ffi::xkb_keymap_layout_get_name(self.keymap.as_ptr(), 0),
                "?",
            )
        }
    }

    /// Feeds one key event (evdev code) and returns what it means. The
    /// meaning is taken before the state update, so a key reads with the
    /// modifiers that were active when it went down.
    pub fn key(&mut self, evdev_code: u32, pressed: bool) -> KeyInfo {
        let key = evdev_code + EVDEV_OFFSET;
        let state = self.state.as_ptr();
        let mut buf = [0 as c_char; 64];
        // SAFETY: state is valid; buffers are writable with the given sizes and
        // xkbcommon NUL-terminates what it writes (truncating if needed).
        let info = unsafe {
            let sym = ffi::xkb_state_key_get_one_sym(state, key);
            ffi::xkb_keysym_get_name(sym, buf.as_mut_ptr(), buf.len());
            let keysym = cstr_or(buf.as_ptr(), "");
            buf[0] = 0;
            ffi::xkb_state_key_get_utf8(state, key, buf.as_mut_ptr(), buf.len());
            let text = cstr_or(buf.as_ptr(), "");
            KeyInfo { keysym, text }
        };
        let direction = if pressed {
            ffi::XKB_KEY_DOWN
        } else {
            ffi::XKB_KEY_UP
        };
        // SAFETY: state is valid.
        unsafe { ffi::xkb_state_update_key(state, key, direction) };
        info
    }
}

impl Drop for Keyboard {
    fn drop(&mut self) {
        // SAFETY: each object is valid and released exactly once, state first.
        unsafe {
            ffi::xkb_state_unref(self.state.as_ptr());
            ffi::xkb_keymap_unref(self.keymap.as_ptr());
            ffi::xkb_context_unref(self.ctx.as_ptr());
        }
    }
}

/// Copies a C string, or returns `fallback` for NULL.
///
/// # Safety
/// `p` is NULL or points to a NUL-terminated string.
pub(crate) unsafe fn cstr_or(p: *const c_char, fallback: &str) -> String {
    if p.is_null() {
        fallback.to_owned()
    } else {
        // SAFETY: non-NULL and NUL-terminated per the caller's contract.
        unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const KEY_A: u32 = 30;
    const KEY_W: u32 = 17;
    const KEY_LEFTSHIFT: u32 = 42;
    const KEY_ENTER: u32 = 28;

    /// Needs the xkeyboard-config data, like the target image does.
    fn us() -> Keyboard {
        Keyboard::new("us").expect("xkeyboard-config (us layout) must be installed")
    }

    #[test]
    fn us_layout_letters_and_shift() {
        let mut kb = us();
        assert!(kb.layout_name().contains("English"), "{}", kb.layout_name());
        let a = kb.key(KEY_A, true);
        assert_eq!((a.keysym.as_str(), a.text.as_str()), ("a", "a"));
        kb.key(KEY_A, false);

        let shift = kb.key(KEY_LEFTSHIFT, true);
        assert_eq!(
            (shift.keysym.as_str(), shift.text.as_str()),
            ("Shift_L", "")
        );
        let w = kb.key(KEY_W, true);
        assert_eq!((w.keysym.as_str(), w.text.as_str()), ("W", "W"));
        kb.key(KEY_W, false);
        kb.key(KEY_LEFTSHIFT, false);

        let w = kb.key(KEY_W, true);
        assert_eq!(w.text, "w", "shift released");
        assert_eq!(kb.key(KEY_ENTER, true).keysym, "Return");
    }

    #[test]
    fn other_layouts_change_the_meaning() {
        let mut kb = Keyboard::new("fr").expect("fr layout");
        // AZERTY: the key at the QWERTY 'a' position types 'q'.
        assert_eq!(kb.key(KEY_A, true).text, "q");
    }

    #[test]
    fn unknown_layout_is_an_error() {
        let err = Keyboard::new("no-such-layout").unwrap_err();
        assert!(err.contains("no-such-layout"), "{err}");
    }
}
