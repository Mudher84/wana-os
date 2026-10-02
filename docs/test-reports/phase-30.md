# Phase 30 — Stable release

Status: **IN PROGRESS**

Stable promotion is deliberately stricter than Beta. One candidate build must
pass the complete native runtime/install/security/hardware gate, and a second
independent build of the exact same commit must reproduce the artifact hashes
without using ccache.

## Implemented

- `make stable-release-test` inherits the Beta runtime gate and then adds the
  graphical gates Beta intentionally omits: GL rendering, seat/focus, text,
  text-on-screen, layer-shell, shell, Dock, and window-management.
- Normal installed boot has a dedicated production-session gate: PID 1 prepares
  `/run/user/1000`, starts `wana-services`, and launches the compositor +
  shell as uid/gid 1000 without `wana.run`.
- The normal service graph now also includes safe root-volume expansion,
  system/user D-Bus, WPA/DHCP, BlueZ, PipeWire, Pulse compatibility,
  WirePlumber and the privileged Waydroid container manager.
- Stable inherits the reproducible extended compatibility gates for audio,
  Bluetooth, Wine64 and the pinned Waydroid runtime. The Android OTA/session
  gate remains separate because it intentionally consumes provisioned external
  Android images rather than changing release reproducibility.
- `make stable-bundle VERSION=X.Y.Z` accepts only a stable version without a
  prerelease suffix and reuses the already-tested image.
- Release versions are bound to `[workspace.package].version`.
- `.github/workflows/stable.yml` is manual and explicit; publication is a
  separate boolean input.
- Candidate A builds normally, executes all release/runtime gates, validates
  the manifest/checksums, and uploads the closed stable bundle.
- Candidate B checks out the same commit and rebuilds with
  `CCACHE_DISABLE=1`.
- The compare job rejects different commits, Buildroot/kernel/toolchain/config
  inputs, package versions, source-date epoch, artifact sets, symlink targets,
  or SHA-256 values.
- The reproducibility comparison remains read-only. A separate publish job is
  created only when `publish=true`; only that job receives `contents: write`.
- A GitHub stable release is created only after the independent reproducibility
  comparison passes and only when the operator explicitly requested
  publication.

## Pre-stable hardening

Before the Stable candidate is promoted, the current pre-stable line also
hardens the destructive and persistent-state paths that the release depends on:

- Settings, Permissions and Notifications atomic writes recover from stale
  temporary files left by an interrupted process.
- Notification and permission-audit sequence exhaustion is rejected instead of
  saturating into duplicate sequence numbers.
- Settings validates the persistent parent directory ownership and mode.
- The installer binds validation to the opened source/target inode/device,
  rejects hard-link and block-device aliases, and refuses final-component
  symlink swaps with `O_NOFOLLOW` before mutating the target.
- Release cleanliness includes untracked non-ignored files; only known generated
  CI logs are ignored.
- Stable source/build/compare jobs are read-only; `contents: write` is granted
  only to the explicit final publish job.
- The production desktop runs as the dedicated `wana` user (uid/gid 1000),
  with a private `/run/user/1000`, user-owned Settings/Permissions/Notifications
  state, and only DRM/input device access granted through udev.

These changes remain pre-stable until the source CI and the complete
Buildroot/runtime suite pass on the same final commit.

## Exit gate

Phase 30 is PASS only when an actual stable candidate has:

1. green normal CI;
2. green full Buildroot/runtime test suite;
3. green Phase 28 hardware matrix;
4. green stable candidate A runtime gates;
5. green independent rebuild B;
6. reproducibility PASS for every manifest artifact;
7. a release verifier PASS for the clean candidate commit;
8. a published GitHub release bundle whose version equals the workspace
   version;
9. the production desktop gate proves the compositor/shell session runs as
   uid/gid 1000 with `/run/user/1000` and no inherited root supplementary
   groups;
10. the reproducible Phase 31–34 runtime payload gates pass.

Until that evidence exists, Wana OS is not labeled Stable.

## Evidence

Pending. The pipeline is implemented, but no Stable release is claimed merely
because the workflow exists.
