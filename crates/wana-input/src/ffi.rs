//! Minimal FFI declarations for libudev, libinput and libxkbcommon.
//! Constants are the values from libinput.h and xkbcommon/xkbcommon.h; only
//! what `wana-input` uses is declared, and only API present in both the
//! Buildroot (libinput 1.31, xkbcommon 1.9) and older host versions
//! (libinput 1.25, xkbcommon 1.6).

#![allow(non_camel_case_types)]

use std::os::raw::{c_char, c_int, c_void};

// --- libudev --------------------------------------------------------------
#[derive(Debug)]
pub enum udev {}

#[link(name = "udev")]
extern "C" {
    pub fn udev_new() -> *mut udev;
    pub fn udev_unref(udev: *mut udev) -> *mut udev;
}

// --- libinput -------------------------------------------------------------
#[derive(Debug)]
pub enum libinput {}
#[derive(Debug)]
pub enum libinput_event {}
#[derive(Debug)]
pub enum libinput_device {}
#[derive(Debug)]
pub enum libinput_event_keyboard {}
#[derive(Debug)]
pub enum libinput_event_pointer {}

/// How libinput opens device nodes: the caller decides (a compositor would
/// go through a seat manager; Wana's bring-up tool runs as root).
#[repr(C)]
#[derive(Debug)]
pub struct libinput_interface {
    pub open_restricted:
        unsafe extern "C" fn(path: *const c_char, flags: c_int, user_data: *mut c_void) -> c_int,
    pub close_restricted: unsafe extern "C" fn(fd: c_int, user_data: *mut c_void),
}

pub const LIBINPUT_LOG_PRIORITY_ERROR: c_int = 30;

pub const LIBINPUT_DEVICE_CAP_KEYBOARD: c_int = 0;
pub const LIBINPUT_DEVICE_CAP_POINTER: c_int = 1;
pub const LIBINPUT_DEVICE_CAP_TOUCH: c_int = 2;
pub const LIBINPUT_DEVICE_CAP_TABLET_TOOL: c_int = 3;
pub const LIBINPUT_DEVICE_CAP_TABLET_PAD: c_int = 4;
pub const LIBINPUT_DEVICE_CAP_GESTURE: c_int = 5;
pub const LIBINPUT_DEVICE_CAP_SWITCH: c_int = 6;

pub const LIBINPUT_EVENT_DEVICE_ADDED: c_int = 1;
pub const LIBINPUT_EVENT_DEVICE_REMOVED: c_int = 2;
pub const LIBINPUT_EVENT_KEYBOARD_KEY: c_int = 300;
pub const LIBINPUT_EVENT_POINTER_MOTION: c_int = 400;
pub const LIBINPUT_EVENT_POINTER_MOTION_ABSOLUTE: c_int = 401;
pub const LIBINPUT_EVENT_POINTER_BUTTON: c_int = 402;
pub const LIBINPUT_EVENT_POINTER_SCROLL_WHEEL: c_int = 404;

pub const LIBINPUT_KEY_STATE_PRESSED: c_int = 1;
pub const LIBINPUT_BUTTON_STATE_PRESSED: c_int = 1;
pub const LIBINPUT_POINTER_AXIS_SCROLL_VERTICAL: c_int = 0;
pub const LIBINPUT_POINTER_AXIS_SCROLL_HORIZONTAL: c_int = 1;

#[link(name = "input")]
extern "C" {
    pub fn libinput_udev_create_context(
        interface: *const libinput_interface,
        user_data: *mut c_void,
        udev: *mut udev,
    ) -> *mut libinput;
    pub fn libinput_udev_assign_seat(li: *mut libinput, seat_id: *const c_char) -> c_int;
    pub fn libinput_unref(li: *mut libinput) -> *mut libinput;
    pub fn libinput_log_set_priority(li: *mut libinput, priority: c_int);
    pub fn libinput_get_fd(li: *mut libinput) -> c_int;
    pub fn libinput_dispatch(li: *mut libinput) -> c_int;
    pub fn libinput_get_event(li: *mut libinput) -> *mut libinput_event;

    pub fn libinput_event_get_type(ev: *mut libinput_event) -> c_int;
    pub fn libinput_event_destroy(ev: *mut libinput_event);
    pub fn libinput_event_get_device(ev: *mut libinput_event) -> *mut libinput_device;
    pub fn libinput_device_get_name(dev: *mut libinput_device) -> *const c_char;
    pub fn libinput_device_get_sysname(dev: *mut libinput_device) -> *const c_char;
    pub fn libinput_device_has_capability(dev: *mut libinput_device, cap: c_int) -> c_int;

    pub fn libinput_event_get_keyboard_event(
        ev: *mut libinput_event,
    ) -> *mut libinput_event_keyboard;
    pub fn libinput_event_keyboard_get_key(ev: *mut libinput_event_keyboard) -> u32;
    pub fn libinput_event_keyboard_get_key_state(ev: *mut libinput_event_keyboard) -> c_int;

    pub fn libinput_event_get_pointer_event(ev: *mut libinput_event)
        -> *mut libinput_event_pointer;
    pub fn libinput_event_pointer_get_dx(ev: *mut libinput_event_pointer) -> f64;
    pub fn libinput_event_pointer_get_dy(ev: *mut libinput_event_pointer) -> f64;
    pub fn libinput_event_pointer_get_absolute_x_transformed(
        ev: *mut libinput_event_pointer,
        width: u32,
    ) -> f64;
    pub fn libinput_event_pointer_get_absolute_y_transformed(
        ev: *mut libinput_event_pointer,
        height: u32,
    ) -> f64;
    pub fn libinput_event_pointer_get_button(ev: *mut libinput_event_pointer) -> u32;
    pub fn libinput_event_pointer_get_button_state(ev: *mut libinput_event_pointer) -> c_int;
    pub fn libinput_event_pointer_has_axis(ev: *mut libinput_event_pointer, axis: c_int) -> c_int;
    pub fn libinput_event_pointer_get_scroll_value_v120(
        ev: *mut libinput_event_pointer,
        axis: c_int,
    ) -> f64;
}

// --- xkbcommon ------------------------------------------------------------
#[derive(Debug)]
pub enum xkb_context {}
#[derive(Debug)]
pub enum xkb_keymap {}
#[derive(Debug)]
pub enum xkb_state {}

#[repr(C)]
#[derive(Debug)]
pub struct xkb_rule_names {
    pub rules: *const c_char,
    pub model: *const c_char,
    pub layout: *const c_char,
    pub variant: *const c_char,
    pub options: *const c_char,
}

pub const XKB_CONTEXT_NO_FLAGS: c_int = 0;
pub const XKB_KEYMAP_COMPILE_NO_FLAGS: c_int = 0;
pub const XKB_KEY_UP: c_int = 0;
pub const XKB_KEY_DOWN: c_int = 1;
pub const XKB_KEYMAP_FORMAT_TEXT_V1: c_int = 1;
pub const XKB_KEYMAP_USE_ORIGINAL_FORMAT: c_int = -1;
pub const XKB_STATE_MODS_DEPRESSED: c_int = 1 << 0;
pub const XKB_STATE_MODS_LATCHED: c_int = 1 << 1;
pub const XKB_STATE_MODS_LOCKED: c_int = 1 << 2;
pub const XKB_STATE_MODS_EFFECTIVE: c_int = 1 << 3;
pub const XKB_STATE_LAYOUT_EFFECTIVE: c_int = 1 << 7;

#[link(name = "xkbcommon")]
extern "C" {
    pub fn xkb_context_new(flags: c_int) -> *mut xkb_context;
    pub fn xkb_context_unref(ctx: *mut xkb_context);
    pub fn xkb_keymap_new_from_names(
        ctx: *mut xkb_context,
        names: *const xkb_rule_names,
        flags: c_int,
    ) -> *mut xkb_keymap;
    pub fn xkb_keymap_new_from_string(
        ctx: *mut xkb_context,
        string: *const c_char,
        format: c_int,
        flags: c_int,
    ) -> *mut xkb_keymap;
    pub fn xkb_keymap_get_as_string(keymap: *mut xkb_keymap, format: c_int) -> *mut c_char;
    pub fn xkb_keymap_unref(keymap: *mut xkb_keymap);
    pub fn xkb_keymap_layout_get_name(keymap: *mut xkb_keymap, idx: u32) -> *const c_char;
    pub fn xkb_state_new(keymap: *mut xkb_keymap) -> *mut xkb_state;
    pub fn xkb_state_unref(state: *mut xkb_state);
    pub fn xkb_state_update_key(state: *mut xkb_state, key: u32, direction: c_int) -> c_int;
    pub fn xkb_state_key_get_one_sym(state: *mut xkb_state, key: u32) -> u32;
    pub fn xkb_state_key_get_utf8(
        state: *mut xkb_state,
        key: u32,
        buffer: *mut c_char,
        size: usize,
    ) -> c_int;
    pub fn xkb_state_update_mask(
        state: *mut xkb_state,
        depressed_mods: u32,
        latched_mods: u32,
        locked_mods: u32,
        depressed_layout: u32,
        latched_layout: u32,
        locked_layout: u32,
    ) -> c_int;
    pub fn xkb_state_serialize_mods(state: *mut xkb_state, components: c_int) -> u32;
    pub fn xkb_state_serialize_layout(state: *mut xkb_state, components: c_int) -> u32;
    pub fn xkb_keysym_get_name(keysym: u32, buffer: *mut c_char, size: usize) -> c_int;
}

// --- libc -----------------------------------------------------------------
pub const O_CLOEXEC: c_int = 0o2_000_000;

extern "C" {
    pub fn open(path: *const c_char, flags: c_int, ...) -> c_int;
    pub fn close(fd: c_int) -> c_int;
    pub fn free(p: *mut c_void);
}

#[repr(C)]
#[derive(Debug)]
pub struct pollfd {
    pub fd: c_int,
    pub events: i16,
    pub revents: i16,
}
pub const POLLIN: i16 = 1;

extern "C" {
    pub fn poll(fds: *mut pollfd, nfds: u64, timeout_ms: c_int) -> c_int;
}
