#!/usr/bin/env python3
"""Create a throwaway Wana Live ISO with /live/test.cfg added."""
from __future__ import annotations

import os
from pathlib import Path
import subprocess
import sys
import tempfile


def main() -> int:
    if len(sys.argv) != 4:
        print(
            "usage: mk-test-live-iso.py <Wana-OS-Live.iso> <test-copy.iso> <kernel args>",
            file=sys.stderr,
        )
        return 2

    src = Path(sys.argv[1])
    dst = Path(sys.argv[2])
    args = sys.argv[3]
    xorriso = os.environ.get("XORRISO", "xorriso")

    if not src.is_file():
        print(f"[BOOT] error: Live ISO not found: {src}", file=sys.stderr)
        return 2

    dst.parent.mkdir(parents=True, exist_ok=True)
    dst.unlink(missing_ok=True)

    with tempfile.NamedTemporaryFile("w", encoding="utf-8", delete=False) as f:
        cfg = Path(f.name)
        f.write('set timeout=0\n')
        f.write(f'set wana_args="{args}"\n')

    try:
        subprocess.run(
            [
                xorriso,
                "-indev",
                str(src),
                "-outdev",
                str(dst),
                "-boot_image",
                "any",
                "replay",
                "-map",
                str(cfg),
                "/live/test.cfg",
                "-commit",
                "-end",
            ],
            check=True,
            stdout=subprocess.DEVNULL,
        )
    except (OSError, subprocess.CalledProcessError) as e:
        print(f"[BOOT] error: xorriso failed: {e}", file=sys.stderr)
        return 1
    finally:
        cfg.unlink(missing_ok=True)

    if not dst.is_file() or dst.stat().st_size == 0:
        print(f"[BOOT] error: failed to create test Live ISO: {dst}", file=sys.stderr)
        return 1

    print(f'[BOOT] info: {dst}: /live/test.cfg -> wana_args="{args}"')
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
