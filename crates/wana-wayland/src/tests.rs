//! Tests against the real libwayland-server of the build host.

use crate::protocols::{wayland, xdg_shell};
use crate::server::{Arg, ClientEvent, Ctx, Display, Handler, ReqArg, Resource};
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
    assert_eq!(
        crate::server::request_name(&wayland::WL_COMPOSITOR_INTERFACE, 0),
        "wl_compositor.create_surface"
    );
    assert_eq!(
        crate::server::request_name(&wayland::WL_COMPOSITOR_INTERFACE, 99),
        "wl_compositor.#99"
    );
    assert_eq!(wayland::wl_callback::event::DONE, 0);
}

/// Test handler: one wl_shm global announcing two formats on bind.
#[derive(Default)]
struct ShmOnly {
    binds: u32,
    destroyed: u32,
    requests: Vec<u32>,
    post_errors: Vec<String>,
}

impl Handler for ShmOnly {
    fn bind(&mut self, ctx: &mut Ctx, _global: usize, res: Resource) {
        self.binds += 1;
        for fmt in [0u32, 1] {
            ctx.post(res, wayland::wl_shm::event::FORMAT, &[Arg::Uint(fmt)])
                .unwrap();
        }
        // Checked marshalling: wrong kind, wrong count, missing event.
        for bad in [
            ctx.post(res, wayland::wl_shm::event::FORMAT, &[Arg::Int(1)]),
            ctx.post(res, wayland::wl_shm::event::FORMAT, &[]),
            ctx.post(res, 7, &[Arg::Uint(0)]),
        ] {
            self.post_errors.push(bad.unwrap_err());
        }
    }
    fn request(&mut self, _ctx: &mut Ctx, _res: Resource, opcode: u32, _args: Vec<ReqArg>) {
        self.requests.push(opcode);
    }
    fn destroyed(&mut self, ctx: &mut Ctx, res: Resource) {
        assert!(!ctx.is_alive(res));
        assert!(
            ctx.post(res, 0, &[Arg::Uint(0)]).is_err(),
            "stale resource refused"
        );
        self.destroyed += 1;
    }
}

fn header(object: u32, opcode: u32, size: usize) -> Vec<u8> {
    let mut h = Vec::new();
    h.extend(object.to_ne_bytes());
    h.extend((((size as u32) << 16) | opcode).to_ne_bytes());
    h
}

/// Wire string: length including NUL, bytes, NUL, padded to 4.
fn wire_str(s: &str) -> Vec<u8> {
    let mut v = ((s.len() + 1) as u32).to_ne_bytes().to_vec();
    v.extend(s.as_bytes());
    v.push(0);
    while !v.len().is_multiple_of(4) {
        v.push(0);
    }
    v
}

/// Parsed server messages: (object, opcode, body words).
fn messages_of(buf: &[u8]) -> Vec<(u32, u32, Vec<u8>)> {
    let mut out = Vec::new();
    let mut at = 0;
    while at + 8 <= buf.len() {
        let obj = u32::from_ne_bytes(buf[at..at + 4].try_into().unwrap());
        let w = u32::from_ne_bytes(buf[at + 4..at + 8].try_into().unwrap());
        let (size, opcode) = ((w >> 16) as usize, w & 0xffff);
        if at + size > buf.len() {
            break;
        }
        out.push((obj, opcode, buf[at + 8..at + size].to_vec()));
        at += size;
    }
    out
}

fn pump(
    display: &mut Display<ShmOnly>,
    client: &mut UnixStream,
    got: &mut Vec<u8>,
    until: impl Fn(&[u8]) -> bool,
) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while !until(got) && Instant::now() < deadline {
        display.dispatch(Duration::from_millis(20)).unwrap();
        let mut buf = [0u8; 1024];
        if let Ok(n) = client.read(&mut buf) {
            got.extend_from_slice(&buf[..n]);
        }
    }
}

/// A hand-written Wayland client speaks the wire format to the socket:
/// registry + sync, then binds the advertised wl_shm global and syncs
/// again. The global's initial events must arrive before the sync reply;
/// destroying the client destroys its resources and is reported.
#[test]
fn socket_round_trip_with_a_raw_client() {
    let dir = std::env::temp_dir().join(format!("wana-wl-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    // Only this test touches the environment.
    std::env::set_var("XDG_RUNTIME_DIR", &dir);
    let mut display = Display::new(ShmOnly::default()).unwrap();
    assert!(
        display
            .create_global(&wayland::WL_SHM_INTERFACE, 99)
            .is_err(),
        "beyond XML version"
    );
    assert_eq!(
        display
            .create_global(&wayland::WL_SHM_INTERFACE, 1)
            .unwrap(),
        0
    );
    let name = display.add_socket_auto().unwrap();
    let path = display.socket_path().unwrap();
    assert!(name.starts_with("wayland-"), "{name}");

    let mut client = UnixStream::connect(&path).unwrap();
    let mut req = header(1, wayland::wl_display::request::GET_REGISTRY, 12);
    req.extend(2u32.to_ne_bytes());
    req.extend(header(1, wayland::wl_display::request::SYNC, 12));
    req.extend(3u32.to_ne_bytes());
    client.write_all(&req).unwrap();
    client.set_nonblocking(true).unwrap();
    let mut got = Vec::new();
    let done = |obj: u32| move |b: &[u8]| messages_of(b).iter().any(|m| m.0 == obj && m.1 == 0);
    pump(&mut display, &mut client, &mut got, done(3));

    let msgs = messages_of(&got);
    // wl_registry@2.global(name, "wl_shm", 1)
    let global = msgs
        .iter()
        .find(|m| m.0 == 2 && m.1 == wayland::wl_registry::event::GLOBAL)
        .expect("global event");
    let global_name = u32::from_ne_bytes(global.2[..4].try_into().unwrap());
    assert_eq!(
        &global.2[4..],
        &[wire_str("wl_shm"), 1u32.to_ne_bytes().to_vec()].concat()[..]
    );
    assert!(msgs
        .iter()
        .any(|m| m.0 == 3 && m.1 == wayland::wl_callback::event::DONE));
    assert!(msgs.iter().any(|m| m.0 == 1
        && m.1 == wayland::wl_display::event::DELETE_ID
        && m.2 == 3u32.to_ne_bytes()));

    // wl_registry@2.bind(name, "wl_shm", 1, new id 4), then sync -> 5.
    let mut body = global_name.to_ne_bytes().to_vec();
    body.extend(wire_str("wl_shm"));
    body.extend(1u32.to_ne_bytes());
    body.extend(4u32.to_ne_bytes());
    let mut req = header(2, wayland::wl_registry::request::BIND, 8 + body.len());
    req.extend(body);
    req.extend(header(1, wayland::wl_display::request::SYNC, 12));
    req.extend(5u32.to_ne_bytes());
    client.write_all(&req).unwrap();
    got.clear();
    pump(&mut display, &mut client, &mut got, done(5));
    let msgs = messages_of(&got);
    let order: Vec<(u32, u32)> = msgs.iter().map(|m| (m.0, m.1)).collect();
    assert_eq!(
        order,
        vec![
            (4, 0),
            (4, 0),
            (5, 0),
            (1, wayland::wl_display::event::DELETE_ID)
        ],
        "two wl_shm.format events, then the sync reply"
    );
    assert_eq!(msgs[0].2, 0u32.to_ne_bytes());
    assert_eq!(msgs[1].2, 1u32.to_ne_bytes());
    {
        let h = display.handler();
        assert_eq!(h.binds, 1);
        assert_eq!(h.post_errors.len(), 3);
        assert!(
            h.post_errors[0].contains("does not match 'u'"),
            "{:?}",
            h.post_errors
        );
        assert!(
            h.post_errors[1].contains("0 arguments, signature has 1"),
            "{:?}",
            h.post_errors
        );
        assert!(
            h.post_errors[2].contains("no event 7"),
            "{:?}",
            h.post_errors
        );
    }

    let me = std::process::id() as i32;
    drop(client);
    let mut events = display.take_events();
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
        matches!(events.first(), Some(ClientEvent::Connected(c)) if c.pid == me),
        "{events:?}"
    );
    assert!(
        matches!(events.last(), Some(ClientEvent::Disconnected(c)) if c.pid == me),
        "{events:?}"
    );
    assert_eq!(
        display.handler().destroyed,
        1,
        "the bound wl_shm is reported destroyed"
    );
    assert!(display.handler().requests.is_empty());
    drop(display);
    assert!(!path.exists(), "socket removed when the display is dropped");
    std::fs::remove_dir_all(&dir).unwrap();
}
