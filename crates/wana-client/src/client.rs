//! Minimal libwayland-client binding: proxies, requests through
//! `wl_proxy_marshal_array_flags`, and events through one dispatcher that
//! queues owned values (no Rust closure is called from C).

#![allow(non_camel_case_types)]

use std::cell::RefCell;
use std::collections::VecDeque;
use std::ffi::{CStr, CString};
use std::os::raw::{c_char, c_int, c_void};
use std::ptr::{self, NonNull};
use wana_wayland::sys::{wl_argument, wl_interface, wl_message};

#[derive(Debug)]
pub enum wl_proxy {}

#[link(name = "wayland-client")]
extern "C" {
    fn wl_display_connect(name: *const c_char) -> *mut wl_proxy;
    fn wl_display_disconnect(display: *mut wl_proxy);
    fn wl_display_roundtrip(display: *mut wl_proxy) -> c_int;
    fn wl_display_dispatch(display: *mut wl_proxy) -> c_int;
    fn wl_display_get_error(display: *mut wl_proxy) -> c_int;
    fn wl_display_get_protocol_error(
        display: *mut wl_proxy,
        interface: *mut *const wl_interface,
        id: *mut u32,
    ) -> u32;
    fn wl_proxy_marshal_array_flags(
        proxy: *mut wl_proxy,
        opcode: u32,
        interface: *const wl_interface,
        version: u32,
        flags: u32,
        args: *mut wl_argument,
    ) -> *mut wl_proxy;
    fn wl_proxy_add_dispatcher(
        proxy: *mut wl_proxy,
        dispatcher: unsafe extern "C" fn(
            *const c_void,
            *mut c_void,
            u32,
            *const wl_message,
            *mut wl_argument,
        ) -> c_int,
        implementation: *const c_void,
        data: *mut c_void,
    ) -> c_int;
    fn wl_proxy_get_version(proxy: *mut wl_proxy) -> u32;
    fn wl_display_get_fd(display: *mut wl_proxy) -> c_int;
    fn wl_display_flush(display: *mut wl_proxy) -> c_int;
    fn wl_display_dispatch_pending(display: *mut wl_proxy) -> c_int;
}

#[repr(C)]
struct PollFd {
    fd: c_int,
    events: i16,
    revents: i16,
}

extern "C" {
    fn poll(fds: *mut PollFd, nfds: u64, timeout: c_int) -> c_int;
}

const WL_MARSHAL_FLAG_DESTROY: u32 = 1;

/// A client-side object.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Proxy(NonNull<wl_proxy>);

/// An event argument, owned.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Val {
    Int(i32),
    Uint(u32),
    Str(String),
    Array(Vec<u8>),
    /// An object argument (NULL or an object this client knows).
    Object(Option<Proxy>),
    Other,
}

/// One received event.
#[derive(Debug, Clone)]
pub struct Event {
    pub target: Proxy,
    pub opcode: u32,
    pub args: Vec<Val>,
}

/// A request argument.
#[derive(Debug, Clone, Copy)]
pub enum Req<'a> {
    Int(i32),
    Uint(u32),
    Str(&'a str),
    Object(Option<Proxy>),
    /// Placeholder for the new object of a constructor request.
    NewId,
    Fd(i32),
}

type Queue = RefCell<VecDeque<Event>>;

unsafe extern "C" fn dispatcher(
    implementation: *const c_void,
    target: *mut c_void,
    opcode: u32,
    message: *const wl_message,
    args: *mut wl_argument,
) -> c_int {
    // SAFETY: implementation is the Queue owned by the Connection (alive
    // while its display is connected); `args` has one entry per wire value
    // of `message`'s signature.
    unsafe {
        let queue = &*(implementation as *const Queue);
        let sig = CStr::from_ptr((*message).signature).to_string_lossy();
        let mut vals = Vec::new();
        for (k, c) in sig.chars().filter(|c| c.is_ascii_alphabetic()).enumerate() {
            let a = *args.add(k);
            vals.push(match c {
                'i' | 'f' | 'h' => Val::Int(a.i),
                'u' => Val::Uint(a.u),
                's' if a.s.is_null() => Val::Other,
                's' => Val::Str(CStr::from_ptr(a.s).to_string_lossy().into_owned()),
                'o' => Val::Object(NonNull::new(a.o.cast()).map(Proxy)),
                'a' if a.a.is_null() => Val::Array(Vec::new()),
                'a' => Val::Array(
                    std::slice::from_raw_parts((*a.a).data as *const u8, (*a.a).size).to_vec(),
                ),
                _ => Val::Other,
            });
        }
        queue.borrow_mut().push_back(Event {
            target: Proxy(NonNull::new_unchecked(target as *mut wl_proxy)),
            opcode,
            args: vals,
        });
    }
    0
}

/// A connection to the compositor named by `$WAYLAND_DISPLAY`.
#[derive(Debug)]
pub struct Connection {
    display: NonNull<wl_proxy>,
    queue: Box<Queue>,
}

impl Connection {
    pub fn connect() -> Result<Connection, String> {
        // SAFETY: NULL selects $WAYLAND_DISPLAY.
        let d = NonNull::new(unsafe { wl_display_connect(ptr::null()) })
            .ok_or("wl_display_connect failed (is WAYLAND_DISPLAY set?)")?;
        Ok(Connection {
            display: d,
            queue: Box::default(),
        })
    }

    pub fn display(&self) -> Proxy {
        Proxy(self.display)
    }

    /// Sends a request; returns the new object for constructors
    /// (`new` = its interface) and listens to its events.
    pub fn request(
        &self,
        target: Proxy,
        opcode: u32,
        new: Option<(&'static wl_interface, u32)>,
        args: &[Req],
    ) -> Result<Option<Proxy>, String> {
        let mut strings = Vec::new();
        let mut wire: Vec<wl_argument> = Vec::with_capacity(args.len());
        for a in args {
            wire.push(match a {
                Req::Int(v) => wl_argument { i: *v },
                Req::Uint(v) => wl_argument { u: *v },
                Req::Fd(v) => wl_argument { h: *v },
                Req::Str(s) => {
                    let c = CString::new(*s).map_err(|e| e.to_string())?;
                    let p = c.as_ptr();
                    strings.push(c);
                    wl_argument { s: p }
                }
                Req::Object(o) => wl_argument {
                    o: o.map_or(ptr::null_mut(), |p| p.0.as_ptr().cast()),
                },
                Req::NewId => wl_argument { o: ptr::null_mut() },
            });
        }
        // SAFETY: target is a live proxy; `wire` matches the request
        // signature (callers follow the generated opcodes); strings outlive
        // the call.
        let version = new
            .map(|(_, v)| v)
            .unwrap_or_else(|| unsafe { wl_proxy_get_version(target.0.as_ptr()) });
        let iface = new.map_or(ptr::null(), |(i, _)| i as *const wl_interface);
        // SAFETY: as above.
        let p = unsafe {
            wl_proxy_marshal_array_flags(
                target.0.as_ptr(),
                opcode,
                iface,
                version,
                0,
                wire.as_mut_ptr(),
            )
        };
        drop(strings);
        match new {
            None => Ok(None),
            Some(_) => {
                let p = NonNull::new(p).ok_or("constructor request failed")?;
                // SAFETY: new proxy; the queue outlives the connection's proxies.
                unsafe {
                    wl_proxy_add_dispatcher(
                        p.as_ptr(),
                        dispatcher,
                        &*self.queue as *const Queue as *const c_void,
                        ptr::null_mut(),
                    )
                };
                Ok(Some(Proxy(p)))
            }
        }
    }

    /// Sends a destructor request and destroys the proxy.
    pub fn destroy(&self, target: Proxy, opcode: u32) {
        // SAFETY: live proxy; the destructor requests used have no arguments.
        unsafe {
            wl_proxy_marshal_array_flags(
                target.0.as_ptr(),
                opcode,
                ptr::null(),
                wl_proxy_get_version(target.0.as_ptr()),
                WL_MARSHAL_FLAG_DESTROY,
                ptr::null_mut(),
            );
        }
    }

    /// Listens to events of the display object itself (wl_display.error is
    /// handled by libwayland; nothing else is needed).
    pub fn roundtrip(&self) -> Result<(), String> {
        // SAFETY: connected display.
        if unsafe { wl_display_roundtrip(self.display.as_ptr()) } < 0 {
            return Err(self.error());
        }
        Ok(())
    }

    /// Blocks for at least one event.
    pub fn dispatch(&self) -> Result<(), String> {
        // SAFETY: connected display.
        if unsafe { wl_display_dispatch(self.display.as_ptr()) } < 0 {
            return Err(self.error());
        }
        Ok(())
    }

    /// Sends queued requests, then waits up to `timeout_ms` for events and
    /// queues them (a shell redraws its clock between events). Returns true
    /// if events arrived.
    pub fn wait(&self, timeout_ms: i32) -> Result<bool, String> {
        // SAFETY: connected display; one valid pollfd.
        unsafe {
            if wl_display_dispatch_pending(self.display.as_ptr()) < 0 {
                return Err(self.error());
            }
            wl_display_flush(self.display.as_ptr());
            let mut p = PollFd {
                fd: wl_display_get_fd(self.display.as_ptr()),
                events: 1, // POLLIN
                revents: 0,
            };
            let n = poll(&mut p, 1, timeout_ms);
            if n > 0 && p.revents != 0 {
                if wl_display_dispatch(self.display.as_ptr()) < 0 {
                    return Err(self.error());
                }
                return Ok(true);
            }
        }
        Ok(false)
    }

    pub fn next_event(&self) -> Option<Event> {
        self.queue.borrow_mut().pop_front()
    }

    /// The protocol error that ended the connection, if any:
    /// (interface name, object id, code).
    pub fn protocol_error(&self) -> Option<(String, u32, u32)> {
        let mut iface: *const wl_interface = ptr::null();
        let mut id = 0;
        // SAFETY: connected display; out-pointers valid.
        let code =
            unsafe { wl_display_get_protocol_error(self.display.as_ptr(), &mut iface, &mut id) };
        if iface.is_null() {
            return None;
        }
        // SAFETY: libwayland returns a pointer to a protocol table.
        let name = unsafe { CStr::from_ptr((*iface).name) }
            .to_string_lossy()
            .into_owned();
        Some((name, id, code))
    }

    fn error(&self) -> String {
        match self.protocol_error() {
            Some((iface, id, code)) => format!("protocol error: {iface}@{id} code {code}"),
            // SAFETY: connected display.
            None => format!("connection error {}", unsafe {
                wl_display_get_error(self.display.as_ptr())
            }),
        }
    }
}

impl Drop for Connection {
    fn drop(&mut self) {
        // SAFETY: connected display, disconnected once; proxies are gone
        // with it.
        unsafe { wl_display_disconnect(self.display.as_ptr()) };
    }
}
