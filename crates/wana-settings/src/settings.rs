use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Settings {
    pub language: String,
    pub theme: String,
    pub accent: String,
    pub clock: String,
    pub keyboard: String,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            language: "ar".into(),
            theme: "dark".into(),
            accent: "4f8cff".into(),
            clock: "24".into(),
            keyboard: "us".into(),
        }
    }
}

impl Settings {
    pub fn parse(text: &str) -> Result<Self, String> {
        let mut out = Self::default();
        for (line_no, raw) in text.lines().enumerate() {
            let line = raw.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let Some((key, value)) = line.split_once('=') else {
                return Err(format!("line {}: expected key=value", line_no + 1));
            };
            out.set(key.trim(), value.trim())
                .map_err(|e| format!("line {}: {e}", line_no + 1))?;
        }
        Ok(out)
    }

    pub fn load(path: &Path) -> Result<Self, String> {
        match std::fs::read_to_string(path) {
            Ok(text) => Self::parse(&text),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(e) => Err(format!("{}: {e}", path.display())),
        }
    }

    pub fn set(&mut self, key: &str, value: &str) -> Result<(), String> {
        match key {
            "language" if matches!(value, "ar" | "en") => self.language = value.into(),
            "theme" if matches!(value, "dark" | "light" | "system") => self.theme = value.into(),
            "accent" if valid_hex(value) => self.accent = value.to_ascii_lowercase(),
            "clock" if matches!(value, "12" | "24") => self.clock = value.into(),
            "keyboard" if valid_word(value) => self.keyboard = value.into(),
            "language" | "theme" | "accent" | "clock" | "keyboard" => {
                return Err(format!("{key}: invalid value {value:?}"));
            }
            _ => return Err(format!("unknown setting {key:?}")),
        }
        Ok(())
    }

    pub fn canonical(&self) -> String {
        format!(
            "language={}\ntheme={}\naccent={}\nclock={}\nkeyboard={}\n",
            self.language, self.theme, self.accent, self.clock, self.keyboard
        )
    }

    pub fn save(&self, path: &Path) -> Result<(), String> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("{}: {e}", parent.display()))?;
        }
        let tmp = path.with_extension("tmp");
        std::fs::write(&tmp, self.canonical()).map_err(|e| format!("{}: {e}", tmp.display()))?;
        std::fs::rename(&tmp, path).map_err(|e| format!("{}: {e}", path.display()))
    }
}

fn valid_hex(v: &str) -> bool {
    v.len() == 6 && v.bytes().all(|b| b.is_ascii_hexdigit())
}

fn valid_word(v: &str) -> bool {
    !v.is_empty()
        && v.len() <= 32
        && v.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_stable() {
        assert_eq!(
            Settings::default().canonical(),
            "language=ar\ntheme=dark\naccent=4f8cff\nclock=24\nkeyboard=us\n"
        );
    }

    #[test]
    fn parse_rejects_unknown_and_invalid_values() {
        assert!(Settings::parse("theme=neon").is_err());
        assert!(Settings::parse("root=yes").is_err());
        assert!(Settings::parse("accent=xyz").is_err());
    }

    #[test]
    fn canonical_round_trip() {
        let mut s = Settings::default();
        s.set("theme", "light").unwrap();
        s.set("accent", "AABBCC").unwrap();
        s.set("keyboard", "ara").unwrap();
        assert_eq!(Settings::parse(&s.canonical()).unwrap(), s);
    }
}
