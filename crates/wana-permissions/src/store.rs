//! Permission policy store and bounded audit log.
//!
//! The on-disk formats are intentionally tiny, deterministic TSV files.
//! Mutations are atomic, files are mode 0600 and the store directory is
//! mode 0700. System callers run this binary as root, so those files are
//! root-owned without requiring a privileged daemon.

use std::collections::BTreeMap;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};

pub const MAX_AUDIT: usize = 256;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decision {
    Allow,
    Deny,
}

impl Decision {
    pub fn parse(value: &str) -> Result<Self, String> {
        match value {
            "allow" => Ok(Self::Allow),
            "deny" => Ok(Self::Deny),
            _ => Err(format!(
                "invalid decision {value:?}; expected allow or deny"
            )),
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Allow => "allow",
            Self::Deny => "deny",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rule {
    pub app: String,
    pub permission: String,
    pub decision: Decision,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Audit {
    pub seq: u64,
    pub action: String,
    pub actor: String,
    pub app: String,
    pub permission: String,
    pub decision: Decision,
}

#[derive(Debug, Clone)]
pub struct Store {
    root: PathBuf,
}

impl Store {
    pub fn system() -> Self {
        Self::new("/var/lib/wana/permissions")
    }

    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    fn policy_path(&self) -> PathBuf {
        self.root.join("policy.tsv")
    }

    fn audit_path(&self) -> PathBuf {
        self.root.join("audit.tsv")
    }

    pub fn rules(&self) -> Result<Vec<Rule>, String> {
        let path = self.policy_path();
        let Some(text) = read_optional_secure(&path)? else {
            return Ok(Vec::new());
        };
        let mut map = BTreeMap::new();
        for (index, line) in text.lines().enumerate() {
            if line.is_empty() {
                continue;
            }
            let fields: Vec<&str> = line.split('\t').collect();
            if fields.len() != 3 {
                return Err(format!(
                    "{}:{}: expected APP<TAB>PERMISSION<TAB>DECISION",
                    path.display(),
                    index + 1
                ));
            }
            valid_name("app", fields[0])?;
            valid_name("permission", fields[1])?;
            let decision = Decision::parse(fields[2])?;
            let key = (fields[0].to_string(), fields[1].to_string());
            if map
                .insert(
                    key,
                    Rule {
                        app: fields[0].into(),
                        permission: fields[1].into(),
                        decision,
                    },
                )
                .is_some()
            {
                return Err(format!(
                    "{}:{}: duplicate rule {} {}",
                    path.display(),
                    index + 1,
                    fields[0],
                    fields[1]
                ));
            }
        }
        Ok(map.into_values().collect())
    }

    pub fn decision(&self, app: &str, permission: &str) -> Result<Decision, String> {
        valid_name("app", app)?;
        valid_name("permission", permission)?;
        Ok(self
            .rules()?
            .into_iter()
            .find(|rule| rule.app == app && rule.permission == permission)
            .map_or(Decision::Deny, |rule| rule.decision))
    }

    pub fn set(
        &self,
        actor: &str,
        app: &str,
        permission: &str,
        decision: Decision,
    ) -> Result<(), String> {
        valid_name("actor", actor)?;
        valid_name("app", app)?;
        valid_name("permission", permission)?;

        let mut map: BTreeMap<(String, String), Decision> = self
            .rules()?
            .into_iter()
            .map(|rule| ((rule.app, rule.permission), rule.decision))
            .collect();
        map.insert((app.into(), permission.into()), decision);

        let mut text = String::new();
        for ((app, permission), decision) in map {
            text.push_str(&format!("{app}\t{permission}\t{}\n", decision.as_str()));
        }
        write_atomic(&self.root, &self.policy_path(), &text)?;
        self.append_audit("policy-set", actor, app, permission, decision)
    }

    pub fn record_check(
        &self,
        actor: &str,
        app: &str,
        permission: &str,
        decision: Decision,
    ) -> Result<(), String> {
        valid_name("actor", actor)?;
        valid_name("app", app)?;
        valid_name("permission", permission)?;
        self.append_audit("check", actor, app, permission, decision)
    }

    pub fn audit(&self) -> Result<Vec<Audit>, String> {
        let path = self.audit_path();
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
            if fields.len() != 6 {
                return Err(format!(
                    "{}:{}: expected SEQ/ACTION/ACTOR/APP/PERMISSION/DECISION",
                    path.display(),
                    index + 1
                ));
            }
            let seq = fields[0]
                .parse::<u64>()
                .map_err(|e| format!("{}:{}: invalid sequence: {e}", path.display(), index + 1))?;
            if seq <= previous {
                return Err(format!(
                    "{}:{}: non-monotonic audit sequence",
                    path.display(),
                    index + 1
                ));
            }
            previous = seq;
            valid_name("action", fields[1])?;
            valid_name("actor", fields[2])?;
            valid_name("app", fields[3])?;
            valid_name("permission", fields[4])?;
            out.push(Audit {
                seq,
                action: fields[1].into(),
                actor: fields[2].into(),
                app: fields[3].into(),
                permission: fields[4].into(),
                decision: Decision::parse(fields[5])?,
            });
        }
        if out.len() > MAX_AUDIT {
            return Err(format!(
                "{}: audit exceeds bounded limit {}",
                path.display(),
                MAX_AUDIT
            ));
        }
        Ok(out)
    }

    pub fn clear_audit(&self) -> Result<(), String> {
        write_atomic(&self.root, &self.audit_path(), "")
    }

    fn append_audit(
        &self,
        action: &str,
        actor: &str,
        app: &str,
        permission: &str,
        decision: Decision,
    ) -> Result<(), String> {
        let mut entries = self.audit()?;
        let seq = entries
            .last()
            .map_or(1, |entry| entry.seq.saturating_add(1));
        entries.push(Audit {
            seq,
            action: action.into(),
            actor: actor.into(),
            app: app.into(),
            permission: permission.into(),
            decision,
        });
        if entries.len() > MAX_AUDIT {
            let remove = entries.len() - MAX_AUDIT;
            entries.drain(..remove);
        }
        let mut text = String::new();
        for entry in entries {
            text.push_str(&format!(
                "{}\t{}\t{}\t{}\t{}\t{}\n",
                entry.seq,
                entry.action,
                entry.actor,
                entry.app,
                entry.permission,
                entry.decision.as_str()
            ));
        }
        write_atomic(&self.root, &self.audit_path(), &text)
    }
}

fn valid_name(kind: &str, value: &str) -> Result<(), String> {
    if value.is_empty()
        || value.len() > 96
        || !value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-' | b':'))
    {
        return Err(format!("invalid {kind} {value:?}"));
    }
    Ok(())
}

fn secure_metadata(path: &Path) -> Result<Option<fs::Metadata>, String> {
    match fs::symlink_metadata(path) {
        Ok(meta) => {
            if meta.file_type().is_symlink() {
                return Err(format!("{}: symlinks are not allowed", path.display()));
            }
            if !meta.is_file() {
                return Err(format!("{}: not a regular file", path.display()));
            }
            if meta.mode() & 0o077 != 0 {
                return Err(format!(
                    "{}: insecure mode {:o}; expected no group/other access",
                    path.display(),
                    meta.mode() & 0o777
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
            "{}: permission store is not a real directory",
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
        .ok_or_else(|| format!("invalid store path {}", path.display()))?;
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
    use std::sync::atomic::{AtomicUsize, Ordering};

    static NEXT: AtomicUsize = AtomicUsize::new(0);

    fn temp() -> PathBuf {
        std::env::temp_dir().join(format!(
            "wana-permissions-test-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ))
    }

    #[test]
    fn policy_defaults_to_deny_and_round_trips() {
        let root = temp();
        let store = Store::new(&root);
        assert_eq!(
            store.decision("org.wana.Files", "files.read").unwrap(),
            Decision::Deny
        );
        store
            .set("settings", "org.wana.Files", "files.read", Decision::Allow)
            .unwrap();
        assert_eq!(
            store.decision("org.wana.Files", "files.read").unwrap(),
            Decision::Allow
        );
        assert_eq!(store.rules().unwrap().len(), 1);
        assert_eq!(store.audit().unwrap().len(), 1);
        assert_eq!(
            fs::metadata(root.join("policy.tsv"))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn audit_is_bounded_and_sequences_remain_monotonic() {
        let root = temp();
        let store = Store::new(&root);
        for i in 0..(MAX_AUDIT + 17) {
            let decision = if i % 2 == 0 {
                Decision::Allow
            } else {
                Decision::Deny
            };
            store
                .record_check("broker", "org.wana.Test", "files.read", decision)
                .unwrap();
        }
        let audit = store.audit().unwrap();
        assert_eq!(audit.len(), MAX_AUDIT);
        assert_eq!(audit.first().unwrap().seq, 18);
        assert_eq!(audit.last().unwrap().seq, (MAX_AUDIT + 17) as u64);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn malformed_names_and_symlink_store_files_are_rejected() {
        let root = temp();
        let store = Store::new(&root);
        assert!(store.decision("../escape", "files.read").is_err());
        fs::create_dir_all(&root).unwrap();
        std::os::unix::fs::symlink("/etc/passwd", root.join("policy.tsv")).unwrap();
        assert!(store.rules().is_err());
        let _ = fs::remove_dir_all(root);
    }
}
