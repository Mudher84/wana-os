use wana_settings::config::Settings;
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::{Duration, Instant};
use wana_client::app::{App, Window};
use wana_log::{error, info, Subsystem};
use wana_text::font::Font;
use wana_text::layout::FontSet;
use wana_text::{fonts, sha256};

const LOG: Subsystem = Subsystem::Shell;

#[derive(Debug)]
struct Args {
    config: PathBuf,
    fonts: PathBuf,
    set: Option<(String, String)>,
    print: bool,
    hold: Option<u64>,
}

fn default_config_path() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .map(|home| home.join(".config/wana/settings.conf"))
        .unwrap_or_else(|| PathBuf::from("/var/lib/wana/settings.conf"))
}

fn args() -> Result<Args, String> {
    let mut out = Args {
        config: default_config_path(),
        fonts: PathBuf::from("/usr/share/fonts/wana"),
        set: None,
        print: false,
        hold: None,
    };
    let mut it = std::env::args().skip(1);
    while let Some(a) = it.next() {
        match a.as_str() {
            "--config" => out.config = it.next().ok_or("--config needs a path")?.into(),
            "--fonts" => out.fonts = it.next().ok_or("--fonts needs a path")?.into(),
            "--set" => {
                let key = it.next().ok_or("--set needs KEY VALUE")?;
                let value = it.next().ok_or("--set needs KEY VALUE")?;
                out.set = Some((key, value));
            }
            "--print" => out.print = true,
            "--hold" => {
                out.hold = Some(
                    it.next()
                        .ok_or("--hold needs seconds")?
                        .parse()
                        .map_err(|e| format!("--hold: {e}"))?,
                );
            }
            _ => return Err(format!("unknown argument {a}")),
        }
    }
    Ok(out)
}

fn run() -> Result<(), String> {
    let a = args()?;
    let mut state = Settings::load(&a.config)?;
    if let Some((key, value)) = &a.set {
        state.set(key, value)?;
        state.save_atomic(&a.config)?;
        info!(LOG, "settings saved: {key}={value}");
    }
    if a.print || a.set.is_some() {
        print!("{}", state.encode());
        return Ok(());
    }

    fonts::verify_dir(&a.fonts)?;
    let set = FontSet {
        fonts: ["NotoSans-VF.ttf", "NotoSansArabic-VF.ttf"]
            .iter()
            .map(|n| Font::load(&a.fonts.join(n)))
            .collect::<Result<_, _>>()?,
    };
    let canvas = wana_settings::draw::settings(&set, &state)?;
    let hash = sha256::hex(&sha256::digest(&canvas.bytes()));

    let app = App::connect()?;
    let window = Window::new(
        &app,
        "الإعدادات — وانا",
        "org.wana.Settings",
        wana_settings::draw::WIDTH as i32,
        wana_settings::draw::HEIGHT as i32,
    )?;
    window.present(&app, &canvas.bytes())?;
    info!(
        LOG,
        "settings mapped: {}x{}, language={}, theme={}, accent={}, timezone={}, sha256 {hash}",
        wana_settings::draw::WIDTH,
        wana_settings::draw::HEIGHT,
        state.language,
        state.theme,
        state.accent,
        state.timezone
    );

    let deadline = a.hold.map(|s| Instant::now() + Duration::from_secs(s));
    loop {
        if deadline.is_some_and(|d| Instant::now() >= d) {
            break;
        }
        app.conn.wait(250)?;
        while let Some(ev) = app.conn.next_event() {
            if window.close_event(&ev) {
                window.destroy(&app);
                return Ok(());
            }
            app.protocol_event(&ev)?;
        }
    }
    window.destroy(&app);
    Ok(())
}

fn main() -> ExitCode {
    wana_log::init_from_env();
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            error!(LOG, "settings: {e}");
            ExitCode::FAILURE
        }
    }
}
