//! wana-gl: native GPU rendering bring-up tool and test (Phase 8).
//!
//! DRM/KMS output selection (wana-drm), then a GBM surface, an EGL display and
//! ES context, the scene drawn with OpenGL ES, and each rendered buffer
//! scanned out with modeset/page flip. Every layer logs its own tag
//! ([DRM] [GBM] [EGL] [RENDER]) so a failure points to the first broken layer.
//!
//! Usage: wana-gl [--frames N] [--hold SECONDS] [--card /dev/dri/cardN]

use std::process::ExitCode;
use std::time::{Duration, Instant};
use wana_drm::output;
use wana_log::{error, info, warn, Subsystem};
use wana_render::{egl::Egl, ffi, gbm, gl, scene::Scene};

const DRM: Subsystem = Subsystem::Drm;
const GBM: Subsystem = Subsystem::Gbm;
const EGL: Subsystem = Subsystem::Egl;
const RENDER: Subsystem = Subsystem::Render;

struct Args {
    frames: u32,
    hold: u64,
    card: Option<String>,
}

fn parse_args() -> Result<Args, String> {
    let mut a = Args {
        frames: 60,
        hold: 0,
        card: None,
    };
    let mut it = std::env::args().skip(1);
    while let Some(arg) = it.next() {
        let mut val = |name: &str| it.next().ok_or(format!("{name} needs a value"));
        match arg.as_str() {
            "--frames" => {
                a.frames = val("--frames")?
                    .parse()
                    .map_err(|e| format!("--frames: {e}"))?
            }
            "--hold" => a.hold = val("--hold")?.parse().map_err(|e| format!("--hold: {e}"))?,
            "--card" => a.card = Some(val("--card")?),
            other => return Err(format!("unknown argument {other}")),
        }
    }
    a.frames = a.frames.max(1);
    Ok(a)
}

fn main() -> ExitCode {
    wana_log::init_from_env();
    let result = parse_args().and_then(|a| run(&a));
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            error!(RENDER, "{e}");
            ExitCode::FAILURE
        }
    }
}

fn ms(d: Duration) -> f64 {
    d.as_secs_f64() * 1000.0
}

fn run(args: &Args) -> Result<(), String> {
    let out = output::find(args.card.as_deref())?;
    let (w, h) = (out.mode.width(), out.mode.height());

    let device = gbm::Device::new(out.card.raw_fd()).map_err(|e| {
        error!(GBM, "{e}");
        e
    })?;
    info!(GBM, "device created, backend {}", device.backend_name());
    let surface = gbm::Surface::new(&device, w, h)?;
    info!(GBM, "surface {w}x{h} XRGB8888 (scanout | rendering)");

    let egl = Egl::new(&device, &surface).map_err(|e| {
        error!(EGL, "{e}");
        e
    })?;
    info!(
        EGL,
        "EGL {}.{} vendor={} version=\"{}\" apis=\"{}\"",
        egl.version.0,
        egl.version.1,
        egl.query(ffi::EGL_VENDOR),
        egl.query(ffi::EGL_VERSION),
        egl.query(ffi::EGL_CLIENT_APIS)
    );
    info!(EGL, "OpenGL ES context current on GBM window surface");
    info!(
        RENDER,
        "GL_VENDOR={} GL_RENDERER={}",
        gl::string(ffi::GL_VENDOR),
        gl::string(ffi::GL_RENDERER)
    );
    info!(
        RENDER,
        "GL_VERSION={} GLSL={}",
        gl::string(ffi::GL_VERSION),
        gl::string(ffi::GL_SHADING_LANGUAGE_VERSION)
    );

    let scene = Scene::new(w, h)?;
    info!(RENDER, "shaders compiled and linked");

    let mut shown: Option<(gbm::FrontBuffer, u32)> = None;
    let mut render_times = Vec::with_capacity(args.frames as usize);
    let mut flip_times = Vec::with_capacity(args.frames as usize);
    for n in 0..args.frames {
        let progress = if args.frames > 1 {
            n as f32 / (args.frames - 1) as f32
        } else {
            1.0
        };
        let t0 = Instant::now();
        scene.draw(progress)?;
        // SAFETY: current context; glFinish only waits for the GPU.
        unsafe { ffi::glFinish() };
        render_times.push(t0.elapsed());
        egl.swap()?;
        let bo = surface.lock_front()?;
        let fb = out
            .card
            .add_fb_handle(w, h, bo.stride, bo.handle)
            .map_err(|e| format!("ADDFB (GBM bo handle {}): {e}", bo.handle))?;
        if n == 0 {
            out.card
                .set_crtc(out.crtc, fb, out.conn.id, &out.mode)
                .map_err(|e| format!("SETCRTC with GPU buffer: {e}"))?;
            info!(
                DRM,
                "modeset done: {} {} on CRTC {}; first GPU frame on screen (fb {fb}, stride {})",
                out.conn.name,
                out.mode,
                out.crtc,
                bo.stride
            );
        } else {
            out.card
                .page_flip(out.crtc, fb, u64::from(n))
                .map_err(|e| format!("page flip {n}: {e}"))?;
            let events = out
                .card
                .read_flip_events()
                .map_err(|e| format!("flip event: {e}"))?;
            flip_times.extend(events.iter().map(|e| e.time_us));
        }
        if let Some((old_bo, old_fb)) = shown.replace((bo, fb)) {
            let _ = out.card.rm_fb(old_fb);
            surface.release(old_bo);
        }
    }

    let mean = |v: &[Duration]| ms(v.iter().sum::<Duration>()) / v.len() as f64;
    let worst = render_times.iter().max().copied().unwrap_or_default();
    info!(
        RENDER,
        "{} frames rendered, GPU frame time mean {:.2} ms, max {:.2} ms ({:.1} fps possible)",
        render_times.len(),
        mean(&render_times),
        ms(worst),
        1000.0 / mean(&render_times)
    );
    if flip_times.len() >= 2 {
        let iv: Vec<u64> = flip_times
            .windows(2)
            .map(|p| p[1].saturating_sub(p[0]))
            .collect();
        let m = iv.iter().sum::<u64>() as f64 / iv.len() as f64 / 1000.0;
        info!(
            DRM,
            "page flip: {} flips completed, interval mean {m:.2} ms",
            flip_times.len()
        );
    } else if args.frames > 1 {
        warn!(DRM, "page flip: too few flip events to measure");
    }

    if args.hold > 0 {
        info!(RENDER, "holding frame for {}s", args.hold);
        std::thread::sleep(Duration::from_secs(args.hold));
    }
    if let Some((bo, fb)) = shown.take() {
        let _ = out.card.rm_fb(fb);
        surface.release(bo);
    }
    info!(RENDER, "done");
    Ok(())
}
