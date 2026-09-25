# Contributing to Wana OS

## Branches

| Branch | Purpose | Rule |
|--------|---------|------|
| `main` | Stable. Every commit on it passed CI. | No direct pushes. It changes only by merging a reviewed PR from `develop` (or a hotfix). Stable points are tagged `vX.Y.Z` or `milestone-N`. |
| `develop` | Integration | Feature branches merge here after CI passes. |
| `feature/<name>` | One feature or phase, e.g. `feature/native-graphics` | Short-lived. Branch from `develop`. |

A working baseline is never modified in place. Experiments go on a new branch,
so the last stable tag is always a clean rollback point.

## Commits

- Each commit does one thing and leaves the tree building.
- Subject line: `area: summary`, e.g. `drm: enumerate connectors via udev`.
- Never commit build output (`target/`, `out/`, images, ISOs). `.gitignore`
  enforces this and `tools/check-repo.sh` checks it.

## Before pushing

```sh
make check
```

## Reporting results

`PASS`, `VERIFIED`, `COMPLETE` and `DONE` are used only with evidence. Every
significant test is recorded in `docs/test-reports/` with: command, expected
result, actual result, logs, artifact. A failure is written down as `FAIL`,
never hidden.

## Debugging order

Find the first broken layer and fix only that: kernel boot -> rootfs ->
`/dev/dri` present -> DRM open -> GBM -> EGL context -> frame rendered ->
input -> compositor -> shell. The `[TAG]` in each log line tells you the layer.
