# Phase 36 — Atomic A/B updates and recovery

Status: **IN PROGRESS**

## Implemented

- The installed disk uses two fixed-size replaceable ext4 system slots plus a
  persistent Data partition.
- Stable update payloads contain a compressed root filesystem, kernel and
  initramfs bound to strict metadata and SHA-256 values.
- Online stable updates accept only strict `X.Y.Z` versions. A candidate older
  than the installed version is rejected, and reusing the same version with a
  different commit is rejected as version equivocation.
- `wana-updated` is a root broker; the desktop can only request status,
  fetch/stage and clear over a peer-credential-authenticated Unix socket.
- Staging uses a root-owned Data directory, create-new temporary state, fsync
  and atomic rename.
- Update metadata/checksum reads are bound to the same checked inode/device
  that was inspected before opening, so symlink/path-swap races are rejected.
- GRUB detects a fully staged update and boots an isolated update environment
  from the staged kernel/initramfs without mutating the active root.
- PID 1 verifies the staged payload, zstd stream, inactive block device and
  written filesystem before switching slots.
- The new kernel is installed through a temporary ESP file, verified by
  SHA-256 and atomically renamed.
- The old slot remains intact.
- Before committing the slot selector, the updater records a root-owned
  `trial-boot.cfg` containing previous and next slots.
- The service supervisor publishes `/run/wana/services.ready` only after its
  complete readiness-gated service graph has started.
- The first normal boot of the new slot is a trial. A healthy boot clears the
  trial state and confirms the slot. A degraded boot or missing service-ready
  marker restores the previous slot selector and reboots automatically.
- Failed update environments quarantine their metadata and return to the
  previously selected system.
- GRUB keeps explicit force-slot-A and force-slot-B entries for manual recovery
  if a failure occurs before PID 1 can perform automatic rollback.
- `tools/mk-update-test-disk.sh` creates a deterministic offline A/B fixture
  by injecting the exact current build into Data/pending.
- The final A/B gate uses a writable QEMU disk across two boots: apply to B,
  then boot B and confirm the trial.

## Exit gate

`make update-ab-boot-test` must:

1. construct an offline staged-update disk from the exact current build;
2. boot the isolated update environment from slot A;
3. verify and write the root/kernel to inactive slot B;
4. arm trial state and switch the selector to B;
5. boot the same persistent disk again from B;
6. reach the complete service-ready marker;
7. confirm and clear the trial state.

Manual force-slot recovery remains available for failures that happen before
PID 1 starts.

## Evidence

Implementation and the final gate exist. Evidence is intentionally deferred to
the consolidated final validation pass.
