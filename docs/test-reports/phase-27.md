# Phase 27 — Performance

Status: **IN PROGRESS**

Phase 27 starts with the native shell frame hot path introduced by the motion
system. The goal is to remove avoidable allocation and hashing work without
changing final rendered pixels or protocol behavior.

## Implemented

- Shell frame presentation serializes each Canvas to XRGB8888 bytes once
  instead of twice.
- The same serialized byte buffer is reused for wl_shm upload and deterministic
  SHA-256 evidence.
- Transient motion frames use an unhashed presentation path; intermediate
  launcher animation frames no longer pay for SHA-256 values that are never
  consumed.
- Canvas byte serialization reserves the exact final buffer size before
  writing pixels, avoiding Vec growth/reallocation on the hot path.
- Packed XRGB8888 byte order and exact output length are covered by unit tests.

## Exit gate

Phase 27 must pass normal CI and the existing Buildroot graphical gates.
In particular:

1. all rendering/hash tests must keep the same final hashes;
2. the Phase 26 launcher motion runtime marker must still pass;
3. final launcher pixel assertions must remain unchanged;
4. the Canvas XRGB8888 packing unit test must pass on the normal toolchain and
   Rust 1.88 MSRV.

Further performance work can extend this phase only when there is concrete
allocation, CPU, startup, or frame-time evidence; visual behavior must not be
weakened to make a benchmark pass.

## Evidence

Pending CI and Buildroot/QEMU evidence for the Phase 27 branch.
