# Phase 18 — Live ISO

Status: **IN PROGRESS**

## Implemented

- UEFI Live ISO containing the Wana kernel and initramfs.
- GRUB Live boot path and explicit `wana.live=1` mode.
- Live userspace runs without an installed root partition.

## Exit gate

`make live-iso-boot-test` must boot OVMF -> GRUB -> kernel/initramfs, enter
Live mode and reach PID1 ready without INIT errors.

## Evidence

Live media generation and its QEMU gate exist. Final consolidated Buildroot
evidence is pending.
