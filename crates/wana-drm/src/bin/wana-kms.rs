//! wana-kms: DRM/KMS bring-up tool and test (Phase 7).
//!
//! Finds a card with a connected display, sets its best mode, shows a test
//! pattern from a CPU-drawn dumb buffer, then page-flips between two
//! buffers for `--frames` vblanks and reports the measured refresh rate.
//! Every step logs a `[DRM]` line, so a failing boot shows the first
//! broken layer.
//!
//! Usage: wana-kms [--frames N] [--hold SECONDS] [--card /dev/dri/cardN]

use std::process::ExitCode;
use std::time::Duration;
use wana_drm::output::{self, Output};
use wana_drm::{sys, DumbBuffer};
use wana_log::{error, info, warn, Subsystem};

const DRM: Subsystem = Subsystem::Drm;

/// Test pattern colors (XRGB8888). Checked by tools/qemu-graphics-test.py.
const BACKGROUND: u32 = 0x0016_213E;
const ACCENT: u32 = 0x004F_8CFF;
const BORDER: u32 = 0x00FF_FFFF;
const BORDER_PX: u32 = 8;

struct Args {
    frames: u32,
    hold: u64,
    card: Option<String>,
}

fn parse_args() -> Result<Args, String> {
    let mut a = Args {
        frames: 120,
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
    Ok(a)
}

fn main() -> ExitCode {
    wana_log::init_from_env();
    let args = match parse_args() {
        Ok(a) => a,
        Err(e) => {
            error!(DRM, "{e}");
            return ExitCode::from(2);
        }
    };
    match run(&args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            error!(DRM, "{e}");
            ExitCode::FAILURE
        }
    }
}

fn run(args: &Args) -> Result<(), String> {
    let out = output::find(args.card.as_deref())?;
    show(&out, args)
}

/// Draws the Wana test pattern: background, centered accent square, border.
fn paint(pixels: &mut [u8], buf: &DumbBuffer) {
    let (w, h) = (buf.width, buf.height);
    let side = h / 3;
    let (sx, sy) = ((w - side) / 2, (h - side) / 2);
    for y in 0..h {
        let row = &mut pixels[(y * buf.pitch) as usize..][..(w * 4) as usize];
        for x in 0..w {
            let color =
                if x < BORDER_PX || y < BORDER_PX || x >= w - BORDER_PX || y >= h - BORDER_PX {
                    BORDER
                } else if (sx..sx + side).contains(&x) && (sy..sy + side).contains(&y) {
                    ACCENT
                } else {
                    BACKGROUND
                };
            row[(x * 4) as usize..][..4].copy_from_slice(&color.to_le_bytes());
        }
    }
}

fn show(out: &Output, args: &Args) -> Result<(), String> {
    let card = &out.card;
    let (w, h) = (out.mode.width(), out.mode.height());
    let mut bufs = Vec::new();
    for i in 0..2 {
        let buf = card
            .create_dumb(w, h)
            .map_err(|e| format!("CREATE_DUMB: {e}"))?;
        let mut map = card
            .map_dumb(&buf)
            .map_err(|e| format!("MAP_DUMB/mmap: {e}"))?;
        paint(map.as_mut_slice(), &buf);
        let fb = card.add_fb(&buf).map_err(|e| format!("ADDFB: {e}"))?;
        info!(
            DRM,
            "framebuffer {i}: fb {fb}, {w}x{h} XRGB8888, pitch {} bytes", buf.pitch
        );
        bufs.push((buf, fb, map));
    }

    card.set_crtc(out.crtc, bufs[0].1, out.conn.id, &out.mode)
        .map_err(|e| format!("SETCRTC (modeset) failed: {e}"))?;
    info!(
        DRM,
        "modeset done: {} {} on CRTC {}; frame on screen", out.conn.name, out.mode, out.crtc
    );

    flip_test(
        out,
        &bufs.iter().map(|b| b.1).collect::<Vec<_>>(),
        args.frames,
    );

    if args.hold > 0 {
        info!(DRM, "holding frame for {}s", args.hold);
        std::thread::sleep(Duration::from_secs(args.hold));
    }

    for (buf, fb, map) in bufs {
        drop(map);
        let _ = card.rm_fb(fb);
        let _ = card.destroy_dumb(&buf);
    }
    info!(DRM, "done");
    Ok(())
}

/// Page-flips between the framebuffers for `frames` vblanks and reports the
/// interval between flip completions measured by the kernel.
fn flip_test(out: &Output, fbs: &[u32], frames: u32) {
    if frames == 0 {
        return;
    }
    let monotonic = out.card.cap(sys::DRM_CAP_TIMESTAMP_MONOTONIC).unwrap_or(0) == 1;
    let mut times = Vec::with_capacity(frames as usize);
    for n in 0..frames {
        let fb = fbs[(n as usize + 1) % fbs.len()];
        if let Err(e) = out.card.page_flip(out.crtc, fb, u64::from(n)) {
            warn!(
                DRM,
                "page flip {n}: {e} (driver may not support page flips)"
            );
            return;
        }
        match out.card.read_flip_events() {
            Ok(events) => times.extend(events.iter().map(|e| e.time_us)),
            Err(e) => {
                warn!(DRM, "reading flip event: {e}");
                return;
            }
        }
    }
    let intervals: Vec<u64> = times
        .windows(2)
        .map(|w| w[1].saturating_sub(w[0]))
        .collect();
    if intervals.is_empty() {
        warn!(DRM, "page flip: no events received");
        return;
    }
    let mean = intervals.iter().sum::<u64>() as f64 / intervals.len() as f64;
    let (min, max) = (
        intervals.iter().min().unwrap(),
        intervals.iter().max().unwrap(),
    );
    let mode_hz = f64::from(out.mode.refresh_mhz()) / 1000.0;
    info!(DRM, "page flip: {} flips completed, interval mean {:.2} ms ({:.1} Hz), min {:.2} ms, max {:.2} ms; mode {:.2} Hz; timestamps {}",
        times.len(), mean / 1000.0, 1e6 / mean, *min as f64 / 1000.0, *max as f64 / 1000.0, mode_hz,
        if monotonic { "monotonic" } else { "realtime" });
}
