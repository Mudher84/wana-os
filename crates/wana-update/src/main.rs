use std::path::Path;
use std::process::ExitCode;
use wana_log::{error, info, Subsystem};

const LOG: Subsystem = Subsystem::Security;

fn usage() -> String {
    "usage: wana-update {verify BUNDLE_DIR|stage BUNDLE_DIR|status|clear}".into()
}

fn run() -> Result<(), String> {
    let mut args = std::env::args().skip(1);
    match args.next().as_deref() {
        Some("verify") => {
            let dir = args.next().ok_or_else(usage)?;
            if args.next().is_some() {
                return Err(usage());
            }
            let verified = wana_update::verify_bundle(Path::new(&dir))?;
            println!(
                "WANA_UPDATE_VERIFY_PASS version={} commit={} rootfs_sha256={} kernel_sha256={}",
                verified.metadata.version,
                verified.metadata.commit,
                verified.metadata.rootfs_sha256,
                verified.metadata.kernel_sha256
            );
            Ok(())
        }
        Some("stage") => {
            let dir = args.next().ok_or_else(usage)?;
            if args.next().is_some() {
                return Err(usage());
            }
            let metadata = wana_update::stage_bundle(Path::new(&dir))?;
            println!(
                "WANA_UPDATE_STAGED version={} commit={}",
                metadata.version, metadata.commit
            );
            info!(
                LOG,
                "update staged: version={} commit={}",
                metadata.version,
                metadata.commit
            );
            Ok(())
        }
        Some("status") => {
            if args.next().is_some() {
                return Err(usage());
            }
            if let Some(metadata) = wana_update::pending_metadata()? {
                println!(
                    "WANA_UPDATE_PENDING version={} commit={} rootfs_sha256={} kernel_sha256={}",
                    metadata.version,
                    metadata.commit,
                    metadata.rootfs_sha256,
                    metadata.kernel_sha256
                );
            } else {
                println!("WANA_UPDATE_NONE");
            }
            Ok(())
        }
        Some("clear") => {
            if args.next().is_some() {
                return Err(usage());
            }
            wana_update::clear_pending()?;
            println!("WANA_UPDATE_CLEARED");
            Ok(())
        }
        _ => Err(usage()),
    }
}

fn main() -> ExitCode {
    wana_log::init_from_env();
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            error!(LOG, "update: {e}");
            ExitCode::FAILURE
        }
    }
}
