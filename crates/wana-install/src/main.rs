use std::fs::{self, File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::os::unix::fs::{FileTypeExt, MetadataExt};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use wana_log::{error, info, Subsystem};

const LOG: Subsystem = Subsystem::Init;
const CHUNK: usize = 1024 * 1024;

#[derive(Debug)]
struct Args {
    image: PathBuf,
    target: PathBuf,
    confirm: String,
    allow_regular: bool,
}

#[derive(Debug, Clone, Copy)]
struct ValidatedPaths {
    source_dev: u64,
    source_ino: u64,
    target_dev: u64,
    target_ino: u64,
}

fn args() -> Result<Args, String> {
    let mut image = None;
    let mut target = None;
    let mut confirm = None;
    let mut allow_regular = false;
    let mut it = std::env::args().skip(1);
    while let Some(a) = it.next() {
        match a.as_str() {
            "--image" => image = Some(PathBuf::from(it.next().ok_or("--image needs a path")?)),
            "--target" => target = Some(PathBuf::from(it.next().ok_or("--target needs a path")?)),
            "--confirm" => confirm = Some(it.next().ok_or("--confirm needs ERASE:<target>")?),
            "--allow-regular" => allow_regular = true,
            _ => return Err(format!("unknown argument {a}")),
        }
    }
    Ok(Args {
        image: image.ok_or("--image is required")?,
        target: target.ok_or("--target is required")?,
        confirm: confirm.ok_or("--confirm is required")?,
        allow_regular,
    })
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

fn validate(a: &Args) -> Result<ValidatedPaths, String> {
    let source_meta = fs::symlink_metadata(&a.image)
        .map_err(|e| format!("installer source {}: {e}", a.image.display()))?;
    if source_meta.file_type().is_symlink() {
        return Err(format!(
            "installer source must not be a symlink: {}",
            a.image.display()
        ));
    }
    let source_type = source_meta.file_type();
    if !source_type.is_file() && !source_type.is_block_device() {
        return Err(format!(
            "installer source is not a regular file or block device: {}",
            a.image.display()
        ));
    }
    let expected = format!("ERASE:{}", a.target.display());
    if a.confirm != expected {
        return Err(format!("confirmation mismatch; expected {expected:?}"));
    }
    let meta = fs::symlink_metadata(&a.target)
        .map_err(|e| format!("target {}: {e}", a.target.display()))?;
    if source_meta.dev() == meta.dev() && source_meta.ino() == meta.ino() {
        return Err("installer source and target are the same file/device".into());
    }
    if source_meta.file_type().is_block_device()
        && meta.file_type().is_block_device()
        && source_meta.rdev() == meta.rdev()
    {
        return Err("installer source and target refer to the same block device".into());
    }
    if meta.file_type().is_symlink() {
        return Err(format!(
            "installer target must not be a symlink: {}",
            a.target.display()
        ));
    }
    let ty = meta.file_type();
    if !(ty.is_block_device() || a.allow_regular && ty.is_file()) {
        return Err(format!(
            "target {} is not a block device (regular targets require --allow-regular for tests)",
            a.target.display()
        ));
    }
    if ty.is_block_device() && effective_uid()? != 0 {
        return Err("installing to a block device requires effective uid 0".into());
    }
    Ok(ValidatedPaths {
        source_dev: source_meta.dev(),
        source_ino: source_meta.ino(),
        target_dev: meta.dev(),
        target_ino: meta.ino(),
    })
}

fn copy_and_verify(image: &Path, target: &Path, validated: &ValidatedPaths) -> Result<u64, String> {
    let mut src = File::open(image).map_err(|e| format!("open {}: {e}", image.display()))?;
    let opened_source = src
        .metadata()
        .map_err(|e| format!("metadata {}: {e}", image.display()))?;
    if opened_source.dev() != validated.source_dev || opened_source.ino() != validated.source_ino {
        return Err(format!(
            "installer source changed after validation: {}",
            image.display()
        ));
    }
    let source_len = src
        .seek(SeekFrom::End(0))
        .map_err(|e| format!("size {}: {e}", image.display()))?;
    src.seek(SeekFrom::Start(0))
        .map_err(|e| format!("rewind {}: {e}", image.display()))?;
    if source_len == 0 {
        return Err("installer source is empty".into());
    }

    let mut dst = OpenOptions::new()
        .read(true)
        .write(true)
        .open(target)
        .map_err(|e| format!("open target {}: {e}", target.display()))?;
    let opened_target = dst
        .metadata()
        .map_err(|e| format!("metadata target {}: {e}", target.display()))?;
    if opened_target.dev() != validated.target_dev || opened_target.ino() != validated.target_ino {
        return Err(format!(
            "installer target changed after validation: {}",
            target.display()
        ));
    }
    if opened_target.is_file() {
        dst.set_len(source_len)
            .map_err(|e| format!("size target {}: {e}", target.display()))?;
    }
    dst.seek(SeekFrom::Start(0))
        .map_err(|e| format!("seek target: {e}"))?;

    let mut buf = vec![0u8; CHUNK];
    let mut written = 0u64;
    loop {
        let n = src.read(&mut buf).map_err(|e| format!("read image: {e}"))?;
        if n == 0 {
            break;
        }
        dst.write_all(&buf[..n])
            .map_err(|e| format!("write target at byte {written}: {e}"))?;
        written += n as u64;
    }
    if written != source_len {
        return Err(format!(
            "short installer write: {written}/{source_len} bytes"
        ));
    }
    dst.sync_all().map_err(|e| format!("sync target: {e}"))?;

    src.seek(SeekFrom::Start(0))
        .map_err(|e| format!("rewind image: {e}"))?;
    dst.seek(SeekFrom::Start(0))
        .map_err(|e| format!("rewind target: {e}"))?;
    let mut a = vec![0u8; CHUNK];
    let mut b = vec![0u8; CHUNK];
    let mut checked = 0u64;
    loop {
        let na = src
            .read(&mut a)
            .map_err(|e| format!("verify image read: {e}"))?;
        if na == 0 {
            break;
        }
        let mut got = 0;
        while got < na {
            let n = dst
                .read(&mut b[got..na])
                .map_err(|e| format!("verify target read at byte {}: {e}", checked + got as u64))?;
            if n == 0 {
                return Err(format!(
                    "verify target ended early at byte {}",
                    checked + got as u64
                ));
            }
            got += n;
        }
        if a[..na] != b[..na] {
            let first = a[..na]
                .iter()
                .zip(&b[..na])
                .position(|(x, y)| x != y)
                .unwrap_or(0);
            return Err(format!(
                "readback mismatch at byte {}",
                checked + first as u64
            ));
        }
        checked += na as u64;
    }
    Ok(checked)
}

fn run() -> Result<(), String> {
    let a = args()?;
    let validated = validate(&a)?;
    info!(
        LOG,
        "installer write authorized: image={} target={}",
        a.image.display(),
        a.target.display()
    );
    let bytes = copy_and_verify(&a.image, &a.target, &validated)?;
    info!(LOG, "installer readback PASS: {bytes} bytes");
    Ok(())
}

fn main() -> ExitCode {
    wana_log::init_from_env();
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            error!(LOG, "installer: {e}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    static N: AtomicUsize = AtomicUsize::new(0);
    fn temp(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "wana-install-test-{}-{}-{name}",
            std::process::id(),
            N.fetch_add(1, Ordering::Relaxed)
        ))
    }

    #[test]
    fn regular_file_copy_is_exact_when_explicitly_allowed() {
        let src = temp("src");
        let dst = temp("dst");
        let data: Vec<u8> = (0..2_500_000).map(|i| (i % 251) as u8).collect();
        fs::write(&src, &data).unwrap();
        fs::write(&dst, b"x").unwrap();
        let args = Args {
            image: src.clone(),
            target: dst.clone(),
            confirm: format!("ERASE:{}", dst.display()),
            allow_regular: true,
        };
        let validated = validate(&args).unwrap();
        assert_eq!(
            copy_and_verify(&src, &dst, &validated).unwrap(),
            data.len() as u64
        );
        assert_eq!(fs::read(&dst).unwrap(), data);
        let _ = fs::remove_file(src);
        let _ = fs::remove_file(dst);
    }

    #[test]
    fn copy_rejects_target_replaced_after_validation() {
        let src = temp("src-toctou");
        let dst = temp("dst-toctou");
        let old = temp("old-dst-toctou");
        fs::write(&src, b"image-data").unwrap();
        fs::write(&dst, b"original-target").unwrap();

        let args = Args {
            image: src.clone(),
            target: dst.clone(),
            confirm: format!("ERASE:{}", dst.display()),
            allow_regular: true,
        };
        let validated = validate(&args).unwrap();

        fs::rename(&dst, &old).unwrap();
        fs::write(&dst, b"replacement-target").unwrap();
        let error = copy_and_verify(&src, &dst, &validated).unwrap_err();
        assert!(error.contains("target changed after validation"));
        assert_eq!(fs::read(&dst).unwrap(), b"replacement-target");

        let _ = fs::remove_file(src);
        let _ = fs::remove_file(dst);
        let _ = fs::remove_file(old);
    }

    #[test]
    fn validation_rejects_hardlinked_source_and_target() {
        let src = temp("src-hardlink");
        let dst = temp("dst-hardlink");
        fs::write(&src, b"image").unwrap();
        fs::hard_link(&src, &dst).unwrap();

        let args = Args {
            image: src.clone(),
            target: dst.clone(),
            confirm: format!("ERASE:{}", dst.display()),
            allow_regular: true,
        };
        assert!(validate(&args).is_err());

        let _ = fs::remove_file(src);
        let _ = fs::remove_file(dst);
    }

    #[test]
    fn validation_rejects_symlink_source_and_target() {
        use std::os::unix::fs::symlink;

        let src = temp("src-link-source");
        let real_src = temp("real-src");
        let dst = temp("dst-link-source");
        fs::write(&real_src, b"image").unwrap();
        symlink(&real_src, &src).unwrap();
        fs::write(&dst, b"target").unwrap();
        let a = Args {
            image: src.clone(),
            target: dst.clone(),
            confirm: format!("ERASE:{}", dst.display()),
            allow_regular: true,
        };
        assert!(validate(&a).is_err());

        let real_dst = temp("real-dst");
        let dst_link = temp("dst-link");
        fs::write(&real_dst, b"target").unwrap();
        symlink(&real_dst, &dst_link).unwrap();
        let b = Args {
            image: real_src.clone(),
            target: dst_link.clone(),
            confirm: format!("ERASE:{}", dst_link.display()),
            allow_regular: true,
        };
        assert!(validate(&b).is_err());

        let _ = fs::remove_file(src);
        let _ = fs::remove_file(real_src);
        let _ = fs::remove_file(dst);
        let _ = fs::remove_file(dst_link);
        let _ = fs::remove_file(real_dst);
    }

    #[test]
    fn validation_requires_exact_destructive_confirmation() {
        let src = temp("src");
        let dst = temp("dst");
        fs::write(&src, b"image").unwrap();
        fs::write(&dst, b"target").unwrap();
        let mut a = Args {
            image: src.clone(),
            target: dst.clone(),
            confirm: "no".into(),
            allow_regular: true,
        };
        assert!(validate(&a).is_err());
        a.confirm = format!("ERASE:{}", dst.display());
        assert!(validate(&a).is_ok());
        a.image = dst.clone();
        assert!(validate(&a).is_err());
        let _ = fs::remove_file(src);
        let _ = fs::remove_file(dst);
    }
}
