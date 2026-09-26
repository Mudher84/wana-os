//! `--layers` (run as the shell): layer surfaces through wlr-layer-shell.
//! A full-screen background (zone -1) and a top bar with an exclusive zone,
//! each through the configure handshake; the ordinary window mapped after
//! them must be placed below the bar.
//! `--layer-invalid-size`: a width of 0 without left and right anchors,
//! which must end in zwlr_layer_surface_v1 error 1 (invalid_size).

use crate::client::{Connection, Proxy, Req, Val};
use crate::window::{pattern, Shell, Window};
use crate::{wait_for, LOG};
use wana_log::info;
use wana_wayland::protocols::{wayland, wlr_layer_shell_unstable_v1 as proto};

pub const BACKGROUND: u32 = 0;
pub const TOP: u32 = 2;
pub const ANCHOR_ALL: u32 = 15;
/// Top, left and right.
pub const ANCHOR_TOP_BAR: u32 = 1 | 4 | 8;
pub const DESKTOP_RGB: u32 = 0x1B3A5C;
pub const BAR_RGB: u32 = 0x0B0F1A;
pub const BAR_HEIGHT: u32 = 40;

/// Describes one layer surface to create.
#[derive(Debug, Clone, Copy)]
pub struct Spec {
    pub namespace: &'static str,
    pub layer: u32,
    pub anchor: u32,
    pub width: u32,
    pub height: u32,
    pub zone: i32,
    pub rgb: u32,
}

/// Creates a layer surface, does the configure handshake, and maps it with
/// a solid buffer of the configured size.
pub fn map(conn: &Connection, shell: &Shell, layer_shell: Proxy, spec: Spec) -> Result<(), String> {
    let surface = conn
        .request(
            shell.compositor,
            wayland::wl_compositor::request::CREATE_SURFACE,
            Some((&wayland::WL_SURFACE_INTERFACE, 4)),
            &[Req::NewId],
        )?
        .expect("surface");
    let ls = conn
        .request(
            layer_shell,
            proto::zwlr_layer_shell_v1::request::GET_LAYER_SURFACE,
            Some((&proto::ZWLR_LAYER_SURFACE_V1_INTERFACE, 4)),
            &[
                Req::NewId,
                Req::Object(Some(surface)),
                Req::Object(None),
                Req::Uint(spec.layer),
                Req::Str(spec.namespace),
            ],
        )?
        .expect("layer surface");
    use proto::zwlr_layer_surface_v1::request::*;
    conn.request(
        ls,
        SET_SIZE,
        None,
        &[Req::Uint(spec.width), Req::Uint(spec.height)],
    )?;
    conn.request(ls, SET_ANCHOR, None, &[Req::Uint(spec.anchor)])?;
    conn.request(ls, SET_EXCLUSIVE_ZONE, None, &[Req::Int(spec.zone)])?;
    conn.request(surface, wayland::wl_surface::request::COMMIT, None, &[])?;
    let (serial, w, h) = wait_for(conn, shell.wm_base, None, |ev| {
        if ev.target == ls && ev.opcode == proto::zwlr_layer_surface_v1::event::CONFIGURE {
            if let [Val::Uint(s), Val::Uint(w), Val::Uint(h)] = &ev.args[..] {
                return Some((*s, *w, *h));
            }
        }
        None
    })?;
    info!(
        LOG,
        "client: layer {:?} configured {w}x{h} (serial {serial})", spec.namespace
    );
    conn.request(ls, ACK_CONFIGURE, None, &[Req::Uint(serial)])?;
    let (wi, hi) = (w as i32, h as i32);
    let (buffer, _file) =
        Window::buffer(conn, shell, wi, hi, &pattern(wi, hi, spec.rgb, spec.rgb))?;
    conn.request(
        surface,
        wayland::wl_surface::request::ATTACH,
        None,
        &[Req::Object(Some(buffer)), Req::Int(0), Req::Int(0)],
    )?;
    conn.request(
        surface,
        wayland::wl_surface::request::DAMAGE,
        None,
        &[Req::Int(0), Req::Int(0), Req::Int(wi), Req::Int(hi)],
    )?;
    conn.request(surface, wayland::wl_surface::request::COMMIT, None, &[])?;
    // The objects live until the connection ends.
    conn.roundtrip()
}

/// A width of 0 without left and right anchors is a protocol error.
pub fn invalid_size(conn: &Connection, shell: &Shell, layer_shell: Proxy) -> Result<(), String> {
    let surface = conn
        .request(
            shell.compositor,
            wayland::wl_compositor::request::CREATE_SURFACE,
            Some((&wayland::WL_SURFACE_INTERFACE, 4)),
            &[Req::NewId],
        )?
        .expect("surface");
    let ls = conn
        .request(
            layer_shell,
            proto::zwlr_layer_shell_v1::request::GET_LAYER_SURFACE,
            Some((&proto::ZWLR_LAYER_SURFACE_V1_INTERFACE, 4)),
            &[
                Req::NewId,
                Req::Object(Some(surface)),
                Req::Object(None),
                Req::Uint(TOP),
                Req::Str("broken"),
            ],
        )?
        .expect("layer surface");
    use proto::zwlr_layer_surface_v1::request::*;
    conn.request(ls, SET_SIZE, None, &[Req::Uint(0), Req::Uint(40)])?;
    conn.request(ls, SET_ANCHOR, None, &[Req::Uint(1)])?;
    info!(
        LOG,
        "client: committing a layer surface with width 0 anchored only to the top (protocol violation on purpose)"
    );
    conn.request(surface, wayland::wl_surface::request::COMMIT, None, &[])?;
    crate::expect_error(conn, "zwlr_layer_surface_v1", 1)
}
