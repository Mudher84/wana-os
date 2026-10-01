//! Persistent Wana settings: strict key=value format and atomic writes.

use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Settings {
    pub language: String,
    pub theme: String,
    pub accent: String,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            language: "ar".into(),
            theme: "dark".into(),
            accent: "blue".into(),
        }
    }
}

fn effective_uid() -> Result<u32, String> {
    let status = fs::read_to_string("/proc/self/status")
        .map_err(|e| format!("read /proc/self/status: {e}"))?;
    let line = status
        .lines()
        .find(|line| line.starts_with("Uid:"))
        .ok_or("/proc/self/status: Uid field missing")?;
    line.split_whitespace()
        .nth(2)
        .ok_or("/proc/self/status: effective uid missing")?
        .parse()
        .map_err(|e| format!("/proc/self/status: invalid effective uid: {e}"))
}

fn secure_metadata(path: &Path) -> Result<Option<fs::Metadata>, String> {
    match fs::symlink_metadata(path) {
        Ok(meta) => {
            if meta.file_type().is_symlink() || !meta.is_file() {
                return Err(format!(
                    "{}: settings path must be a regular non-symlink file",
                    path.display()
                ));
            }
            let euid = effective_uid()?;
            if meta.uid() != euid {
                return Err(format!(
                    "{}: settings owner uid {} does not match effective uid {euid}",
                    path.display(),
                    meta.uid()
                ));
            }
            if meta.mode() & 0o022 != 0 {
                return Err(format!(
                    "{}: settings file is group/other writable (mode {:o})",
                    path.display(),
                    meta.mode() & 0o777
                ));
            }
            Ok(Some(meta))
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(format!("{}: {e}", path.display())),
    }
}

fn secure_parent_directory(parent: &Path) -> Result<(), String> {
    fs::create_dir_all(parent).map_err(|e| format!("create {}: {e}", parent.display()))?;
    let meta = fs::symlink_metadata(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
    if meta.file_type().is_symlink() || !meta.is_dir() {
        return Err(format!(
            "{}: settings directory must be a real directory",
            parent.display()
        ));
    }
    let euid = effective_uid()?;
    if meta.uid() != euid {
        return Err(format!(
            "{}: settings directory owner uid {} does not match effective uid {euid}",
            parent.display(),
            meta.uid()
        ));
    }
    if meta.mode() & 0o022 != 0 {
        return Err(format!(
            "{}: settings directory is group/other writable (mode {:o})",
            parent.display(),
            meta.mode() & 0o777
        ));
    }
    Ok(())
}

impl Settings {
    pub fn load(path: &Path) -> Result<Self, String> {
        let Some(before) = secure_metadata(path)? else {
            return Ok(Self::default());
        };
        let mut file = File::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let opened = file
            .metadata()
            .map_err(|e| format!("metadata {}: {e}", path.display()))?;
        if before.dev() != opened.dev() || before.ino() != opened.ino() {
            return Err(format!(
                "{}: settings file changed while opening",
                path.display()
            ));
        }
        let mut text = String::new();
        file.read_to_string(&mut text)
            .map_err(|e| format!("{}: {e}", path.display()))?;
        Self::parse(&text)
    }

    pub fn parse(text: &str) -> Result<Self, String> {
        let mut out = Self::default();
        let mut seen = std::collections::BTreeSet::new();
        for (line_no, raw) in text.lines().enumerate() {
            let line = raw.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let (key, value) = line
                .split_once('=')
                .ok_or_else(|| format!("line {}: expected key=value", line_no + 1))?;
            let key = key.trim();
            let value = value.trim();
            if !seen.insert(key.to_string()) {
                return Err(format!("line {}: duplicate key {key}", line_no + 1));
            }
            out.set(key, value)?;
        }
        Ok(out)
    }

    pub fn set(&mut self, key: &str, value: &str) -> Result<(), String> {
        if value.contains(['\n', '\r', '=']) {
            return Err(format!("{key}: invalid value"));
        }
        match key {
            "language" if matches!(value, "ar" | "en") => self.language = value.into(),
            "theme" if matches!(value, "dark" | "light") => self.theme = value.into(),
            "accent" if matches!(value, "blue" | "teal" | "violet") => self.accent = value.into(),
            "language" | "theme" | "accent" => {
                return Err(format!("{key}: unsupported value {value:?}"));
            }
            _ => return Err(format!("unknown setting {key:?}")),
        }
        Ok(())
    }

    pub fn encode(&self) -> String {
        format!(
            "language={}\ntheme={}\naccent={}\n",
            self.language, self.theme, self.accent
        )
    }

    pub fn save_atomic(&self, path: &Path) -> Result<(), String> {
        let parent = path.parent().unwrap_or_else(|| Path::new("."));
        secure_parent_directory(parent)?;

        let file_name = path
            .file_name()
            .and_then(|n| n.to_str())
            .ok_or_else(|| format!("invalid settings path {}", path.display()))?;
        secure_metadata(path)?;

        let pid = std::process::id();
        let (tmp, mut file) = (0..64)
            .find_map(|slot| {
                let tmp: PathBuf = parent.join(format!(".{file_name}.tmp-{pid}-{slot}"));
                match OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .mode(0o600)
                    .open(&tmp)
                {
                    Ok(file) => Some(Ok((tmp, file))),
                    Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => None,
                    Err(e) => Some(Err(format!("create {}: {e}", tmp.display()))),
                }
            })
            .transpose()?
            .ok_or_else(|| format!("no free atomic temp slot for {}", path.display()))?;

        let result = (|| -> Result<(), String> {
            file.write_all(self.encode().as_bytes())
                .map_err(|e| format!("write {}: {e}", tmp.display()))?;
            file.sync_all()
                .map_err(|e| format!("sync {}: {e}", tmp.display()))?;
            fs::set_permissions(&tmp, fs::Permissions::from_mode(0o600))
                .map_err(|e| format!("chmod {}: {e}", tmp.display()))?;
            fs::rename(&tmp, path)
                .map_err(|e| format!("rename {} -> {}: {e}", tmp.display(), path.display()))?;
            File::open(parent)
                .and_then(|d| d.sync_all())
                .map_err(|e| format!("sync directory {}: {e}", parent.display()))?;
            Ok(())
        })();

        if result.is_err() {
            let _ = fs::remove_file(&tmp);
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    static N: AtomicUsize = AtomicUsize::new(0);

    fn path() -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "wana-settings-test-{}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&dir).unwrap();
        fs::set_permissions(&dir, fs::Permissions::from_mode(0o700)).unwrap();
        dir.join("settings.conf")
    }

    fn cleanup(path: &Path) {
        if let Some(parent) = path.parent() {
            let _ = fs::remove_dir_all(parent);
        }
    }

    #[test]
    fn stale_temp_file_does_not_block_settings_save() {
        let p = path();
        let parent = p.parent().unwrap();
        fs::create_dir_all(parent).unwrap();
        let file_name = p.file_name().unwrap().to_str().unwrap();
        let stale = parent.join(format!(".{file_name}.tmp-{}-0", std::process::id()));
        fs::write(&stale, "stale").unwrap();

        Settings::default().save_atomic(&p).unwrap();
        assert_eq!(Settings::load(&p).unwrap(), Settings::default());
        assert!(stale.exists());

        cleanup(&p);
    }

    #[test]
    fn defaults_and_round_trip_are_stable() {
        let p = path();
        let mut s = Settings::default();
        s.set("theme", "light").unwrap();
        s.set("accent", "teal").unwrap();
        s.save_atomic(&p).unwrap();
        assert_eq!(Settings::load(&p).unwrap(), s);
        assert_eq!(
            fs::read_to_string(&p).unwrap(),
            "language=ar\ntheme=light\naccent=teal\n"
        );
        assert_eq!(
            fs::metadata(&p).unwrap().permissions().mode() & 0o777,
            0o600
        );
        cleanup(&p);
    }

    #[test]
    fn symlink_settings_are_rejected() {
        use std::os::unix::fs::symlink;

        let real = path();
        let link = path();
        fs::write(&real, "theme=dark\n").unwrap();
        symlink(&real, &link).unwrap();
        assert!(Settings::load(&link).is_err());
        assert!(Settings::default().save_atomic(&link).is_err());
        cleanup(&link);
        cleanup(&real);
    }

    #[test]
    fn insecure_settings_directory_is_rejected() {
        let p = path();
        let parent = p.parent().unwrap();
        fs::set_permissions(parent, fs::Permissions::from_mode(0o777)).unwrap();
        assert!(Settings::default().save_atomic(&p).is_err());
        cleanup(&p);
    }

    #[test]
    fn invalid_or_duplicate_values_are_rejected() {
        assert!(Settings::parse("theme=dark\ntheme=light\n").is_err());
        assert!(Settings::parse("theme=neon\n").is_err());
        assert!(Settings::parse("unknown=x\n").is_err());
        assert!(Settings::parse("broken\n").is_err());
    }
}
