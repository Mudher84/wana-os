//! Small reusable xdg_toplevel helper for native Wana applications.
//!
//! It owns the common Wayland client setup (registry, wl_compositor, wl_shm,
//! xdg_wm_base), the initial configure handshake and buffer presentation.
//! Settings, Files and the installer GUI use this instead of duplicating
//! protocol plumbing.

use crate::client::{Connection, Event, Proxy, Req, Val};
use crate::shm::Buffer;
use wana_wayland::protocols::{wayland, xdg_shell};

#[derive(Debug)]
pub struct App {
    pub conn: Connection,
    pub compositor: Proxy,
    pub shm: Proxy,
    pub wm_base: Proxy,
}

impl App {
    pub fn connect() -> Result<Self, String> {
        let conn = Connection::connect()?;
        let registry = conn
            .request(
                conn.display(),
                wayland::wl_display::request::GET_REGISTRY,
                Some((&wayland::WL_REGISTRY_INTERFACE, 1)),
                &[Req::NewId],
            )?
            .ok_or("registry constructor returned no object")?;
        conn.roundtrip()?;

        let mut globals: Vec<(u32, String, u32)> = Vec::new();
        while let Some(ev) = conn.next_event() {
            if ev.target == registry && ev.opcode == wayland::wl_registry::event::GLOBAL {
                if let [Val::Uint(id), Val::Str(name), Val::Uint(version)] = &ev.args[..] {
                    globals.push((*id, name.clone(), *version));
                }
            }
        }

        let bind = |name: &str,
                    iface: &'static wana_wayland::sys::wl_interface,
                    want: u32|
         -> Result<Proxy, String> {
            let (id, version) = globals
                .iter()
                .find(|g| g.1 == name)
                .map(|g| (g.0, g.2))
                .ok_or_else(|| format!("{name} not advertised"))?;
            let version = version.min(want);
            conn.request(
                registry,
                wayland::wl_registry::request::BIND,
                Some((iface, version)),
                &[
                    Req::Uint(id),
                    Req::Str(name),
                    Req::Uint(version),
                    Req::NewId,
                ],
            )?
            .ok_or_else(|| format!("bind {name} returned no object"))
        };

        Ok(Self {
            compositor: bind("wl_compositor", &wayland::WL_COMPOSITOR_INTERFACE, 4)?,
            shm: bind("wl_shm", &wayland::WL_SHM_INTERFACE, 1)?,
            wm_base: bind("xdg_wm_base", &xdg_shell::XDG_WM_BASE_INTERFACE, 1)?,
            conn,
        })
    }

    /// Handles xdg_wm_base ping and returns true if the event was consumed.
    pub fn protocol_event(&self, ev: &Event) -> Result<bool, String> {
        if ev.target == self.wm_base && ev.opcode == xdg_shell::xdg_wm_base::event::PING {
            let Some(Val::Uint(serial)) = ev.args.first() else {
                return Err("xdg_wm_base ping without serial".into());
            };
            self.conn.request(
                self.wm_base,
                xdg_shell::xdg_wm_base::request::PONG,
                None,
                &[Req::Uint(*serial)],
            )?;
            return Ok(true);
        }
        Ok(false)
    }
}

#[derive(Debug)]
pub struct Window {
    pub surface: Proxy,
    pub xdg_surface: Proxy,
    pub toplevel: Proxy,
    pub width: i32,
    pub height: i32,
}

impl Window {
    pub fn new(
        app: &App,
        title: &str,
        app_id: &str,
        width: i32,
        height: i32,
    ) -> Result<Self, String> {
        if width <= 0 || height <= 0 {
            return Err("window size must be positive".into());
        }
        let surface = app
            .conn
            .request(
                app.compositor,
                wayland::wl_compositor::request::CREATE_SURFACE,
                Some((&wayland::WL_SURFACE_INTERFACE, 4)),
                &[Req::NewId],
            )?
            .ok_or("create_surface returned no object")?;
        let xdg_surface = app
            .conn
            .request(
                app.wm_base,
                xdg_shell::xdg_wm_base::request::GET_XDG_SURFACE,
                Some((&xdg_shell::XDG_SURFACE_INTERFACE, 1)),
                &[Req::NewId, Req::Object(Some(surface))],
            )?
            .ok_or("get_xdg_surface returned no object")?;
        let toplevel = app
            .conn
            .request(
                xdg_surface,
                xdg_shell::xdg_surface::request::GET_TOPLEVEL,
                Some((&xdg_shell::XDG_TOPLEVEL_INTERFACE, 1)),
                &[Req::NewId],
            )?
            .ok_or("get_toplevel returned no object")?;
        app.conn.request(
            toplevel,
            xdg_shell::xdg_toplevel::request::SET_TITLE,
            None,
            &[Req::Str(title)],
        )?;
        app.conn.request(
            toplevel,
            xdg_shell::xdg_toplevel::request::SET_APP_ID,
            None,
            &[Req::Str(app_id)],
        )?;

        app.conn
            .request(surface, wayland::wl_surface::request::COMMIT, None, &[])?;

        let serial = loop {
            app.conn.dispatch()?;
            let mut found = None;
            while let Some(ev) = app.conn.next_event() {
                app.protocol_event(&ev)?;
                if ev.target == xdg_surface
                    && ev.opcode == xdg_shell::xdg_surface::event::CONFIGURE
                {
                    if let Some(Val::Uint(serial)) = ev.args.first() {
                        found = Some(*serial);
                    }
                }
            }
            if let Some(serial) = found {
                break serial;
            }
        };
        app.conn.request(
            xdg_surface,
            xdg_shell::xdg_surface::request::ACK_CONFIGURE,
            None,
            &[Req::Uint(serial)],
        )?;

        Ok(Self {
            surface,
            xdg_surface,
            toplevel,
            width,
            height,
        })
    }

    pub fn present(&self, app: &App, pixels: &[u8]) -> Result<(), String> {
        let buffer = Buffer::new(&app.conn, app.shm, self.width, self.height, pixels)?;
        buffer.commit_to(&app.conn, self.surface)?;
        buffer.destroy(&app.conn);
        Ok(())
    }

    pub fn close_event(&self, ev: &Event) -> bool {
        ev.target == self.toplevel && ev.opcode == xdg_shell::xdg_toplevel::event::CLOSE
    }

    pub fn destroy(self, app: &App) {
        app.conn
            .destroy(self.toplevel, xdg_shell::xdg_toplevel::request::DESTROY);
        app.conn.destroy(
            self.xdg_surface,
            xdg_shell::xdg_surface::request::DESTROY,
        );
        app.conn
            .destroy(self.surface, wayland::wl_surface::request::DESTROY);
    }
}
