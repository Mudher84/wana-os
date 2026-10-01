//! wana-input: input bring-up tool and test (Phase 9).
//!
//! Opens the seat's input devices through udev + libinput, compiles the
//! keyboard layout with xkbcommon, then logs every key, pointer and button
//! event as an `[INPUT]` line. With `--expect-*` it exits 0 once the
//! expected input arrived and 1 on timeout, so a boot test can inject input
//! (QEMU monitor) and check the whole path: evdev -> udev -> libinput ->
//! xkbcommon -> Wana.
//!
//! Usage: wana-input [--seat seat0] [--layout us] [--timeout SECONDS] [--list]
//!                   [--expect-text TEXT] [--expect-pointer] [--expect-button]
//!
//! `--expect-button` waits for a full left click (press, then release).

use std::os::raw::c_int;
use std::process::ExitCode;
use std::time::{Duration, Instant};
use wana_input::ffi;
use wana_input::keyboard::Keyboard;
use wana_input::libinput::{Capability, Event, EventKind, Libinput};
use wana_input::names::{button_name, Typed, BTN_LEFT};
use wana_log::{debug, error, info, Subsystem};

const INPUT: Subsystem = Subsystem::Input;

struct Args {
    seat: String,
    layout: String,
    /// 0 = run until killed.
    timeout: u64,
    list: bool,
    expect_text: Option<String>,
    expect_pointer: bool,
    expect_button: bool,
}

impl Args {
    fn expects_something(&self) -> bool {
        self.expect_text.is_some() || self.expect_pointer || self.expect_button
    }
}

fn parse_args() -> Result<Args, String> {
    let mut a = Args {
        seat: "seat0".into(),
        layout: "us".into(),
        timeout: 0,
        list: false,
        expect_text: None,
        expect_pointer: false,
        expect_button: false,
    };
    let mut it = std::env::args().skip(1);
    while let Some(arg) = it.next() {
        let mut val = |name: &str| it.next().ok_or(format!("{name} needs a value"));
        match arg.as_str() {
            "--seat" => a.seat = val("--seat")?,
            "--layout" => a.layout = val("--layout")?,
            "--timeout" => {
                a.timeout = val("--timeout")?
                    .parse()
                    .map_err(|e| format!("--timeout: {e}"))?
            }
            "--list" => a.list = true,
            "--expect-text" => a.expect_text = Some(val("--expect-text")?),
            "--expect-pointer" => a.expect_pointer = true,
            "--expect-button" => a.expect_button = true,
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
            error!(INPUT, "{e}");
            return ExitCode::from(2);
        }
    };
    match run(&args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            error!(INPUT, "{e}");
            ExitCode::FAILURE
        }
    }
}

/// What has been seen so far.
#[derive(Default)]
struct Seen {
    typed: Typed,
    keys: u32,
    pointer: u32,
    left_down: bool,
    left_clicks: u32,
}

impl Seen {
    fn satisfies(&self, args: &Args) -> bool {
        args.expect_text
            .as_deref()
            .is_none_or(|t| self.typed.ends_with(t))
            && (!args.expect_pointer || self.pointer > 0)
            && (!args.expect_button || self.left_clicks > 0)
    }
}

fn run(args: &Args) -> Result<(), String> {
    info!(
        INPUT,
        "wana-input {}: seat {}, layout {}",
        env!("CARGO_PKG_VERSION"),
        args.seat,
        args.layout
    );
    let mut kb = Keyboard::new(&args.layout).map_err(|e| format!("keymap: {e}"))?;
    info!(INPUT, "keymap compiled: {} (xkbcommon)", kb.layout_name());

    let mut li = Libinput::new(&args.seat)?;
    li.dispatch()
        .map_err(|e| format!("libinput_dispatch: {e}"))?;
    let (mut devices, mut keyboards, mut pointers) = (0, 0, 0);
    let mut early = Vec::new();
    while let Some(ev) = li.next_event() {
        match ev.kind {
            EventKind::DeviceAdded(d) => {
                info!(
                    INPUT,
                    "device added: {} ({}) [{}]",
                    d.name,
                    d.sysname,
                    d.caps_list()
                );
                devices += 1;
                keyboards += u32::from(d.has(Capability::Keyboard));
                pointers += u32::from(d.has(Capability::Pointer));
            }
            _ => early.push(ev),
        }
    }
    if devices == 0 {
        return Err(format!(
            "no input devices on {} (is udevd running and are devices tagged ID_INPUT?)",
            args.seat
        ));
    }
    info!(
        INPUT,
        "{}: {devices} devices, {keyboards} with keyboard, {pointers} with pointer", args.seat
    );
    if args.list {
        return Ok(());
    }

    let mut seen = Seen::default();
    for ev in early {
        handle(ev, &mut kb, &mut seen);
    }
    let deadline = (args.timeout > 0).then(|| Instant::now() + Duration::from_secs(args.timeout));
    match deadline {
        Some(_) => info!(INPUT, "waiting for input (timeout {}s)", args.timeout),
        None => info!(INPUT, "waiting for input"),
    }

    loop {
        if args.expects_something() && seen.satisfies(args) {
            if let Some(t) = &args.expect_text {
                info!(INPUT, "typed text {:?} matches {t:?}", seen.typed.as_str());
            }
            info!(
                INPUT,
                "done: {} key presses, {} pointer events, {} left clicks",
                seen.keys,
                seen.pointer,
                seen.left_clicks
            );
            return Ok(());
        }
        let wait_ms: c_int = match deadline {
            None => -1,
            Some(d) => {
                let left = d.saturating_duration_since(Instant::now());
                if left.is_zero() {
                    return timeout(args, &seen);
                }
                left.as_millis().min(1000) as c_int
            }
        };
        let mut pfd = ffi::pollfd {
            fd: li.fd(),
            events: ffi::POLLIN,
            revents: 0,
        };
        // SAFETY: one valid pollfd for the duration of the call.
        let rc = unsafe { ffi::poll(&mut pfd, 1, wait_ms) };
        if rc < 0 {
            let e = std::io::Error::last_os_error();
            if e.kind() != std::io::ErrorKind::Interrupted {
                return Err(format!("poll: {e}"));
            }
        }
        li.dispatch()
            .map_err(|e| format!("libinput_dispatch: {e}"))?;
        while let Some(ev) = li.next_event() {
            handle(ev, &mut kb, &mut seen);
        }
    }
}

fn timeout(args: &Args, seen: &Seen) -> Result<(), String> {
    if !args.expects_something() {
        info!(INPUT, "timeout reached after {}s", args.timeout);
        return Ok(());
    }
    Err(format!(
        "timeout after {}s: typed {:?} (expected {:?}), {} pointer events, {} left clicks",
        args.timeout,
        seen.typed.as_str(),
        args.expect_text.as_deref().unwrap_or(""),
        seen.pointer,
        seen.left_clicks
    ))
}

fn handle(ev: Event, kb: &mut Keyboard, seen: &mut Seen) {
    let dev = ev.device;
    match ev.kind {
        EventKind::Key { code, pressed } => {
            let k = kb.key(code, pressed);
            if pressed {
                seen.keys += 1;
                seen.typed.push(&k.text);
                info!(
                    INPUT,
                    "{dev}: key {code} pressed: {} text {:?}", k.keysym, k.text
                );
            } else {
                debug!(INPUT, "{dev}: key {code} released: {}", k.keysym);
            }
        }
        EventKind::Motion { dx, dy } => {
            seen.pointer += 1;
            info!(INPUT, "{dev}: pointer motion dx {dx:.2} dy {dy:.2}");
        }
        EventKind::MotionAbsolute { x, y } => {
            seen.pointer += 1;
            info!(INPUT, "{dev}: pointer position x {x:.3} y {y:.3}");
        }
        EventKind::Button { code, pressed } => {
            if code == BTN_LEFT {
                if !pressed && seen.left_down {
                    seen.left_clicks += 1;
                }
                seen.left_down = pressed;
            }
            let state = if pressed { "pressed" } else { "released" };
            info!(INPUT, "{dev}: button {} {state}", button_name(code));
        }
        EventKind::Scroll {
            vertical,
            horizontal,
        } => {
            seen.pointer += 1;
            info!(
                INPUT,
                "{dev}: scroll vertical {vertical} horizontal {horizontal} (1/120 notch)"
            );
        }
        EventKind::DeviceAdded(d) => info!(
            INPUT,
            "device added: {} ({}) [{}]",
            d.name,
            d.sysname,
            d.caps_list()
        ),
        EventKind::DeviceRemoved(d) => info!(INPUT, "device removed: {} ({})", d.name, d.sysname),
        EventKind::Other(kind) => debug!(INPUT, "{dev}: unhandled libinput event type {kind}"),
    }
}
