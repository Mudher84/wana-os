# Phase 29 — Beta release

Status: **IN PROGRESS**

Phase 29 introduces a release-candidate promotion path. A Beta is not produced
from a different build recipe: the same Buildroot image and runtime gates used
throughout development are rerun, then the already-generated manifest and
checksums are validated before packaging.

## Implemented

- `tools/prepare-release.py` validates a clean, traceable build manifest.
- Manifest project/schema and the exact Git commit are required.
- `SHA256SUMS` must contain exactly the same hashed artifact set as the
  manifest; missing or extra entries fail the release.
- Every artifact is re-hashed before packaging.
- Required payload: Live ISO, installed disk image, kernel and compressed
  initramfs.
- A closed release directory contains the payload, build manifest,
  `release.json`, and `RELEASE-SHA256SUMS`; the checksum file covers every
  shipped file except itself, including both metadata files.
- Beta versions must use `X.Y.Z-beta.N`.
- `make beta-release-test` reruns critical kernel, boot, graphics, input,
  compositor, application, installer, permissions, security and Phase 28
  hardware-compatibility gates.
- `make beta-bundle VERSION=X.Y.Z-beta.N` packages only after the image and
  manifest pass the release verifier.
- `.github/workflows/beta.yml` supports manual candidates and beta tags.
  Manual runs upload a 30-day workflow artifact; a matching Git tag publishes
  a GitHub prerelease using the exact verified bundle.

## Exit gate

Phase 29 is PASS only when one Beta candidate commit has all of the following:

1. normal CI is green;
2. Buildroot image and runtime gates are green;
3. Phase 28 hardware matrix is green;
4. the Beta workflow completes `beta-release-test`;
5. the release verifier reports PASS for the candidate commit;
6. the resulting release bundle is uploaded without missing payload files.

A GitHub prerelease is published only from an explicit `vX.Y.Z-beta.N` tag.

## Evidence

Pending. No Beta release is claimed until an actual candidate workflow run
passes all gates.
