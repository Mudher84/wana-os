# Wana OS Roadmap

A phase is marked **PASS** only when its exit test has been run and the
evidence is linked. Status values: `PASS`, `FAIL`, `IN PROGRESS`,
`NOT STARTED`.

## Milestone 1: Native graphical output (**complete**)

| # | Phase | Exit test | Status | Evidence |
|---|-------|-----------|--------|----------|
| 0 | Repository audit | Audit written from the actual repo state | PASS | [audit 0000](audit/0000-repository-audit.md) |
| 1 | Project architecture | Layout, docs, Rust workspace; `make check` green locally and in CI | PASS | [phase 01 report](test-reports/phase-01.md) |
| 2 | Build environment | Pinned Buildroot fetched and verified; defconfig round-trips; CI builds the cross toolchain and a C/C++ smoke test passes | PASS | [phase 02 report](test-reports/phase-02.md) |
| 3 | Linux kernel | CI builds `bzImage` (6.18.33 + fragment); all fragment options verified; boots under QEMU+OVMF to the root mount | PASS | [phase 03 report](test-reports/phase-03.md) |
| 4 | Minimal rootfs | Buildroot initramfs with `wana-init` (Rust) as PID 1; UEFI boot reaches `[INIT] info: ready` and powers off | PASS | [phase 04 report](test-reports/phase-04.md) |
| 5 | UEFI boot with bootloader | GRUB (x86_64-efi) on an EFI System Partition boots kernel + rootfs in QEMU+OVMF (no `-kernel` shortcut) | PASS | [phase 05 report](test-reports/phase-05.md) |
| 6 | Reproducible build | Build manifest (commit, configs, toolchain, versions, SHA-256) per image; two independent CI builds of one commit produce identical artifacts | PASS | [phase 06 report](test-reports/phase-06.md) |
| 7 | DRM/KMS | `[DRM]` discovers device and connector, sets mode in QEMU virtio-gpu; screenshot pixels verified | PASS | [phase 07 report](test-reports/phase-07.md) |
| 8 | GBM/EGL/GLES | Frame rendered via GLES (GBM + EGL on wana-drm), scanned out, QEMU `screendump` pixel check in CI | PASS | [phase 08 report](test-reports/phase-08.md) |

## Milestone 2: Interactive compositor

| # | Phase | Status |
|---|-------|--------|
| 9 | Input stack (udev + libinput -> wana-input): QEMU-injected keys, pointer and click decoded by `wana-input` in the Buildroot image, in CI ([phase 09 report](test-reports/phase-09.md)) | PASS |
| 10 | Compositor MVP (surfaces, focus, z-order, pointer, keyboard) on libwayland-server ([decision 0001](decisions/0001-wayland-protocol-layer.md), [phase 10 report](test-reports/phase-10.md)); steps 1 (protocol layer + socket) and 2 (globals) PASS; step 3 (client windows on screen) in CI; step 4 (input, focus) next | IN PROGRESS |
| 11 | Desktop shell MVP | NOT STARTED |
| 12 | Window management | NOT STARTED |

## Milestone 3: Usable desktop

| # | Phase | Status |
|---|-------|--------|
| 13 | Dock + launcher | NOT STARTED |
| 14 | Settings | NOT STARTED |
| 15 | Networking | NOT STARTED |
| 16 | Files | NOT STARTED |
| 17 | System services | NOT STARTED |

## Milestone 4: Installable system

| # | Phase | Status |
|---|-------|--------|
| 18 | Live ISO | NOT STARTED |
| 19 | Installer core | NOT STARTED |
| 20 | Installer GUI | NOT STARTED |
| 21 | Installed-disk boot | NOT STARTED |

## Milestone 5: Product quality

| # | Phase | Status |
|---|-------|--------|
| 22 | Permissions + audit center | NOT STARTED |
| 23 | Security hardening | NOT STARTED |
| 24 | Notifications + control center | NOT STARTED |
| 25 | Visual polish | NOT STARTED |
| 26 | Motion system | NOT STARTED |
| 27 | Performance | NOT STARTED |
| 28 | Hardware compatibility | NOT STARTED |
| 29 | Beta release | NOT STARTED |
| 30 | Stable release | NOT STARTED |

Audio (PipeWire), Bluetooth, Wine, and Android compatibility come after
Milestone 4 and are not scheduled yet.
