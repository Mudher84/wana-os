# Phase 26 — Motion system

Status: **IN PROGRESS**

Phase 26 introduces one deterministic motion vocabulary for the native Wana OS
desktop instead of application-specific timing and interpolation.

## Implemented

- New `wana-motion` workspace crate with fixed-point progress (0..=1024).
- Shared durations: quick 120 ms, standard 180 ms, emphasized 240 ms.
- Shared curves: linear, ease-out cubic, and ease-in-out cubic.
- Deterministic integer interpolation and RGB mixing.
- Unit tests require bounded, monotonic curves with exact endpoints.
- The Wana launcher uses the shared quick/ease-out motion for keyboard
  selection changes.
- Launcher selection transitions render six committed frames over 120 ms.
- The old and new selection rows cross-fade between panel and accent colors.
- The final animation frame is pixel-identical to the pre-motion static
  launcher frame, preserving visual regression hashes.
- Wayland events received while the short motion is running stay queued for
  the normal shell event loop.

## Exit gate

The existing `make launcher-boot-test` is extended to require the runtime
marker:

`motion launcher-selection: 1 -> 2, 6 frames, 120ms, ease-out-cubic`

The same QEMU test still verifies the final launcher pixels, application
launch, descriptor isolation, Arabic text rendering, and clean compositor
shutdown.

The phase must also pass normal CI, including Rust 1.88 MSRV tests, Clippy,
formatting, repository checks, and all `wana-motion` unit tests.

## Evidence

Pending CI and Buildroot/QEMU evidence for the Phase 26 branch. Do not mark
this phase PASS until those gates complete successfully.
