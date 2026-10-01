use std::fs::{self, File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::os::unix::fs::FileTypeExt;
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

fn same_file(a: &Path, b: &Path) -> bool {
    match (fs::canonicalize(a), fs::canonicalize(b)) {
        (Ok(a), Ok(b)) => a == b,
        _ => a == b,
    }
}

fn validate(a: &Args) -> Result<(), String> {
    let source_meta = fs::metadata(&a.image)
        .map_err(|e| format!("installer source {}: {e}", a.image.display()))?;
    let source_type = source_meta.file_type();
    if !source_type.is_file() && !source_type.is_block_device() {
        return Err(format!(
            "installer source is not a regular file or block device: {}",
            a.image.display()
        ));
    }
    if same_file(&a.image, &a.target) {
        return Err("installer source and target are the same file/device".into());
    }
    let expected = format!("ERASE:{}", a.target.display());
    if a.confirm != expected {
        return Err(format!("confirmation mismatch; expected {expected:?}"));
    }
    let meta =
        fs::metadata(&a.target).map_err(|e| format!("target {}: {e}", a.target.display()))?;
    let ty = meta.file_type();
    if !ty.is_block_device() && !(a.allow_regular && ty.is_file()) {
        return Err(format!(
            "target {} is not a block device (regular targets require --allow-regular for tests)",
            a.target.display()
        ));
    }
    Ok(())
}

fn copy_and_verify(image: &Path, target: &Path) -> Result<u64, String> {
    let mut src = File::open(image).map_err(|e| format!("open {}: {e}", image.display()))?;
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
    if dst.metadata().map(|m| m.is_file()).unwrap_or(false) {
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
    validate(&a)?;
    info!(
        LOG,
        "installer write authorized: image={} target={}",
        a.image.display(),
        a.target.display()
    );
    let bytes = copy_and_verify(&a.image, &a.target)?;
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
        assert_eq!(copy_and_verify(&src, &dst).unwrap(), data.len() as u64);
        assert_eq!(fs::read(&dst).unwrap(), data);
        let _ = fs::remove_file(src);
        let _ = fs::remove_file(dst);
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
