# Phase 20 — Installer GUI

Status: **IN PROGRESS**

## Implemented

- Native Arabic installer GUI.
- Real disk discovery and explicit confirmation.
- The GUI reads the actual Live `Wana-OS.img` size and excludes candidate
  disks that are smaller than the installation image.
- GUI drives the production installer core rather than a mock installer; the
  core independently repeats its own target-size and safety checks.

## Exit gate

`make installer-gui-boot-test` must drive the GUI under QEMU, install to the
target disk and pass the production-core readback verification.

## Evidence

Implementation and the interactive QEMU gate exist. Final consolidated
Buildroot evidence is pending.
