//! Kernel command line options read by wana-init (`/proc/cmdline`).
//!
//! Options use the `wana.` prefix. The kernel does not pass dotted options to
//! init as arguments or environment, so init reads them from `/proc/cmdline`.
//!
//! | Option | Meaning |
//! |--------|---------|
//! | `wana.log=error\|warn\|info\|debug` | init log level (default `info`) |
//! | `wana.test=poweroff\|reboot` | automated test boot: stop the machine once init is ready |
//! | `wana.shell=0` | do not start the debug shell on the console |
//! | `wana.udev=0` | do not start udevd (static `/dev` from devtmpfs only) |
//! | `wana.run=/abs/path[,arg...]` | run one program after `ready` and wait for it (bring-up/tests); commas separate arguments |

use wana_log::Level;

/// What an automated test boot should do after reaching `ready`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TestAction {
    PowerOff,
    Reboot,
}

/// Options parsed from the kernel command line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Options {
    pub log_level: Level,
    pub test: Option<TestAction>,
    pub shell: bool,
    /// Start udevd and coldplug devices (when udevd is installed).
    pub udev: bool,
    /// Program (absolute path) and arguments to run once after `ready`.
    pub run: Option<Vec<String>>,
    /// Unknown `wana.*` options or bad values, reported as warnings.
    pub warnings: Vec<String>,
}

impl Default for Options {
    fn default() -> Self {
        Options {
            log_level: Level::Info,
            test: None,
            shell: true,
            udev: true,
            run: None,
            warnings: Vec::new(),
        }
    }
}

/// Parses a kernel command line. Never fails: an unusable option becomes a
/// warning and the default is kept, because a typo must not stop the boot.
pub fn parse(cmdline: &str) -> Options {
    let mut opts = Options::default();
    for word in cmdline.split_ascii_whitespace() {
        let Some(rest) = word.strip_prefix("wana.") else {
            continue;
        };
        let (key, value) = rest.split_once('=').unwrap_or((rest, ""));
        match key {
            "log" => match value.parse() {
                Ok(level) => opts.log_level = level,
                Err(e) => opts.warnings.push(format!("wana.log: {e}")),
            },
            "test" => match value {
                "poweroff" => opts.test = Some(TestAction::PowerOff),
                "reboot" => opts.test = Some(TestAction::Reboot),
                other => opts
                    .warnings
                    .push(format!("wana.test: unknown action {other:?}")),
            },
            "shell" | "udev" => match parse_bool(value) {
                Some(on) if key == "shell" => opts.shell = on,
                Some(on) => opts.udev = on,
                None => opts
                    .warnings
                    .push(format!("wana.{key}: unknown value {value:?}")),
            },
            "run" => {
                let argv: Vec<String> = value.split(',').map(str::to_owned).collect();
                if argv[0].starts_with('/') {
                    opts.run = Some(argv);
                } else {
                    opts.warnings
                        .push(format!("wana.run: {value:?} is not an absolute path"));
                }
            }
            other => opts.warnings.push(format!("unknown option wana.{other}")),
        }
    }
    opts
}

fn parse_bool(value: &str) -> Option<bool> {
    match value {
        "0" | "off" | "no" => Some(false),
        "1" | "on" | "yes" => Some(true),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_when_no_wana_options() {
        assert_eq!(parse("console=ttyS0 panic=-1 quiet"), Options::default());
    }

    #[test]
    fn parses_all_options() {
        let o = parse("console=ttyS0 wana.log=debug wana.test=poweroff wana.shell=0\n");
        assert_eq!(o.log_level, Level::Debug);
        assert_eq!(o.test, Some(TestAction::PowerOff));
        assert!(!o.shell);
        assert!(o.warnings.is_empty());
    }

    #[test]
    fn bad_values_become_warnings_and_keep_defaults() {
        let o = parse("wana.log=loud wana.test=explode wana.shell=maybe wana.color=blue");
        assert_eq!(o.log_level, Level::Info);
        assert_eq!(o.test, None);
        assert!(o.shell);
        assert_eq!(o.warnings.len(), 4);
    }

    #[test]
    fn udev_can_be_disabled() {
        assert!(parse("").udev);
        let o = parse("wana.udev=0");
        assert!(!o.udev);
        assert!(o.shell);
        assert!(o.warnings.is_empty());
        assert_eq!(parse("wana.udev=later").warnings.len(), 1);
    }

    #[test]
    fn last_value_wins() {
        assert_eq!(
            parse("wana.test=poweroff wana.test=reboot").test,
            Some(TestAction::Reboot)
        );
    }

    #[test]
    fn run_splits_program_and_arguments() {
        let o = parse("wana.run=/usr/bin/wana-kms,--hold,3 wana.test=poweroff");
        assert_eq!(
            o.run,
            Some(vec![
                "/usr/bin/wana-kms".into(),
                "--hold".into(),
                "3".into()
            ])
        );
        assert!(o.warnings.is_empty());
    }

    #[test]
    fn run_requires_absolute_path() {
        let o = parse("wana.run=wana-kms");
        assert_eq!(o.run, None);
        assert_eq!(o.warnings.len(), 1);
    }

    #[test]
    fn option_without_value_is_a_warning() {
        let o = parse("wana.test");
        assert_eq!(o.test, None);
        assert_eq!(o.warnings.len(), 1);
    }
}
