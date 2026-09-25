//! Minimal FFI declarations for libwayland-server (wayland-server-core.h,
//! wayland-util.h). Only what Wana uses is declared. The layouts of
//! `wl_message`, `wl_interface`, `wl_list` and `wl_listener` are part of
//! libwayland's stable ABI (unchanged since 1.0).

#![allow(non_camel_case_types)]

use std::os::raw::{c_char, c_int, c_void};

/// One request or event: name, signature string, argument interfaces.
#[repr(C)]
#[derive(Debug)]
pub struct wl_message {
    pub name: *const c_char,
    pub signature: *const c_char,
    pub types: *const TypeRef,
}

/// Describes one protocol interface to libwayland.
#[repr(C)]
#[derive(Debug)]
pub struct wl_interface {
    pub name: *const c_char,
    pub version: c_int,
    pub method_count: c_int,
    pub methods: *const wl_message,
    pub event_count: c_int,
    pub events: *const wl_message,
}

/// An entry of a message's `types` array: the interface of an object or
/// new_id argument, or NULL. Same layout as `const struct wl_interface *`.
#[repr(transparent)]
#[derive(Debug, Clone, Copy)]
pub struct TypeRef(pub *const wl_interface);

// SAFETY: the protocol tables are immutable statics that only point to other
// immutable statics (names, signatures, interfaces); sharing them between
// threads cannot cause a data race.
unsafe impl Sync for wl_message {}
// SAFETY: as above.
unsafe impl Sync for wl_interface {}
// SAFETY: as above.
unsafe impl Sync for TypeRef {}

impl TypeRef {
    pub const NULL: TypeRef = TypeRef(std::ptr::null());
}

/// Intrusive doubly linked list node.
#[repr(C)]
#[derive(Debug)]
pub struct wl_list {
    pub prev: *mut wl_list,
    pub next: *mut wl_list,
}

pub type wl_notify_func_t = unsafe extern "C" fn(listener: *mut wl_listener, data: *mut c_void);

/// Signal listener; embedded as the first field of a Rust struct so the
/// notify callback can recover the struct from the listener pointer.
#[repr(C)]
#[derive(Debug)]
pub struct wl_listener {
    pub link: wl_list,
    pub notify: wl_notify_func_t,
}

#[derive(Debug)]
pub enum wl_display {}
#[derive(Debug)]
pub enum wl_event_loop {}
#[derive(Debug)]
pub enum wl_client {}

#[link(name = "wayland-server")]
extern "C" {
    pub fn wl_display_create() -> *mut wl_display;
    pub fn wl_display_destroy(display: *mut wl_display);
    pub fn wl_display_destroy_clients(display: *mut wl_display);
    pub fn wl_display_get_event_loop(display: *mut wl_display) -> *mut wl_event_loop;
    pub fn wl_display_add_socket_auto(display: *mut wl_display) -> *const c_char;
    pub fn wl_display_flush_clients(display: *mut wl_display);
    pub fn wl_display_add_client_created_listener(
        display: *mut wl_display,
        listener: *mut wl_listener,
    );

    pub fn wl_event_loop_dispatch(event_loop: *mut wl_event_loop, timeout_ms: c_int) -> c_int;
    pub fn wl_event_loop_get_fd(event_loop: *mut wl_event_loop) -> c_int;

    pub fn wl_client_get_credentials(
        client: *mut wl_client,
        pid: *mut i32,
        uid: *mut u32,
        gid: *mut u32,
    );
    pub fn wl_client_add_destroy_listener(client: *mut wl_client, listener: *mut wl_listener);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::mem::{align_of, size_of};

    /// Sizes on LP64 (x86_64), as in wayland-util.h.
    #[test]
    fn abi_layouts() {
        assert_eq!(size_of::<wl_message>(), 24);
        assert_eq!(size_of::<wl_interface>(), 40);
        assert_eq!(size_of::<TypeRef>(), size_of::<*const wl_interface>());
        assert_eq!(size_of::<wl_list>(), 16);
        assert_eq!(size_of::<wl_listener>(), 24);
        assert_eq!(align_of::<wl_interface>(), 8);
    }
}
