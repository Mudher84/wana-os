# Phase 12 — Window management

Status: **IN PROGRESS**

## Implemented

- Native xdg_toplevel maximize, fullscreen, restore and minimize policy.
- Size constraints are preserved when changing window state.
- Focus and z-order remain compositor-owned.

## Exit gate

`make window-management-boot-test` must boot the real image and verify
maximize, fullscreen, restore, constrained maximize and minimize through the
Wayland protocol with clean compositor shutdown.

## Evidence

Implementation and the runtime gate exist. Final Buildroot/QEMU evidence on
the consolidated release lineage is pending.
