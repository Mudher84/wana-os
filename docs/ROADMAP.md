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
| 10 | Compositor MVP (surfaces, focus, z-order, pointer, keyboard) on libwayland-server ([decision 0001](decisions/0001-wayland-protocol-layer.md), [phase 10 report](test-reports/phase-10.md)): client windows composited with GLES, input routed to the focused client, click-to-raise, client cursors; CI boot tests with screenshot pixels; reproducible 12/12 | PASS |
| 11 | Desktop shell MVP; Arabic text stack, privileged layer-shell, desktop/top bar, launcher, live Dock via ext-foreign-toplevel-list-v1, bounded shell restart, descriptor isolation; CI + Buildroot/QEMU + reproducibility PASS ([decision 0002](decisions/0002-text-stack.md), [decision 0003](decisions/0003-shell-surfaces.md), [phase 11 report](test-reports/phase-11.md)) | PASS |
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
| 22 | Permissions + audit center | IN PROGRESS |
| 23 | Security hardening ([phase 23 report](test-reports/phase-23.md)) | IN PROGRESS |
| 24 | Notifications + control center ([phase 24 report](test-reports/phase-24.md)) | IN PROGRESS |
| 25 | Visual polish ([phase 25 report](test-reports/phase-25.md)) | IN PROGRESS |
| 26 | Motion system ([phase 26 report](test-reports/phase-26.md)) | IN PROGRESS |
| 27 | Performance ([phase 27 report](test-reports/phase-27.md)) | IN PROGRESS |
| 28 | Hardware compatibility ([phase 28 report](test-reports/phase-28.md)) | IN PROGRESS |
| 29 | Beta release ([phase 29 report](test-reports/phase-29.md)) | IN PROGRESS |
| 30 | Stable release ([phase 30 report](test-reports/phase-30.md)) | IN PROGRESS |

Audio (PipeWire), Bluetooth, Wine, and Android compatibility come after
Milestone 4 and are not scheduled yet.
