//! A layer surface on the client side (wlr-layer-shell, decision 0003):
//! create, set the state, initial commit, wait for the configure, ack.
//! Later configures (a resize) arrive as events for the owner to handle.

use crate::client::{Connection, Event, Proxy, Req, Val};
use wana_wayland::protocols::{wayland, wlr_layer_shell_unstable_v1 as proto};

pub const BACKGROUND: u32 = 0;
pub const BOTTOM: u32 = 1;
pub const TOP: u32 = 2;
pub const OVERLAY: u32 = 3;
pub const ANCHOR_TOP: u32 = 1;
pub const ANCHOR_BOTTOM: u32 = 2;
pub const ANCHOR_LEFT: u32 = 4;
pub const ANCHOR_RIGHT: u32 = 8;
pub const KEYBOARD_NONE: u32 = 0;
pub const KEYBOARD_EXCLUSIVE: u32 = 1;
pub const KEYBOARD_ON_DEMAND: u32 = 2;

/// What to ask for.
#[derive(Debug, Clone, Copy)]
pub struct Spec<'a> {
    pub namespace: &'a str,
    pub layer: u32,
    pub anchor: u32,
    pub width: u32,
    pub height: u32,
    pub zone: i32,
    pub keyboard: u32,
}

/// A configured layer surface.
#[derive(Debug)]
pub struct LayerSurface {
    pub surface: Proxy,
    pub layer_surface: Proxy,
    pub width: u32,
    pub height: u32,
}

/// A configure event for `ls`: (serial, width, height).
pub fn configure_of(ev: &Event, ls: Proxy) -> Option<(u32, u32, u32)> {
    if ev.target == ls && ev.opcode == proto::zwlr_layer_surface_v1::event::CONFIGURE {
        if let [Val::Uint(s), Val::Uint(w), Val::Uint(h)] = &ev.args[..] {
            return Some((*s, *w, *h));
        }
    }
    None
}

/// True if `ev` closes `ls` (the compositor will not show it anymore).
pub fn closed(ev: &Event, ls: Proxy) -> bool {
    ev.target == ls && ev.opcode == proto::zwlr_layer_surface_v1::event::CLOSED
}

pub fn ack(conn: &Connection, ls: Proxy, serial: u32) -> Result<(), String> {
    conn.request(
        ls,
        proto::zwlr_layer_surface_v1::request::ACK_CONFIGURE,
        None,
        &[Req::Uint(serial)],
    )
    .map(|_| ())
}

impl LayerSurface {
    /// Creates the surface, commits its state and waits for the first
    /// configure (acked). Other events that arrive meanwhile are returned
    /// to the caller in order.
    pub fn new(
        conn: &Connection,
        compositor: Proxy,
        layer_shell: Proxy,
        spec: Spec,
        other: &mut Vec<Event>,
    ) -> Result<LayerSurface, String> {
        let surface = conn
            .request(
                compositor,
                wayland::wl_compositor::request::CREATE_SURFACE,
                Some((&wayland::WL_SURFACE_INTERFACE, 4)),
                &[Req::NewId],
            )?
            .ok_or("create_surface returned no object")?;
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
            .ok_or("get_layer_surface returned no object")?;
        use proto::zwlr_layer_surface_v1::request::*;
        conn.request(
            ls,
            SET_SIZE,
            None,
            &[Req::Uint(spec.width), Req::Uint(spec.height)],
        )?;
        conn.request(ls, SET_ANCHOR, None, &[Req::Uint(spec.anchor)])?;
        conn.request(ls, SET_EXCLUSIVE_ZONE, None, &[Req::Int(spec.zone)])?;
        conn.request(
            ls,
            SET_KEYBOARD_INTERACTIVITY,
            None,
            &[Req::Uint(spec.keyboard)],
        )?;
        conn.request(surface, wayland::wl_surface::request::COMMIT, None, &[])?;
        loop {
            conn.dispatch()?;
            while let Some(ev) = conn.next_event() {
                if let Some((serial, w, h)) = configure_of(&ev, ls) {
                    ack(conn, ls, serial)?;
                    // Anything queued after the configure stays for later.
                    while let Some(rest) = conn.next_event() {
                        other.push(rest);
                    }
                    return Ok(LayerSurface {
                        surface,
                        layer_surface: ls,
                        width: w,
                        height: h,
                    });
                }
                if closed(&ev, ls) {
                    return Err(format!(
                        "layer {:?} closed before configure",
                        spec.namespace
                    ));
                }
                other.push(ev);
            }
        }
    }
}
