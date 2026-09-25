//! The globals Wana advertises and what their objects do (Phase 10 step 2).
//!
//! | Global | Version | Now | Later |
//! |---|---|---|---|
//! | `wl_compositor` | 4 | bind | surfaces and regions: step 3 |
//! | `wl_shm` | 1 | formats ARGB8888, XRGB8888 | pools and buffers: step 3 |
//! | `wl_output` | 4 | geometry, mode, scale, name, description, done from DRM | |
//! | `wl_seat` | 7 | name `seat0`, no capabilities yet | pointer and keyboard from wana-input: step 4 |
//! | `xdg_wm_base` | 1 | ping/pong, destroy | xdg_surface, toplevel: step 3 |
//!
//! A version is advertised only when every request of that version is
//! handled or answered with a clear protocol error; it is never above the
//! protocol XML the tables were generated from. Requests that belong to a
//! later step end the client with an implementation error that names the
//! step, instead of being silently ignored.

use wana_log::{debug, info, warn, Subsystem};
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

/// The globals of step 2, versions capped at the protocol XML's.
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

/// The compositor's protocol state.
#[derive(Debug)]
pub struct Compositor {
    pub output: OutputInfo,
    pub globals: Vec<Global>,
    /// Bind count per global (for the log and tests).
    pub binds: Vec<u32>,
}

impl Compositor {
    pub fn new(output: OutputInfo) -> Compositor {
        let globals = globals();
        let binds = vec![0; globals.len()];
        Compositor {
            output,
            globals,
            binds,
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
            // No capabilities until input is routed to clients (step 4).
            ctx.post(res, wayland::wl_seat::event::CAPABILITIES, &[Arg::Uint(0)])?;
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
}

/// What to do with a request in step 2.
#[derive(Debug, PartialEq, Eq)]
enum Action {
    /// Handled (or a destructor, which the protocol layer applies).
    Done,
    /// Not implemented yet: end the client with this message.
    Later(&'static str),
    /// Not a valid request for the interface (cannot happen with the
    /// generated tables; kept as a guard).
    Unknown,
}

fn classify(iface: &'static wl_interface, opcode: u32) -> Action {
    use std::ptr::eq;
    if eq(iface, &wayland::WL_COMPOSITOR_INTERFACE) {
        return Action::Later("surfaces and regions arrive in Phase 10 step 3");
    }
    if eq(iface, &wayland::WL_SHM_INTERFACE) {
        return match opcode {
            wayland::wl_shm::request::CREATE_POOL => {
                Action::Later("shm pools arrive in Phase 10 step 3")
            }
            _ => Action::Done,
        };
    }
    if eq(iface, &wayland::WL_OUTPUT_INTERFACE) {
        return Action::Done; // release (destructor)
    }
    if eq(iface, &wayland::WL_SEAT_INTERFACE) {
        return match opcode {
            wayland::wl_seat::request::RELEASE => Action::Done,
            _ => Action::Later(
                "this seat has no capabilities yet (input arrives in Phase 10 step 4)",
            ),
        };
    }
    if eq(iface, &xdg_shell::XDG_WM_BASE_INTERFACE) {
        return match opcode {
            xdg_shell::xdg_wm_base::request::DESTROY | xdg_shell::xdg_wm_base::request::PONG => {
                Action::Done
            }
            _ => Action::Later("xdg surfaces arrive in Phase 10 step 3"),
        };
    }
    Action::Unknown
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
        if let Err(e) = self.send_initial(ctx, g.interface, res) {
            warn!(COMPOSITOR, "{}: {e}", interface_name(g.interface));
        }
    }

    fn request(&mut self, ctx: &mut Ctx, res: Resource, opcode: u32, _args: Vec<ReqArg>) {
        let Some(iface) = ctx.interface(res) else {
            return;
        };
        let req = request_name(iface, opcode);
        let id = ctx.id(res);
        match classify(iface, opcode) {
            Action::Done => debug!(COMPOSITOR, "{req} (object {id})"),
            Action::Later(why) => {
                warn!(COMPOSITOR, "{req} (object {id}) not implemented yet: {why}");
                ctx.implementation_error(res, &format!("{req}: {why}"));
            }
            Action::Unknown => ctx.implementation_error(res, &format!("{req}: unexpected")),
        }
        // Received fds (e.g. wl_shm.create_pool) are closed when `_args` drops.
    }

    fn destroyed(&mut self, ctx: &mut Ctx, res: Resource) {
        let _ = (ctx, res);
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
    fn later_requests_are_refused_with_the_step() {
        use wayland::*;
        assert_eq!(
            classify(
                &WL_COMPOSITOR_INTERFACE,
                wl_compositor::request::CREATE_SURFACE
            ),
            Action::Later("surfaces and regions arrive in Phase 10 step 3")
        );
        assert!(matches!(
            classify(&WL_SHM_INTERFACE, wl_shm::request::CREATE_POOL),
            Action::Later(_)
        ));
        assert!(matches!(
            classify(&WL_SEAT_INTERFACE, wl_seat::request::GET_POINTER),
            Action::Later(_)
        ));
        assert_eq!(
            classify(&WL_SEAT_INTERFACE, wl_seat::request::RELEASE),
            Action::Done
        );
        assert_eq!(
            classify(
                &xdg_shell::XDG_WM_BASE_INTERFACE,
                xdg_shell::xdg_wm_base::request::PONG
            ),
            Action::Done
        );
        assert_eq!(classify(&WL_REGION_INTERFACE, 0), Action::Unknown);
    }
}
