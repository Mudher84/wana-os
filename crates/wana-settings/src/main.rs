mod draw;
mod settings;

use settings::Settings;
use std::path::PathBuf;
use std::process::ExitCode;
use wana_client::window::{Window, WindowEvent};
use wana_log::{error, info, Subsystem};
use wana_text::font::Font;
use wana_text::layout::FontSet;
use wana_text::fonts;

const LOG: Subsystem = Subsystem::Shell;

fn run() -> Result<(), String> {
    let mut config = PathBuf::from("/var/lib/wana/settings.conf");
    let mut set = Vec::new();
    let mut dump = false;
    let mut it = std::env::args().skip(1);
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--config" => config = it.next().ok_or("--config needs a path")?.into(),
            "--set" => set.push(it.next().ok_or("--set needs key=value")?),
            "--dump" => dump = true,
            other => return Err(format!("unknown argument {other}")),
        }
    }

    let mut state = Settings::load(&config)?;
    for change in &set {
        let (key, value) = change
            .split_once('=')
            .ok_or_else(|| format!("--set {change:?}: expected key=value"))?;
        state.set(key, value)?;
    }
    if !set.is_empty() {
        state.save(&config)?;
        info!(LOG, "settings saved: {}", config.display());
    }
    if dump {
        print!("{}", state.canonical());
        return Ok(());
    }
    if !set.is_empty() {
        return Ok(());
    }

    fonts::verify_dir(std::path::Path::new(fonts::DEFAULT_DIR))?;
    let fonts = FontSet {
        fonts: ["NotoSans-VF.ttf", "NotoSansArabic-VF.ttf"]
            .iter()
            .map(|name| Font::load(&std::path::Path::new(fonts::DEFAULT_DIR).join(name)))
            .collect::<Result<_, _>>()?,
    };

    let mut window = Window::connect("Wana Settings", "org.wana.settings", 720, 520)?;
    loop {
        let (w, h) = window.size();
        let canvas = draw::screen(w as u32, h as u32, &fonts, &state)?;
        window.present_xrgb(w, h, &canvas.bytes())?;
        match window.next_event(1000)? {
            WindowEvent::Close => break,
            WindowEvent::Configure { .. } | WindowEvent::Other => {}
        }
    }
    Ok(())
}

fn main() -> ExitCode {
    wana_log::init_from_env();
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            error!(LOG, "{e}");
            ExitCode::FAILURE
        }
    }
}
