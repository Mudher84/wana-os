# Phase 19 — Installer core

Status: **IN PROGRESS**

## Implemented

- Native destructive installer core.
- Explicit erase confirmation.
- Copy of the canonical Wana disk image from read-only Live media.
- Full byte-for-byte readback verification.

## Exit gate

`make installer-core-boot-test` must boot the Live ISO, install to a writable
target disk and report successful full readback.

## Evidence

Implementation and the QEMU installer-core gate exist. Final consolidated
Buildroot evidence is pending.
