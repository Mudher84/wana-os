# Test report: Phase 6 (Reproducible build and build manifest)

What Phase 6 adds:
- `tools/build-manifest.py` writes `images/build-manifest.json` and `images/SHA256SUMS` after every
  `make image`. The manifest records:
  - the Git commit (and whether the tree was dirty);
  - the Buildroot version, pinned commit and the commit actually used;
  - `SOURCE_DATE_EPOCH`;
  - the kernel version and the SHA-256 of its resolved config;
  - the toolchain (gcc, glibc, kernel headers, rustc);
  - SHA-256 of the input configs and of the resolved Buildroot and kernel configs;
  - name and version of every package;
  - SHA-256 and size of every artifact.
- `tools/compare-manifests.py` compares two builds artifact by artifact.
- Workflow `reproducibility`: two independent builds of one commit on two runners, then a
  comparison. Build B runs with `CCACHE_DISABLE=1`, so it really recompiles.
- Disk image determinism fixes, all found by a local double run:
  - fixed GPT disk GUID and ESP PARTUUID;
  - fixed FAT volume ID;
  - `mkfs.fat --invariant` for the volume-label timestamp;
  - ESP files pinned to `SOURCE_DATE_EPOCH`.

`SOURCE_DATE_EPOCH` is Buildroot's default: the commit time of the pinned Buildroot tree
(1781643700 for 2026.02.3). It is the same for every build of every Wana commit on that
Buildroot release.

## T1: Manifest content (local, stand-in output tree)

- Setup: output dir with the real resolved `.config`, the local kernel `.config`, and the Phase 5 local images; `show-info` from the real Buildroot tree
- Command: `make manifest BR_OUT=<dir>`
- Expected: all artifacts hashed, symlinks recorded as symlinks, Buildroot commit verified, packages listed
- Actual:
  - 7 artifacts hashed; `rootfs.ext4` recorded as `symlink -> rootfs.ext2`;
  - `buildroot.commit_used == commit_pinned` (679b9ead…);
  - `source_date_epoch` 1781643700;
  - 61 packages, including `linux 6.18.33`, `wana-init 0.1.0`, `host-rust-bin 1.88.0`, `grub2 2.12`, `glibc 2.42-67-g4ebd33dd…`;
  - a dirty working tree produced the warning `the build is not traceable to a commit`.
- Result: **PASS**

## T2: Comparison tool (local)

- Same manifest twice: `reproducibility: PASS (8/8 artifacts identical)`, exit 0
- One artifact changed and one removed: both named (`DIFFERENT esp.vfat …`, `DIFFERENT … grub.cfg (only in A)`),
  `FAIL (6/8 …, 2 problem(s))`, exit 1
- Result: **PASS**

## T3: Disk image assembly is deterministic (local)

- Command: run `post-image.sh` twice on identical inputs, 2 s apart
- First attempt, before the fixes: **FAIL**. `esp.vfat` and `disk.img` differed:
  - random GPT disk GUID;
  - random ESP partition GUID;
  - FAT volume serial (bytes 40-42);
  - FAT timestamps.
- After fixing identifiers and file times: still **FAIL**. 2 bytes differed, in the volume-label directory entry (`WANA-ESP`), where `mkfs.fat` writes the current time.
- After adding `--invariant`: **identical** `esp.vfat` (d7f2e8d7…) and `disk.img` (192627dc…). The GUIDs and volume serial match `disk.env`.
- The image still boots: firmware → GRUB → `[INIT] info: ready` → power off
- Result: **PASS**

## T4: Buildroot build with manifest in CI

- Workflow `buildroot`: `make image` now ends with `make manifest`; boot tests unchanged
- Attempt 1, run [36120779779](https://github.com/Mudher84/wana-os/actions/runs/36120779779), commit `5a9e8f8`: **FAIL**. The
  `Build image` step exited 2 right after `Executing post-image script`. The console filter showed only `>>>` lines, and
  the log artifact could not be fetched from the development sandbox (blob storage blocked).
  - Isolation, first layer: `post-image.sh` run with Buildroot's genimage version (19, built from source): succeeds. Not the cause.
  - Isolation, second layer: `make manifest` run as a sub-make, as `make image` does: **reproduced**, exit 2.
    `out/show-info.json` started with `make[2]: Entering directory '…'`. GNU make prints that line in sub-makes, so the JSON was invalid.
  - Why the local test missed it: T1 ran `make manifest` at the top level, not through `make image`.
  - Fix: `--no-print-directory` on the Buildroot calls in `manifest`. The generator now reports invalid show-info with a
    clear `[BUILD] error` instead of a traceback. CI prints the last 80 log lines on failure and shows `[BUILD]` lines.
  - Re-test through a sub-make locally: exit 0, valid JSON, `source_date_epoch` 1781643700.
- Attempt 2: *pending*

## T5: Two independent builds produce identical artifacts (CI)

- Workflow `reproducibility`, build A (ccache) vs build B (no ccache), then `compare-manifests.py`
- Actual: *pending*. Any artifact that differs will be listed here with its cause and fix.
