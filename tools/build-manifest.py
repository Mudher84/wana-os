#!/usr/bin/env python3
"""Write the Wana OS build manifest and SHA256SUMS for a Buildroot output dir.

The manifest answers, for any image: which commit produced it, with which
configuration, toolchain, kernel, Buildroot and Rust versions, and what the
SHA-256 of every artifact is. Two builds of the same commit should produce
identical artifact hashes; tools/compare-manifests.py checks that.

Usage: build-manifest.py --br-out DIR [--show-info FILE] [--source-date-epoch N]
Writes DIR/images/build-manifest.json and DIR/images/SHA256SUMS.
"""

import argparse
import hashlib
import json
import os
import re
import subprocess
import sys
from pathlib import Path

SCHEMA = 1
ROOT = Path(__file__).resolve().parent.parent
MANIFEST = "build-manifest.json"
SUMS = "SHA256SUMS"


def sha256(path: Path) -> str:
    h = hashlib.sha256()
    with path.open("rb") as f:
        for block in iter(lambda: f.read(1 << 20), b""):
            h.update(block)
    return h.hexdigest()


def run(*cmd, cwd=None) -> str:
    try:
        out = subprocess.run(cmd, cwd=cwd, check=True, capture_output=True, text=True)
        return out.stdout.strip()
    except (OSError, subprocess.CalledProcessError):
        return ""


def read_kv(path: Path) -> dict:
    """KEY=VALUE files (buildroot.env, Buildroot/kernel .config)."""
    values = {}
    if not path.is_file():
        return values
    for line in path.read_text().splitlines():
        m = re.match(r'^([A-Za-z0-9_]+)=(.*)$', line)
        if m:
            values[m.group(1)] = m.group(2).strip('"')
    return values


def git_info() -> dict:
    commit = run("git", "rev-parse", "HEAD", cwd=ROOT)
    dirty = bool(run("git", "status", "--porcelain", "--untracked-files=no", cwd=ROOT))
    return {
        "commit": commit or None,
        "describe": run("git", "describe", "--always", "--dirty", "--tags", cwd=ROOT) or None,
        "dirty": dirty,
    }


def file_entry(path: Path, base: Path) -> dict:
    return {"path": str(path.relative_to(base)), "size": path.stat().st_size, "sha256": sha256(path)}


def artifacts(images: Path) -> list:
    """Every regular file under images/, except the outputs of this script.
    Symlinks are recorded with their target, not hashed twice."""
    entries = []
    for path in sorted(images.rglob("*")):
        rel = path.relative_to(images)
        if str(rel) in (MANIFEST, SUMS):
            continue
        if path.is_symlink():
            entries.append({"path": str(rel), "symlink": os.readlink(path)})
        elif path.is_file():
            entries.append(file_entry(path, images))
    return entries


def first_line(text: str):
    return text.splitlines()[0] if text else None


def toolchain(br_out: Path, brconfig: dict) -> dict:
    gcc = next(iter(sorted((br_out / "host" / "bin").glob("*-linux-gnu-gcc"))), None)
    rustc = br_out / "host" / "bin" / "rustc"
    return {
        "gcc": first_line(run(str(gcc), "--version")) if gcc else None,
        "gcc_version": brconfig.get("BR2_GCC_VERSION"),
        "libc": "glibc" if brconfig.get("BR2_TOOLCHAIN_BUILDROOT_GLIBC") == "y" else None,
        "kernel_headers": brconfig.get("BR2_DEFAULT_KERNEL_HEADERS"),
        "rustc": first_line(run(str(rustc), "--version")) if rustc.exists() else None,
    }


def packages(show_info: Path) -> dict:
    """name -> version for every target and host package in the build."""
    if not show_info or not show_info.is_file():
        return {}
    text = show_info.read_text()
    try:
        info = json.loads(text)
    except json.JSONDecodeError as e:
        raise SystemExit(f"[BUILD] error: {show_info} is not valid JSON ({e}); "
                         f"starts with: {text[:80]!r}")
    return {name: pkg.get("version", "") for name, pkg in sorted(info.items())
            if pkg.get("type") in ("target", "host", "toolchain", "bootloader", "linux")}


def build(br_out: Path, show_info, source_date_epoch) -> dict:
    brconfig = read_kv(br_out / ".config")
    env = read_kv(ROOT / "platform" / "buildroot.env")
    kver = brconfig.get("BR2_LINUX_KERNEL_VERSION")
    kconfig = br_out / "build" / f"linux-{kver}" / ".config"
    br_src = ROOT / "out" / f"buildroot-{env.get('BUILDROOT_VERSION')}"

    configs = [ROOT / "platform" / "configs" / "wana_x86_64_defconfig",
               ROOT / "platform" / "board" / "x86_64" / "linux.fragment",
               ROOT / "platform" / "board" / "x86_64" / "grub.cfg",
               ROOT / "platform" / "board" / "x86_64" / "genimage.cfg",
               ROOT / "Cargo.lock"]
    resolved = {"buildroot.config": br_out / ".config", "linux.config": kconfig}

    return {
        "schema": SCHEMA,
        "project": "wana-os",
        "git": git_info(),
        "source_date_epoch": source_date_epoch,
        "buildroot": {
            "version": env.get("BUILDROOT_VERSION"),
            "commit_pinned": env.get("BUILDROOT_COMMIT"),
            "commit_used": run("git", "rev-parse", "HEAD", cwd=br_src) or None,
        },
        "kernel": {"version": kver, "config_sha256": sha256(kconfig) if kconfig.is_file() else None},
        "toolchain": toolchain(br_out, brconfig),
        "configs": [file_entry(p, ROOT) for p in configs if p.is_file()],
        "resolved_configs": {k: sha256(p) for k, p in resolved.items() if p.is_file()},
        "packages": packages(show_info),
        "artifacts": artifacts(br_out / "images"),
    }


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--br-out", required=True, type=Path)
    ap.add_argument("--show-info", type=Path)
    ap.add_argument("--source-date-epoch", type=int)
    args = ap.parse_args()

    images = args.br_out / "images"
    if not images.is_dir():
        print(f"[BUILD] error: {images} does not exist", file=sys.stderr)
        return 1
    manifest = build(args.br_out, args.show_info, args.source_date_epoch)
    if manifest["buildroot"]["commit_used"] not in (None, manifest["buildroot"]["commit_pinned"]):
        print("[BUILD] error: Buildroot tree does not match the pinned commit", file=sys.stderr)
        return 1

    (images / MANIFEST).write_text(json.dumps(manifest, indent=2, sort_keys=True) + "\n")
    with (images / SUMS).open("w") as f:
        for a in manifest["artifacts"]:
            if "sha256" in a:
                f.write(f"{a['sha256']}  {a['path']}\n")
    files = sum(1 for a in manifest["artifacts"] if "sha256" in a)
    print(f"[BUILD] info: manifest: {files} artifacts, commit {manifest['git']['describe']}, "
          f"{len(manifest['packages'])} packages -> {images / MANIFEST}")
    if manifest["git"]["dirty"]:
        print("[BUILD] warn: working tree has uncommitted changes; the build is not traceable to a commit")
    return 0


if __name__ == "__main__":
    sys.exit(main())
