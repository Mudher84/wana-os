# Phase 21 — Installed-disk boot

Status: **IN PROGRESS**

## Implemented

- The disk produced by the installer is booted through OVMF and GRUB.
- Root filesystem selection uses the fixed PARTUUID rather than device names.

## Exit gate

`make installed-disk-boot-test` must boot the installer-produced disk through
firmware -> GRUB -> kernel -> ext4 root -> Wana PID1 and power down cleanly.

## Evidence

The installed-disk gate exists. Final consolidated Buildroot evidence is
pending.
