//! Persistent Wana settings: strict key=value format and atomic writes.

use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
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

impl Settings {
    pub fn load(path: &Path) -> Result<Self, String> {
        if !path.exists() {
            return Ok(Self::default());
        }
        let mut text = String::new();
        File::open(path)
            .map_err(|e| format!("{}: {e}", path.display()))?
            .read_to_string(&mut text)
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
        fs::create_dir_all(parent)
            .map_err(|e| format!("create {}: {e}", parent.display()))?;

        let file_name = path
            .file_name()
            .and_then(|n| n.to_str())
            .ok_or_else(|| format!("invalid settings path {}", path.display()))?;
        let tmp: PathBuf = parent.join(format!(".{file_name}.tmp-{}", std::process::id()));

        let result = (|| -> Result<(), String> {
            let mut file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&tmp)
                .map_err(|e| format!("create {}: {e}", tmp.display()))?;
            file.write_all(self.encode().as_bytes())
                .map_err(|e| format!("write {}: {e}", tmp.display()))?;
            file.sync_all()
                .map_err(|e| format!("sync {}: {e}", tmp.display()))?;
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
        std::env::temp_dir().join(format!(
            "wana-settings-test-{}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::Relaxed)
        ))
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
        let _ = fs::remove_file(p);
    }

    #[test]
    fn invalid_or_duplicate_values_are_rejected() {
        assert!(Settings::parse("theme=dark\ntheme=light\n").is_err());
        assert!(Settings::parse("theme=neon\n").is_err());
        assert!(Settings::parse("unknown=x\n").is_err());
        assert!(Settings::parse("broken\n").is_err());
    }
}
