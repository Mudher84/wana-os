use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};
use wana_log::{error, info, Subsystem};

const LOG: Subsystem = Subsystem::Init;

#[derive(Debug, Clone, PartialEq, Eq)]
struct Interface {
    name: String,
    address: String,
    state: String,
    carrier: Option<bool>,
    wireless: bool,
}

fn read(path: impl AsRef<Path>) -> String {
    fs::read_to_string(path)
        .unwrap_or_default()
        .trim()
        .to_string()
}

fn interfaces(root: &Path) -> Result<Vec<Interface>, String> {
    let mut out = Vec::new();
    let dir = root.join("sys/class/net");
    for entry in fs::read_dir(&dir).map_err(|e| format!("{}: {e}", dir.display()))? {
        let entry = entry.map_err(|e| e.to_string())?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if name == "lo" {
            continue;
        }
        let p = entry.path();
        let carrier = match read(p.join("carrier")).as_str() {
            "1" => Some(true),
            "0" => Some(false),
            _ => None,
        };
        out.push(Interface {
            name,
            address: read(p.join("address")),
            state: read(p.join("operstate")),
            carrier,
            wireless: p.join("wireless").exists(),
        });
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(out)
}

fn validate_iface(s: &str) -> Result<(), String> {
    if s.is_empty()
        || s.len() > 15
        || !s
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'.'))
    {
        return Err(format!("invalid interface name {s:?}"));
    }
    Ok(())
}

fn quote_wpa(s: &str) -> Result<String, String> {
    if s.bytes().any(|b| b < 0x20 || b == 0x7f) {
        return Err("wireless value contains control characters".into());
    }
    Ok(s.replace('\\', "\\\\").replace('"', "\\\""))
}

fn wifi_config(ssid: &str, psk: &str) -> Result<String, String> {
    if ssid.is_empty() || ssid.len() > 32 {
        return Err("SSID must be 1..32 bytes".into());
    }
    if !(8..=63).contains(&psk.len()) {
        return Err("WPA passphrase must be 8..63 bytes".into());
    }
    let ssid = quote_wpa(ssid)?;
    let psk = quote_wpa(psk)?;
    Ok(format!(
        "ctrl_interface=/run/wpa_supplicant\nupdate_config=0\nnetwork={{\n    ssid=\"{ssid}\"\n    psk=\"{psk}\"\n    key_mgmt=WPA-PSK SAE\n    ieee80211w=1\n}}\n"
    ))
}

fn write_secret(path: &Path, data: &str) -> Result<(), String> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent).map_err(|e| format!("create {}: {e}", parent.display()))?;
    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| format!("invalid path {}", path.display()))?;
    let tmp: PathBuf = parent.join(format!(".{name}.tmp-{}", std::process::id()));
    let result = (|| -> Result<(), String> {
        let mut f = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&tmp)
            .map_err(|e| format!("create {}: {e}", tmp.display()))?;
        f.write_all(data.as_bytes())
            .map_err(|e| format!("write {}: {e}", tmp.display()))?;
        f.sync_all()
            .map_err(|e| format!("sync {}: {e}", tmp.display()))?;
        fs::rename(&tmp, path).map_err(|e| format!("rename {}: {e}", path.display()))?;
        File::open(parent)
            .and_then(|d| d.sync_all())
            .map_err(|e| format!("sync {}: {e}", parent.display()))?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(tmp);
    }
    result
}

fn run_command(program: &str, args: &[&str]) -> Result<(), String> {
    let status = Command::new(program)
        .args(args)
        .status()
        .map_err(|e| format!("{program}: {e}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("{program} exited {status}"))
    }
}

fn run() -> Result<(), String> {
    let mut args = std::env::args().skip(1);
    let cmd = args.next().unwrap_or_else(|| "status".into());
    match cmd.as_str() {
        "status" => {
            let mut expect = None;
            while let Some(a) = args.next() {
                if a == "--expect" {
                    expect = Some(args.next().ok_or("--expect needs an interface")?);
                } else {
                    return Err(format!("status: unknown argument {a}"));
                }
            }
            let list = interfaces(Path::new("/"))?;
            for i in &list {
                info!(
                    LOG,
                    "network interface {}: state={}, carrier={}, wireless={}, mac={}",
                    i.name,
                    i.state,
                    i.carrier
                        .map(|v| if v { "up" } else { "down" })
                        .unwrap_or("unknown"),
                    i.wireless,
                    i.address
                );
            }
            if let Some(name) = expect {
                if !list.iter().any(|i| i.name == name) {
                    return Err(format!("expected interface {name} not found"));
                }
                info!(LOG, "network expected interface {name}: PASS");
            }
            Ok(())
        }
        "dhcp" => {
            let iface = args.next().ok_or("dhcp needs IFACE")?;
            validate_iface(&iface)?;
            run_command("/sbin/dhcpcd", &["-4", "-q", &iface])?;
            info!(LOG, "DHCP started on {iface}");
            Ok(())
        }
        "wifi-config" => {
            let iface = args.next().ok_or("wifi-config needs IFACE SSID PSK")?;
            let ssid = args.next().ok_or("wifi-config needs IFACE SSID PSK")?;
            let psk = args.next().ok_or("wifi-config needs IFACE SSID PSK")?;
            validate_iface(&iface)?;
            let cfg = wifi_config(&ssid, &psk)?;
            let path = PathBuf::from(format!("/var/lib/wana/network/{iface}.conf"));
            write_secret(&path, &cfg)?;
            info!(
                LOG,
                "Wi-Fi configuration saved for {iface} (credentials hidden)"
            );
            Ok(())
        }
        "wifi-up" => {
            let iface = args.next().ok_or("wifi-up needs IFACE")?;
            validate_iface(&iface)?;
            let cfg = format!("/var/lib/wana/network/{iface}.conf");
            if !Path::new(&cfg).is_file() {
                return Err(format!("{cfg} does not exist"));
            }
            run_command(
                "/usr/sbin/wpa_supplicant",
                &["-B", "-i", &iface, "-c", &cfg],
            )?;
            run_command("/sbin/dhcpcd", &["-4", "-q", &iface])?;
            info!(LOG, "Wi-Fi connection started on {iface}");
            Ok(())
        }
        _ => Err(format!("unknown command {cmd}")),
    }
}

fn main() -> ExitCode {
    wana_log::init_from_env();
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            error!(LOG, "network: {e}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;
    use std::sync::atomic::{AtomicUsize, Ordering};

    static N: AtomicUsize = AtomicUsize::new(0);

    fn temp() -> PathBuf {
        std::env::temp_dir().join(format!(
            "wana-network-test-{}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::Relaxed)
        ))
    }

    #[test]
    fn wifi_config_is_strict_and_does_not_weaken_security() {
        let c = wifi_config("Wana Lab", "password123").unwrap();
        assert!(c.contains("key_mgmt=WPA-PSK SAE"));
        assert!(c.contains("ieee80211w=1"));
        assert!(wifi_config("", "password123").is_err());
        assert!(wifi_config("x", "short").is_err());
        assert!(wifi_config("bad\nssid", "password123").is_err());
    }

    #[test]
    fn secret_file_is_atomic_and_mode_0600() {
        let dir = temp();
        let p = dir.join("wifi.conf");
        write_secret(&p, "secret").unwrap();
        assert_eq!(fs::read_to_string(&p).unwrap(), "secret");
        assert_eq!(
            fs::metadata(&p).unwrap().permissions().mode() & 0o777,
            0o600
        );
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn interface_discovery_reads_sysfs_shape() {
        let root = temp();
        let eth = root.join("sys/class/net/eth0");
        fs::create_dir_all(&eth).unwrap();
        fs::write(eth.join("address"), "52:54:00:12:34:56\n").unwrap();
        fs::write(eth.join("operstate"), "up\n").unwrap();
        fs::write(eth.join("carrier"), "1\n").unwrap();
        let got = interfaces(&root).unwrap();
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].name, "eth0");
        assert_eq!(got[0].carrier, Some(true));
        assert!(!got[0].wireless);
        let _ = fs::remove_dir_all(root);
    }
}
