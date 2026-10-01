# Phase 13 — Dock + launcher

Status: **IN PROGRESS**

## Implemented

- Arabic launcher on the privileged shell connection.
- Keyboard navigation and application launch through the public Wayland socket.
- Live Dock backed by the private foreign-toplevel list.
- Descriptor isolation for launched applications.

## Exit gate

`make launcher-boot-test` and `make dock-boot-test` must pass their
runtime markers, hashes and screenshot pixel assertions.

## Evidence

Implementation and both QEMU gates exist. Final consolidated Buildroot evidence
is pending.
