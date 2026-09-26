//! Layer surfaces on the wire (wlr-layer-shell, decision 0003): the
//! requests, the configure handshake (as for xdg: an initial commit
//! without a buffer, configure, ack, then a buffer maps the surface), the
//! arrangement after every change, and the stacking order used for drawing
//! and input. The rules themselves are in `layer.rs`.

use crate::globals::{Compositor, Content};
use crate::layer::{self, err, Rect, State};
use crate::surface::{Ack, Attach, Role, XdgSurface};
use wana_log::{info, warn, Subsystem};
use wana_wayland::protocols::wlr_layer_shell_unstable_v1 as proto;
use wana_wayland::server::{Arg, Ctx, ReqArg, Resource};

const COMPOSITOR: Subsystem = Subsystem::Compositor;

/// A layer surface and its double-buffered state.
#[derive(Debug)]
pub struct LayerSurface {
    pub surface: Option<Resource>,
    pub namespace: String,
    pub pending: State,
    pub current: State,
    /// Configure serials and acks (the same bookkeeping as xdg_surface).
    pub cfg: XdgSurface,
    pub mapped: bool,
    pub rect: Rect,
    /// Size of the last configure sent.
    pub sent: (i32, i32),
}

fn uint(args: &[ReqArg], i: usize) -> Option<u32> {
    match args.get(i) {
        Some(ReqArg::Uint(v)) => Some(*v),
        _ => None,
    }
}

fn int(args: &[ReqArg], i: usize) -> Option<i32> {
    match args.get(i) {
        Some(ReqArg::Int(v)) => Some(*v),
        _ => None,
    }
}

impl Compositor {
    pub(crate) fn output_rect(&self) -> Rect {
        Rect {
            x: 0,
            y: 0,
            w: self.output.width,
            h: self.output.height,
        }
    }

    // --- zwlr_layer_shell_v1 --------------------------------------------------
    pub(crate) fn layer_shell_request(
        &mut self,
        ctx: &Ctx,
        res: Resource,
        opcode: u32,
        args: &[ReqArg],
    ) {
        if opcode != proto::zwlr_layer_shell_v1::request::GET_LAYER_SURFACE {
            return; // destroy: protocol layer
        }
        // get_layer_surface(new_id, surface, output?, layer, namespace)
        let (Some(ReqArg::NewId(id)), Some(ReqArg::Object(Some(surface))), Some(layer_v)) =
            (args.first(), args.get(1), uint(args, 3))
        else {
            return;
        };
        let namespace = match args.get(4) {
            Some(ReqArg::Str(Some(s))) => s.clone(),
            _ => String::new(),
        };
        let Some(s) = self.surfaces.get(surface) else {
            ctx.implementation_error(res, "get_layer_surface: not a wl_surface");
            return;
        };
        if s.role != Role::None {
            ctx.post_error(res, err::SHELL_ROLE, "wl_surface already has a role");
            return;
        }
        if s.had_buffer || matches!(s.attach, Attach::Buffer(_)) {
            ctx.post_error(
                res,
                err::SHELL_ALREADY_CONSTRUCTED,
                "wl_surface has a buffer attached or committed",
            );
            return;
        }
        if !layer::valid_layer(layer_v) {
            ctx.post_error(
                res,
                err::SHELL_INVALID_LAYER,
                &format!("layer {layer_v} is not 0..3"),
            );
            return;
        }
        let Some(ls) = self.create(ctx, res, &proto::ZWLR_LAYER_SURFACE_V1_INTERFACE, *id) else {
            return;
        };
        let state = State::new(layer_v);
        self.layers.insert(
            ls,
            LayerSurface {
                surface: Some(*surface),
                namespace,
                pending: state,
                current: state,
                cfg: XdgSurface::default(),
                mapped: false,
                rect: Rect {
                    x: 0,
                    y: 0,
                    w: 0,
                    h: 0,
                },
                sent: (-1, -1),
            },
        );
        self.layer_order.push(ls);
        if let Some(s) = self.surfaces.get_mut(surface) {
            s.role = Role::Layer(ls);
        }
    }

    // --- zwlr_layer_surface_v1 ------------------------------------------------
    pub(crate) fn layer_surface_request(
        &mut self,
        ctx: &Ctx,
        res: Resource,
        opcode: u32,
        args: &[ReqArg],
    ) {
        use proto::zwlr_layer_surface_v1::request::*;
        let Some(l) = self.layers.get_mut(&res) else {
            return;
        };
        let p = &mut l.pending;
        match opcode {
            SET_SIZE => {
                if let (Some(w), Some(h)) = (uint(args, 0), uint(args, 1)) {
                    (p.width, p.height) = (w, h);
                }
            }
            SET_ANCHOR => match uint(args, 0) {
                Some(a) if layer::valid_anchor(a) => p.anchor = a,
                Some(a) => ctx.post_error(res, err::INVALID_ANCHOR, &format!("anchor {a:#x}")),
                None => {}
            },
            SET_EXCLUSIVE_ZONE => {
                if let Some(z) = int(args, 0) {
                    p.zone = z;
                }
            }
            SET_MARGIN => {
                if let (Some(t), Some(r), Some(b), Some(lf)) =
                    (int(args, 0), int(args, 1), int(args, 2), int(args, 3))
                {
                    p.margin = [t, r, b, lf];
                }
            }
            SET_KEYBOARD_INTERACTIVITY => match uint(args, 0) {
                Some(k) if layer::valid_keyboard(k) => p.keyboard = k,
                Some(k) => ctx.post_error(
                    res,
                    err::INVALID_KEYBOARD_INTERACTIVITY,
                    &format!("keyboard interactivity {k}"),
                ),
                None => {}
            },
            SET_LAYER => match uint(args, 0) {
                Some(v) if layer::valid_layer(v) => p.layer = v,
                Some(v) => ctx.post_error(
                    res,
                    err::INVALID_SURFACE_STATE,
                    &format!("layer {v} is not 0..3"),
                ),
                None => {}
            },
            GET_POPUP => ctx.implementation_error(
                res,
                "zwlr_layer_surface_v1.get_popup: popups arrive in a later step",
            ),
            ACK_CONFIGURE => {
                if let Some(serial) = uint(args, 0) {
                    if l.cfg.ack(serial) == Ack::Invalid {
                        ctx.post_error(
                            res,
                            err::INVALID_SURFACE_STATE,
                            &format!("ack_configure({serial}): no such configure"),
                        );
                    }
                }
            }
            _ => {} // destroy: protocol layer
        }
    }

    /// The layer part of a wl_surface commit, before the buffer is applied.
    /// Returns false if the client got a protocol error.
    pub(crate) fn layer_commit(&mut self, ctx: &Ctx, ls: Resource, attach: Attach) -> bool {
        let Some(l) = self.layers.get_mut(&ls) else {
            return true;
        };
        l.current = l.pending;
        if let Err((code, msg)) = l.current.validate() {
            ctx.post_error(ls, code, &msg);
            return false;
        }
        match attach {
            Attach::Remove => {
                // Unmapped: back to the state right after get_layer_surface;
                // this commit, without a buffer, asks for a new configure.
                l.cfg = XdgSurface::default();
                l.sent = (-1, -1);
                l.cfg.configured = true;
            }
            Attach::Buffer(_) if l.cfg.acked.is_none() => {
                ctx.post_error(
                    ls,
                    err::INVALID_SURFACE_STATE,
                    "buffer attached before the first configure was acknowledged",
                );
                return false;
            }
            _ if !l.cfg.configured && l.cfg.acked.is_none() => {
                // Initial commit: take part in the arrangement, which sends
                // the configure.
                l.cfg.configured = true;
            }
            _ => {}
        }
        true
    }

    /// After a layer surface's commit: mapping and arrangement.
    pub(crate) fn layer_update_mapping(&mut self, ctx: &Ctx, surface: Resource) {
        let Some(Role::Layer(ls)) = self.surfaces.get(&surface).map(|s| s.role) else {
            return;
        };
        let has_content = self
            .surfaces
            .get(&surface)
            .is_some_and(|s| s.content.is_some());
        let Some(l) = self.layers.get_mut(&ls) else {
            return;
        };
        let visible = l.cfg.acked.is_some() && has_content;
        let changed = visible != l.mapped;
        l.mapped = visible;
        self.rearrange(ctx);
        if changed {
            let l = &self.layers[&ls];
            if visible {
                info!(
                    COMPOSITOR,
                    "layer surface mapped: {:?} on layer {} at {},{} {}x{} (exclusive zone {})",
                    l.namespace,
                    layer_name(l.current.layer),
                    l.rect.x,
                    l.rect.y,
                    l.rect.w,
                    l.rect.h,
                    l.current.zone
                );
                if l.current.keyboard != layer::KEYBOARD_NONE {
                    self.seat.focus_request = Some(surface);
                }
            } else {
                info!(COMPOSITOR, "layer surface unmapped: {:?}", l.namespace);
            }
        }
        self.needs_redraw = true;
    }

    /// Arranges every configured layer surface and sends a configure to
    /// each whose size changed. Updates the area left for windows.
    pub(crate) fn rearrange(&mut self, ctx: &Ctx) {
        let ids: Vec<Resource> = self
            .layer_order
            .iter()
            .copied()
            .filter(|r| self.layers[r].cfg.configured)
            .collect();
        // A surface reserves its exclusive zone only while mapped: a panel
        // that is not on screen yet does not push windows away.
        let states: Vec<State> = ids
            .iter()
            .map(|r| {
                let l = &self.layers[r];
                let mut s = l.current;
                if !l.mapped {
                    s.zone = s.zone.min(0);
                }
                s
            })
            .collect();
        let (rects, usable) = layer::arrange(self.output_rect(), &states);
        for (r, rect) in ids.iter().zip(rects) {
            let l = self.layers.get_mut(r).expect("listed");
            l.rect = rect;
            if (rect.w, rect.h) != l.sent {
                let serial = ctx.next_serial();
                match ctx.post(
                    *r,
                    proto::zwlr_layer_surface_v1::event::CONFIGURE,
                    &[
                        Arg::Uint(serial),
                        Arg::Uint(rect.w as u32),
                        Arg::Uint(rect.h as u32),
                    ],
                ) {
                    Ok(()) => {
                        l.cfg.sent(serial);
                        l.sent = (rect.w, rect.h);
                    }
                    Err(e) => warn!(COMPOSITOR, "layer configure: {e}"),
                }
            }
        }
        if usable != self.usable {
            info!(
                COMPOSITOR,
                "usable area for windows: {},{} {}x{}", usable.x, usable.y, usable.w, usable.h
            );
            self.usable = usable;
        }
    }

    /// A layer surface object was destroyed.
    pub(crate) fn layer_destroyed(&mut self, ctx: &Ctx, ls: Resource) {
        if let Some(l) = self.layers.remove(&ls) {
            self.layer_order.retain(|r| *r != ls);
            if let Some(s) = l.surface.and_then(|s| self.surfaces.get_mut(&s)) {
                s.role = Role::None;
            }
            if l.mapped {
                info!(COMPOSITOR, "layer surface destroyed: {:?}", l.namespace);
            }
            self.rearrange(ctx);
            self.needs_redraw = true;
        }
    }

    /// The wl_surface of a layer surface was destroyed first.
    pub(crate) fn layer_surface_gone(&mut self, ctx: &Ctx, ls: Resource) {
        if let Some(l) = self.layers.get_mut(&ls) {
            l.surface = None;
            l.mapped = false;
            l.cfg.configured = false;
            self.rearrange(ctx);
            self.needs_redraw = true;
        }
    }

    /// Visible surfaces bottom to top (background, bottom, windows, top,
    /// overlay) with position and size: for drawing and for input.
    pub fn stack(&self) -> Vec<(Resource, i32, i32, i32, i32)> {
        let size = |s: Resource| self.surfaces.get(&s).and_then(|s| s.content);
        let layers = |which: &[u32]| -> Vec<(Resource, i32, i32, i32, i32)> {
            let mut v = Vec::new();
            for &lv in which {
                for r in &self.layer_order {
                    let l = &self.layers[r];
                    if l.mapped && l.current.layer == lv {
                        if let Some((s, (w, h))) = l.surface.and_then(|s| size(s).map(|z| (s, z))) {
                            v.push((s, l.rect.x, l.rect.y, w, h));
                        }
                    }
                }
            }
            v
        };
        let mut out = layers(&[layer::BACKGROUND, layer::BOTTOM]);
        out.extend(
            self.windows
                .iter()
                .filter_map(|w| size(w.surface).map(|(sw, sh)| (w.surface, w.x, w.y, sw, sh))),
        );
        out.extend(layers(&[layer::TOP, layer::OVERLAY]));
        out
    }

    /// The layer surface of `surface`, if it has that role.
    pub(crate) fn layer_of(&self, surface: Resource) -> Option<&LayerSurface> {
        match self.surfaces.get(&surface)?.role {
            Role::Layer(ls) => self.layers.get(&ls),
            _ => None,
        }
    }

    /// The topmost mapped top/overlay layer surface that wants the keyboard
    /// exclusively (it keeps the focus while mapped).
    pub(crate) fn exclusive_keyboard_layer(&self) -> Option<Resource> {
        let mut best: Option<(u32, Resource)> = None;
        for r in &self.layer_order {
            let l = &self.layers[r];
            if l.mapped
                && l.current.layer >= layer::TOP
                && l.current.keyboard == layer::KEYBOARD_EXCLUSIVE
                && best.is_none_or(|(lv, _)| l.current.layer >= lv)
            {
                best = l.surface.map(|s| (l.current.layer, s));
            }
        }
        best.map(|(_, s)| s)
    }

    /// Textures in stacking order, then the cursor.
    pub fn scene(&self) -> Vec<(&wana_render::compose::Texture, i32, i32)> {
        self.stack()
            .into_iter()
            .filter_map(|(s, x, y, _, _)| match self.content.get(&s) {
                Some(Content::Texture(t)) => Some((t, x, y)),
                _ => None,
            })
            .chain(self.cursor_image())
            .collect()
    }
}

pub fn layer_name(l: u32) -> &'static str {
    match l {
        layer::BACKGROUND => "background",
        layer::BOTTOM => "bottom",
        layer::TOP => "top",
        _ => "overlay",
    }
}
