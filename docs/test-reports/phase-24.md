# Phase 24 — Notifications + control center

Status: **IN PROGRESS**

Phase 24 adds a native Arabic control center backed by a bounded persistent
notification history. The phase remains open until its real Buildroot/QEMU
boot gate passes on top of the Phase 23 hardening line.

## Implemented

- Native `wana-control-center` xdg application, rendered with the Wana text
  stack and exposed in the launcher.
- Persistent notification history at `/var/lib/wana/notifications/history.tsv`.
- History is bounded to 64 entries.
- Notification app IDs and text fields are validated before persistence.
- Store directory is mode 0700; history and atomic replacement files are mode
  0600.
- Symlink, ownership, insecure-mode, and open-time inode/device checks protect
  the history file.
- Mutating commands (`notify`, `clear`, deterministic test seed) require
  effective uid 0.
- Writes use create-new temporary files, fsync, atomic rename, and directory
  fsync.
- Native UI shows notification count and the newest four notifications.
- CLI supports `notify`, `list`, and `clear`.

## Exit test

`make control-center-boot-test` boots the real installed disk image under
QEMU + OVMF, seeds three notifications through the production store path,
launches the native control-center client through the compositor, and requires:

1. the 820x600 Arabic control-center window maps;
2. three notifications are loaded and rendered;
3. the rendered frame hash is reported;
4. screenshot pixel assertions pass after the QEMU scanout settles;
5. the client and compositor exit successfully with no warn/error subsystem logs.

The Buildroot workflow runs this gate after Phase 23's security-hardening gate.

## Evidence

- Host CI for the original Phase 24 implementation passed in workflow run
  `36891943493`.
- Phase 23 is now an ancestor of the Phase 24 branch (0 commits behind).
- Final Buildroot/QEMU evidence is pending; do not mark this phase PASS until
  the merged Phase 24 branch completes the full gate successfully.
