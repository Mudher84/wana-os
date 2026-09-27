//! Reusable native xdg_toplevel window for Wana desktop applications.
//!
//! This is intentionally small: registry discovery, the initial configure
//! handshake, xdg_wm_base ping/pong, close/configure events and XRGB8888
//! presentation. Higher-level widgets live in applications, not here.

use crate::client::{Connection, Event, Proxy, Req, Val};
use crate::shm::Buffer;
use wana_wayland::protocols::{wayland, xdg_shell};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WindowEvent {
    Configure { width: i32, height: i32 },
    Close,
    Other,
}

#[derive(Debug)]
pub struct Window {
    conn: Connection,
    pub surface: Proxy,
    pub xdg_surface: Proxy,
    pub toplevel: Proxy,
    pub shm: Proxy,
    wm_base: Proxy,
    width: i32,
    height: i32,
    configured: bool,
}

impl Window {
    pub fn connect(
        title: &str,
        app_id: &str,
        width: i32,
        height: i32,
    ) -> Result<Self, String> {
        if width <= 0 || height <= 0 {
            return Err("window size must be positive".into());
        }
        let conn = Connection::connect()?;
        let registry = conn
            .request(
                conn.display(),
                wayland::wl_display::request::GET_REGISTRY,
                Some((&wayland::WL_REGISTRY_INTERFACE, 1)),
                &[Req::NewId],
            )?
            .ok_or("registry returned no object")?;
        conn.roundtrip()?;

        let mut compositor = None;
        let mut shm = None;
        let mut wm = None;
        while let Some(ev) = conn.next_event() {
            if ev.target != registry || ev.opcode != wayland::wl_registry::event::GLOBAL {
                continue;
            }
            let [Val::Uint(name), Val::Str(iface), Val::Uint(version)] = &ev.args[..] else {
                continue;
            };
            let bind = |table: &'static wana_wayland::sys::wl_interface, want: u32| {
                let v = (*version).min(want);
                conn.request(
                    registry,
                    wayland::wl_registry::request::BIND,
                    Some((table, v)),
                    &[
                        Req::Uint(*name),
                        Req::Str(iface),
                        Req::Uint(v),
                        Req::NewId,
                    ],
                )
                .ok()
                .flatten()
            };
            match iface.as_str() {
                "wl_compositor" => {
                    compositor = bind(&wayland::WL_COMPOSITOR_INTERFACE, 4);
                }
                "wl_shm" => shm = bind(&wayland::WL_SHM_INTERFACE, 1),
                "xdg_wm_base" => wm = bind(&xdg_shell::XDG_WM_BASE_INTERFACE, 1),
                _ => {}
            }
        }

        let compositor = compositor.ok_or("wl_compositor not advertised")?;
        let shm = shm.ok_or("wl_shm not advertised")?;
        let wm_base = wm.ok_or("xdg_wm_base not advertised")?;
        let surface = conn
            .request(
                compositor,
                wayland::wl_compositor::request::CREATE_SURFACE,
                Some((&wayland::WL_SURFACE_INTERFACE, 4)),
                &[Req::NewId],
            )?
            .ok_or("create_surface returned no object")?;
        let xdg_surface = conn
            .request(
                wm_base,
                xdg_shell::xdg_wm_base::request::GET_XDG_SURFACE,
                Some((&xdg_shell::XDG_SURFACE_INTERFACE, 1)),
                &[Req::NewId, Req::Object(Some(surface))],
            )?
            .ok_or("get_xdg_surface returned no object")?;
        let toplevel = conn
            .request(
                xdg_surface,
                xdg_shell::xdg_surface::request::GET_TOPLEVEL,
                Some((&xdg_shell::XDG_TOPLEVEL_INTERFACE, 1)),
                &[Req::NewId],
            )?
            .ok_or("get_toplevel returned no object")?;
        conn.request(
            toplevel,
            xdg_shell::xdg_toplevel::request::SET_TITLE,
            None,
            &[Req::Str(title)],
        )?;
        conn.request(
            toplevel,
            xdg_shell::xdg_toplevel::request::SET_APP_ID,
            None,
            &[Req::Str(app_id)],
        )?;
        conn.request(
            toplevel,
            xdg_shell::xdg_toplevel::request::SET_MIN_SIZE,
            None,
            &[Req::Int(320), Req::Int(240)],
        )?;
        conn.request(surface, wayland::wl_surface::request::COMMIT, None, &[])?;

        let mut out = Self {
            conn,
            surface,
            xdg_surface,
            toplevel,
            shm,
            wm_base,
            width,
            height,
            configured: false,
        };
        out.wait_initial_configure()?;
        Ok(out)
    }

    pub fn size(&self) -> (i32, i32) {
        (self.width, self.height)
    }

    pub fn connection(&self) -> &Connection {
        &self.conn
    }

    fn ping(&self, ev: &Event) -> Result<bool, String> {
        if ev.target == self.wm_base && ev.opcode == xdg_shell::xdg_wm_base::event::PING {
            if let Some(Val::Uint(serial)) = ev.args.first() {
                self.conn.request(
                    self.wm_base,
                    xdg_shell::xdg_wm_base::request::PONG,
                    None,
                    &[Req::Uint(*serial)],
                )?;
            }
            return Ok(true);
        }
        Ok(false)
    }

    fn apply(&mut self, ev: &Event) -> Result<WindowEvent, String> {
        if self.ping(ev)? {
            return Ok(WindowEvent::Other);
        }
        if ev.target == self.toplevel {
            if ev.opcode == xdg_shell::xdg_toplevel::event::CLOSE {
                return Ok(WindowEvent::Close);
            }
            if ev.opcode == xdg_shell::xdg_toplevel::event::CONFIGURE {
                if let [Val::Int(w), Val::Int(h), _] = &ev.args[..] {
                    if *w > 0 {
                        self.width = *w;
                    }
                    if *h > 0 {
                        self.height = *h;
                    }
                }
            }
        }
        if ev.target == self.xdg_surface
            && ev.opcode == xdg_shell::xdg_surface::event::CONFIGURE
        {
            let Some(Val::Uint(serial)) = ev.args.first() else {
                return Err("xdg_surface.configure without serial".into());
            };
            self.conn.request(
                self.xdg_surface,
                xdg_shell::xdg_surface::request::ACK_CONFIGURE,
                None,
                &[Req::Uint(*serial)],
            )?;
            self.configured = true;
            return Ok(WindowEvent::Configure {
                width: self.width,
                height: self.height,
            });
        }
        Ok(WindowEvent::Other)
    }

    fn wait_initial_configure(&mut self) -> Result<(), String> {
        for _ in 0..1000 {
            while let Some(ev) = self.conn.next_event() {
                if matches!(self.apply(&ev)?, WindowEvent::Configure { .. }) {
                    return Ok(());
                }
            }
            self.conn.dispatch()?;
        }
        Err("initial window configure never arrived".into())
    }

    pub fn next_event(&mut self, timeout_ms: i32) -> Result<WindowEvent, String> {
        if let Some(ev) = self.conn.next_event() {
            return self.apply(&ev);
        }
        self.conn.wait(timeout_ms)?;
        if let Some(ev) = self.conn.next_event() {
            self.apply(&ev)
        } else {
            Ok(WindowEvent::Other)
        }
    }

    pub fn present_xrgb(&self, width: i32, height: i32, pixels: &[u8]) -> Result<(), String> {
        if !self.configured {
            return Err("window has not been configured".into());
        }
        let buffer = Buffer::new(&self.conn, self.shm, width, height, pixels)?;
        buffer.commit_to(&self.conn, self.surface)?;
        buffer.destroy(&self.conn);
        Ok(())
    }
}

impl Drop for Window {
    fn drop(&mut self) {
        self.conn
            .destroy(self.toplevel, xdg_shell::xdg_toplevel::request::DESTROY);
        self.conn.destroy(
            self.xdg_surface,
            xdg_shell::xdg_surface::request::DESTROY,
        );
        self.conn
            .destroy(self.surface, wayland::wl_surface::request::DESTROY);
    }
}
