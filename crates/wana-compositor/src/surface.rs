//! Surface and xdg-shell state machines (Phase 10 step 3), kept free of
//! libwayland so the protocol rules are unit-tested directly.
//!
//! wl_surface state is double-buffered: attach, damage, frame, and scale go
//! to `pending` and take effect atomically on commit.
//!
//! xdg-shell handshake for a toplevel:
//! 1. get_xdg_surface + get_toplevel give the surface its role;
//! 2. the client commits with no buffer (initial commit);
//! 3. the compositor answers with xdg_toplevel.configure then
//!    xdg_surface.configure(serial);
//! 4. the client acks that serial, attaches a buffer and commits: the
//!    window is mapped;
//! 5. committing a NULL buffer unmaps it.
//!
//! Attaching a buffer before the first ack is the protocol error
//! `xdg_surface.unconfigured_buffer`.

use wana_wayland::server::Resource;

/// What the pending state says about the buffer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Attach {
    /// No attach since the last commit: keep the current content.
    Unchanged,
    Buffer(Resource),
    /// `attach(NULL)`: remove the content (unmaps a window).
    Remove,
}

/// The surface's role, assigned once for its lifetime.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    None,
    /// An xdg_surface was created for it (toplevel or not yet decided).
    Xdg(Resource),
    /// wl_pointer.set_cursor made it a cursor image.
    Cursor,
}

/// Double-buffered wl_surface state.
#[derive(Debug)]
pub struct Surface {
    pub attach: Attach,
    /// Frame callbacks requested since the last commit.
    pub pending_frames: Vec<Resource>,
    /// Committed callbacks waiting for the next presented frame.
    pub frames: Vec<Resource>,
    pub pending_scale: i32,
    pub scale: i32,
    /// Size of the committed content in buffer pixels, if any.
    pub content: Option<(i32, i32)>,
    pub role: Role,
    /// A buffer was ever attached (get_xdg_surface refuses such surfaces).
    pub had_buffer: bool,
}

impl Default for Surface {
    fn default() -> Self {
        Surface {
            attach: Attach::Unchanged,
            pending_frames: Vec::new(),
            frames: Vec::new(),
            pending_scale: 1,
            scale: 1,
            content: None,
            role: Role::None,
            had_buffer: false,
        }
    }
}

/// xdg_surface configure bookkeeping.
#[derive(Debug, Default)]
pub struct XdgSurface {
    pub surface: Option<Resource>,
    pub toplevel: Option<Resource>,
    /// Serials sent and not yet acked, oldest first.
    pub sent: Vec<u32>,
    /// The last acked serial: set once the client has seen a configure.
    pub acked: Option<u32>,
    /// The initial configure has been sent.
    pub configured: bool,
}

/// Result of an ack_configure.
#[derive(Debug, PartialEq, Eq)]
pub enum Ack {
    Ok,
    /// Not a serial the compositor sent (or already acked): invalid_serial.
    Invalid,
}

impl XdgSurface {
    /// Records a sent configure serial.
    pub fn sent(&mut self, serial: u32) {
        self.sent.push(serial);
        self.configured = true;
    }

    /// Acking a serial also acknowledges every older outstanding one.
    pub fn ack(&mut self, serial: u32) -> Ack {
        match self.sent.iter().position(|s| *s == serial) {
            Some(i) => {
                self.sent.drain(..=i);
                self.acked = Some(serial);
                Ack::Ok
            }
            None => Ack::Invalid,
        }
    }
}

/// What a commit on an xdg toplevel surface means.
#[derive(Debug, PartialEq, Eq)]
pub enum XdgCommit {
    /// First commit without a buffer: send the initial configure.
    SendInitialConfigure,
    /// A buffer before any ack: xdg_surface.unconfigured_buffer.
    UnconfiguredBuffer,
    /// Configured: apply the state.
    Apply,
    /// No role object yet (only get_xdg_surface): nothing to configure;
    /// a buffer here is also unconfigured_buffer.
    NotConstructed,
}

/// Decides what a commit does for a surface with the xdg role.
pub fn xdg_commit(xdg: &XdgSurface, attach: Attach) -> XdgCommit {
    let has_buffer = matches!(attach, Attach::Buffer(_));
    if xdg.toplevel.is_none() {
        return if has_buffer {
            XdgCommit::UnconfiguredBuffer
        } else {
            XdgCommit::NotConstructed
        };
    }
    if xdg.acked.is_none() {
        if has_buffer {
            XdgCommit::UnconfiguredBuffer
        } else if !xdg.configured {
            XdgCommit::SendInitialConfigure
        } else {
            // Configure already sent; waiting for the ack.
            XdgCommit::Apply
        }
    } else {
        XdgCommit::Apply
    }
}

/// Places the n-th window: centered, then cascaded by 32 px.
pub fn place(n: usize, out_w: i32, out_h: i32, w: i32, h: i32) -> (i32, i32) {
    let step = 32 * n as i32;
    let x = ((out_w - w) / 2 + step).clamp(0, (out_w - w).max(0));
    let y = ((out_h - h) / 2 + step).clamp(0, (out_h - h).max(0));
    (x, y)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn res() -> Resource {
        // Distinct fake identities; never dereferenced by these tests.
        use std::sync::atomic::{AtomicUsize, Ordering};
        static N: AtomicUsize = AtomicUsize::new(0x1000);
        let p = N.fetch_add(0x10, Ordering::Relaxed);
        Resource::from_raw_for_tests(p)
    }

    #[test]
    fn toplevel_handshake() {
        let mut x = XdgSurface {
            toplevel: Some(res()),
            ..Default::default()
        };
        let buf = Attach::Buffer(res());
        assert_eq!(
            xdg_commit(&x, Attach::Unchanged),
            XdgCommit::SendInitialConfigure
        );
        assert_eq!(
            xdg_commit(&x, buf),
            XdgCommit::UnconfiguredBuffer,
            "buffer before configure"
        );
        x.sent(10);
        assert_eq!(
            xdg_commit(&x, Attach::Unchanged),
            XdgCommit::Apply,
            "no second initial configure"
        );
        assert_eq!(
            xdg_commit(&x, buf),
            XdgCommit::UnconfiguredBuffer,
            "configure sent but not acked"
        );
        assert_eq!(x.ack(10), Ack::Ok);
        assert_eq!(xdg_commit(&x, buf), XdgCommit::Apply);
    }

    #[test]
    fn acks_follow_the_serials_sent() {
        let mut x = XdgSurface::default();
        x.sent(5);
        x.sent(6);
        x.sent(7);
        assert_eq!(x.ack(4), Ack::Invalid, "never sent");
        assert_eq!(x.ack(6), Ack::Ok, "acking 6 also acks 5");
        assert_eq!(x.sent, vec![7]);
        assert_eq!(x.ack(5), Ack::Invalid, "already acknowledged");
        assert_eq!(x.ack(7), Ack::Ok);
        assert_eq!(x.acked, Some(7));
    }

    #[test]
    fn role_object_is_required_for_buffers() {
        let x = XdgSurface::default();
        assert_eq!(xdg_commit(&x, Attach::Unchanged), XdgCommit::NotConstructed);
        assert_eq!(
            xdg_commit(&x, Attach::Buffer(res())),
            XdgCommit::UnconfiguredBuffer
        );
    }

    #[test]
    fn windows_are_centered_then_cascaded_on_screen() {
        assert_eq!(place(0, 1280, 800, 480, 320), (400, 240));
        assert_eq!(place(1, 1280, 800, 480, 320), (432, 272));
        assert_eq!(
            place(0, 1280, 800, 2000, 100),
            (0, 350),
            "clamped when larger"
        );
        assert_eq!(
            place(50, 1280, 800, 480, 320),
            (800, 480),
            "stays on screen"
        );
    }
}
