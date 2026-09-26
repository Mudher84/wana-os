//! Safe server API over libwayland-server: the listening socket, the event
//! loop, globals, resources and request dispatch.
//!
//! All requests of all interfaces go through one dispatcher
//! (`wl_resource_set_dispatcher`) into the compositor's [`Handler`]:
//! libwayland decodes the wire format (checked against the generated
//! tables), Rust receives owned values. Events are checked against the
//! message signature and the object's version before they are marshalled.
//!
//! Object lifetime: a [`Resource`] is only an identity. Every operation
//! goes through [`Ctx`], which refuses resources that no longer exist, so a
//! stale handle can never reach libwayland. Destroyed resources are
//! reported to [`Handler::destroyed`] after the current callback returns.
//! Panics in the handler abort the process instead of unwinding into C.

use crate::sys::{self, wl_argument, wl_interface, wl_resource};
use std::cell::RefCell;
use std::collections::{HashMap, VecDeque};
use std::ffi::{CStr, CString, OsStr};
use std::io;
use std::os::raw::{c_char, c_int, c_void};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::io::{AsRawFd, BorrowedFd, FromRawFd, OwnedFd, RawFd};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::PathBuf;
use std::ptr::{self, NonNull};
use std::time::Duration;
use wana_log::{error, Subsystem};

const COMPOSITOR: Subsystem = Subsystem::Compositor;

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

/// Identity of a protocol object owned by a client. Only meaningful while
/// it exists; use it through [`Ctx`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Resource(NonNull<wl_resource>);

impl Resource {
    /// A fake identity for unit tests of code that stores resources. It is
    /// never alive, so every [`Ctx`] operation on it is refused.
    #[doc(hidden)]
    pub fn from_raw_for_tests(addr: usize) -> Resource {
        Resource(NonNull::new(addr as *mut wl_resource).expect("non-zero test address"))
    }
}

/// A request argument, decoded and owned.
#[derive(Debug)]
pub enum ReqArg {
    Int(i32),
    Uint(u32),
    /// `wl_fixed_t` raw value (24.8); see [`fixed_to_f64`].
    Fixed(i32),
    Str(Option<String>),
    Object(Option<Resource>),
    /// Id for a new object the handler creates with [`Ctx::create_resource`].
    NewId(u32),
    Array(Vec<u8>),
    /// Received file descriptor, now owned by the handler (closed on drop).
    Fd(OwnedFd),
}

/// An event argument.
#[derive(Debug, Clone, Copy)]
pub enum Arg<'a> {
    Int(i32),
    Uint(u32),
    Fixed(i32),
    Str(Option<&'a str>),
    Object(Option<Resource>),
    NewId(Resource),
    Array(&'a [u8]),
    Fd(BorrowedFd<'a>),
}

pub fn fixed_from_f64(v: f64) -> i32 {
    (v * 256.0).round() as i32
}

pub fn fixed_to_f64(v: i32) -> f64 {
    f64::from(v) / 256.0
}

/// The compositor's side of the protocol.
pub trait Handler {
    /// A client bound global number `global` (as returned by
    /// [`Display::create_global`]); `resource` already exists. Send the
    /// global's initial events here: they must reach the client before the
    /// reply to its next `wl_display.sync`.
    fn bind(&mut self, ctx: &mut Ctx, global: usize, resource: Resource);
    /// A request arrived. Destructor requests destroy the resource after
    /// this returns.
    fn request(&mut self, ctx: &mut Ctx, resource: Resource, opcode: u32, args: Vec<ReqArg>);
    /// `resource` no longer exists (destroyed by a request, by the handler,
    /// or because its client disconnected). Forget it.
    fn destroyed(&mut self, _ctx: &mut Ctx, _resource: Resource) {}
}

/// Per-resource user data; freed by the resource's destroy callback.
struct ResData {
    inner: *const Inner,
}

struct GlobalData {
    state: *const c_void,
    dispatcher: sys::wl_dispatcher_func_t,
    index: usize,
    interface: &'static wl_interface,
}

/// Non-generic state shared by all callbacks.
struct Inner {
    display: NonNull<sys::wl_display>,
    live: RefCell<HashMap<NonNull<wl_resource>, &'static wl_interface>>,
    pending_destroyed: RefCell<Vec<Resource>>,
    client_events: RefCell<VecDeque<ClientEvent>>,
    destructors: HashMap<*const wl_interface, &'static [u32]>,
}

struct State<H> {
    inner: Inner,
    handler: RefCell<H>,
}

/// Access to the protocol objects, for handlers and the compositor.
pub struct Ctx<'a> {
    inner: &'a Inner,
    /// Routing for resources created from here (the display's dispatcher).
    dispatcher: sys::wl_dispatcher_func_t,
    implementation: *const c_void,
}

impl std::fmt::Debug for Ctx<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Ctx({} live resources)", self.inner.live.borrow().len())
    }
}

impl Ctx<'_> {
    fn raw(&self, res: Resource) -> Result<*mut wl_resource, String> {
        if self.inner.live.borrow().contains_key(&res.0) {
            Ok(res.0.as_ptr())
        } else {
            Err("resource no longer exists".into())
        }
    }

    pub fn is_alive(&self, res: Resource) -> bool {
        self.inner.live.borrow().contains_key(&res.0)
    }

    /// The resource's interface, if it is alive.
    pub fn interface(&self, res: Resource) -> Option<&'static wl_interface> {
        self.inner.live.borrow().get(&res.0).copied()
    }

    /// True if `res` is alive and implements `iface`.
    pub fn is(&self, res: Resource, iface: &'static wl_interface) -> bool {
        self.interface(res).is_some_and(|i| ptr::eq(i, iface))
    }

    /// Protocol version the client bound the object with (0 if gone).
    pub fn version(&self, res: Resource) -> u32 {
        // SAFETY: the resource is alive.
        self.raw(res)
            .map(|r| unsafe { sys::wl_resource_get_version(r) } as u32)
            .unwrap_or(0)
    }

    /// Object id in the client's id space (0 if gone).
    pub fn id(&self, res: Resource) -> u32 {
        // SAFETY: the resource is alive.
        self.raw(res)
            .map(|r| unsafe { sys::wl_resource_get_id(r) })
            .unwrap_or(0)
    }

    /// Sends event `opcode` with `args`. Checked first: the resource exists,
    /// the opcode and argument kinds match the event's signature, the
    /// object's version has the event, and object arguments exist.
    pub fn post(&self, res: Resource, opcode: u32, args: &[Arg]) -> Result<(), String> {
        let raw = self.raw(res)?;
        let iface = self.interface(res).ok_or("resource no longer exists")?;
        let iname = cstr(iface.name);
        if opcode >= iface.event_count as u32 {
            return Err(format!("{iname}: no event {opcode}"));
        }
        // SAFETY: opcode < event_count; the table has event_count entries.
        let msg = unsafe { &*iface.events.add(opcode as usize) };
        let mname = cstr(msg.name);
        let sig = parse_signature(&cstr(msg.signature));
        let version = self.version(res);
        if version < sig.since {
            return Err(format!(
                "{iname}.{mname} needs version {}, object has {version}",
                sig.since
            ));
        }
        if sig.args.len() != args.len() {
            return Err(format!(
                "{iname}.{mname}: {} arguments, signature has {}",
                args.len(),
                sig.args.len()
            ));
        }
        // Keep marshalled strings and arrays alive until the call returns.
        let mut strings = Vec::new();
        let mut arrays: Vec<Box<sys::wl_array>> = Vec::new();
        let mut wire = Vec::with_capacity(args.len());
        for (k, ((code, nullable), arg)) in sig.args.iter().zip(args).enumerate() {
            let bad = || format!("{iname}.{mname}: argument {k} does not match '{code}'");
            let v = match (*code, arg) {
                ('i', Arg::Int(v)) => wl_argument { i: *v },
                ('u', Arg::Uint(v)) => wl_argument { u: *v },
                ('f', Arg::Fixed(v)) => wl_argument { f: *v },
                ('s', Arg::Str(Some(s))) => {
                    let c = CString::new(*s).map_err(|_| format!("{}: NUL in string", bad()))?;
                    let p = c.as_ptr();
                    strings.push(c);
                    wl_argument { s: p }
                }
                ('o', Arg::Object(Some(o))) | ('n', Arg::NewId(o)) => {
                    wl_argument { o: self.raw(*o)? }
                }
                ('s', Arg::Str(None)) if *nullable => wl_argument { s: ptr::null() },
                ('o', Arg::Object(None)) if *nullable => wl_argument { o: ptr::null_mut() },
                ('a', Arg::Array(bytes)) => {
                    let mut a = Box::new(sys::wl_array {
                        size: bytes.len(),
                        alloc: bytes.len(),
                        data: bytes.as_ptr() as *mut c_void,
                    });
                    let p: *mut sys::wl_array = &mut *a;
                    arrays.push(a);
                    wl_argument { a: p }
                }
                ('h', Arg::Fd(fd)) => wl_argument { h: fd.as_raw_fd() },
                _ => return Err(bad()),
            };
            wire.push(v);
        }
        // SAFETY: raw is alive, `wire` matches the signature, and every
        // pointer in it (strings, arrays, objects) outlives the call.
        // libwayland copies what it sends (and dups fds).
        unsafe { sys::wl_resource_post_event_array(raw, opcode, wire.as_mut_ptr()) };
        drop((strings, arrays));
        Ok(())
    }

    /// Creates the object a client asked for with a new_id argument of a
    /// request on `parent`, at the parent's version, with the same routing.
    pub fn create_resource(
        &self,
        parent: Resource,
        interface: &'static wl_interface,
        id: u32,
    ) -> Result<Resource, String> {
        let raw = self.raw(parent)?;
        let version = self.version(parent);
        // SAFETY: parent is alive, so its client is valid; the dispatcher and
        // implementation belong to this display's state.
        unsafe {
            let client = sys::wl_resource_get_client(raw);
            create_raw(
                self.inner,
                client,
                interface,
                version,
                id,
                self.dispatcher,
                self.implementation,
            )
        }
        .ok_or_else(|| format!("wl_resource_create {} failed", cstr(interface.name)))
    }

    /// Destroys the resource now. The handler hears about it through
    /// [`Handler::destroyed`] after the current callback.
    pub fn destroy(&self, res: Resource) {
        if let Ok(raw) = self.raw(res) {
            // SAFETY: alive; the destroy callback removes it from `live`.
            unsafe { sys::wl_resource_destroy(raw) };
        }
    }

    /// Sends the resource's client a fatal protocol error (`wl_display.error`,
    /// code `implementation`); the client is disconnected.
    pub fn implementation_error(&self, res: Resource, message: &str) {
        let Ok(raw) = self.raw(res) else { return };
        let msg = CString::new(message.replace('\0', " ")).expect("NULs replaced");
        // SAFETY: alive resource and its client; printf format "%s" with a
        // NUL-terminated string argument.
        unsafe {
            let client = sys::wl_resource_get_client(raw);
            sys::wl_client_post_implementation_error(client, c"%s".as_ptr(), msg.as_ptr());
        }
    }

    /// Sends the resource's client a fatal protocol error with the
    /// interface's own error `code` (`wl_display.error` naming `res`); the
    /// client is disconnected.
    pub fn post_error(&self, res: Resource, code: u32, message: &str) {
        let Ok(raw) = self.raw(res) else { return };
        let msg = CString::new(message.replace('\0', " ")).expect("NULs replaced");
        // SAFETY: alive resource; printf format "%s" with a NUL-terminated
        // string argument.
        unsafe { sys::wl_resource_post_error(raw, code, c"%s".as_ptr(), msg.as_ptr()) };
    }

    /// The next display serial (for configure events and input).
    pub fn next_serial(&self) -> u32 {
        // SAFETY: the display outlives every Ctx.
        unsafe { sys::wl_display_next_serial(self.inner.display.as_ptr()) }
    }
}

fn cstr(p: *const c_char) -> String {
    // SAFETY: table and libwayland strings are NUL-terminated.
    unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned()
}

/// Interface name of a protocol table.
pub fn interface_name(iface: &wl_interface) -> String {
    cstr(iface.name)
}

/// `interface.request` name for logs, e.g. `wl_compositor.create_surface`.
pub fn request_name(iface: &wl_interface, opcode: u32) -> String {
    let name = cstr(iface.name);
    if opcode >= iface.method_count as u32 {
        return format!("{name}.#{opcode}");
    }
    // SAFETY: opcode < method_count; the table has method_count entries.
    let msg = unsafe { &*iface.methods.add(opcode as usize) };
    format!("{name}.{}", cstr(msg.name))
}

/// A parsed message signature.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Signature {
    pub since: u32,
    /// (type code, nullable) per wire value.
    pub args: Vec<(char, bool)>,
}

pub(crate) fn parse_signature(sig: &str) -> Signature {
    let digits: String = sig.chars().take_while(char::is_ascii_digit).collect();
    let since = digits.parse().unwrap_or(1);
    let mut args = Vec::new();
    let mut nullable = false;
    for c in sig[digits.len()..].chars() {
        if c == '?' {
            nullable = true;
        } else {
            args.push((c, nullable));
            nullable = false;
        }
    }
    Signature { since, args }
}

/// # Safety
/// `client` is a live client, `implementation` is the state `dispatcher`
/// expects.
unsafe fn create_raw(
    inner: &Inner,
    client: *mut sys::wl_client,
    interface: &'static wl_interface,
    version: u32,
    id: u32,
    dispatcher: sys::wl_dispatcher_func_t,
    implementation: *const c_void,
) -> Option<Resource> {
    // SAFETY: per the function contract.
    let raw = unsafe { sys::wl_resource_create(client, interface, version as c_int, id) };
    let res = NonNull::new(raw)?;
    let data = Box::into_raw(Box::new(ResData {
        inner: inner as *const Inner,
    }));
    // SAFETY: fresh resource; data is freed by resource_destroyed.
    unsafe {
        sys::wl_resource_set_dispatcher(
            raw,
            dispatcher,
            implementation,
            data.cast(),
            resource_destroyed,
        )
    };
    inner.live.borrow_mut().insert(res, interface);
    Some(Resource(res))
}

unsafe extern "C" fn resource_destroyed(resource: *mut wl_resource) {
    // SAFETY: user data is the ResData boxed in create_raw; libwayland calls
    // this once per resource, while the display state is alive.
    unsafe {
        let data = Box::from_raw(sys::wl_resource_get_user_data(resource) as *mut ResData);
        let inner = &*data.inner;
        let res = NonNull::new_unchecked(resource);
        inner.live.borrow_mut().remove(&res);
        inner.pending_destroyed.borrow_mut().push(Resource(res));
    }
}

fn ctx_for<H: Handler>(state: &State<H>) -> Ctx<'_> {
    Ctx {
        inner: &state.inner,
        dispatcher: dispatch::<H>,
        implementation: state as *const State<H> as *const c_void,
    }
}

/// Runs a handler callback from C: no unwinding into C, then delivers
/// pending destroyed notifications.
fn from_c<H: Handler>(state: &State<H>, f: impl FnOnce(&mut H, &mut Ctx)) {
    let run = catch_unwind(AssertUnwindSafe(|| {
        f(&mut state.handler.borrow_mut(), &mut ctx_for(state));
        deliver_destroyed(state);
    }));
    if run.is_err() {
        error!(
            COMPOSITOR,
            "handler panicked inside a libwayland callback; aborting"
        );
        std::process::abort();
    }
}

fn deliver_destroyed<H: Handler>(state: &State<H>) {
    loop {
        let next = state.inner.pending_destroyed.borrow_mut().pop();
        let Some(res) = next else { break };
        state
            .handler
            .borrow_mut()
            .destroyed(&mut ctx_for(state), res);
    }
}

unsafe extern "C" fn bind_global<H: Handler>(
    client: *mut sys::wl_client,
    data: *mut c_void,
    version: u32,
    id: u32,
) {
    // SAFETY: data is the GlobalData of a global created by Display<H>,
    // whose state outlives the display.
    unsafe {
        let g = &*(data as *const GlobalData);
        let state = &*(g.state as *const State<H>);
        match create_raw(
            &state.inner,
            client,
            g.interface,
            version,
            id,
            g.dispatcher,
            g.state,
        ) {
            Some(res) => from_c(state, |h, ctx| h.bind(ctx, g.index, res)),
            None => sys::wl_client_post_no_memory(client),
        }
    }
}

unsafe extern "C" fn dispatch<H: Handler>(
    implementation: *const c_void,
    target: *mut c_void,
    opcode: u32,
    message: *const sys::wl_message,
    args: *mut wl_argument,
) -> c_int {
    // SAFETY: implementation is the State<H> set in create_raw; target is a
    // live resource; `args` holds one entry per wire value of `message`.
    unsafe {
        let state = &*(implementation as *const State<H>);
        let res = Resource(NonNull::new_unchecked(target as *mut wl_resource));
        let sig = parse_signature(&cstr((*message).signature));
        let mut decoded = Vec::with_capacity(sig.args.len());
        for (k, (code, _)) in sig.args.iter().enumerate() {
            let a = *args.add(k);
            decoded.push(match code {
                'i' => ReqArg::Int(a.i),
                'u' => ReqArg::Uint(a.u),
                'f' => ReqArg::Fixed(a.f),
                's' => ReqArg::Str((!a.s.is_null()).then(|| cstr(a.s))),
                'o' => ReqArg::Object(NonNull::new(a.o).map(Resource)),
                'n' => ReqArg::NewId(a.n),
                'a' => ReqArg::Array(if a.a.is_null() || (*a.a).size == 0 {
                    Vec::new()
                } else {
                    std::slice::from_raw_parts((*a.a).data as *const u8, (*a.a).size).to_vec()
                }),
                // The server side owns received fds (libwayland does not
                // close them after dispatch).
                'h' => ReqArg::Fd(OwnedFd::from_raw_fd(a.h)),
                other => unreachable!("signature code {other}"),
            });
        }
        let destructor = state
            .inner
            .live
            .borrow()
            .get(&res.0)
            .and_then(|i| state.inner.destructors.get(&(*i as *const wl_interface)))
            .is_some_and(|d| d.contains(&opcode));
        from_c(state, |h, ctx| {
            h.request(ctx, res, opcode, decoded);
            if destructor {
                ctx.destroy(res);
            }
        });
        0
    }
}

/// The display-lifetime "client created" listener. `listener` must stay the
/// first field: the notify callback casts the listener pointer back.
#[repr(C)]
struct CreatedListener {
    listener: sys::wl_listener,
    inner: *const Inner,
}

/// One per client, freed when the client is destroyed.
#[repr(C)]
struct DestroyListener {
    listener: sys::wl_listener,
    inner: *const Inner,
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
        (*this.inner)
            .client_events
            .borrow_mut()
            .push_back(ClientEvent::Connected(creds));
        let destroy = Box::into_raw(Box::new(DestroyListener {
            listener: sys::wl_listener {
                link: empty_link(),
                notify: client_destroyed,
            },
            inner: this.inner,
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
        (*this.inner)
            .client_events
            .borrow_mut()
            .push_back(ClientEvent::Disconnected(this.creds));
    }
}

/// A Wayland display: socket, event loop, globals, and the handler.
pub struct Display<H: Handler> {
    event_loop: NonNull<sys::wl_event_loop>,
    socket: Option<String>,
    created: *mut CreatedListener,
    // Each GlobalData is boxed on purpose: libwayland holds a pointer to
    // it, and the Vec moves its elements when it grows.
    #[allow(clippy::vec_box)]
    globals: Vec<Box<GlobalData>>,
    // Boxed so callbacks can hold its address; dropped after the display.
    state: Box<State<H>>,
}

impl<H: Handler> std::fmt::Debug for Display<H> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Display({:?}, {} globals)",
            self.socket,
            self.globals.len()
        )
    }
}

impl<H: Handler> Display<H> {
    /// Creates the display around `handler` and starts tracking clients.
    pub fn new(handler: H) -> io::Result<Display<H>> {
        // SAFETY: no arguments.
        let display = NonNull::new(unsafe { sys::wl_display_create() })
            .ok_or_else(|| io::Error::other("wl_display_create failed"))?;
        // SAFETY: display is valid; the event loop belongs to it.
        let event_loop = NonNull::new(unsafe { sys::wl_display_get_event_loop(display.as_ptr()) })
            .ok_or_else(|| io::Error::other("wl_display_get_event_loop failed"))?;
        let destructors = crate::protocols::wayland::DESTRUCTOR_TABLE
            .iter()
            .chain(crate::protocols::xdg_shell::DESTRUCTOR_TABLE.iter())
            .map(|(iface, list)| (*iface as *const wl_interface, *list))
            .collect();
        let state = Box::new(State {
            inner: Inner {
                display,
                live: RefCell::default(),
                pending_destroyed: RefCell::default(),
                client_events: RefCell::default(),
                destructors,
            },
            handler: RefCell::new(handler),
        });
        let created = Box::into_raw(Box::new(CreatedListener {
            listener: sys::wl_listener {
                link: empty_link(),
                notify: client_created,
            },
            inner: &state.inner,
        }));
        // SAFETY: display is valid; the listener outlives the display (freed
        // in Drop after wl_display_destroy).
        unsafe {
            sys::wl_display_add_client_created_listener(display.as_ptr(), &mut (*created).listener)
        };
        Ok(Display {
            event_loop,
            socket: None,
            created,
            globals: Vec::new(),
            state,
        })
    }

    fn display(&self) -> *mut sys::wl_display {
        self.state.inner.display.as_ptr()
    }

    /// Advertises a global at `version` (at most the protocol XML's).
    /// Returns its index for [`Handler::bind`].
    pub fn create_global(
        &mut self,
        interface: &'static wl_interface,
        version: u32,
    ) -> Result<usize, String> {
        let name = cstr(interface.name);
        if version == 0 || version > interface.version as u32 {
            return Err(format!(
                "{name}: version {version} not in 1..={} (protocol XML)",
                interface.version
            ));
        }
        let index = self.globals.len();
        let data = Box::new(GlobalData {
            state: &*self.state as *const State<H> as *const c_void,
            dispatcher: dispatch::<H>,
            index,
            interface,
        });
        let data_ptr = &*data as *const GlobalData as *mut c_void;
        // SAFETY: display valid; data lives in self.globals until the
        // display is destroyed.
        let g = unsafe {
            sys::wl_global_create(
                self.display(),
                interface,
                version as c_int,
                data_ptr,
                bind_global::<H>,
            )
        };
        if g.is_null() {
            return Err(format!("wl_global_create {name} failed"));
        }
        self.globals.push(data);
        Ok(index)
    }

    /// Listens on the first free `wayland-N` socket in `$XDG_RUNTIME_DIR` and
    /// returns its name (the value for clients' `WAYLAND_DISPLAY`).
    pub fn add_socket_auto(&mut self) -> io::Result<String> {
        // SAFETY: display is valid; the returned name is owned by it.
        let name = unsafe { sys::wl_display_add_socket_auto(self.display()) };
        if name.is_null() {
            return Err(io::Error::other(
                "wl_display_add_socket_auto failed (is XDG_RUNTIME_DIR set and writable?)",
            ));
        }
        let name = cstr(name);
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
            sys::wl_display_flush_clients(self.display());
            sys::wl_event_loop_dispatch(self.event_loop.as_ptr(), ms)
        };
        // Resources of disconnected clients are destroyed without a handler
        // callback running; report them now.
        deliver_destroyed(&self.state);
        // SAFETY: display valid.
        unsafe { sys::wl_display_flush_clients(self.display()) };
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
        self.state
            .inner
            .client_events
            .borrow_mut()
            .drain(..)
            .collect()
    }

    /// The handler, outside of callbacks.
    pub fn handler(&self) -> std::cell::Ref<'_, H> {
        self.state.handler.borrow()
    }

    /// Runs `f` with the handler and a [`Ctx`], outside of callbacks (for
    /// events the compositor originates, e.g. input).
    pub fn with_handler<R>(&mut self, f: impl FnOnce(&mut H, &mut Ctx) -> R) -> R {
        let r = f(
            &mut self.state.handler.borrow_mut(),
            &mut ctx_for(&self.state),
        );
        deliver_destroyed(&self.state);
        r
    }
}

impl<H: Handler> Drop for Display<H> {
    fn drop(&mut self) {
        // SAFETY: display is valid and destroyed exactly once. Destroying the
        // clients first runs their destroy listeners and resource destroy
        // callbacks while the state is alive; wl_display_destroy also
        // removes the socket and lock files and the globals.
        unsafe {
            sys::wl_display_destroy_clients(self.display());
            sys::wl_display_destroy(self.display());
            drop(Box::from_raw(self.created));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signatures_are_parsed() {
        assert_eq!(
            parse_signature("2?oih?s"),
            Signature {
                since: 2,
                args: vec![('o', true), ('i', false), ('h', false), ('s', true)]
            }
        );
        assert_eq!(parse_signature("").args.len(), 0);
        assert_eq!(parse_signature("usun").since, 1);
    }

    #[test]
    fn fixed_point_round_trips() {
        assert_eq!(fixed_from_f64(1.5), 384);
        assert_eq!(fixed_to_f64(-384), -1.5);
    }
}
