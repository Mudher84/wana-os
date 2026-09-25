# Building Wana OS

Everything goes through the top-level `Makefile`. Run `make help` for the
current list of targets.

## What builds today

| Target | What it does | Needs |
|--------|--------------|-------|
| `make check` | rustfmt check, clippy (warnings are errors), unit tests, repository checks | Rust (rustup installs the version pinned in `rust-toolchain.toml`) |
| `make buildroot-src` | Clones the pinned Buildroot (`platform/buildroot.env`) into `out/` and verifies the commit hash | git, github.com |
| `make config` | Loads `wana_x86_64_defconfig` into `out/build/wana_x86_64` | host gcc, make |
| `make config-check` | Checks that the defconfig loads and round-trips through `savedefconfig` unchanged | host gcc, make |
| `make toolchain` | Builds the cross toolchain: gcc 14.3, glibc, Linux 6.18 headers, C++ | Host packages in `tools/host-packages-ubuntu.txt`, unrestricted network, about 30 min |
| `make kernel` | Builds Linux 6.18.33 (`x86_64_defconfig` + `platform/board/x86_64/linux.fragment`) into `out/build/wana_x86_64/images/bzImage` | as `make toolchain` |
| `make kernel-config-check` | Checks every fragment option reached the kernel `.config` | a built kernel |
| `make kernel-boot-test` | Boots `bzImage` under QEMU + OVMF (UEFI) and checks the serial log | `qemu-system-x86`, `ovmf` |
| `make msrv` | Unit tests with Buildroot's Rust version (`rust-version` in `Cargo.toml`) | `rustup toolchain install 1.88` |
| `make image` | Full Buildroot build (toolchain, kernel, `wana-init`, rootfs, `disk.img`), then `make manifest` | as `make toolchain` |
| `make manifest` | Writes `images/build-manifest.json` (commit, versions, config and artifact hashes) and `images/SHA256SUMS` | python3 |
| `make repro-compare A=… B=…` | Compares two manifests artifact by artifact; exit 1 on any difference | python3 |
| `make system-boot-test` | Boots `bzImage` + `rootfs.cpio.zst` under QEMU+OVMF with `wana.test=poweroff`; `wana-init` must print `ready` | `qemu-system-x86`, `ovmf` |
| `make disk-boot-test` | Boots `images/disk.img` (GPT: ESP with GRUB + kernel, ext4 root) through OVMF with no `-kernel`; `wana-init` must reach `ready` | `qemu-system-x86`, `ovmf`, `mtools` |
| `make br-<target>` | Runs any Buildroot target, e.g. `make br-menuconfig` | |

The ISO target does not exist yet (Phase 18).

Build layout:

```
out/buildroot-<version>/   pinned Buildroot source (fetched)
out/build/wana_x86_64/     Buildroot output (O=)
dl/                        source tarball cache (BR2_DL_DIR)
~/.buildroot-ccache        compiler cache (BR2_CCACHE)
```

## Host packages (Ubuntu 24.04)

```sh
grep -v '^#' tools/host-packages-ubuntu.txt | xargs sudo apt-get install -y
```

## Host requirements

- Linux x86_64 host
- `rustup` (it installs the toolchain pinned in `rust-toolchain.toml`)
- GNU make, git

## Network note

A Buildroot build downloads upstream source tarballs from kernel.org, gnu.org
and other hosts. It needs unrestricted outbound HTTPS. GitHub Actions runners
have it. Restricted sandboxes may not (see
[audit 0000](audit/0000-repository-audit.md)).

## Reproducibility

Every `make image` writes `images/build-manifest.json` and `images/SHA256SUMS`.
To check an image, compare its hashes with the manifest of the CI build of the
same commit. The `reproducibility` workflow builds each `develop`/`main` commit
twice on separate runners and requires identical artifacts. `SOURCE_DATE_EPOCH`
is the pinned Buildroot commit time. Disk identifiers are fixed in
`platform/board/x86_64/disk.env`.
