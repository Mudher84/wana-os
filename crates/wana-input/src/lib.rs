//! Wana OS input (Phase 9): input devices and events from libinput (on top
//! of udev and evdev), and keyboard layouts from xkbcommon.
//!
//! Stack: kernel evdev (`/dev/input/event*`) -> udev (device discovery and
//! classification, `ID_INPUT_*`) -> libinput (device handling, pointer
//! acceleration, scroll, buttons) -> xkbcommon (keycode -> keysym/text for
//! the active layout) -> Wana (compositor, Phase 10).

pub mod ffi;
pub mod keyboard;
pub mod libinput;
pub mod names;
