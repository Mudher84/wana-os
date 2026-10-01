//! Kernel command line options read by wana-init (`/proc/cmdline`).
//!
//! Options use the `wana.` prefix. The kernel does not pass dotted options to
//! init as arguments or environment, so init reads them from `/proc/cmdline`.
//!
//! | Option | Meaning |
//! |--------|---------|
//! | `wana.log=error\|warn\|info\|debug` | init log level (default `info`) |
//! | `wana.test=poweroff\|reboot` | automated test boot: stop the machine once init is ready |
//! | `wana.shell=1` | explicitly start the debug root shell on the console (default off) |
//! | `wana.udev=0` | do not start udevd (static `/dev` from devtmpfs only) |
//! | `wana.services=0|1` | disable/force system services; automated `wana.test` and one-shot `wana.run` boots skip them by default |
//! | `wana.run=/abs/path[,arg...]` | run one program after `ready` and wait for it (bring-up/tests); commas separate arguments |
//! | `wana.live=1` | booted from the read-only Live ISO/initramfs path |
//! | `wana.update=1` | boot the isolated A/B update environment |
//! | `wana.active=A|B` | currently selected system slot |
//! | `wana.root_a=PARTUUID=...` / `wana.root_b=PARTUUID=...` | A/B root devices |
//! | `wana.data=UUID=...` | persistent Data filesystem used for staged updates |

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
    /// Explicit system-service policy. None means services on for normal boot,
    /// off for automated `wana.test` and one-shot `wana.run` boots.
    pub services: Option<bool>,
    /// Program (absolute path) and arguments to run once after `ready`.
    pub run: Option<Vec<String>>,
    /// Booted from the Live ISO path.
    pub live: bool,
    /// Isolated update environment loaded by GRUB from the staged bundle.
    pub update: bool,
    /// Slot selected before entering the update environment.
    pub active_slot: char,
    pub root_a: Option<String>,
    pub root_b: Option<String>,
    pub data: Option<String>,
    /// Unknown `wana.*` options or bad values, reported as warnings.
    pub warnings: Vec<String>,
}

impl Default for Options {
    fn default() -> Self {
        Options {
            log_level: Level::Info,
            test: None,
            shell: false,
            udev: true,
            services: None,
            run: None,
            live: false,
            update: false,
            active_slot: 'A',
            root_a: None,
            root_b: None,
            data: None,
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
            "live" | "update" => match parse_bool(value) {
                Some(on) if key == "live" => opts.live = on,
                Some(on) => opts.update = on,
                None => opts
                    .warnings
                    .push(format!("wana.{key}: unknown value {value:?}")),
            },
            "active" => match value {
                "A" => opts.active_slot = 'A',
                "B" => opts.active_slot = 'B',
                _ => opts
                    .warnings
                    .push(format!("wana.active: unknown slot {value:?}")),
            },
            "root_a" | "root_b" | "data" => {
                if valid_device_spec(value) {
                    match key {
                        "root_a" => opts.root_a = Some(value.to_owned()),
                        "root_b" => opts.root_b = Some(value.to_owned()),
                        _ => opts.data = Some(value.to_owned()),
                    }
                } else {
                    opts.warnings
                        .push(format!("wana.{key}: invalid device spec {value:?}"));
                }
            }
            "shell" | "udev" | "services" => match parse_bool(value) {
                Some(on) if key == "shell" => opts.shell = on,
                Some(on) if key == "udev" => opts.udev = on,
                Some(on) => opts.services = Some(on),
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

fn valid_device_spec(value: &str) -> bool {
    let Some((kind, id)) = value.split_once('=') else {
        return false;
    };
    matches!(kind, "PARTUUID" | "UUID")
        && !id.is_empty()
        && id.len() <= 64
        && id
            .bytes()
            .all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f' | b'A'..=b'F' | b'-'))
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
        let o = parse("console=ttyS0 wana.log=debug wana.test=poweroff wana.shell=1\n");
        assert_eq!(o.log_level, Level::Debug);
        assert_eq!(o.test, Some(TestAction::PowerOff));
        assert!(o.shell);
        assert!(o.warnings.is_empty());
    }

    #[test]
    fn live_mode_is_explicit_and_validated() {
        let o = parse("wana.live=1");
        assert!(o.live);
        assert!(o.warnings.is_empty());
        assert_eq!(parse("wana.live=maybe").warnings.len(), 1);
    }

    #[test]
    fn bad_values_become_warnings_and_keep_defaults() {
        let o = parse("wana.log=loud wana.test=explode wana.shell=maybe wana.color=blue");
        assert_eq!(o.log_level, Level::Info);
        assert_eq!(o.test, None);
        assert!(!o.shell);
        assert_eq!(o.warnings.len(), 4);
    }

    #[test]
    fn udev_can_be_disabled() {
        assert!(parse("").udev);
        let o = parse("wana.udev=0");
        assert!(!o.udev);
        assert!(!o.shell);
        assert!(o.warnings.is_empty());
        assert_eq!(parse("wana.udev=later").warnings.len(), 1);
    }

    #[test]
    fn service_policy_is_optional_and_validated() {
        assert_eq!(parse("").services, None);
        assert_eq!(parse("wana.services=0").services, Some(false));
        assert_eq!(parse("wana.services=1").services, Some(true));
        assert_eq!(parse("wana.services=maybe").warnings.len(), 1);
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
    fn update_mode_parses_slot_devices_strictly() {
        let o = parse(
            "wana.update=1 wana.active=B wana.root_a=PARTUUID=aaaa-1111 wana.root_b=PARTUUID=bbbb-2222 wana.data=UUID=cccc-3333",
        );
        assert!(o.update);
        assert_eq!(o.active_slot, 'B');
        assert_eq!(o.root_a.as_deref(), Some("PARTUUID=aaaa-1111"));
        assert_eq!(o.root_b.as_deref(), Some("PARTUUID=bbbb-2222"));
        assert_eq!(o.data.as_deref(), Some("UUID=cccc-3333"));
        assert!(o.warnings.is_empty());

        let bad = parse("wana.active=C wana.root_a=/dev/vda");
        assert_eq!(bad.warnings.len(), 2);
    }

    #[test]
    fn option_without_value_is_a_warning() {
        let o = parse("wana.test");
        assert_eq!(o.test, None);
        assert_eq!(o.warnings.len(), 1);
    }
}
