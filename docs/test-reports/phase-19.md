# Phase 19 — Installer core

Status: **IN PROGRESS**

## Implemented

- Native destructive installer core.
- Explicit erase confirmation.
- Copy of the canonical Wana disk image from read-only Live media.
- Source/target validation is bound to the exact opened inode/device and final
  symlink swaps are rejected with `O_NOFOLLOW`.
- Block-device targets are sized before the first write; a target smaller than
  the source disk image is rejected before destructive I/O begins.
- Full byte-for-byte readback verification.

## Exit gate

`make installer-core-boot-test` must boot the Live ISO, install to a writable
target disk and report successful full readback.

## Evidence

Implementation and the QEMU installer-core gate exist. Final consolidated
Buildroot evidence is pending.
