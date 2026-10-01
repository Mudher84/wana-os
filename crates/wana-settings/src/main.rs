use std::path::PathBuf;
use std::process::ExitCode;
use std::time::{Duration, Instant};
use wana_client::app::{App, Window};
use wana_log::{error, info, Subsystem};
use wana_settings::config::Settings;
use wana_text::font::Font;
use wana_text::layout::FontSet;
use wana_text::{fonts, sha256};

const LOG: Subsystem = Subsystem::Shell;
const KEY_ESC: u32 = 1;
const KEY_ENTER: u32 = 28;
const KEY_LEFT: u32 = 105;
const KEY_RIGHT: u32 = 106;
const KEY_UP: u32 = 103;
const KEY_DOWN: u32 = 108;

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

fn rotate(current: &str, values: &[&str], forward: bool) -> String {
    let index = values.iter().position(|value| *value == current).unwrap_or(0);
    let next = if forward {
        (index + 1) % values.len()
    } else if index == 0 {
        values.len() - 1
    } else {
        index - 1
    };
    values[next].to_string()
}

fn change(state: &mut Settings, selected: usize, forward: bool) -> Result<(), String> {
    let (key, value) = match selected {
        0 => (
            "language",
            rotate(&state.language, &["ar", "en"], forward),
        ),
        1 => ("theme", rotate(&state.theme, &["dark", "light"], forward)),
        2 => (
            "accent",
            rotate(&state.accent, &["blue", "teal", "violet"], forward),
        ),
        3 => (
            "timezone",
            rotate(
                &state.timezone,
                &[
                    "Asia/Baghdad",
                    "Etc/UTC",
                    "Europe/London",
                    "Europe/Paris",
                    "America/New_York",
                    "Asia/Dubai",
                    "Asia/Riyadh",
                ],
                forward,
            ),
        ),
        _ => return Ok(()),
    };
    state.set(key, &value)
}

fn present(
    app: &App,
    window: &Window,
    fonts: &FontSet,
    state: &Settings,
    selected: usize,
) -> Result<String, String> {
    let canvas = wana_settings::draw::settings_selected(fonts, state, Some(selected))?;
    let hash = sha256::hex(&sha256::digest(&canvas.bytes()));
    window.present(app, &canvas.bytes())?;
    Ok(hash)
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

    let app = App::connect()?;
    let keyboard = app
        .keyboard()?
        .ok_or("settings requires a keyboard seat")?;
    let window = Window::new(
        &app,
        "الإعدادات — وانا",
        "org.wana.Settings",
        wana_settings::draw::WIDTH as i32,
        wana_settings::draw::HEIGHT as i32,
    )?;

    let mut selected = 0usize;
    let hash = present(&app, &window, &set, &state, selected)?;
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
            if let Some((key, pressed)) = app.key_event(keyboard, &ev) {
                if !pressed {
                    continue;
                }
                let mut redraw = false;
                match key {
                    KEY_UP => {
                        selected = selected.saturating_sub(1);
                        redraw = true;
                    }
                    KEY_DOWN => {
                        selected = (selected + 1).min(3);
                        redraw = true;
                    }
                    KEY_LEFT => {
                        change(&mut state, selected, false)?;
                        redraw = true;
                    }
                    KEY_RIGHT => {
                        change(&mut state, selected, true)?;
                        redraw = true;
                    }
                    KEY_ENTER => {
                        state.save_atomic(&a.config)?;
                        info!(
                            LOG,
                            "settings saved from UI: language={} theme={} accent={} timezone={}; session-wide changes apply on next desktop session",
                            state.language,
                            state.theme,
                            state.accent,
                            state.timezone
                        );
                    }
                    KEY_ESC => {
                        window.destroy(&app);
                        return Ok(());
                    }
                    _ => {}
                }
                if redraw {
                    let hash = present(&app, &window, &set, &state, selected)?;
                    info!(
                        LOG,
                        "settings preview: selected={} language={} theme={} accent={} timezone={} sha256 {hash}",
                        selected + 1,
                        state.language,
                        state.theme,
                        state.accent,
                        state.timezone
                    );
                }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rotation_wraps_in_both_directions() {
        assert_eq!(rotate("ar", &["ar", "en"], true), "en");
        assert_eq!(rotate("en", &["ar", "en"], true), "ar");
        assert_eq!(rotate("ar", &["ar", "en"], false), "en");
    }

    #[test]
    fn ui_changes_use_valid_settings_values() {
        let mut state = Settings::default();
        change(&mut state, 0, true).unwrap();
        change(&mut state, 1, true).unwrap();
        change(&mut state, 2, true).unwrap();
        change(&mut state, 3, true).unwrap();
        assert_eq!(state.language, "en");
        assert_eq!(state.theme, "light");
        assert_eq!(state.accent, "teal");
        assert_eq!(state.timezone, "Etc/UTC");
    }
}
