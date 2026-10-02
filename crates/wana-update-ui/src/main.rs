use std::path::PathBuf;
use std::process::ExitCode;
use wana_client::app::{App, Window};
use wana_log::{error, info, Subsystem};
use wana_power::{request as power_request, Command as PowerCommand};
use wana_update::broker_request as update_request;
use wana_text::bidi::Base;
use wana_text::font::Font;
use wana_text::layout::{layout, Align, FontSet, Style};
use wana_text::raster::{draw, Canvas};
use wana_text::fonts;

const LOG: Subsystem = Subsystem::Shell;
const WIDTH: u32 = 720;
const HEIGHT: u32 = 420;
const KEY_ESC: u32 = 1;
const KEY_ENTER: u32 = 28;
const KEY_DELETE: u32 = 111;
const PR_SET_DUMPABLE: i32 = 4;

unsafe extern "C" {
    fn prctl(option: i32, arg2: usize, arg3: usize, arg4: usize, arg5: usize) -> i32;
}

fn protect_trusted_process() -> Result<(), String> {
    // SAFETY: PR_SET_DUMPABLE with arg2=0 changes only this process attribute.
    if unsafe { prctl(PR_SET_DUMPABLE, 0, 0, 0, 0) } == 0 {
        Ok(())
    } else {
        Err(format!("PR_SET_DUMPABLE=0: {}", std::io::Error::last_os_error()))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum State {
    Ready,
    Downloading,
    Current(String),
    Pending(String),
    Error(String),
}

fn initial_state() -> State {
    match update_request("status") {
        Ok(reply) if reply.starts_with("PENDING ") => State::Pending(reply),
        Ok(_) => State::Ready,
        Err(e) => State::Error(e),
    }
}

fn fill(c: &mut Canvas, x: u32, y: u32, w: u32, h: u32, rgb: u32) {
    for row in y..(y + h).min(c.height) {
        let start = (row * c.width + x.min(c.width)) as usize;
        let end = (row * c.width + (x + w).min(c.width)) as usize;
        c.pixels[start..end].fill(0xFF00_0000 | rgb);
    }
}

fn label(
    c: &mut Canvas,
    fonts: &FontSet,
    value: &str,
    y: f32,
    size: f32,
    rgb: u32,
) -> Result<(), String> {
    let style = Style {
        size,
        base: Base::Rtl,
        align: Align::Start,
        width: Some(c.width as f32 - 96.0),
        language: "ar".into(),
    };
    let shaped = layout(value, fonts, &style)?;
    draw(c, &shaped, fonts, size, 48.0, y, rgb);
    Ok(())
}

fn short_detail(raw: &str) -> String {
    raw.split_whitespace()
        .find(|part| part.starts_with("version="))
        .map(|part| part.trim_start_matches("version=").to_string())
        .unwrap_or_else(|| raw.chars().take(48).collect())
}

fn render(fonts: &FontSet, state: &State) -> Result<Canvas, String> {
    let palette = wana_theme::current();
    let mut canvas = Canvas::new(WIDTH, HEIGHT, palette.bg);
    label(&mut canvas, fonts, "تحديث النظام", 28.0, 30.0, palette.text)?;

    fill(&mut canvas, 40, 98, WIDTH - 80, 190, palette.card);
    match state {
        State::Ready => {
            label(
                &mut canvas,
                fonts,
                "افحص آخر إصدار Stable الرسمي من Wana OS",
                126.0,
                21.0,
                palette.text,
            )?;
            label(
                &mut canvas,
                fonts,
                "Enter — فحص وتنزيل التحديث",
                190.0,
                17.0,
                palette.accent,
            )?;
        }
        State::Downloading => {
            label(
                &mut canvas,
                fonts,
                "جاري تنزيل والتحقق من التحديث…",
                150.0,
                23.0,
                palette.accent,
            )?;
            label(
                &mut canvas,
                fonts,
                "يبقى النظام الحالي فعالاً إلى أن يكتمل staging بالكامل",
                205.0,
                15.0,
                palette.dim,
            )?;
        }
        State::Current(version) => {
            label(
                &mut canvas,
                fonts,
                "أنت على آخر إصدار Stable",
                140.0,
                24.0,
                palette.accent,
            )?;
            label(
                &mut canvas,
                fonts,
                &format!("الإصدار: {}", short_detail(version)),
                205.0,
                16.0,
                palette.dim,
            )?;
        }
        State::Pending(version) => {
            label(
                &mut canvas,
                fonts,
                "التحديث جاهز للتطبيق",
                132.0,
                24.0,
                palette.accent,
            )?;
            label(
                &mut canvas,
                fonts,
                &format!("الإصدار: {}", short_detail(version)),
                185.0,
                16.0,
                palette.text,
            )?;
            label(
                &mut canvas,
                fonts,
                "Enter — إعادة التشغيل والتحديث   Delete — إلغاء",
                235.0,
                15.0,
                palette.dim,
            )?;
        }
        State::Error(message) => {
            label(
                &mut canvas,
                fonts,
                "تعذر إكمال عملية التحديث",
                132.0,
                23.0,
                palette.danger,
            )?;
            label(
                &mut canvas,
                fonts,
                &message.chars().take(80).collect::<String>(),
                190.0,
                14.0,
                palette.dim,
            )?;
            label(
                &mut canvas,
                fonts,
                "Enter — المحاولة من جديد",
                242.0,
                15.0,
                palette.accent,
            )?;
        }
    }
    label(
        &mut canvas,
        fonts,
        "Esc — إغلاق",
        365.0,
        13.0,
        palette.dim,
    )?;
    Ok(canvas)
}

fn present(app: &App, window: &Window, fonts: &FontSet, state: &State) -> Result<(), String> {
    let canvas = render(fonts, state)?;
    window.present(app, &canvas.bytes())
}

fn fetch() -> State {
    match update_request("fetch-stage") {
        Ok(reply) if reply.starts_with("CURRENT ") => State::Current(reply),
        Ok(reply) if reply.starts_with("STAGED ") => State::Pending(reply),
        Ok(reply) => State::Error(format!("رد غير متوقع: {reply}")),
        Err(e) => State::Error(e),
    }
}

fn run() -> Result<(), String> {
    protect_trusted_process()?;
    let font_dir = PathBuf::from(fonts::DEFAULT_DIR);
    fonts::verify_dir(&font_dir)?;
    let fonts = FontSet {
        fonts: ["NotoSans-VF.ttf", "NotoSansArabic-VF.ttf"]
            .iter()
            .map(|name| Font::load(&font_dir.join(name)))
            .collect::<Result<_, _>>()?,
    };
    let app = App::connect()?;
    let keyboard = app.keyboard()?.ok_or("update UI requires a keyboard seat")?;
    let window = Window::new(
        &app,
        "تحديث النظام — وانا",
        "org.wana.Update",
        WIDTH as i32,
        HEIGHT as i32,
    )?;

    let mut state = initial_state();
    present(&app, &window, &fonts, &state)?;
    info!(LOG, "update UI mapped: {}x{} state={state:?}", WIDTH, HEIGHT);

    loop {
        app.conn.dispatch()?;
        while let Some(event) = app.conn.next_event() {
            if window.close_event(&event) {
                window.destroy(&app);
                return Ok(());
            }
            if let Some((key, pressed)) = app.key_event(keyboard, &event) {
                if !pressed {
                    continue;
                }
                match key {
                    KEY_ENTER => match &state {
                        State::Pending(_) => {
                            info!(LOG, "update UI confirmed reboot into staged update");
                            power_request(PowerCommand::Reboot)?;
                        }
                        _ => {
                            state = State::Downloading;
                            present(&app, &window, &fonts, &state)?;
                            state = fetch();
                            present(&app, &window, &fonts, &state)?;
                            info!(LOG, "update UI fetch result: {state:?}");
                        }
                    },
                    KEY_DELETE if matches!(state, State::Pending(_)) => {
                        match update_request("clear") {
                            Ok(_) => state = State::Ready,
                            Err(e) => state = State::Error(e),
                        }
                        present(&app, &window, &fonts, &state)?;
                    }
                    KEY_ESC => {
                        window.destroy(&app);
                        return Ok(());
                    }
                    _ => {}
                }
            }
            app.protocol_event(&event)?;
        }
    }
}

fn main() -> ExitCode {
    wana_log::init_from_env();
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            error!(LOG, "update UI: {e}");
            ExitCode::FAILURE
        }
    }
}
