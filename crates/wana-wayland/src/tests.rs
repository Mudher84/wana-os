//! Tests against the real libwayland-server of the build host.

use crate::protocols::{wayland, xdg_shell};
use crate::server::{ClientEvent, Display};
use crate::sys::{wl_interface, wl_message};
use std::ffi::{CStr, CString};
use std::io::{Read, Write};
use std::os::raw::{c_char, c_int, c_void};
use std::os::unix::net::UnixStream;
use std::time::{Duration, Instant};

extern "C" {
    fn dlopen(file: *const c_char, mode: c_int) -> *mut c_void;
    fn dlsym(handle: *mut c_void, name: *const c_char) -> *mut c_void;
}
const RTLD_NOW: c_int = 2;

fn text(p: *const c_char) -> String {
    // SAFETY: table strings are NUL-terminated statics.
    unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned()
}

/// Interface name of each `types` entry of a message (None for NULL).
fn type_names(m: &wl_message, n: usize) -> Vec<Option<String>> {
    (0..n)
        .map(|k| {
            // SAFETY: `types` has one entry per wire value (n of them).
            let t = unsafe { *m.types.add(k) };
            // SAFETY: non-NULL entries point to interfaces.
            (!t.0.is_null()).then(|| text(unsafe { (*t.0).name }))
        })
        .collect()
}

/// Wire values in a signature: type codes, without version digits and '?'.
fn wire_len(sig: &str) -> usize {
    sig.chars().filter(|c| c.is_ascii_alphabetic()).count()
}

/// (name, signature, argument interfaces) of each message.
type Messages = Vec<(String, String, Vec<Option<String>>)>;

fn messages(p: *const wl_message, n: c_int) -> Messages {
    (0..n as usize)
        .map(|k| {
            // SAFETY: the table has n messages.
            let m = unsafe { &*p.add(k) };
            let sig = text(m.signature);
            let types = type_names(m, wire_len(&sig));
            (text(m.name), sig, types)
        })
        .collect()
}

fn describe(i: &wl_interface) -> (String, i32, Messages, Messages) {
    (
        text(i.name),
        i.version,
        messages(i.methods, i.method_count),
        messages(i.events, i.event_count),
    )
}

/// The generated core tables must equal the ones libwayland-server exports
/// (`wl_*_interface`), which wayland-scanner generated from the same XML:
/// names, versions, every message signature and argument interface.
#[test]
fn core_tables_match_libwayland() {
    // SAFETY: loading a system library by soname.
    let lib = unsafe { dlopen(c"libwayland-server.so.0".as_ptr(), RTLD_NOW) };
    assert!(!lib.is_null(), "libwayland-server.so.0 not loadable");
    let mut compared = 0;
    for ours in wayland::INTERFACES {
        let name = text(ours.name);
        let sym = CString::new(format!("{name}_interface")).unwrap();
        // SAFETY: valid handle and NUL-terminated symbol name.
        let theirs = unsafe { dlsym(lib, sym.as_ptr()) } as *const wl_interface;
        assert!(
            !theirs.is_null(),
            "libwayland-server does not export {name}_interface"
        );
        // SAFETY: the symbol is libwayland's wl_interface table.
        assert_eq!(describe(ours), describe(unsafe { &*theirs }), "{name}");
        compared += 1;
    }
    assert_eq!(compared, wayland::INTERFACES.len());
    assert!(compared >= 22, "core protocol has at least 22 interfaces");
}

#[test]
fn xdg_shell_references_core_tables() {
    assert_eq!(xdg_shell::INTERFACES.len(), 5);
    assert_eq!(text(xdg_shell::XDG_WM_BASE_INTERFACE.name), "xdg_wm_base");
    // xdg_wm_base.get_xdg_surface(new_id xdg_surface, object wl_surface)
    let base = &xdg_shell::XDG_WM_BASE_INTERFACE;
    let op = xdg_shell::xdg_wm_base::request::GET_XDG_SURFACE as usize;
    assert!(op < base.method_count as usize);
    // SAFETY: op is within the request table.
    let get = unsafe { &*base.methods.add(op) };
    assert_eq!(text(get.signature), "no");
    // SAFETY: the message has two wire values.
    let (a, b) = unsafe { (*get.types, *get.types.add(1)) };
    assert!(std::ptr::eq(a.0, &xdg_shell::XDG_SURFACE_INTERFACE));
    assert!(
        std::ptr::eq(b.0, &wayland::WL_SURFACE_INTERFACE),
        "cross-protocol reference"
    );
    assert_eq!(wayland::wl_display::request::GET_REGISTRY, 1);
    assert_eq!(wayland::wl_callback::event::DONE, 0);
}

fn header(object: u32, opcode: u16, size: u16) -> [u8; 8] {
    let mut h = [0u8; 8];
    h[..4].copy_from_slice(&object.to_ne_bytes());
    h[4..].copy_from_slice(&(((size as u32) << 16) | opcode as u32).to_ne_bytes());
    h
}

/// A hand-written Wayland client speaks the wire format to the socket:
/// wl_display.get_registry + wl_display.sync, and must receive
/// wl_callback.done and wl_display.delete_id. Connect and disconnect must be
/// reported with the client's credentials.
#[test]
fn socket_round_trip_with_a_raw_client() {
    let dir = std::env::temp_dir().join(format!("wana-wl-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    // Only this test touches the environment.
    std::env::set_var("XDG_RUNTIME_DIR", &dir);
    let mut display = Display::new().unwrap();
    let name = display.add_socket_auto().unwrap();
    let path = display.socket_path().unwrap();
    assert!(name.starts_with("wayland-"), "{name}");
    assert!(path.exists(), "{}", path.display());

    let mut client = UnixStream::connect(&path).unwrap();
    let mut req = Vec::new();
    req.extend(header(
        1,
        wayland::wl_display::request::GET_REGISTRY as u16,
        12,
    ));
    req.extend(2u32.to_ne_bytes());
    req.extend(header(1, wayland::wl_display::request::SYNC as u16, 12));
    req.extend(3u32.to_ne_bytes());
    client.write_all(&req).unwrap();
    client.set_nonblocking(true).unwrap();

    let mut got = Vec::new();
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut events = Vec::new();
    while got.len() < 24 && Instant::now() < deadline {
        display.dispatch(Duration::from_millis(20)).unwrap();
        events.extend(display.take_events());
        let mut buf = [0u8; 256];
        if let Ok(n) = client.read(&mut buf) {
            got.extend_from_slice(&buf[..n]);
        }
    }
    let word = |i: usize| u32::from_ne_bytes(got[i..i + 4].try_into().unwrap());
    assert!(got.len() >= 24, "only {} bytes from the server", got.len());
    // wl_callback@3.done(serial)
    assert_eq!(word(0), 3);
    assert_eq!(word(4), (12 << 16) | wayland::wl_callback::event::DONE);
    // wl_display@1.delete_id(3)
    assert_eq!(word(12), 1);
    assert_eq!(word(16), (12 << 16) | wayland::wl_display::event::DELETE_ID);
    assert_eq!(word(20), 3);

    let me = std::process::id() as i32;
    assert!(
        matches!(events.first(), Some(ClientEvent::Connected(c)) if c.pid == me),
        "{events:?}"
    );
    drop(client);
    for _ in 0..50 {
        display.dispatch(Duration::from_millis(20)).unwrap();
        events.extend(display.take_events());
        if events
            .iter()
            .any(|e| matches!(e, ClientEvent::Disconnected(_)))
        {
            break;
        }
    }
    assert!(
        matches!(events.last(), Some(ClientEvent::Disconnected(c)) if c.pid == me),
        "{events:?}"
    );
    drop(display);
    assert!(!path.exists(), "socket removed when the display is dropped");
    std::fs::remove_dir_all(&dir).unwrap();
}
