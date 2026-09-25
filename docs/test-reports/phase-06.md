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
- Attempt 2, run [36124722997](https://github.com/Mudher84/wana-os/actions/runs/36124722997), commit `4dff4d5`: **PASS**
  - `[BUILD] info: manifest: 11 artifacts, commit 4dff4d5, 61 packages`
  - Manifest summary: `dirty: false`; Buildroot `commit_used == commit_pinned` (679b9ead…);
    gcc `x86_64-buildroot-linux-gnu-gcc.br_real (Buildroot 2026.02.3) 14.3.0`; rustc `1.88.0 (6b00bc388 2025-06-23)`;
    headers 6.18.33; glibc; kernel config sha256 `837bb6cf…`; `source_date_epoch` 1781643700
  - SHA256SUMS (excerpt): `bzImage 469b6182…`, `disk.img 01dc4fdd…`, `esp.vfat 003431fc…`, `rootfs.cpio.zst 3f049803…`,
    `efi-part/EFI/BOOT/grub.cfg 9e851875…`. The last one equals the hash from the local T1 run, because the same template produces the same file.
  - All boot tests still pass: kernel config (49), kernel alone, kernel + initramfs, disk through GRUB
- Result: **PASS** (after 1 failed attempt, root-caused and fixed)

## T5: Two independent builds produce identical artifacts (CI)

- Workflow `reproducibility`: build A (ccache) and build B (`CCACHE_DISABLE=1`), on separate runners, then `compare-manifests.py`

### Attempt 1: run [36129247384](https://github.com/Mudher84/wana-os/actions/runs/36129247384), commit `466ca4c`: **FAIL**

```
identical  bzImage
DIFFERENT  disk.img  e2386fda996a447b vs efb3307ef0da4b51
identical  efi-part/EFI/BOOT/bootx64.efi
identical  efi-part/EFI/BOOT/grub.cfg
identical  efi-part/wana/bzImage
identical  esp.vfat
identical  genimage.cfg
identical  rootfs.cpio
identical  rootfs.cpio.zst
DIFFERENT  rootfs.ext2  7ef610a9a8e3d53c vs 9689a4aadf30f4ea
identical  rootfs.ext4          (symlink -> rootfs.ext2)
identical  rootfs.tar
[BUILD] reproducibility: FAIL (10/12 artifacts identical, 2 problem(s))
```

What this already proves: B was compiled without ccache on a different runner and still matched A for:
- the kernel;
- every target binary, including wana-init, since the cpio and tar hold the whole root filesystem;
- GRUB;
- the ESP.

Only one source of non-determinism remains. `disk.img` embeds `rootfs.ext2`, so it follows from it.

Root cause, isolated locally with Buildroot's exact tools (`mke2fs` 1.47.3 built from the v1.47.3 tag), same options as `fs/ext2/ext2.mk`:
- Default options, `SOURCE_DATE_EPOCH` set: 690 bytes differ. The timestamps already match (1.47.3 honors `SOURCE_DATE_EPOCH`: created/write time = epoch), but
  **Filesystem UUID** and **Directory Hash Seed** are random per run. The checksums seeded from the UUID account for the rest of the bytes.
- Also checked: `mke2fs -d` copies each file's atime, so a file read during the build could leak its atime into the image. This does not apply
  to Buildroot. `fs/common.mk` (`ROOTFS_REPRODUCIBLE`) runs `touch -hd @SOURCE_DATE_EPOCH` on every file right before `mkfs`.

Fix:
- `BR2_TARGET_ROOTFS_EXT2_MKFS_OPTIONS="-O ^64bit -U <uuid> -E hash_seed=<uuid>"`. `-O ^64bit` is Buildroot's default and stays.
- Both values are recorded in `disk.env`; `tools/check-repo.sh` fails if the defconfig and `disk.env` disagree (negative test: exit 1).
- A fixed directory hash seed is acceptable for a shipped, read-only root image. The installer will create each installed
  filesystem with a fresh UUID and seed (Phase 19).

Local end-to-end re-test, run twice: Buildroot's touch step, `mke2fs` 1.47.3 with the new options, then `post-image.sh` with genimage 19:
- `rootfs.ext2` 720359a0…, `esp.vfat` 322b5ef3… and `disk.img` 5c7bf551… are **identical** across both runs.
- The disk boots: `EXT4-fs (vda2): mounted filesystem 5d9f6a52-7e1b-4c3a-9b8e-0a1a57a0a0f5`, then `[INIT] info: ready`, then power off.

### Attempt 2: run [36135286543](https://github.com/Mudher84/wana-os/actions/runs/36135286543), commit `5b79d7a`: **PASS**

Build A (ccache, 44m33s) and build B (`CCACHE_DISABLE=1`, full recompilation, 52m22s), on two different runners:

```
identical  bzImage
identical  disk.img
identical  efi-part/EFI/BOOT/bootx64.efi
identical  efi-part/EFI/BOOT/grub.cfg
identical  efi-part/wana/bzImage
identical  esp.vfat
identical  genimage.cfg
identical  rootfs.cpio
identical  rootfs.cpio.zst
identical  rootfs.ext2
identical  rootfs.ext4
identical  rootfs.tar
[BUILD] reproducibility: PASS (12/12 artifacts identical)
```

The `buildroot` workflow on the same commit ([36135286549](https://github.com/Mudher84/wana-os/actions/runs/36135286549)) also
passed, including the disk boot with the new ext4 options.

- Result: **PASS**

## Phase 6 status

**PASS.** T1-T5 all pass. Every artifact of a Wana OS image is traceable to a
commit through its manifest, and two independent builds of that commit, one of them
with no compiler cache, produce bit-identical output: kernel, root filesystems
(cpio, tar, ext4), GRUB, ESP and the complete disk image.

Scope: reproducibility is verified on the same build path (`/home/runner/work/wana-os/wana-os`) and the
same host OS image (GitHub `ubuntu-24.04`). Building from a different directory or host
distribution has not been tested yet.
