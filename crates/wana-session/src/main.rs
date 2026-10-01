use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use wana_log::{error, info, warn, Subsystem};
use wana_settings::config::Settings;

const LOG: Subsystem = Subsystem::Shell;

fn settings_path() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .unwrap_or_else(|| PathBuf::from("/home/wana"))
        .join(".config/wana/settings.conf")
}

fn locale(language: &str) -> &'static str {
    if language == "en" {
        "en_US.UTF-8"
    } else {
        "ar_IQ.UTF-8"
    }
}

fn timezone(settings: &Settings) -> String {
    let zone = Path::new("/usr/share/zoneinfo").join(&settings.timezone);
    if zone.is_file() {
        format!(":{}", settings.timezone)
    } else {
        warn!(
            LOG,
            "timezone {} missing from tzdata; falling back to Asia/Baghdad",
            settings.timezone
        );
        ":Asia/Baghdad".into()
    }
}

fn run() -> Result<(), String> {
    let mut argv = std::env::args_os().skip(1);
    let program = argv
        .next()
        .ok_or("usage: wana-session PROGRAM [ARG ...]")?;
    let args: Vec<_> = argv.collect();

    let path = settings_path();
    let settings = Settings::load(&path)?;
    let lang = locale(&settings.language);
    let tz = timezone(&settings);

    info!(
        LOG,
        "session environment: language={} locale={} timezone={} theme={} accent={}",
        settings.language,
        lang,
        settings.timezone,
        settings.theme,
        settings.accent
    );

    let mut command = Command::new(&program);
    command
        .args(&args)
        .env("LANG", lang)
        .env("LC_ALL", lang)
        .env("TZ", tz)
        .env("WANA_LANGUAGE", &settings.language)
        .env("WANA_THEME", &settings.theme)
        .env("WANA_ACCENT", &settings.accent);
    let err = command.exec();
    Err(format!("exec {:?}: {err}", program))
}

fn main() {
    wana_log::init_from_env();
    if let Err(e) = run() {
        error!(LOG, "session: {e}");
        std::process::exit(1);
    }
}
