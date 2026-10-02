# Building Wana OS

Everything goes through the top-level `Makefile`. Run `make help` for the
authoritative target list.

## Host

Primary build host: Linux x86_64. Windows development uses WSL2 with a Linux
distribution; the actual Buildroot build still runs in Linux.

Install the Ubuntu host package set with:

```sh
grep -v '^#' tools/host-packages-ubuntu.txt | xargs sudo apt-get install -y
```

Rust is pinned by `rust-toolchain.toml`. The Buildroot/MSRV gate uses the Rust
version declared by the workspace (`1.88` for the current Buildroot line).

## Source and static gates

```sh
make check
make msrv
make wayland-host-test
make config-check
```

`make check` runs formatting/lint/unit/repository checks. `make msrv` proves
the Rust code on the Buildroot-compatible toolchain. `make wayland-host-test`
runs protocol/compositor scenarios headlessly.

## Buildroot and images

```sh
make buildroot-src
make config
make image
```

Important outputs are under `out/build/wana_x86_64/images/`:

- `bzImage`;
- `rootfs.cpio.zst`;
- `rootfs.ext4` and `rootfs.ext4.zst`;
- `disk.img`: UEFI/GPT installed image with ESP, system slots A/B and Data;
- the Live ISO;
- `build-manifest.json`;
- `SHA256SUMS`.

The disk layout is generated from `platform/board/x86_64/`. Slot A contains
the factory root; slot B is reserved for a verified update. Data is persistent
and last on disk so the installed system can expand it to the target device.

## Boot/runtime gates

The Makefile exposes focused QEMU gates for the kernel, disk, graphics, input,
compositor, shell, applications, installer, permissions/security and hardware
matrix. Examples:

```sh
make kernel-boot-test
make disk-boot-test
make compositor-boot-test
make installed-disk-boot-test
make security-hardening-boot-test
make hardware-compatibility-test
```

Compatibility/lifecycle gates include:

```sh
make audio-compatibility-boot-test
make bluetooth-compatibility-boot-test
make windows-compatibility-boot-test
make android-compatibility-boot-test
make auth-login-boot-test
make power-ui-boot-test
make update-ab-boot-test
make time-sync-boot-test
make diagnostics-boot-test
```

## Final frozen-source validation

Implementation is allowed to land before consolidated evidence. When the source
line is frozen, run:

```sh
make final-validation-test
```

That target executes source/MSRV/protocol/config/image gates and the complete
Stable runtime gate, including phases 35–39.

Full Android UI evidence is intentionally separate because the reproducible
release image does not embed multi-gigabyte Android OTA images. Use a
pre-provisioned validation disk and deterministic installed test APK as
described in [FINAL-VALIDATION.md](FINAL-VALIDATION.md).

## Beta and Stable bundles

After an image has passed its required gates:

```sh
make beta-bundle VERSION=0.1.0-beta.1
make stable-bundle VERSION=0.1.0
```

`tools/prepare-release.py` binds the release to the exact Git commit and
closed SHA-256 artifact set. Stable update assets include strict `update.txt`,
compressed rootfs, kernel and initramfs metadata.

The Stable GitHub workflow performs an independent second no-ccache build and
compares manifests/artifact hashes before publication. Publication permission
is isolated to the final publish job.

## A/B update behavior

The online updater discovers the latest stable version, then pins subsequent
metadata/checksum/payload downloads to that exact `vX.Y.Z` release tag.
Downloads have size ceilings, payloads are verified by SHA-256, downgrades are
rejected, and reusing one stable version for a different commit is rejected.

The current trust boundary for online release origin is GitHub HTTPS plus
repository/release control. The release payload itself is closed and
hash-verified; a separate detached offline signing key is not part of the
current Phase 36 exit gate.

## Reproducibility

`make image` writes `build-manifest.json` and `SHA256SUMS`.
`make repro-compare A=... B=...` compares two manifests artifact-by-artifact.

The reproducibility workflow performs independent builds of the same commit.
Fixed disk identifiers and `SOURCE_DATE_EPOCH` remove expected image
nondeterminism; the installer generates fresh identifiers for real installed
media.
