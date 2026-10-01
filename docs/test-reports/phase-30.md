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
- Normal installed boot has a dedicated production-session gate: without
  `wana.run`, PID 1 prepares `/run/user/1000`, starts `wana-services`, and
  launches the persistent compositor + shell session as uid/gid 1000.
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
- A GitHub stable release is created only after the independent reproducibility
  comparison passes and only when the operator explicitly requested
  publication.

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
   version.
9. the production desktop gate proves the compositor/shell session runs as
   uid/gid 1000 with its own XDG runtime directory and no inherited root
   supplementary groups.

Until that evidence exists, Wana OS is not labeled Stable. The implementation
now contains the uid/gid 1000 production-session path, but Stable publication
remains blocked until CI/Buildroot prove the privilege drop and normal desktop
boot on the built image.

## Evidence

Pending. The pipeline is implemented, but no Stable release is claimed merely
because the workflow exists.
