//! The compositor's screen (Phase 10 step 3): DRM output, GBM + EGL +
//! GLES (wana-render), page flips (wana-drm).
//!
//! Buffer life cycle, one flip in flight at most:
//! - `draw` renders into the GBM surface's back buffer, swaps, locks the new
//!   front buffer and asks for a page flip to it (the first frame uses
//!   SETCRTC, which is synchronous);
//! - `handle_drm` receives the flip completion; the buffer that was on
//!   screen before is released back to GBM only then.
//!
//! So the buffer being scanned out is never drawn into (no tearing), and
//! GBM always has a free buffer for the next frame. A redraw requested
//! while a flip is pending waits for that flip (the caller checks
//! `flip_pending`).

use std::os::unix::io::RawFd;
use wana_drm::output::Output;
use wana_log::{info, Subsystem};
use wana_render::compose::{Composer, Texture};
use wana_render::egl::Egl;
use wana_render::gbm;

const DRM: Subsystem = Subsystem::Drm;

/// Scanout buffer and its DRM framebuffer id.
type Shown = (gbm::FrontBuffer, u32);

pub struct Screen {
    // Field order is drop order: EGL before the GBM surface it renders to.
    egl: Egl,
    composer: Composer,
    gsurf: gbm::Surface<'static>,
    out: Output,
    shown: Option<Shown>,
    pending: Option<Shown>,
    modeset_done: bool,
    frames: u64,
}

impl std::fmt::Debug for Screen {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Screen({} {}, {} frames)",
            self.out.conn.name, self.out.mode, self.frames
        )
    }
}

impl Screen {
    /// Sets up GBM, EGL and the compositing program on `out`.
    pub fn new(out: Output) -> Result<Screen, String> {
        let (w, h) = (out.mode.width(), out.mode.height());
        // The GBM device lives as long as the compositor: leaked on purpose
        // so the surface can borrow it for 'static.
        let device: &'static gbm::Device =
            Box::leak(Box::new(gbm::Device::new(out.card.raw_fd())?));
        let gsurf = gbm::Surface::new(device, w, h)?;
        let egl = Egl::new(device, &gsurf)?;
        let composer = Composer::new(w, h)?;
        info!(
            Subsystem::Render,
            "compositor renderer: GL_RENDERER={}, {w}x{h}",
            wana_render::gl::string(wana_render::ffi::GL_RENDERER)
        );
        Ok(Screen {
            egl,
            composer,
            gsurf,
            out,
            shown: None,
            pending: None,
            modeset_done: false,
            frames: 0,
        })
    }

    pub fn drm_fd(&self) -> RawFd {
        self.out.card.raw_fd()
    }

    pub fn flip_pending(&self) -> bool {
        self.pending.is_some()
    }

    pub fn frames(&self) -> u64 {
        self.frames
    }

    /// Renders background + windows (bottom to top) and queues the frame.
    /// Returns true when the frame is already on screen (first frame).
    pub fn draw(
        &mut self,
        background: u32,
        windows: &[(&Texture, i32, i32)],
    ) -> Result<bool, String> {
        if self.pending.is_some() {
            return Err("draw while a page flip is pending".into());
        }
        self.composer.begin(background);
        for (tex, x, y) in windows {
            self.composer.draw(tex, *x, *y)?;
        }
        self.egl.swap()?;
        let bo = self.gsurf.lock_front()?;
        let (w, h) = (self.out.mode.width(), self.out.mode.height());
        let fb = match self.out.card.add_fb_handle(w, h, bo.stride, bo.handle) {
            Ok(fb) => fb,
            Err(e) => {
                self.gsurf.release(bo);
                return Err(format!("ADDFB: {e}"));
            }
        };
        self.frames += 1;
        if !self.modeset_done {
            if let Err(e) =
                self.out
                    .card
                    .set_crtc(self.out.crtc, fb, self.out.conn.id, &self.out.mode)
            {
                let _ = self.out.card.rm_fb(fb);
                self.gsurf.release(bo);
                return Err(format!("SETCRTC: {e}"));
            }
            self.modeset_done = true;
            info!(
                DRM,
                "modeset done: {} {} on CRTC {}; compositor frame on screen",
                self.out.conn.name,
                self.out.mode,
                self.out.crtc
            );
            self.retire_shown((bo, fb));
            return Ok(true);
        }
        if let Err(e) = self.out.card.page_flip(self.out.crtc, fb, self.frames) {
            let _ = self.out.card.rm_fb(fb);
            self.gsurf.release(bo);
            return Err(format!("page flip: {e}"));
        }
        self.pending = Some((bo, fb));
        Ok(false)
    }

    /// Reads DRM events (call when the DRM fd is readable). Returns the
    /// completion time in microseconds if the pending flip completed.
    pub fn handle_drm(&mut self) -> Result<Option<u64>, String> {
        let events = self
            .out
            .card
            .read_flip_events()
            .map_err(|e| format!("reading DRM events: {e}"))?;
        let mut done = None;
        for ev in events {
            if let Some(next) = self.pending.take() {
                self.retire_shown(next);
                done = Some(ev.time_us);
            }
        }
        Ok(done)
    }

    /// `next` is now on screen: release the buffer it replaced.
    fn retire_shown(&mut self, next: Shown) {
        if let Some((bo, fb)) = self.shown.replace(next) {
            let _ = self.out.card.rm_fb(fb);
            self.gsurf.release(bo);
        }
    }
}

impl Drop for Screen {
    fn drop(&mut self) {
        for (bo, fb) in [self.pending.take(), self.shown.take()]
            .into_iter()
            .flatten()
        {
            let _ = self.out.card.rm_fb(fb);
            self.gsurf.release(bo);
        }
    }
}
