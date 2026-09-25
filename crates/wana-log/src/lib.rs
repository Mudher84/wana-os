//! Subsystem-tagged logging for Wana OS.
//!
//! Every line has the form `[TAG] level: message`, e.g.
//! `[DRM] info: connector HDMI-A-1 connected, 1920x1080@60`.
//! The fixed prefix lets CI and developers grep a serial console log for the
//! first failing layer of the boot/graphics stack.
//!
//! Output goes to stderr, with each record written in a single `write` call
//! so lines from concurrent processes sharing a console do not interleave.

#![forbid(unsafe_code)]

use std::fmt;
use std::io::Write;
use std::str::FromStr;
use std::sync::atomic::{AtomicU8, Ordering};

/// The layer of the system a log line comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Subsystem {
    Boot,
    Kernel,
    Init,
    Drm,
    Gbm,
    Egl,
    Render,
    Input,
    Compositor,
    Shell,
    Network,
    Installer,
    Security,
}

impl Subsystem {
    /// All subsystems, in boot-stack order.
    pub const ALL: [Subsystem; 13] = [
        Subsystem::Boot,
        Subsystem::Kernel,
        Subsystem::Init,
        Subsystem::Drm,
        Subsystem::Gbm,
        Subsystem::Egl,
        Subsystem::Render,
        Subsystem::Input,
        Subsystem::Compositor,
        Subsystem::Shell,
        Subsystem::Network,
        Subsystem::Installer,
        Subsystem::Security,
    ];

    /// The tag printed between brackets.
    pub const fn tag(self) -> &'static str {
        match self {
            Subsystem::Boot => "BOOT",
            Subsystem::Kernel => "KERNEL",
            Subsystem::Init => "INIT",
            Subsystem::Drm => "DRM",
            Subsystem::Gbm => "GBM",
            Subsystem::Egl => "EGL",
            Subsystem::Render => "RENDER",
            Subsystem::Input => "INPUT",
            Subsystem::Compositor => "COMPOSITOR",
            Subsystem::Shell => "SHELL",
            Subsystem::Network => "NETWORK",
            Subsystem::Installer => "INSTALLER",
            Subsystem::Security => "SECURITY",
        }
    }
}

impl fmt::Display for Subsystem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.tag())
    }
}

/// Severity. Lower values are more severe.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(u8)]
pub enum Level {
    Error = 0,
    Warn = 1,
    Info = 2,
    Debug = 3,
}

impl Level {
    pub const fn as_str(self) -> &'static str {
        match self {
            Level::Error => "error",
            Level::Warn => "warn",
            Level::Info => "info",
            Level::Debug => "debug",
        }
    }

    const fn from_u8(v: u8) -> Level {
        match v {
            0 => Level::Error,
            1 => Level::Warn,
            2 => Level::Info,
            _ => Level::Debug,
        }
    }
}

impl fmt::Display for Level {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Error returned when parsing an unknown level name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseLevelError(String);

impl fmt::Display for ParseLevelError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "unknown log level {:?} (expected error|warn|info|debug)",
            self.0
        )
    }
}

impl std::error::Error for ParseLevelError {}

impl FromStr for Level {
    type Err = ParseLevelError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim().to_ascii_lowercase().as_str() {
            "error" => Ok(Level::Error),
            "warn" | "warning" => Ok(Level::Warn),
            "info" => Ok(Level::Info),
            "debug" => Ok(Level::Debug),
            _ => Err(ParseLevelError(s.to_owned())),
        }
    }
}

static MAX_LEVEL: AtomicU8 = AtomicU8::new(Level::Info as u8);

/// Environment variable read by [`init_from_env`].
pub const LEVEL_ENV: &str = "WANA_LOG";

/// Sets the most verbose level that will be emitted.
pub fn set_max_level(level: Level) {
    MAX_LEVEL.store(level as u8, Ordering::Relaxed);
}

/// Returns the most verbose level that will be emitted.
pub fn max_level() -> Level {
    Level::from_u8(MAX_LEVEL.load(Ordering::Relaxed))
}

/// Returns true if a record at `level` would be emitted.
pub fn enabled(level: Level) -> bool {
    level <= max_level()
}

/// Sets the max level from `WANA_LOG` (default `info`).
///
/// An invalid value keeps the default and is reported as a warning, because
/// logging setup must never abort early boot.
pub fn init_from_env() {
    if let Ok(value) = std::env::var(LEVEL_ENV) {
        match value.parse() {
            Ok(level) => set_max_level(level),
            Err(err) => log(
                Subsystem::Boot,
                Level::Warn,
                format_args!("{LEVEL_ENV}: {err}"),
            ),
        }
    }
}

/// Formats one record. Embedded newlines produce additional lines that carry
/// the same prefix, so every output line stays greppable by tag.
pub fn format_record(subsystem: Subsystem, level: Level, args: fmt::Arguments<'_>) -> String {
    let message = args.to_string();
    let mut out = String::with_capacity(message.len() + 24);
    for line in message.trim_end_matches('\n').split('\n') {
        out.push('[');
        out.push_str(subsystem.tag());
        out.push_str("] ");
        out.push_str(level.as_str());
        out.push_str(": ");
        out.push_str(line);
        out.push('\n');
    }
    out
}

/// Emits one record to stderr if `level` is enabled.
pub fn log(subsystem: Subsystem, level: Level, args: fmt::Arguments<'_>) {
    if !enabled(level) {
        return;
    }
    let line = format_record(subsystem, level, args);
    // Logging failures are deliberately ignored: there is nowhere to report them.
    let _ = std::io::stderr().lock().write_all(line.as_bytes());
}

/// `error!(Subsystem::Drm, "open {}: {}", path, err)`
#[macro_export]
macro_rules! error {
    ($sub:expr, $($arg:tt)+) => { $crate::log($sub, $crate::Level::Error, format_args!($($arg)+)) };
}

/// `warn!(Subsystem::Drm, "...")`
#[macro_export]
macro_rules! warn {
    ($sub:expr, $($arg:tt)+) => { $crate::log($sub, $crate::Level::Warn, format_args!($($arg)+)) };
}

/// `info!(Subsystem::Drm, "...")`
#[macro_export]
macro_rules! info {
    ($sub:expr, $($arg:tt)+) => { $crate::log($sub, $crate::Level::Info, format_args!($($arg)+)) };
}

/// `debug!(Subsystem::Drm, "...")`
#[macro_export]
macro_rules! debug {
    ($sub:expr, $($arg:tt)+) => { $crate::log($sub, $crate::Level::Debug, format_args!($($arg)+)) };
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn tags_match_the_documented_set() {
        let tags: Vec<&str> = Subsystem::ALL.iter().map(|s| s.tag()).collect();
        assert_eq!(
            tags,
            [
                "BOOT",
                "KERNEL",
                "INIT",
                "DRM",
                "GBM",
                "EGL",
                "RENDER",
                "INPUT",
                "COMPOSITOR",
                "SHELL",
                "NETWORK",
                "INSTALLER",
                "SECURITY"
            ]
        );
        let unique: HashSet<&str> = tags.iter().copied().collect();
        assert_eq!(unique.len(), tags.len());
    }

    #[test]
    fn formats_single_line() {
        let line = format_record(
            Subsystem::Drm,
            Level::Info,
            format_args!("mode {}x{}", 1920, 1080),
        );
        assert_eq!(line, "[DRM] info: mode 1920x1080\n");
    }

    #[test]
    fn every_output_line_carries_the_prefix() {
        let out = format_record(Subsystem::Egl, Level::Error, format_args!("a\nb\n"));
        assert_eq!(out, "[EGL] error: a\n[EGL] error: b\n");
    }

    #[test]
    fn parses_levels() {
        assert_eq!("debug".parse::<Level>(), Ok(Level::Debug));
        assert_eq!(" WARNING ".parse::<Level>(), Ok(Level::Warn));
        assert!("verbose".parse::<Level>().is_err());
    }

    #[test]
    fn level_ordering_and_filtering() {
        assert!(Level::Error < Level::Warn && Level::Info < Level::Debug);
        set_max_level(Level::Warn);
        assert!(enabled(Level::Error));
        assert!(enabled(Level::Warn));
        assert!(!enabled(Level::Info));
        set_max_level(Level::Debug);
        assert!(enabled(Level::Debug));
        set_max_level(Level::Info);
        assert_eq!(max_level(), Level::Info);
    }

    #[test]
    fn level_roundtrips_through_u8() {
        for level in [Level::Error, Level::Warn, Level::Info, Level::Debug] {
            assert_eq!(Level::from_u8(level as u8), level);
        }
    }
}
