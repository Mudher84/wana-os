//! Generates the Wayland protocol tables from the protocol XML of the
//! libwayland and wayland-protocols packages the image ships, so the tables
//! always match the library they are used with.
//!
//! - `WANA_WAYLAND_XML`: wayland.xml (default `/usr/share/wayland/wayland.xml`)
//! - `WANA_WAYLAND_PROTOCOLS_DIR`: wayland-protocols data directory
//!   (default `/usr/share/wayland-protocols`)
//!
//! Buildroot points both at its staging directory (package wana-compositor).
//!
//! Protocols that are in neither package are kept in `protocols/` of this
//! crate, pinned (see protocols/README.md): wlr-layer-shell (decision 0003).

#[allow(dead_code)]
#[path = "src/scanner.rs"]
mod scanner;

use std::path::{Path, PathBuf};
use std::{env, fs};

/// Extension protocols, relative to the wayland-protocols directory.
const EXTENSIONS: &[&str] = &["stable/xdg-shell/xdg-shell.xml"];
/// Protocols kept in this crate, relative to its `protocols/` directory.
const PINNED: &[&str] = &["wlr-layer-shell-unstable-v1.xml"];

fn from_env(var: &str, default: &str) -> PathBuf {
    println!("cargo:rerun-if-env-changed={var}");
    env::var_os(var).map_or_else(|| PathBuf::from(default), PathBuf::from)
}

fn load(path: &Path) -> scanner::Protocol {
    println!("cargo:rerun-if-changed={}", path.display());
    let xml = fs::read_to_string(path).unwrap_or_else(|e| {
        panic!(
            "[BUILD] error: cannot read protocol XML {}: {e} \
             (install wayland/wayland-protocols or set WANA_WAYLAND_XML / WANA_WAYLAND_PROTOCOLS_DIR)",
            path.display()
        )
    });
    scanner::parse(&xml).unwrap_or_else(|e| panic!("[BUILD] error: {}: {e}", path.display()))
}

fn main() {
    println!("cargo:rerun-if-changed=src/scanner.rs");
    let core = from_env("WANA_WAYLAND_XML", "/usr/share/wayland/wayland.xml");
    let dir = from_env("WANA_WAYLAND_PROTOCOLS_DIR", "/usr/share/wayland-protocols");
    let mut protocols = vec![load(&core)];
    protocols.extend(EXTENSIONS.iter().map(|rel| load(&dir.join(rel))));
    let pinned = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR"))
        .join("protocols");
    protocols.extend(PINNED.iter().map(|rel| load(&pinned.join(rel))));
    let code = scanner::generate(&protocols)
        .unwrap_or_else(|e| panic!("[BUILD] error: protocol tables: {e}"));
    let out = PathBuf::from(env::var_os("OUT_DIR").expect("OUT_DIR")).join("protocols.rs");
    fs::write(&out, code).unwrap_or_else(|e| panic!("[BUILD] error: {}: {e}", out.display()));
}
