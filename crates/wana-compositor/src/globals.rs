//! The globals Wana advertises and what their objects do.
//!
//! | Global | Version | Now | Later |
//! |---|---|---|---|
//! | `wl_compositor` | 4 | surfaces (attach, damage, frame, commit, scale 1), regions | offsets, transforms |
//! | `wl_shm` | 1 | pools and buffers, ARGB8888 / XRGB8888 | |
//! | `wl_output` | 4 | geometry, mode, scale, name, description, done from DRM | |
//! | `wl_seat` | 7 | `seat0`: pointer (enter/leave/motion/button/axis/frame, set_cursor) and keyboard (xkb keymap, enter/leave/key/modifiers, repeat info), capabilities from the devices present (`input.rs`) | touch |
//! | `xdg_wm_base` | 1 | xdg_surface + xdg_toplevel with the configure handshake | popups, interactive move/resize |
//!
//! A version is advertised only when every request of that version is
//! handled or answered with a clear protocol error; it is never above the
//! protocol XML the tables were generated from. Requests that belong to a
//! later step end the client with an implementation error that names the
//! step, instead of being silently ignored.
//!
//! Buffers are copied at commit time (see `shm.rs`): the pixels go into a
//! GL texture (or, headless, only their size is kept) and wl_buffer.release
//! is sent right away, so client memory is never read after the commit and
//! a client can reuse its buffer immediately.

use crate::input::Keymap;
use crate::seat::{Seat, ARROW_SIZE};
use crate::shm::{BufferLayout, Pool, ShmError};
use crate::surface::{place, xdg_commit, Ack, Attach, Role, Surface, XdgCommit, XdgSurface};
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use wana_input::keyboard::Keyboard;
use wana_log::{debug, info, warn, Subsystem};
use wana_render::compose::Texture;
use wana_wayland::protocols::{wayland, xdg_shell};
use wana_wayland::server::{interface_name, request_name, Arg, Ctx, Handler, ReqArg, Resource};
use wana_wayland::sys::wl_interface;

const COMPOSITOR: Subsystem = Subsystem::Compositor;

/// wl_shm formats: ARGB8888 and XRGB8888 (the two every client may assume).
const SHM_FORMATS: [u32; 2] = [0, 1];
/// wl_output.mode flags.
const MODE_CURRENT: u32 = 0x1;
const MODE_PREFERRED: u32 = 0x2;
/// wl_output subpixel `unknown`, transform `normal`.
const SUBPIXEL_UNKNOWN: i32 = 0;
const TRANSFORM_NORMAL: i32 = 0;

/// Protocol error codes used below (from the XML enums).
mod err {
    pub const WM_BASE_ROLE: u32 = 0;
    pub const WM_BASE_INVALID_SURFACE_STATE: u32 = 4;
    pub const XDG_SURFACE_NOT_CONSTRUCTED: u32 = 1;
    pub const XDG_SURFACE_ALREADY_CONSTRUCTED: u32 = 2;
    pub const XDG_SURFACE_UNCONFIGURED_BUFFER: u32 = 3;
    pub const XDG_SURFACE_INVALID_SERIAL: u32 = 4;
    pub const WL_SURFACE_INVALID_SCALE: u32 = 0;
}

/// The display, as reported to clients through wl_output.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutputInfo {
    /// Connector name, e.g. `Virtual-1`.
    pub name: String,
    pub description: String,
    pub make: String,
    pub model: String,
    pub mm_width: i32,
    pub mm_height: i32,
    pub width: i32,
    pub height: i32,
    pub refresh_mhz: i32,
    pub preferred: bool,
}

/// One advertised global.
#[derive(Debug, Clone, Copy)]
pub struct Global {
    pub interface: &'static wl_interface,
    pub version: u32,
}

/// The advertised globals, versions capped at the protocol XML's.
pub fn globals() -> Vec<Global> {
    let cap = |i: &'static wl_interface, v: u32| Global {
        interface: i,
        version: v.min(i.version as u32),
    };
    vec![
        cap(&wayland::WL_COMPOSITOR_INTERFACE, 4),
        cap(&wayland::WL_SHM_INTERFACE, 1),
        cap(&wayland::WL_OUTPUT_INTERFACE, 4),
        cap(&wayland::WL_SEAT_INTERFACE, 7),
        cap(&xdg_shell::XDG_WM_BASE_INTERFACE, 1),
    ]
}

/// A surface's committed pixels.
#[derive(Debug)]
pub enum Content {
    /// Uploaded to the GPU (with a screen).
    Texture(Texture),
    /// Headless: only the size is kept.
    Size(i32, i32),
}

impl Content {
    fn size(&self) -> (i32, i32) {
        match self {
            Content::Texture(t) => (t.width as i32, t.height as i32),
            Content::Size(w, h) => (*w, *h),
        }
    }
}

/// A mapped toplevel, in stacking order (bottom first).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Window {
    pub surface: Resource,
    pub x: i32,
    pub y: i32,
}

#[derive(Debug, Default)]
struct Toplevel {
    xdg: Option<Resource>,
    title: String,
    app_id: String,
}

type SharedPool = Rc<RefCell<Pool>>;

/// The compositor's protocol state.
#[derive(Debug)]
pub struct Compositor {
    pub output: OutputInfo,
    pub globals: Vec<Global>,
    /// Bind count per global (for the log and tests).
    pub binds: Vec<u32>,
    /// Upload pixels to GL textures (a GL context is current).
    gpu: bool,
    pub(crate) surfaces: HashMap<Resource, Surface>,
    pub(crate) content: HashMap<Resource, Content>,
    pools: HashMap<Resource, SharedPool>,
    buffers: HashMap<Resource, (SharedPool, BufferLayout)>,
    xdg: HashMap<Resource, XdgSurface>,
    toplevels: HashMap<Resource, Toplevel>,
    regions: HashMap<Resource, ()>,
    positioners: HashMap<Resource, ()>,
    pub windows: Vec<Window>,
    /// Something visible changed, or clients wait for a frame.
    pub needs_redraw: bool,
    /// Windows mapped so far (for placement and the log).
    mapped_total: usize,
    pub(crate) seat: Seat,
    /// The compositor's keyboard state (xkb), if a keymap was compiled.
    pub(crate) xkb: Option<Keyboard>,
    pub(crate) keymap: Option<Keymap>,
    /// The default cursor (with a GPU).
    pub(crate) arrow: Option<Texture>,
}

impl Compositor {
    pub fn new(output: OutputInfo, gpu: bool) -> Compositor {
        let globals = globals();
        let binds = vec![0; globals.len()];
        let arrow = if gpu {
            match Texture::upload(&crate::seat::arrow(), ARROW_SIZE.0, ARROW_SIZE.1, false) {
                Ok(t) => Some(t),
                Err(e) => {
                    warn!(COMPOSITOR, "cursor texture: {e}");
                    None
                }
            }
        } else {
            None
        };
        let seat = Seat::new(output.width, output.height);
        Compositor {
            output,
            globals,
            binds,
            gpu,
            surfaces: HashMap::new(),
            content: HashMap::new(),
            pools: HashMap::new(),
            buffers: HashMap::new(),
            xdg: HashMap::new(),
            toplevels: HashMap::new(),
            regions: HashMap::new(),
            positioners: HashMap::new(),
            windows: Vec::new(),
            needs_redraw: true,
            mapped_total: 0,
            seat,
            xkb: None,
            keymap: None,
            arrow,
        }
    }

    /// Textures of the mapped windows, bottom to top, with positions, then
    /// the cursor.
    pub fn scene(&self) -> Vec<(&Texture, i32, i32)> {
        self.windows
            .iter()
            .filter_map(|w| match self.content.get(&w.surface) {
                Some(Content::Texture(t)) => Some((t, w.x, w.y)),
                _ => None,
            })
            .chain(self.cursor_image())
            .collect()
    }

    /// `"title" (app_id)` of the window on `surface`, for the log.
    pub(crate) fn title_of(&self, surface: Resource) -> String {
        let Some(Role::Xdg(xs)) = self.surfaces.get(&surface).map(|s| s.role) else {
            return format!("surface {surface:?}");
        };
        self.xdg
            .get(&xs)
            .and_then(|x| x.toplevel)
            .and_then(|t| self.toplevels.get(&t))
            .map(|t| format!("{:?} ({})", t.title, t.app_id))
            .unwrap_or_else(|| format!("surface {surface:?}"))
    }

    /// A frame reached the screen at `time_ms`: fire the frame callbacks
    /// committed before it.
    pub fn presented(&mut self, ctx: &mut Ctx, time_ms: u32) {
        let mut done = 0;
        for s in self.surfaces.values_mut() {
            for cb in s.frames.drain(..) {
                if ctx
                    .post(cb, wayland::wl_callback::event::DONE, &[Arg::Uint(time_ms)])
                    .is_ok()
                {
                    done += 1;
                }
                ctx.destroy(cb);
            }
        }
        if done > 0 {
            debug!(COMPOSITOR, "frame presented: {done} frame callback(s) done");
        }
    }

    fn send_output(&self, ctx: &Ctx, res: Resource) -> Result<(), String> {
        use wayland::wl_output::event::*;
        let o = &self.output;
        let v = ctx.version(res);
        ctx.post(
            res,
            GEOMETRY,
            &[
                Arg::Int(0),
                Arg::Int(0),
                Arg::Int(o.mm_width),
                Arg::Int(o.mm_height),
                Arg::Int(SUBPIXEL_UNKNOWN),
                Arg::Str(Some(&o.make)),
                Arg::Str(Some(&o.model)),
                Arg::Int(TRANSFORM_NORMAL),
            ],
        )?;
        let flags = MODE_CURRENT | if o.preferred { MODE_PREFERRED } else { 0 };
        ctx.post(
            res,
            MODE,
            &[
                Arg::Uint(flags),
                Arg::Int(o.width),
                Arg::Int(o.height),
                Arg::Int(o.refresh_mhz),
            ],
        )?;
        if v >= 2 {
            ctx.post(res, SCALE, &[Arg::Int(1)])?;
        }
        if v >= 4 {
            ctx.post(res, NAME, &[Arg::Str(Some(&o.name))])?;
            ctx.post(res, DESCRIPTION, &[Arg::Str(Some(&o.description))])?;
        }
        if v >= 2 {
            ctx.post(res, DONE, &[])?;
        }
        Ok(())
    }

    fn send_initial(
        &self,
        ctx: &Ctx,
        iface: &'static wl_interface,
        res: Resource,
    ) -> Result<(), String> {
        if std::ptr::eq(iface, &wayland::WL_SHM_INTERFACE) {
            for f in SHM_FORMATS {
                ctx.post(res, wayland::wl_shm::event::FORMAT, &[Arg::Uint(f)])?;
            }
        } else if std::ptr::eq(iface, &wayland::WL_OUTPUT_INTERFACE) {
            self.send_output(ctx, res)?;
        } else if std::ptr::eq(iface, &wayland::WL_SEAT_INTERFACE) {
            ctx.post(
                res,
                wayland::wl_seat::event::CAPABILITIES,
                &[Arg::Uint(self.seat.caps)],
            )?;
            if ctx.version(res) >= 2 {
                ctx.post(
                    res,
                    wayland::wl_seat::event::NAME,
                    &[Arg::Str(Some("seat0"))],
                )?;
            }
        }
        Ok(())
    }

    /// Creates a child object; on failure the client gets no_memory-like
    /// treatment through an implementation error.
    pub(crate) fn create(
        &mut self,
        ctx: &Ctx,
        parent: Resource,
        iface: &'static wl_interface,
        id: u32,
    ) -> Option<Resource> {
        match ctx.create_resource(parent, iface, id) {
            Ok(r) => Some(r),
            Err(e) => {
                warn!(COMPOSITOR, "{e}");
                ctx.implementation_error(parent, &e);
                None
            }
        }
    }

    // --- wl_compositor / wl_region --------------------------------------
    fn compositor_request(&mut self, ctx: &Ctx, res: Resource, opcode: u32, args: &[ReqArg]) {
        use wayland::wl_compositor::request::*;
        let Some(id) = new_id(args, 0) else { return };
        match opcode {
            CREATE_SURFACE => {
                if let Some(s) = self.create(ctx, res, &wayland::WL_SURFACE_INTERFACE, id) {
                    self.surfaces.insert(s, Surface::default());
                }
            }
            CREATE_REGION => {
                if let Some(r) = self.create(ctx, res, &wayland::WL_REGION_INTERFACE, id) {
                    // Opaque and input regions are optimizations and input
                    // shapes; this compositor draws and hit-tests whole
                    // surfaces for now.
                    self.regions.insert(r, ());
                }
            }
            _ => {}
        }
    }

    // --- wl_shm / wl_shm_pool --------------------------------------------
    fn shm_request(&mut self, ctx: &Ctx, res: Resource, args: Vec<ReqArg>) {
        // create_pool(new_id, fd, size)
        let mut it = args.into_iter();
        let (Some(ReqArg::NewId(id)), Some(ReqArg::Fd(fd)), Some(ReqArg::Int(size))) =
            (it.next(), it.next(), it.next())
        else {
            return;
        };
        match Pool::new(fd, size) {
            Ok(pool) => {
                if let Some(p) = self.create(ctx, res, &wayland::WL_SHM_POOL_INTERFACE, id) {
                    debug!(
                        COMPOSITOR,
                        "shm pool {} bytes (object {})",
                        pool.size(),
                        ctx.id(p)
                    );
                    self.pools.insert(p, Rc::new(RefCell::new(pool)));
                }
            }
            Err(e) => shm_error(ctx, res, &e),
        }
    }

    fn pool_request(&mut self, ctx: &Ctx, res: Resource, opcode: u32, args: &[ReqArg]) {
        use wayland::wl_shm_pool::request::*;
        let Some(pool) = self.pools.get(&res).cloned() else {
            return;
        };
        match opcode {
            CREATE_BUFFER => {
                let (Some(id), Some(offset), Some(width), Some(height), Some(stride), Some(format)) = (
                    new_id(args, 0),
                    int(args, 1),
                    int(args, 2),
                    int(args, 3),
                    int(args, 4),
                    uint(args, 5),
                ) else {
                    return;
                };
                let layout = BufferLayout {
                    offset,
                    width,
                    height,
                    stride,
                    format,
                };
                if let Err(e) = layout.check(pool.borrow().size()) {
                    shm_error(ctx, res, &e);
                    return;
                }
                if let Some(b) = self.create(ctx, res, &wayland::WL_BUFFER_INTERFACE, id) {
                    self.buffers.insert(b, (pool, layout));
                }
            }
            RESIZE => {
                if let Some(size) = int(args, 0) {
                    if let Err(e) = pool.borrow_mut().resize(size) {
                        shm_error(ctx, res, &e);
                    }
                }
            }
            _ => {} // destroy: applied by the protocol layer
        }
    }

    // --- wl_surface -----------------------------------------------------------
    fn surface_request(&mut self, ctx: &Ctx, res: Resource, opcode: u32, args: &[ReqArg]) {
        use wayland::wl_surface::request::*;
        match opcode {
            ATTACH => {
                let attach = match args.first() {
                    Some(ReqArg::Object(Some(b))) if self.buffers.contains_key(b) => Attach::Buffer(*b),
                    Some(ReqArg::Object(Some(_))) => {
                        ctx.implementation_error(res, "wl_surface.attach: not a wl_shm buffer");
                        return;
                    }
                    _ => Attach::Remove,
                };
                if let Some(s) = self.surfaces.get_mut(&res) {
                    s.attach = attach;
                }
            }
            FRAME => {
                let Some(id) = new_id(args, 0) else { return };
                if let Some(cb) = self.create(ctx, res, &wayland::WL_CALLBACK_INTERFACE, id) {
                    if let Some(s) = self.surfaces.get_mut(&res) {
                        s.pending_frames.push(cb);
                    }
                }
            }
            SET_BUFFER_TRANSFORM => {
                if int(args, 0) != Some(0) {
                    ctx.implementation_error(res, "wl_surface.set_buffer_transform: only normal (0) is supported yet");
                }
            }
            SET_BUFFER_SCALE => match int(args, 0) {
                Some(scale) if scale < 1 => {
                    ctx.post_error(res, err::WL_SURFACE_INVALID_SCALE, &format!("buffer scale {scale} < 1"));
                }
                Some(1) => {
                    if let Some(s) = self.surfaces.get_mut(&res) {
                        s.pending_scale = 1;
                    }
                }
                _ => ctx.implementation_error(
                    res,
                    "wl_surface.set_buffer_scale: only scale 1 is supported yet (the output advertises scale 1)",
                ),
            },
            COMMIT => self.commit(ctx, res),
            // damage, damage_buffer: whole frames are redrawn; opaque and
            // input regions are not used yet; destroy: protocol layer.
            _ => {}
        }
    }

    fn commit(&mut self, ctx: &Ctx, res: Resource) {
        let Some(s) = self.surfaces.get_mut(&res) else {
            return;
        };
        let attach = std::mem::replace(&mut s.attach, Attach::Unchanged);
        if let Role::Xdg(xs) = s.role {
            let Some(xdg) = self.xdg.get_mut(&xs) else {
                return;
            };
            match xdg_commit(xdg, attach) {
                XdgCommit::UnconfiguredBuffer => {
                    ctx.post_error(
                        xs,
                        err::XDG_SURFACE_UNCONFIGURED_BUFFER,
                        "buffer attached before the first configure was acknowledged",
                    );
                    return;
                }
                XdgCommit::NotConstructed => {
                    ctx.post_error(
                        xs,
                        err::XDG_SURFACE_NOT_CONSTRUCTED,
                        "commit on an xdg_surface without a role object (get_toplevel first)",
                    );
                    return;
                }
                XdgCommit::SendInitialConfigure => {
                    if let Some(tl) = xdg.toplevel {
                        let serial = ctx.next_serial();
                        let ok = ctx
                            .post(
                                tl,
                                xdg_shell::xdg_toplevel::event::CONFIGURE,
                                &[Arg::Int(0), Arg::Int(0), Arg::Array(&[])],
                            )
                            .and_then(|_| {
                                ctx.post(
                                    xs,
                                    xdg_shell::xdg_surface::event::CONFIGURE,
                                    &[Arg::Uint(serial)],
                                )
                            });
                        match ok {
                            Ok(()) => {
                                xdg.sent(serial);
                                debug!(
                                    COMPOSITOR,
                                    "xdg_surface@{}: configure serial {serial}",
                                    ctx.id(xs)
                                );
                            }
                            Err(e) => warn!(COMPOSITOR, "configure: {e}"),
                        }
                    }
                }
                XdgCommit::Apply => {}
            }
        }

        match attach {
            Attach::Buffer(b) => {
                if !self.apply_buffer(ctx, res, b) {
                    return;
                }
            }
            Attach::Remove => {
                self.content.remove(&res);
                if let Some(s) = self.surfaces.get_mut(&res) {
                    s.content = None;
                }
            }
            Attach::Unchanged => {}
        }
        let Some(s) = self.surfaces.get_mut(&res) else {
            return;
        };
        let frames = std::mem::take(&mut s.pending_frames);
        if !frames.is_empty() {
            s.frames.extend(frames);
            self.needs_redraw = true;
        }
        s.scale = s.pending_scale;
        self.update_mapping(ctx, res);
    }

    /// Copies the buffer's pixels (texture or size), releases the buffer.
    /// Returns false if the client was sent a protocol error.
    fn apply_buffer(&mut self, ctx: &Ctx, res: Resource, b: Resource) -> bool {
        let Some((pool, layout)) = self.buffers.get(&b).cloned() else {
            // Destroyed after attach: treat as no buffer.
            self.content.remove(&res);
            return true;
        };
        let content = if self.gpu {
            let pixels = match pool.borrow_mut().read(&layout) {
                Ok(p) => p,
                Err(e) => {
                    shm_error(ctx, b, &e);
                    return false;
                }
            };
            match Texture::upload(
                &pixels,
                layout.width as u32,
                layout.height as u32,
                layout.opaque(),
            ) {
                Ok(t) => Content::Texture(t),
                Err(e) => {
                    warn!(COMPOSITOR, "texture upload: {e}");
                    ctx.implementation_error(res, &format!("texture upload failed: {e}"));
                    return false;
                }
            }
        } else {
            // Headless: still read the pixels, so a truncated pool is
            // caught the same way.
            if let Err(e) = pool.borrow_mut().read(&layout) {
                shm_error(ctx, b, &e);
                return false;
            }
            Content::Size(layout.width, layout.height)
        };
        let (w, h) = content.size();
        self.content.insert(res, content);
        if let Some(s) = self.surfaces.get_mut(&res) {
            s.content = Some((w, h));
            s.had_buffer = true;
        }
        // The pixels are copied: the client may reuse the buffer now.
        let _ = ctx.post(b, wayland::wl_buffer::event::RELEASE, &[]);
        self.needs_redraw = true;
        true
    }

    /// Maps or unmaps the toplevel of `surface` after a commit.
    fn update_mapping(&mut self, ctx: &Ctx, surface: Resource) {
        let Some(s) = self.surfaces.get(&surface) else {
            return;
        };
        let Role::Xdg(xs) = s.role else { return };
        let Some(xdg) = self.xdg.get(&xs) else { return };
        let visible = xdg.toplevel.is_some() && xdg.acked.is_some() && s.content.is_some();
        let index = self.windows.iter().position(|w| w.surface == surface);
        match (visible, index) {
            (true, None) => {
                let (w, h) = s.content.unwrap_or((0, 0));
                let (x, y) = place(
                    self.mapped_total,
                    self.output.width,
                    self.output.height,
                    w,
                    h,
                );
                self.mapped_total += 1;
                self.windows.push(Window { surface, x, y });
                let title = self.title_of(surface);
                info!(
                    COMPOSITOR,
                    "window mapped: {title} {w}x{h} at {x},{y} (surface {})",
                    ctx.id(surface)
                );
                // A new window gets the keyboard (applied by sync_focus).
                self.seat.focus_request = Some(surface);
                self.needs_redraw = true;
            }
            (false, Some(i)) => {
                self.windows.remove(i);
                info!(COMPOSITOR, "window unmapped (surface {})", ctx.id(surface));
                self.needs_redraw = true;
            }
            _ => {}
        }
    }

    // --- xdg_wm_base / xdg_surface / xdg_toplevel ---------------------------
    fn wm_base_request(&mut self, ctx: &Ctx, res: Resource, opcode: u32, args: &[ReqArg]) {
        use xdg_shell::xdg_wm_base::request::*;
        match opcode {
            CREATE_POSITIONER => {
                if let Some(id) = new_id(args, 0) {
                    if let Some(p) = self.create(ctx, res, &xdg_shell::XDG_POSITIONER_INTERFACE, id)
                    {
                        self.positioners.insert(p, ());
                    }
                }
            }
            GET_XDG_SURFACE => {
                let (Some(id), Some(ReqArg::Object(Some(surface)))) =
                    (new_id(args, 0), args.get(1))
                else {
                    return;
                };
                let Some(s) = self.surfaces.get(surface) else {
                    ctx.implementation_error(res, "get_xdg_surface: not a wl_surface");
                    return;
                };
                if s.role != Role::None {
                    ctx.post_error(res, err::WM_BASE_ROLE, "wl_surface already has a role");
                    return;
                }
                if s.had_buffer || matches!(s.attach, Attach::Buffer(_)) {
                    ctx.post_error(
                        res,
                        err::WM_BASE_INVALID_SURFACE_STATE,
                        "wl_surface has a buffer attached or committed",
                    );
                    return;
                }
                if let Some(xs) = self.create(ctx, res, &xdg_shell::XDG_SURFACE_INTERFACE, id) {
                    self.xdg.insert(
                        xs,
                        XdgSurface {
                            surface: Some(*surface),
                            ..Default::default()
                        },
                    );
                    if let Some(s) = self.surfaces.get_mut(surface) {
                        s.role = Role::Xdg(xs);
                    }
                }
            }
            _ => {} // pong: nothing is pinged yet; destroy: protocol layer
        }
    }

    fn xdg_surface_request(&mut self, ctx: &Ctx, res: Resource, opcode: u32, args: &[ReqArg]) {
        use xdg_shell::xdg_surface::request::*;
        match opcode {
            GET_TOPLEVEL => {
                let Some(id) = new_id(args, 0) else { return };
                if self.xdg.get(&res).is_some_and(|x| x.toplevel.is_some()) {
                    ctx.post_error(
                        res,
                        err::XDG_SURFACE_ALREADY_CONSTRUCTED,
                        "xdg_surface already has a role object",
                    );
                    return;
                }
                if let Some(tl) = self.create(ctx, res, &xdg_shell::XDG_TOPLEVEL_INTERFACE, id) {
                    self.toplevels.insert(
                        tl,
                        Toplevel {
                            xdg: Some(res),
                            ..Default::default()
                        },
                    );
                    if let Some(x) = self.xdg.get_mut(&res) {
                        x.toplevel = Some(tl);
                    }
                }
            }
            GET_POPUP => ctx
                .implementation_error(res, "xdg_surface.get_popup: popups arrive in a later step"),
            ACK_CONFIGURE => {
                let Some(serial) = uint(args, 0) else { return };
                if let Some(x) = self.xdg.get_mut(&res) {
                    if x.ack(serial) == Ack::Invalid {
                        ctx.post_error(
                            res,
                            err::XDG_SURFACE_INVALID_SERIAL,
                            &format!("ack_configure({serial}): no such configure"),
                        );
                    }
                }
            }
            _ => {} // set_window_geometry: whole surface for now; destroy
        }
    }

    fn toplevel_request(&mut self, res: Resource, opcode: u32, args: &[ReqArg]) {
        use xdg_shell::xdg_toplevel::request::*;
        let Some(t) = self.toplevels.get_mut(&res) else {
            return;
        };
        match (opcode, args.first()) {
            (SET_TITLE, Some(ReqArg::Str(Some(s)))) => t.title = s.clone(),
            (SET_APP_ID, Some(ReqArg::Str(Some(s)))) => t.app_id = s.clone(),
            // Parent, menus, interactive move/resize, size limits,
            // maximize/fullscreen/minimize: the compositor may ignore them;
            // window management arrives in Phase 12.
            _ => {}
        }
    }
}

/// wl_shm errors go to the object the request came through.
fn shm_error(ctx: &Ctx, res: Resource, e: &ShmError) {
    warn!(COMPOSITOR, "shm error {}: {}", e.code(), e.message());
    ctx.post_error(res, e.code(), e.message());
}

fn new_id(args: &[ReqArg], i: usize) -> Option<u32> {
    match args.get(i) {
        Some(ReqArg::NewId(id)) => Some(*id),
        _ => None,
    }
}

fn int(args: &[ReqArg], i: usize) -> Option<i32> {
    match args.get(i) {
        Some(ReqArg::Int(v)) => Some(*v),
        _ => None,
    }
}

fn uint(args: &[ReqArg], i: usize) -> Option<u32> {
    match args.get(i) {
        Some(ReqArg::Uint(v)) => Some(*v),
        _ => None,
    }
}

impl Handler for Compositor {
    fn bind(&mut self, ctx: &mut Ctx, global: usize, res: Resource) {
        let g = self.globals[global];
        self.binds[global] += 1;
        info!(
            COMPOSITOR,
            "bound {} version {} (object {})",
            interface_name(g.interface),
            ctx.version(res),
            ctx.id(res)
        );
        if std::ptr::eq(g.interface, &wayland::WL_SEAT_INTERFACE) {
            self.seat.seats.push(res);
        }
        if let Err(e) = self.send_initial(ctx, g.interface, res) {
            warn!(COMPOSITOR, "{}: {e}", interface_name(g.interface));
        }
    }

    fn request(&mut self, ctx: &mut Ctx, res: Resource, opcode: u32, args: Vec<ReqArg>) {
        let Some(iface) = ctx.interface(res) else {
            return;
        };
        debug!(
            COMPOSITOR,
            "{} (object {})",
            request_name(iface, opcode),
            ctx.id(res)
        );
        let is = |i: &wl_interface| std::ptr::eq(iface, i);
        if is(&wayland::WL_COMPOSITOR_INTERFACE) {
            self.compositor_request(ctx, res, opcode, &args);
        } else if is(&wayland::WL_SURFACE_INTERFACE) {
            self.surface_request(ctx, res, opcode, &args);
        } else if is(&wayland::WL_SHM_INTERFACE) {
            self.shm_request(ctx, res, args);
        } else if is(&wayland::WL_SHM_POOL_INTERFACE) {
            self.pool_request(ctx, res, opcode, &args);
        } else if is(&xdg_shell::XDG_WM_BASE_INTERFACE) {
            self.wm_base_request(ctx, res, opcode, &args);
        } else if is(&xdg_shell::XDG_SURFACE_INTERFACE) {
            self.xdg_surface_request(ctx, res, opcode, &args);
        } else if is(&xdg_shell::XDG_TOPLEVEL_INTERFACE) {
            self.toplevel_request(res, opcode, &args);
        } else if is(&wayland::WL_SEAT_INTERFACE) {
            self.seat_request(ctx, res, opcode, &args);
        } else if is(&wayland::WL_POINTER_INTERFACE) {
            self.pointer_request(ctx, res, opcode, &args);
        }
        // wl_buffer, wl_region, wl_output, xdg_positioner, wl_keyboard:
        // only destructors or requests without effect here; destructors are
        // applied by the protocol layer.
    }

    fn destroyed(&mut self, ctx: &mut Ctx, res: Resource) {
        if self.seat.forget(res) {
            self.needs_redraw = true;
        }
        if let Some(s) = self.surfaces.remove(&res) {
            for cb in s.pending_frames.into_iter().chain(s.frames) {
                ctx.destroy(cb);
            }
            self.content.remove(&res);
            if let Some(i) = self.windows.iter().position(|w| w.surface == res) {
                self.windows.remove(i);
                info!(COMPOSITOR, "window destroyed (surface {res:?})");
                self.needs_redraw = true;
            }
            if let Role::Xdg(xs) = s.role {
                if let Some(x) = self.xdg.get_mut(&xs) {
                    x.surface = None;
                }
            }
        } else if self.buffers.remove(&res).is_some() {
            // A pending attach of this buffer becomes "no buffer" at commit.
        } else if self.pools.remove(&res).is_some() {
            // Buffers keep their pool mapped until they are destroyed.
        } else if let Some(x) = self.xdg.remove(&res) {
            if let Some(surface) = x.surface {
                if let Some(s) = self.surfaces.get_mut(&surface) {
                    s.role = Role::None;
                }
                if let Some(i) = self.windows.iter().position(|w| w.surface == surface) {
                    self.windows.remove(i);
                    self.needs_redraw = true;
                }
            }
        } else if let Some(t) = self.toplevels.remove(&res) {
            if let Some(xs) = t.xdg {
                if let Some(x) = self.xdg.get_mut(&xs) {
                    x.toplevel = None;
                    if let Some(surface) = x.surface {
                        if let Some(i) = self.windows.iter().position(|w| w.surface == surface) {
                            self.windows.remove(i);
                            info!(COMPOSITOR, "window closed by client: {:?}", t.title);
                            self.needs_redraw = true;
                        }
                    }
                }
            }
        } else {
            self.regions.remove(&res);
            self.positioners.remove(&res);
            for s in self.surfaces.values_mut() {
                s.frames.retain(|c| *c != res);
                s.pending_frames.retain(|c| *c != res);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_are_capped_by_the_protocol_xml() {
        let g = globals();
        assert_eq!(g.len(), 5);
        for x in &g {
            assert!(x.version >= 1 && x.version <= x.interface.version as u32);
        }
        assert_eq!(interface_name(g[2].interface), "wl_output");
        assert_eq!(g[2].version, 4, "name/description need wl_output v4");
    }

    #[test]
    fn argument_helpers_check_kinds() {
        let args = vec![ReqArg::NewId(5), ReqArg::Int(-3), ReqArg::Uint(7)];
        assert_eq!(new_id(&args, 0), Some(5));
        assert_eq!(int(&args, 1), Some(-3));
        assert_eq!(uint(&args, 2), Some(7));
        assert_eq!(int(&args, 0), None, "wrong kind");
        assert_eq!(uint(&args, 9), None, "missing");
    }
}
