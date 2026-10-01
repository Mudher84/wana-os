//! Bounded persistent notification history.
//!
//! The store is deliberately small and deterministic. Mutations are atomic,
//! the directory is mode 0700, the history file is mode 0600, symlinks are
//! rejected, and the on-disk file must be owned by the effective uid.

use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};

pub const MAX_NOTIFICATIONS: usize = 64;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Notification {
    pub seq: u64,
    pub app: String,
    pub title: String,
    pub body: String,
}

#[derive(Debug, Clone)]
pub struct Store {
    root: PathBuf,
}

impl Store {
    pub fn system() -> Self {
        let root = std::env::var_os("HOME")
            .map(PathBuf::from)
            .filter(|path| path.is_absolute())
            .map(|home| home.join(".local/state/wana/notifications"))
            .unwrap_or_else(|| PathBuf::from("/var/lib/wana/notifications"));
        Self::new(root)
    }

    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    fn history_path(&self) -> PathBuf {
        self.root.join("history.tsv")
    }

    pub fn list(&self) -> Result<Vec<Notification>, String> {
        let path = self.history_path();
        let Some(text) = read_optional_secure(&path)? else {
            return Ok(Vec::new());
        };
        let mut out = Vec::new();
        let mut previous = 0u64;
        for (index, line) in text.lines().enumerate() {
            if line.is_empty() {
                continue;
            }
            let fields: Vec<&str> = line.split('\t').collect();
            if fields.len() != 4 {
                return Err(format!(
                    "{}:{}: expected SEQ/APP/TITLE/BODY",
                    path.display(),
                    index + 1
                ));
            }
            let seq = fields[0]
                .parse::<u64>()
                .map_err(|e| format!("{}:{}: invalid sequence: {e}", path.display(), index + 1))?;
            if seq <= previous {
                return Err(format!(
                    "{}:{}: non-monotonic notification sequence",
                    path.display(),
                    index + 1
                ));
            }
            previous = seq;
            valid_app(fields[1])?;
            valid_text("title", fields[2], 160)?;
            valid_text("body", fields[3], 480)?;
            out.push(Notification {
                seq,
                app: fields[1].into(),
                title: fields[2].into(),
                body: fields[3].into(),
            });
        }
        if out.len() > MAX_NOTIFICATIONS {
            return Err(format!(
                "{}: notification history exceeds bounded limit {}",
                path.display(),
                MAX_NOTIFICATIONS
            ));
        }
        Ok(out)
    }

    pub fn push(&self, app: &str, title: &str, body: &str) -> Result<u64, String> {
        valid_app(app)?;
        valid_text("title", title, 160)?;
        valid_text("body", body, 480)?;

        let mut entries = self.list()?;
        let seq = entries
            .last()
            .map_or(1, |entry| entry.seq.saturating_add(1));
        entries.push(Notification {
            seq,
            app: app.into(),
            title: title.into(),
            body: body.into(),
        });
        if entries.len() > MAX_NOTIFICATIONS {
            let remove = entries.len() - MAX_NOTIFICATIONS;
            entries.drain(..remove);
        }

        let mut text = String::new();
        for entry in entries {
            text.push_str(&format!(
                "{}\t{}\t{}\t{}\n",
                entry.seq, entry.app, entry.title, entry.body
            ));
        }
        write_atomic(&self.root, &self.history_path(), &text)?;
        Ok(seq)
    }

    pub fn clear(&self) -> Result<(), String> {
        write_atomic(&self.root, &self.history_path(), "")
    }
}

fn valid_app(value: &str) -> Result<(), String> {
    if value.is_empty()
        || value.len() > 96
        || !value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
    {
        return Err(format!("invalid app id {value:?}"));
    }
    Ok(())
}

fn valid_text(kind: &str, value: &str, max: usize) -> Result<(), String> {
    if value.is_empty()
        || value.len() > max
        || value
            .chars()
            .any(|c| matches!(c, '\t' | '\n' | '\r') || c.is_control())
    {
        return Err(format!("invalid {kind}"));
    }
    Ok(())
}

fn secure_metadata(path: &Path) -> Result<Option<fs::Metadata>, String> {
    match fs::symlink_metadata(path) {
        Ok(meta) => {
            if meta.file_type().is_symlink() || !meta.is_file() {
                return Err(format!(
                    "{}: notification history must be a regular non-symlink file",
                    path.display()
                ));
            }
            let euid = effective_uid()?;
            if meta.uid() != euid {
                return Err(format!(
                    "{}: owner uid {} does not match effective uid {euid}",
                    path.display(),
                    meta.uid()
                ));
            }
            if meta.mode() & 0o077 != 0 {
                return Err(format!(
                    "{}: insecure mode {:o}; expected no group/other access",
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

fn read_optional_secure(path: &Path) -> Result<Option<String>, String> {
    let Some(before) = secure_metadata(path)? else {
        return Ok(None);
    };
    let mut file = File::open(path).map_err(|e| format!("open {}: {e}", path.display()))?;
    let opened = file
        .metadata()
        .map_err(|e| format!("metadata {}: {e}", path.display()))?;
    if before.dev() != opened.dev() || before.ino() != opened.ino() {
        return Err(format!("{}: changed while opening", path.display()));
    }
    let mut text = String::new();
    file.read_to_string(&mut text)
        .map_err(|e| format!("read {}: {e}", path.display()))?;
    Ok(Some(text))
}

fn ensure_dir(root: &Path) -> Result<(), String> {
    fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(root)
        .map_err(|e| format!("create {}: {e}", root.display()))?;
    let meta = fs::symlink_metadata(root).map_err(|e| format!("{}: {e}", root.display()))?;
    if meta.file_type().is_symlink() || !meta.is_dir() {
        return Err(format!(
            "{}: notification store is not a real directory",
            root.display()
        ));
    }
    let euid = effective_uid()?;
    if meta.uid() != euid {
        return Err(format!(
            "{}: owner uid {} does not match effective uid {euid}",
            root.display(),
            meta.uid()
        ));
    }
    fs::set_permissions(root, fs::Permissions::from_mode(0o700))
        .map_err(|e| format!("chmod {}: {e}", root.display()))
}

fn write_atomic(root: &Path, path: &Path, text: &str) -> Result<(), String> {
    ensure_dir(root)?;
    secure_metadata(path)?;

    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| format!("invalid notification path {}", path.display()))?;
    let tmp = root.join(format!(".{name}.tmp-{}", std::process::id()));
    let result = (|| -> Result<(), String> {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&tmp)
            .map_err(|e| format!("create {}: {e}", tmp.display()))?;
        file.write_all(text.as_bytes())
            .map_err(|e| format!("write {}: {e}", tmp.display()))?;
        file.sync_all()
            .map_err(|e| format!("sync {}: {e}", tmp.display()))?;
        fs::set_permissions(&tmp, fs::Permissions::from_mode(0o600))
            .map_err(|e| format!("chmod {}: {e}", tmp.display()))?;
        fs::rename(&tmp, path)
            .map_err(|e| format!("rename {} -> {}: {e}", tmp.display(), path.display()))?;
        File::open(root)
            .and_then(|dir| dir.sync_all())
            .map_err(|e| format!("sync directory {}: {e}", root.display()))?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    result
}

pub fn effective_uid() -> Result<u32, String> {
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::{symlink, PermissionsExt};
    use std::sync::atomic::{AtomicUsize, Ordering};

    static NEXT: AtomicUsize = AtomicUsize::new(0);

    fn temp() -> PathBuf {
        std::env::temp_dir().join(format!(
            "wana-notifications-test-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ))
    }

    #[test]
    fn history_is_bounded_and_atomic() {
        let root = temp();
        let store = Store::new(&root);
        for i in 0..(MAX_NOTIFICATIONS + 7) {
            store
                .push("org.wana.Test", &format!("title {i}"), "body")
                .unwrap();
        }
        let list = store.list().unwrap();
        assert_eq!(list.len(), MAX_NOTIFICATIONS);
        assert_eq!(list.first().unwrap().seq, 8);
        assert_eq!(list.last().unwrap().seq, (MAX_NOTIFICATIONS + 7) as u64);
        assert_eq!(
            fs::metadata(root.join("history.tsv"))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
        assert_eq!(
            fs::metadata(&root).unwrap().permissions().mode() & 0o777,
            0o700
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn malformed_text_and_symlinks_are_rejected() {
        let root = temp();
        let store = Store::new(&root);
        assert!(store.push("../escape", "title", "body").is_err());
        assert!(store.push("org.wana.Test", "bad\ttitle", "body").is_err());
        fs::create_dir_all(&root).unwrap();
        symlink("/etc/passwd", root.join("history.tsv")).unwrap();
        assert!(store.list().is_err());
        let _ = fs::remove_dir_all(root);
    }
}
