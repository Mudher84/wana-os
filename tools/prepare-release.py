#!/usr/bin/env python3
"""Validate a Wana OS build and prepare a closed release bundle."""

import argparse
import hashlib
import json
import re
import shutil
import sys
from pathlib import Path

VERSION_RE = re.compile(r"^[0-9]+\.[0-9]+\.[0-9]+(?:-(?:beta|rc)\.[0-9]+)?$")
COMMIT_RE = re.compile(r"^[0-9a-f]{40}$")
PAYLOAD = (
    "Wana-OS-Live.iso",
    "disk.img",
    "bzImage",
    "rootfs.cpio.zst",
    "rootfs.ext4.zst",
)
ROOT = Path(__file__).resolve().parent.parent


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as source:
        for block in iter(lambda: source.read(1 << 20), b""):
            digest.update(block)
    return digest.hexdigest()


def fail(message: str) -> None:
    raise SystemExit(f"[RELEASE] error: {message}")


def read_sums(path: Path) -> dict[str, str]:
    sums = {}
    for line_no, line in enumerate(path.read_text().splitlines(), 1):
        if not line.strip():
            continue
        try:
            digest, name = line.split("  ", 1)
        except ValueError:
            fail(f"{path}:{line_no}: expected '<sha256>  <path>'")
        if not re.fullmatch(r"[0-9a-f]{64}", digest):
            fail(f"{path}:{line_no}: invalid SHA-256")
        if name in sums:
            fail(f"{path}:{line_no}: duplicate artifact {name}")
        sums[name] = digest
    return sums


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--images", required=True, type=Path)
    ap.add_argument("--out", required=True, type=Path)
    ap.add_argument("--channel", choices=["beta", "stable"], required=True)
    ap.add_argument("--version", required=True)
    ap.add_argument("--expected-commit")
    args = ap.parse_args()

    if not VERSION_RE.fullmatch(args.version):
        fail(f"invalid version {args.version!r}")
    if args.channel == "beta" and "-beta." not in args.version:
        fail("beta channel version must contain '-beta.N'")
    if args.channel == "stable" and "-" in args.version:
        fail("stable channel version must not contain a prerelease suffix")

    cargo_text = (ROOT / "Cargo.toml").read_text()
    workspace = re.search(
        r'(?ms)^\[workspace\.package\]\s*.*?^version\s*=\s*"([^"]+)"',
        cargo_text,
    )
    if not workspace:
        fail("workspace package version is missing")
    base_version = args.version.split("-", 1)[0]
    if base_version != workspace.group(1):
        fail(
            f"release base version {base_version} != workspace version {workspace.group(1)}"
        )

    manifest_path = args.images / "build-manifest.json"
    sums_path = args.images / "SHA256SUMS"
    if not manifest_path.is_file() or not sums_path.is_file():
        fail("build-manifest.json and SHA256SUMS are required")

    manifest = json.loads(manifest_path.read_text())
    if manifest.get("schema") != 1 or manifest.get("project") != "wana-os":
        fail("unsupported or foreign build manifest")

    git = manifest.get("git", {})
    commit = git.get("commit")
    if not isinstance(commit, str) or not COMMIT_RE.fullmatch(commit):
        fail("manifest git.commit is missing or invalid")
    if git.get("dirty") is not False:
        fail("release builds must come from a clean Git tree")
    if args.expected_commit and commit != args.expected_commit:
        fail(f"manifest commit {commit} != expected {args.expected_commit}")

    entries = {
        artifact["path"]: artifact
        for artifact in manifest.get("artifacts", [])
        if "sha256" in artifact
    }
    sums = read_sums(sums_path)
    if set(entries) != set(sums):
        missing = sorted(set(entries) - set(sums))
        extra = sorted(set(sums) - set(entries))
        fail(f"SHA256SUMS is not closed over manifest artifacts; missing={missing}, extra={extra}")

    for name, entry in sorted(entries.items()):
        path = args.images / name
        if not path.is_file():
            fail(f"manifest artifact missing: {name}")
        actual = sha256(path)
        if actual != entry["sha256"] or actual != sums[name]:
            fail(f"hash mismatch: {name}")

    for name in PAYLOAD:
        if name not in entries:
            fail(f"required release payload missing from manifest: {name}")

    if args.out.exists():
        shutil.rmtree(args.out)
    args.out.mkdir(parents=True)

    release_entries = []
    for name in PAYLOAD:
        source = args.images / name
        target = args.out / name
        shutil.copyfile(source, target)
        release_entries.append({
            "path": name,
            "size": target.stat().st_size,
            "sha256": entries[name]["sha256"],
        })

    manifest_target = args.out / manifest_path.name
    shutil.copyfile(manifest_path, manifest_target)
    metadata = {
        "schema": 1,
        "project": "wana-os",
        "channel": args.channel,
        "version": args.version,
        "commit": commit,
        "payload": release_entries,
    }
    release_json = args.out / "release.json"
    release_json.write_text(
        json.dumps(metadata, indent=2, sort_keys=True) + "\n"
    )

    by_name = {entry["path"]: entry for entry in release_entries}
    rootfs = by_name["rootfs.ext4.zst"]
    rootfs_raw = args.images / "rootfs.ext4"
    if not rootfs_raw.is_file():
        fail("rootfs.ext4 is required to bind update slot capacity")
    rootfs_raw_size = rootfs_raw.stat().st_size
    if rootfs_raw_size <= 0:
        fail("rootfs.ext4 is empty")
    kernel = by_name["bzImage"]
    initrd = by_name["rootfs.cpio.zst"]
    update_txt = args.out / "update.txt"
    update_txt.write_text(
        "WANA-UPDATE-1\n"
        f"version={args.version}\n"
        f"commit={commit}\n"
        "rootfs=rootfs.ext4.zst\n"
        f"rootfs_size={rootfs['size']}\n"
        f"rootfs_raw_size={rootfs_raw_size}\n"
        f"rootfs_sha256={rootfs['sha256']}\n"
        "kernel=bzImage\n"
        f"kernel_size={kernel['size']}\n"
        f"kernel_sha256={kernel['sha256']}\n"
        "initrd=rootfs.cpio.zst\n"
        f"initrd_size={initrd['size']}\n"
        f"initrd_sha256={initrd['sha256']}\n"
    )

    bundle_entries = [
        *release_entries,
        {"path": manifest_target.name, "sha256": sha256(manifest_target)},
        {"path": release_json.name, "sha256": sha256(release_json)},
        {"path": update_txt.name, "sha256": sha256(update_txt)},
    ]
    with (args.out / "RELEASE-SHA256SUMS").open("w") as output:
        for entry in bundle_entries:
            output.write(f"{entry['sha256']}  {entry['path']}\n")

    print(
        f"[RELEASE] {args.channel} {args.version}: PASS "
        f"commit={commit} payload={len(release_entries)}"
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
