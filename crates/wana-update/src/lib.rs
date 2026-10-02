use std::collections::BTreeMap;
use std::fs::{self, DirBuilder, File, OpenOptions};
use std::io::{Read, Write};
use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::process::Command;

pub const UPDATE_HEADER: &str = "WANA-UPDATE-1";
pub const UPDATE_FILE: &str = "update.txt";
pub const SUMS_FILE: &str = "RELEASE-SHA256SUMS";
pub const ROOTFS_FILE: &str = "rootfs.ext4.zst";
pub const KERNEL_FILE: &str = "bzImage";
pub const INITRD_FILE: &str = "rootfs.cpio.zst";
pub const STATE_ROOT: &str = "/var/lib/wana/update";
pub const PENDING_DIR: &str = "/var/lib/wana/update/pending";
pub const INSTALLED_RELEASE: &str = "/etc/wana-release";
pub const STABLE_RELEASE_BASE: &str =
    "https://github.com/Mudher84/wana-os/releases/latest/download";
const RELEASE_DOWNLOAD_PREFIX: &str =
    "https://github.com/Mudher84/wana-os/releases/download";
const MAX_UPDATE_METADATA_BYTES: u64 = 64 * 1024;
const MAX_RELEASE_SUMS_BYTES: u64 = 1024 * 1024;
const MAX_ROOTFS_DOWNLOAD_BYTES: u64 = 1024 * 1024 * 1024;
const MAX_KERNEL_DOWNLOAD_BYTES: u64 = 256 * 1024 * 1024;
const MAX_INITRD_DOWNLOAD_BYTES: u64 = 1024 * 1024 * 1024;

const K: [u32; 64] = [
    0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5,
    0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174,
    0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
    0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967,
    0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85,
    0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
    0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
    0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
];

const H0: [u32; 8] = [
    0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19,
];

#[derive(Debug, Clone)]
struct Sha256 {
    h: [u32; 8],
    tail: [u8; 64],
    tail_len: usize,
    len: u64,
}

impl Sha256 {
    fn new() -> Self {
        Self {
            h: H0,
            tail: [0; 64],
            tail_len: 0,
            len: 0,
        }
    }

    fn compress(&mut self, block: &[u8; 64]) {
        let mut w = [0u32; 64];
        for (i, word) in block.chunks_exact(4).enumerate() {
            w[i] = u32::from_be_bytes([word[0], word[1], word[2], word[3]]);
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16]
                .wrapping_add(s0)
                .wrapping_add(w[i - 7])
                .wrapping_add(s1);
        }
        let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut hh] = self.h;
        for i in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ (!e & g);
            let t1 = hh
                .wrapping_add(s1)
                .wrapping_add(ch)
                .wrapping_add(K[i])
                .wrapping_add(w[i]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let t2 = s0.wrapping_add(maj);
            hh = g;
            g = f;
            f = e;
            e = d.wrapping_add(t1);
            d = c;
            c = b;
            b = a;
            a = t1.wrapping_add(t2);
        }
        for (slot, value) in self.h.iter_mut().zip([a, b, c, d, e, f, g, hh]) {
            *slot = slot.wrapping_add(value);
        }
    }

    fn update(&mut self, mut data: &[u8]) {
        self.len = self.len.saturating_add(data.len() as u64);
        if self.tail_len != 0 {
            let take = (64 - self.tail_len).min(data.len());
            self.tail[self.tail_len..self.tail_len + take].copy_from_slice(&data[..take]);
            self.tail_len += take;
            data = &data[take..];
            if self.tail_len == 64 {
                let block = self.tail;
                self.compress(&block);
                self.tail_len = 0;
            }
        }
        while data.len() >= 64 {
            let mut block = [0u8; 64];
            block.copy_from_slice(&data[..64]);
            self.compress(&block);
            data = &data[64..];
        }
        self.tail[..data.len()].copy_from_slice(data);
        self.tail_len = data.len();
    }

    fn finish(mut self) -> [u8; 32] {
        let bit_len = self.len.wrapping_mul(8);
        self.tail[self.tail_len] = 0x80;
        self.tail_len += 1;
        if self.tail_len > 56 {
            self.tail[self.tail_len..].fill(0);
            let block = self.tail;
            self.compress(&block);
            self.tail = [0; 64];
            self.tail_len = 0;
        }
        self.tail[self.tail_len..56].fill(0);
        self.tail[56..64].copy_from_slice(&bit_len.to_be_bytes());
        let block = self.tail;
        self.compress(&block);
        let mut out = [0u8; 32];
        for (bytes, value) in out.chunks_exact_mut(4).zip(self.h) {
            bytes.copy_from_slice(&value.to_be_bytes());
        }
        out
    }
}

pub fn hex(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        use std::fmt::Write as _;
        let _ = write!(&mut out, "{byte:02x}");
    }
    out
}

pub fn sha256_file(path: &Path) -> Result<(u64, String), String> {
    let before = fs::symlink_metadata(path).map_err(|e| format!("{}: {e}", path.display()))?;
    if before.file_type().is_symlink() || !before.is_file() {
        return Err(format!("{}: expected regular non-symlink file", path.display()));
    }
    let mut file = File::open(path).map_err(|e| format!("open {}: {e}", path.display()))?;
    let opened = file.metadata().map_err(|e| format!("metadata {}: {e}", path.display()))?;
    if before.dev() != opened.dev() || before.ino() != opened.ino() {
        return Err(format!("{}: changed while opening", path.display()));
    }
    let mut hash = Sha256::new();
    let mut size = 0u64;
    let mut buf = [0u8; 1024 * 1024];
    loop {
        let n = file.read(&mut buf).map_err(|e| format!("read {}: {e}", path.display()))?;
        if n == 0 {
            break;
        }
        size = size.checked_add(n as u64).ok_or("file size overflow")?;
        hash.update(&buf[..n]);
    }
    Ok((size, hex(&hash.finish())))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Metadata {
    pub version: String,
    pub commit: String,
    pub rootfs_size: u64,
    pub rootfs_sha256: String,
    pub kernel_size: u64,
    pub kernel_sha256: String,
    pub initrd_size: u64,
    pub initrd_sha256: String,
}

fn valid_hash(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
}

fn valid_version(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && value.as_bytes()[0].is_ascii_digit()
        && value.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-'))
        && value.bytes().filter(|b| *b == b'.').count() >= 2
}

fn valid_commit(value: &str) -> bool {
    value.len() == 40 && value.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

fn stable_version_parts(value: &str) -> Result<[u64; 3], String> {
    let parts: Vec<_> = value.split('.').collect();
    if parts.len() != 3 {
        return Err(format!("stable version must be X.Y.Z, got {value:?}"));
    }
    let mut out = [0u64; 3];
    for (index, part) in parts.into_iter().enumerate() {
        if part.is_empty()
            || !part.bytes().all(|b| b.is_ascii_digit())
            || (part.len() > 1 && part.starts_with('0'))
        {
            return Err(format!("invalid stable version component {part:?} in {value:?}"));
        }
        out[index] = part
            .parse::<u64>()
            .map_err(|e| format!("stable version {value:?}: {e}"))?;
    }
    Ok(out)
}

pub fn parse_update(text: &str) -> Result<Metadata, String> {
    let mut lines = text.lines();
    if lines.next() != Some(UPDATE_HEADER) {
        return Err(format!("update metadata must start with {UPDATE_HEADER}"));
    }
    let mut values = BTreeMap::new();
    for line in lines {
        if line.is_empty() {
            continue;
        }
        let (key, value) = line.split_once('=').ok_or("update metadata expects key=value")?;
        if !matches!(
            key,
            "version" | "commit" | "rootfs" | "rootfs_size" | "rootfs_sha256"
                | "kernel" | "kernel_size" | "kernel_sha256"
                | "initrd" | "initrd_size" | "initrd_sha256"
        ) {
            return Err(format!("unknown update metadata field {key:?}"));
        }
        if values.insert(key, value).is_some() {
            return Err(format!("duplicate update metadata field {key:?}"));
        }
    }
    if values.get("rootfs") != Some(&ROOTFS_FILE)
        || values.get("kernel") != Some(&KERNEL_FILE)
        || values.get("initrd") != Some(&INITRD_FILE)
    {
        return Err("update payload paths must be the canonical rootfs/kernel/initrd names".into());
    }
    let version = values.get("version").ok_or("missing version")?.to_string();
    let commit = values.get("commit").ok_or("missing commit")?.to_string();
    let rootfs_sha256 = values.get("rootfs_sha256").ok_or("missing rootfs_sha256")?.to_string();
    let kernel_sha256 = values.get("kernel_sha256").ok_or("missing kernel_sha256")?.to_string();
    let initrd_sha256 = values.get("initrd_sha256").ok_or("missing initrd_sha256")?.to_string();
    if !valid_version(&version) || !valid_commit(&commit) {
        return Err("invalid update version or commit".into());
    }
    if !valid_hash(&rootfs_sha256)
        || !valid_hash(&kernel_sha256)
        || !valid_hash(&initrd_sha256)
    {
        return Err("invalid update SHA-256".into());
    }
    let rootfs_size = values
        .get("rootfs_size")
        .ok_or("missing rootfs_size")?
        .parse::<u64>()
        .map_err(|e| format!("rootfs_size: {e}"))?;
    let kernel_size = values
        .get("kernel_size")
        .ok_or("missing kernel_size")?
        .parse::<u64>()
        .map_err(|e| format!("kernel_size: {e}"))?;
    let initrd_size = values
        .get("initrd_size")
        .ok_or("missing initrd_size")?
        .parse::<u64>()
        .map_err(|e| format!("initrd_size: {e}"))?;
    if rootfs_size == 0 || kernel_size == 0 || initrd_size == 0 {
        return Err("update payload sizes must be non-zero".into());
    }
    Ok(Metadata {
        version,
        commit,
        rootfs_size,
        rootfs_sha256,
        kernel_size,
        kernel_sha256,
        initrd_size,
        initrd_sha256,
    })
}

fn read_text_regular(path: &Path, max: u64) -> Result<String, String> {
    let before = fs::symlink_metadata(path).map_err(|e| format!("{}: {e}", path.display()))?;
    if before.file_type().is_symlink() || !before.is_file() || before.len() > max {
        return Err(format!("{}: invalid metadata file", path.display()));
    }

    let mut file = File::open(path).map_err(|e| format!("open {}: {e}", path.display()))?;
    let opened = file
        .metadata()
        .map_err(|e| format!("metadata {}: {e}", path.display()))?;
    if before.dev() != opened.dev()
        || before.ino() != opened.ino()
        || !opened.is_file()
        || opened.len() > max
    {
        return Err(format!("{}: changed or became invalid while opening", path.display()));
    }

    let mut text = String::new();
    file.read_to_string(&mut text)
        .map_err(|e| format!("read {}: {e}", path.display()))?;
    Ok(text)
}

fn release_sums(path: &Path) -> Result<BTreeMap<String, String>, String> {
    let text = read_text_regular(path, 1024 * 1024)?;
    let mut out = BTreeMap::new();
    for (line_no, line) in text.lines().enumerate() {
        if line.is_empty() {
            continue;
        }
        let (digest, name) = line
            .split_once("  ")
            .ok_or_else(|| format!("{}:{}: invalid checksum line", path.display(), line_no + 1))?;
        if !valid_hash(digest)
            || name.starts_with('/')
            || name.split('/').any(|part| matches!(part, "" | "." | ".."))
        {
            return Err(format!("{}:{}: invalid checksum entry", path.display(), line_no + 1));
        }
        if out.insert(name.to_string(), digest.to_string()).is_some() {
            return Err(format!("{}:{}: duplicate checksum entry", path.display(), line_no + 1));
        }
    }
    Ok(out)
}

#[derive(Debug, Clone)]
pub struct Verified {
    pub metadata: Metadata,
    pub update_sha256: String,
}

pub fn verify_bundle(dir: &Path) -> Result<Verified, String> {
    let update_path = dir.join(UPDATE_FILE);
    let text = read_text_regular(&update_path, 64 * 1024)?;
    let metadata = parse_update(&text)?;
    let sums = release_sums(&dir.join(SUMS_FILE))?;
    for required in [UPDATE_FILE, ROOTFS_FILE, KERNEL_FILE, INITRD_FILE] {
        if !sums.contains_key(required) {
            return Err(format!("{SUMS_FILE}: missing {required}"));
        }
    }
    let (update_size, update_sha) = sha256_file(&update_path)?;
    if update_size == 0 || sums[UPDATE_FILE] != update_sha {
        return Err("update.txt checksum mismatch".into());
    }
    let (rootfs_size, rootfs_sha) = sha256_file(&dir.join(ROOTFS_FILE))?;
    if rootfs_size != metadata.rootfs_size
        || rootfs_sha != metadata.rootfs_sha256
        || sums[ROOTFS_FILE] != rootfs_sha
    {
        return Err("rootfs update payload mismatch".into());
    }
    let (kernel_size, kernel_sha) = sha256_file(&dir.join(KERNEL_FILE))?;
    if kernel_size != metadata.kernel_size
        || kernel_sha != metadata.kernel_sha256
        || sums[KERNEL_FILE] != kernel_sha
    {
        return Err("kernel update payload mismatch".into());
    }
    let (initrd_size, initrd_sha) = sha256_file(&dir.join(INITRD_FILE))?;
    if initrd_size != metadata.initrd_size
        || initrd_sha != metadata.initrd_sha256
        || sums[INITRD_FILE] != initrd_sha
    {
        return Err("initrd update payload mismatch".into());
    }
    Ok(Verified {
        metadata,
        update_sha256: update_sha,
    })
}

pub fn verify_staged(dir: &Path) -> Result<Metadata, String> {
    let text = read_text_regular(&dir.join(UPDATE_FILE), 64 * 1024)?;
    let metadata = parse_update(&text)?;

    let (rootfs_size, rootfs_sha) = sha256_file(&dir.join(ROOTFS_FILE))?;
    if rootfs_size != metadata.rootfs_size || rootfs_sha != metadata.rootfs_sha256 {
        return Err("staged rootfs payload mismatch".into());
    }

    let (kernel_size, kernel_sha) = sha256_file(&dir.join(KERNEL_FILE))?;
    if kernel_size != metadata.kernel_size || kernel_sha != metadata.kernel_sha256 {
        return Err("staged kernel payload mismatch".into());
    }

    let (initrd_size, initrd_sha) = sha256_file(&dir.join(INITRD_FILE))?;
    if initrd_size != metadata.initrd_size || initrd_sha != metadata.initrd_sha256 {
        return Err("staged initrd payload mismatch".into());
    }

    Ok(metadata)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstalledRelease {
    pub version: String,
    pub commit: String,
}

pub fn installed_release() -> Result<Option<InstalledRelease>, String> {
    let path = Path::new(INSTALLED_RELEASE);
    let text = match fs::symlink_metadata(path) {
        Ok(_) => read_text_regular(path, 4096)?,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(format!("{}: {e}", path.display())),
    };
    let mut lines = text.lines();
    if lines.next() != Some("WANA-RELEASE-1") {
        return Err(format!("{}: unsupported release identity", path.display()));
    }
    let mut values = BTreeMap::new();
    for line in lines {
        let (key, value) = line
            .split_once('=')
            .ok_or_else(|| format!("{}: expected key=value", path.display()))?;
        if !matches!(key, "version" | "commit") || values.insert(key, value).is_some() {
            return Err(format!("{}: invalid or duplicate field {key:?}", path.display()));
        }
    }
    let version = values.get("version").ok_or("installed version missing")?.to_string();
    let commit = values.get("commit").ok_or("installed commit missing")?.to_string();
    if !valid_version(&version) || !valid_commit(&commit) {
        return Err("installed release identity is invalid".into());
    }
    Ok(Some(InstalledRelease { version, commit }))
}

fn tagged_release_base(version: &str) -> Result<String, String> {
    stable_version_parts(version)?;
    Ok(format!("{RELEASE_DOWNLOAD_PREFIX}/v{version}"))
}

fn curl_download(base: &str, name: &str, target: &Path, max_bytes: u64) -> Result<(), String> {
    if name.starts_with('/') || name.split('/').any(|part| matches!(part, "" | "." | "..")) {
        return Err(format!("unsafe release asset name {name:?}"));
    }
    if base != STABLE_RELEASE_BASE && !base.starts_with(&format!("{RELEASE_DOWNLOAD_PREFIX}/v")) {
        return Err(format!("unsafe release base {base:?}"));
    }
    let url = format!("{base}/{name}");
    let status = Command::new("/usr/bin/curl")
        .args([
            "--fail",
            "--location",
            "--silent",
            "--show-error",
            "--proto",
            "=https",
            "--tlsv1.2",
            "--connect-timeout",
            "20",
            "--retry",
            "3",
            "--retry-all-errors",
            "--user-agent",
            "Wana-OS-Updater/1",
            "--max-filesize",
        ])
        .arg(max_bytes.to_string())
        .arg("--output")
        .arg(target)
        .arg(&url)
        .status()
        .map_err(|e| format!("start curl for {url}: {e}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("download {url} failed: {status}"))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FetchOutcome {
    Current(Metadata),
    Staged(Metadata),
}

pub fn fetch_latest_and_stage() -> Result<FetchOutcome, String> {
    require_root()?;
    if fs::symlink_metadata(PENDING_DIR).is_ok() {
        return Err("an update is already pending; apply or clear it first".into());
    }
    let root = secure_state_root()?;
    let download = root.join(format!(".download-{}", std::process::id()));
    if fs::symlink_metadata(&download).is_ok() {
        return Err(format!("{} already exists", download.display()));
    }
    DirBuilder::new()
        .mode(0o700)
        .create(&download)
        .map_err(|e| format!("create {}: {e}", download.display()))?;

    let result = (|| -> Result<FetchOutcome, String> {
        // Discover the current stable version from GitHub's latest stable
        // release, then pin every subsequent request to that immutable tag.
        curl_download(
            STABLE_RELEASE_BASE,
            UPDATE_FILE,
            &download.join(UPDATE_FILE),
            MAX_UPDATE_METADATA_BYTES,
        )?;
        let latest_text = read_text_regular(&download.join(UPDATE_FILE), MAX_UPDATE_METADATA_BYTES)?;
        let latest_metadata = parse_update(&latest_text)?;
        let tagged_base = tagged_release_base(&latest_metadata.version)?;

        curl_download(
            &tagged_base,
            UPDATE_FILE,
            &download.join(UPDATE_FILE),
            MAX_UPDATE_METADATA_BYTES,
        )?;
        curl_download(
            &tagged_base,
            SUMS_FILE,
            &download.join(SUMS_FILE),
            MAX_RELEASE_SUMS_BYTES,
        )?;

        let update_text = read_text_regular(&download.join(UPDATE_FILE), MAX_UPDATE_METADATA_BYTES)?;
        let metadata = parse_update(&update_text)?;
        if metadata.version != latest_metadata.version {
            return Err("latest release changed while pinning its stable tag".into());
        }
        let sums = release_sums(&download.join(SUMS_FILE))?;
        let (_, update_sha) = sha256_file(&download.join(UPDATE_FILE))?;
        if sums.get(UPDATE_FILE) != Some(&update_sha) {
            return Err("downloaded update.txt does not match release checksums".into());
        }

        if let Some(installed) = installed_release()? {
            if installed.commit == metadata.commit {
                return Ok(FetchOutcome::Current(metadata));
            }

            let current_version = stable_version_parts(&installed.version)?;
            let candidate_version = stable_version_parts(&metadata.version)?;
            if candidate_version < current_version {
                return Err(format!(
                    "refusing release downgrade: installed {} ({}) -> candidate {} ({})",
                    installed.version, installed.commit, metadata.version, metadata.commit
                ));
            }
            if candidate_version == current_version {
                return Err(format!(
                    "release version {} is already installed with a different commit; refusing equivocation",
                    installed.version
                ));
            }
        } else {
            // Online stable updates must still use a stable X.Y.Z version even
            // on a system whose release identity predates this updater.
            stable_version_parts(&metadata.version)?;
        }

        for (name, max_bytes) in [
            (ROOTFS_FILE, MAX_ROOTFS_DOWNLOAD_BYTES),
            (KERNEL_FILE, MAX_KERNEL_DOWNLOAD_BYTES),
            (INITRD_FILE, MAX_INITRD_DOWNLOAD_BYTES),
        ] {
            curl_download(&tagged_base, name, &download.join(name), max_bytes)?;
        }
        let verified = verify_bundle(&download)?;
        let staged = stage_bundle(&download)?;
        if staged != verified.metadata {
            return Err("staged update metadata changed unexpectedly".into());
        }
        Ok(FetchOutcome::Staged(staged))
    })();

    let cleanup = fs::remove_dir_all(&download)
        .map_err(|e| format!("remove {}: {e}", download.display()));
    match (result, cleanup) {
        (Ok(value), Ok(())) => Ok(value),
        (Err(e), _) => Err(e),
        (Ok(_), Err(e)) => Err(e),
    }
}

pub fn effective_uid() -> Result<u32, String> {
    let text = fs::read_to_string("/proc/self/status").map_err(|e| format!("read uid: {e}"))?;
    text.lines()
        .find(|line| line.starts_with("Uid:"))
        .and_then(|line| line.split_whitespace().nth(2))
        .ok_or("effective uid missing")?
        .parse::<u32>()
        .map_err(|e| format!("effective uid: {e}"))
}

fn require_root() -> Result<(), String> {
    let uid = effective_uid()?;
    if uid == 0 {
        Ok(())
    } else {
        Err(format!("update mutation requires uid 0; current effective uid is {uid}"))
    }
}

fn secure_state_root() -> Result<PathBuf, String> {
    let root = PathBuf::from(STATE_ROOT);
    DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(&root)
        .map_err(|e| format!("create {}: {e}", root.display()))?;
    let meta = fs::symlink_metadata(&root).map_err(|e| format!("{}: {e}", root.display()))?;
    if meta.file_type().is_symlink() || !meta.is_dir() || meta.uid() != 0 {
        return Err(format!("{}: update state root must be a real root-owned directory", root.display()));
    }
    fs::set_permissions(&root, fs::Permissions::from_mode(0o700))
        .map_err(|e| format!("chmod {}: {e}", root.display()))?;
    Ok(root)
}

fn copy_verified(source: &Path, target: &Path, expected_size: u64, expected_hash: &str) -> Result<(), String> {
    let before = fs::symlink_metadata(source).map_err(|e| format!("{}: {e}", source.display()))?;
    if before.file_type().is_symlink() || !before.is_file() {
        return Err(format!("{}: source must be a regular file", source.display()));
    }
    let mut input = File::open(source).map_err(|e| format!("open {}: {e}", source.display()))?;
    let opened = input.metadata().map_err(|e| format!("metadata {}: {e}", source.display()))?;
    if before.dev() != opened.dev() || before.ino() != opened.ino() {
        return Err(format!("{}: source changed while opening", source.display()));
    }
    let mut output = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(target)
        .map_err(|e| format!("create {}: {e}", target.display()))?;
    let mut hash = Sha256::new();
    let mut size = 0u64;
    let mut buf = [0u8; 1024 * 1024];
    loop {
        let n = input.read(&mut buf).map_err(|e| format!("read {}: {e}", source.display()))?;
        if n == 0 {
            break;
        }
        size = size.checked_add(n as u64).ok_or("staged file size overflow")?;
        hash.update(&buf[..n]);
        output
            .write_all(&buf[..n])
            .map_err(|e| format!("write {}: {e}", target.display()))?;
    }
    output.sync_all().map_err(|e| format!("sync {}: {e}", target.display()))?;
    let digest = hex(&hash.finish());
    if size != expected_size || digest != expected_hash {
        let _ = fs::remove_file(target);
        return Err(format!("{}: source changed during staging", source.display()));
    }
    Ok(())
}

pub fn stage_bundle(dir: &Path) -> Result<Metadata, String> {
    require_root()?;
    let verified = verify_bundle(dir)?;
    let root = secure_state_root()?;
    let pending = PathBuf::from(PENDING_DIR);
    if fs::symlink_metadata(&pending).is_ok() {
        return Err(format!("{} already exists; clear it before staging another update", pending.display()));
    }
    let temp = root.join(format!(".pending-{}", std::process::id()));
    if fs::symlink_metadata(&temp).is_ok() {
        return Err(format!("{} already exists", temp.display()));
    }
    DirBuilder::new()
        .mode(0o700)
        .create(&temp)
        .map_err(|e| format!("create {}: {e}", temp.display()))?;

    let result = (|| -> Result<(), String> {
        copy_verified(
            &dir.join(ROOTFS_FILE),
            &temp.join(ROOTFS_FILE),
            verified.metadata.rootfs_size,
            &verified.metadata.rootfs_sha256,
        )?;
        copy_verified(
            &dir.join(KERNEL_FILE),
            &temp.join(KERNEL_FILE),
            verified.metadata.kernel_size,
            &verified.metadata.kernel_sha256,
        )?;
        copy_verified(
            &dir.join(INITRD_FILE),
            &temp.join(INITRD_FILE),
            verified.metadata.initrd_size,
            &verified.metadata.initrd_sha256,
        )?;
        let update_size = fs::metadata(dir.join(UPDATE_FILE))
            .map_err(|e| format!("metadata update.txt: {e}"))?
            .len();
        copy_verified(
            &dir.join(UPDATE_FILE),
            &temp.join(UPDATE_FILE),
            update_size,
            &verified.update_sha256,
        )?;
        File::open(&temp)
            .and_then(|file| file.sync_all())
            .map_err(|e| format!("sync {}: {e}", temp.display()))?;
        fs::rename(&temp, &pending)
            .map_err(|e| format!("rename {} -> {}: {e}", temp.display(), pending.display()))?;
        File::open(&root)
            .and_then(|file| file.sync_all())
            .map_err(|e| format!("sync {}: {e}", root.display()))?;
        Ok(())
    })();

    if result.is_err() {
        let _ = fs::remove_dir_all(&temp);
    }
    result?;
    Ok(verified.metadata)
}

pub fn pending_metadata() -> Result<Option<Metadata>, String> {
    let path = Path::new(PENDING_DIR).join(UPDATE_FILE);
    match fs::symlink_metadata(&path) {
        Ok(_) => Ok(Some(parse_update(&read_text_regular(&path, 64 * 1024)?)?)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(format!("{}: {e}", path.display())),
    }
}

pub fn clear_pending() -> Result<(), String> {
    require_root()?;
    let pending = PathBuf::from(PENDING_DIR);
    match fs::symlink_metadata(&pending) {
        Ok(meta) => {
            if meta.file_type().is_symlink() || !meta.is_dir() || meta.uid() != 0 {
                return Err(format!("{}: refusing to remove unsafe pending path", pending.display()));
            }
            fs::remove_dir_all(&pending).map_err(|e| format!("remove {}: {e}", pending.display()))
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(format!("{}: {e}", pending.display())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sha256_stream_matches_standard_vectors() {
        let mut h = Sha256::new();
        h.update(b"a");
        h.update(b"bc");
        assert_eq!(
            hex(&h.finish()),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn stable_tag_download_base_is_fixed_to_project_release() {
        assert_eq!(
            tagged_release_base("1.2.3").unwrap(),
            "https://github.com/Mudher84/wana-os/releases/download/v1.2.3"
        );
        assert!(tagged_release_base("1.2.3-beta.1").is_err());
    }

    #[test]
    fn stable_versions_are_strict_and_orderable() {
        assert_eq!(stable_version_parts("0.1.0").unwrap(), [0, 1, 0]);
        assert_eq!(stable_version_parts("10.20.30").unwrap(), [10, 20, 30]);
        assert!(stable_version_parts("0.1.0-beta.1").is_err());
        assert!(stable_version_parts("01.2.3").is_err());
        assert!(stable_version_parts("1.2").is_err());
        assert!(stable_version_parts("1.2.3.4").is_err());
        assert!(stable_version_parts("1.two.3").is_err());
        assert!(stable_version_parts("1.2.4").unwrap() > stable_version_parts("1.2.3").unwrap());
    }

    #[test]
    fn update_metadata_is_strict() {
        let text = concat!(
            "WANA-UPDATE-1\n",
            "version=0.1.0\n",
            "commit=0123456789abcdef0123456789abcdef01234567\n",
            "rootfs=rootfs.ext4.zst\n",
            "rootfs_size=123\n",
            "rootfs_sha256=aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\n",
            "kernel=bzImage\n",
            "kernel_size=456\n",
            "kernel_sha256=bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb\n",
            "initrd=rootfs.cpio.zst\n",
            "initrd_size=789\n",
            "initrd_sha256=cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc\n",
        );
        let meta = parse_update(text).unwrap();
        assert_eq!(meta.version, "0.1.0");
        assert_eq!(meta.rootfs_size, 123);
        assert!(parse_update(&(text.to_string() + "escape=1\n")).is_err());
    }
}
