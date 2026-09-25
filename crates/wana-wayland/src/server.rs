//! Safe wrapper over the libwayland-server display: the listening socket,
//! the event loop, and client connect/disconnect tracking.
//!
//! Callbacks from libwayland only push `ClientEvent`s into a queue owned by
//! the `Display`; the compositor drains it after each dispatch, so no Rust
//! closure is ever called from C.

use crate::sys;
use std::cell::RefCell;
use std::collections::VecDeque;
use std::ffi::{CStr, OsStr};
use std::io;
use std::os::raw::c_void;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::io::RawFd;
use std::path::PathBuf;
use std::ptr::{self, NonNull};
use std::time::Duration;

/// Credentials of a connected client (from SO_PEERCRED).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Credentials {
    pub pid: i32,
    pub uid: u32,
    pub gid: u32,
}

/// Something that happened to a client during a dispatch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClientEvent {
    Connected(Credentials),
    Disconnected(Credentials),
}

type Queue = RefCell<VecDeque<ClientEvent>>;

/// The display-lifetime "client created" listener. `listener` must stay the
/// first field: the notify callback casts the listener pointer back.
#[repr(C)]
struct CreatedListener {
    listener: sys::wl_listener,
    queue: *const Queue,
}

/// One per client, freed when the client is destroyed.
#[repr(C)]
struct DestroyListener {
    listener: sys::wl_listener,
    queue: *const Queue,
    creds: Credentials,
}

fn empty_link() -> sys::wl_list {
    sys::wl_list {
        prev: ptr::null_mut(),
        next: ptr::null_mut(),
    }
}

unsafe extern "C" fn client_created(listener: *mut sys::wl_listener, data: *mut c_void) {
    // SAFETY: `listener` is the first field of a live CreatedListener (it
    // lives as long as the display), and `data` is the new wl_client.
    unsafe {
        let this = &*(listener as *const CreatedListener);
        let client = data as *mut sys::wl_client;
        let mut creds = Credentials {
            pid: 0,
            uid: 0,
            gid: 0,
        };
        sys::wl_client_get_credentials(client, &mut creds.pid, &mut creds.uid, &mut creds.gid);
        (*this.queue)
            .borrow_mut()
            .push_back(ClientEvent::Connected(creds));
        let destroy = Box::into_raw(Box::new(DestroyListener {
            listener: sys::wl_listener {
                link: empty_link(),
                notify: client_destroyed,
            },
            queue: this.queue,
            creds,
        }));
        sys::wl_client_add_destroy_listener(client, &mut (*destroy).listener);
    }
}

unsafe extern "C" fn client_destroyed(listener: *mut sys::wl_listener, _data: *mut c_void) {
    // SAFETY: `listener` is the first field of the DestroyListener leaked in
    // client_created. libwayland removes it from the client's signal before
    // calling it (final emit), and calls it exactly once, so it can be freed.
    unsafe {
        let this = Box::from_raw(listener as *mut DestroyListener);
        (*this.queue)
            .borrow_mut()
            .push_back(ClientEvent::Disconnected(this.creds));
    }
}

/// A Wayland display with one listening socket.
#[derive(Debug)]
pub struct Display {
    display: NonNull<sys::wl_display>,
    event_loop: NonNull<sys::wl_event_loop>,
    socket: Option<String>,
    created: *mut CreatedListener,
    // Boxed so its address is stable; freed after the display in Drop.
    queue: Box<Queue>,
}

impl Display {
    /// Creates the display and starts tracking clients.
    pub fn new() -> io::Result<Display> {
        // SAFETY: no arguments.
        let display = NonNull::new(unsafe { sys::wl_display_create() })
            .ok_or_else(|| io::Error::other("wl_display_create failed"))?;
        // SAFETY: display is valid; the event loop belongs to it.
        let event_loop = NonNull::new(unsafe { sys::wl_display_get_event_loop(display.as_ptr()) })
            .ok_or_else(|| io::Error::other("wl_display_get_event_loop failed"))?;
        let queue: Box<Queue> = Box::default();
        let created = Box::into_raw(Box::new(CreatedListener {
            listener: sys::wl_listener {
                link: empty_link(),
                notify: client_created,
            },
            queue: &*queue,
        }));
        // SAFETY: display is valid; the listener outlives the display (freed
        // in Drop after wl_display_destroy).
        unsafe {
            sys::wl_display_add_client_created_listener(display.as_ptr(), &mut (*created).listener)
        };
        Ok(Display {
            display,
            event_loop,
            socket: None,
            created,
            queue,
        })
    }

    /// Listens on the first free `wayland-N` socket in `$XDG_RUNTIME_DIR` and
    /// returns its name (the value for clients' `WAYLAND_DISPLAY`).
    pub fn add_socket_auto(&mut self) -> io::Result<String> {
        // SAFETY: display is valid; the returned name is owned by it.
        let name = unsafe { sys::wl_display_add_socket_auto(self.display.as_ptr()) };
        if name.is_null() {
            return Err(io::Error::other(
                "wl_display_add_socket_auto failed (is XDG_RUNTIME_DIR set and writable?)",
            ));
        }
        // SAFETY: non-NULL, NUL-terminated, owned by the display.
        let name = unsafe { CStr::from_ptr(name) }
            .to_string_lossy()
            .into_owned();
        self.socket = Some(name.clone());
        Ok(name)
    }

    /// Full path of the listening socket, if any.
    pub fn socket_path(&self) -> Option<PathBuf> {
        let dir = std::env::var_os("XDG_RUNTIME_DIR")?;
        let name = self.socket.as_ref()?;
        Some(PathBuf::from(dir).join(OsStr::from_bytes(name.as_bytes())))
    }

    /// The event loop's epoll fd (readable when there is work).
    pub fn event_loop_fd(&self) -> RawFd {
        // SAFETY: event loop is valid.
        unsafe { sys::wl_event_loop_get_fd(self.event_loop.as_ptr()) }
    }

    /// Sends buffered events to clients, then waits up to `timeout` for and
    /// handles client requests and connections.
    pub fn dispatch(&mut self, timeout: Duration) -> io::Result<()> {
        let ms = timeout.as_millis().min(i32::MAX as u128) as i32;
        // SAFETY: display and event loop are valid.
        let rc = unsafe {
            sys::wl_display_flush_clients(self.display.as_ptr());
            sys::wl_event_loop_dispatch(self.event_loop.as_ptr(), ms)
        };
        if rc < 0 {
            let e = io::Error::last_os_error();
            if e.kind() != io::ErrorKind::Interrupted {
                return Err(e);
            }
        }
        Ok(())
    }

    /// Takes the client events recorded since the last call.
    pub fn take_events(&mut self) -> Vec<ClientEvent> {
        self.queue.borrow_mut().drain(..).collect()
    }
}

impl Drop for Display {
    fn drop(&mut self) {
        // SAFETY: display is valid and destroyed exactly once. Destroying the
        // clients first runs their destroy listeners while the queue is
        // alive; wl_display_destroy also removes the socket and lock files.
        unsafe {
            sys::wl_display_destroy_clients(self.display.as_ptr());
            sys::wl_display_destroy(self.display.as_ptr());
            drop(Box::from_raw(self.created));
        }
    }
}
