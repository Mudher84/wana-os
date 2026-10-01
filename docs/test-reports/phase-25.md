# Phase 25 — Visual polish

Status: **IN PROGRESS**

Phase 25 consolidates the native Wana OS visual language into shared,
deterministic design tokens and applies those tokens across the shell and
native applications without weakening existing rendering regression checks.

## Implemented

- New `wana-theme` workspace crate with shared dark/light palette tokens,
  accent colors, shell colors, and spacing constants.
- The shell desktop, top bar, launcher, Dock, and panel colors now read from
  the shared theme instead of maintaining private copies.
- Native Settings, Files, Permissions, Installer, and Control Center code is
  wired to the same visual token source.
- Existing deterministic drawing paths and pixel/hash assertions are retained;
  Phase 25 does not replace regression checks with subjective screenshots.
- Arabic-first shell layout and the existing RTL text pipeline remain intact.

## Exit gate

Phase 25 is not complete merely because the shared theme crate builds.
Completion requires:

1. normal CI green on the Phase 25 branch;
2. Buildroot image build green;
3. existing shell/layer/launcher/Dock/Settings/Files/Installer/Permissions and
   Control Center rendering gates remain green with the centralized palette;
4. no unexpected rendered hash or pixel change outside the intentional visual
   token migration;
5. no warnings/errors from the native graphical boot gates.

## Evidence

- Normal CI is green for commit
  `0f521f9111442b93074d1477491a71632c2b123e` (workflow run
  `36909281404`).
- Full Buildroot/QEMU evidence is still running. Do not mark this phase PASS
  until that run completes successfully.
