#!/usr/bin/env python3
"""Self-test for the Wana OS release bundle verifier."""

import hashlib
import json
import subprocess
import sys
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
PREPARE = ROOT / "tools" / "prepare-release.py"
PAYLOAD = ("Wana-OS-Live.iso", "disk.img", "bzImage", "rootfs.cpio.zst")
COMMIT = "0123456789abcdef0123456789abcdef01234567"


def digest(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def fixture(root: Path) -> tuple[Path, Path]:
    images = root / "images"
    out = root / "release"
    images.mkdir()
    artifacts = []
    for index, name in enumerate(PAYLOAD, 1):
        path = images / name
        path.write_bytes((f"wana-{index}-".encode()) * 17)
        artifacts.append({
            "path": name,
            "size": path.stat().st_size,
            "sha256": digest(path),
        })
    manifest = {
        "schema": 1,
        "project": "wana-os",
        "git": {"commit": COMMIT, "describe": COMMIT[:12], "dirty": False},
        "artifacts": artifacts,
    }
    (images / "build-manifest.json").write_text(
        json.dumps(manifest, indent=2, sort_keys=True) + "\n"
    )
    with (images / "SHA256SUMS").open("w") as sums:
        for entry in artifacts:
            sums.write(f"{entry['sha256']}  {entry['path']}\n")
    return images, out


def run(images: Path, out: Path) -> subprocess.CompletedProcess:
    return subprocess.run(
        [
            sys.executable,
            str(PREPARE),
            "--images",
            str(images),
            "--out",
            str(out),
            "--channel",
            "beta",
            "--version",
            "0.1.0-beta.1",
            "--expected-commit",
            COMMIT,
        ],
        cwd=ROOT,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
    )


def main() -> int:
    with tempfile.TemporaryDirectory(prefix="wana-release-test-") as tmp:
        images, out = fixture(Path(tmp))
        good = run(images, out)
        if good.returncode != 0:
            print(good.stdout, end="")
            raise SystemExit("valid release fixture was rejected")
        for name in (*PAYLOAD, "build-manifest.json", "release.json", "RELEASE-SHA256SUMS"):
            if not (out / name).is_file():
                raise SystemExit(f"release output missing {name}")
        release_sums = {}
        for line in (out / "RELEASE-SHA256SUMS").read_text().splitlines():
            digest_value, name = line.split("  ", 1)
            release_sums[name] = digest_value
        expected = set(PAYLOAD) | {"build-manifest.json", "release.json"}
        if set(release_sums) != expected:
            raise SystemExit(
                f"release checksum closure mismatch: {sorted(release_sums)} != {sorted(expected)}"
            )
        for name, digest_value in release_sums.items():
            if digest(out / name) != digest_value:
                raise SystemExit(f"release checksum mismatch: {name}")

        (images / "bzImage").write_bytes(b"tampered")
        bad = run(images, out)
        if bad.returncode == 0 or "hash mismatch: bzImage" not in bad.stdout:
            print(bad.stdout, end="")
            raise SystemExit("tampered release fixture was not rejected")

    print("[RELEASE] verifier self-test: PASS")
    return 0


if __name__ == "__main__":
    sys.exit(main())
