# Phase 33 — Windows compatibility

Status: **IN PROGRESS**

Buildroot's stock Wine package is i386-only, while Wana OS is an x86_64 system.
Phase 33 therefore carries a Wana-specific 64-bit Wine package instead of
changing the distribution architecture.

## Implemented

- Wine 11.0 is pinned and built as Win64 for the x86_64 target.
- The upstream Wine 11.0 source tarball and license files are covered by
  fixed SHA-256/SHA-512 hashes in the Buildroot package.
- Wine 11 uses the unified `wine` loader; `wana-winrun` also accepts a
  `wine64` binary if a future/package variant provides one.
- The runtime uses Wana's native Wayland path (`--with-wayland --without-x`).
- ALSA, D-Bus, fontconfig, FreeType and udev integration are enabled.
- PulseAudio client support is enabled; the server side is Wana's
  `pipewire-pulse`, so Windows applications share the normal desktop audio
  graph rather than requiring a separate PulseAudio daemon.
- `wana-winrun` keeps the Wine prefix under the user's Wana data directory and
  launches `.exe` programs without requiring a separate X11 desktop.
- `make windows-compatibility-boot-test` is staged in the final compatibility
  gate.

## Exit gate

The final image must build the 64-bit Wine runtime successfully and boot a guest
in which `wana-winrun --version` executes Wine 11.0. Application-specific
compatibility is not implied by that runtime gate and is tested separately per
application.

## Evidence

Pending final consolidated validation. No PASS is claimed yet.
