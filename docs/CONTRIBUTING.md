# Contributing to Wana OS

## Branches

`main` is the release baseline. Do not push experimental work directly to it.

The project has historically used `work/phaseNN-...` branches for isolated
phase work. The current consolidated pre-stable implementation line is
`work/prestable-store-recovery` and is reviewed through PR #35.

For new work before the current release closes:

1. branch from the current pre-stable head;
2. keep one coherent change per branch/PR;
3. merge or supersede it into the consolidated pre-stable line;
4. do not mark a phase PASS merely because implementation merged.

After the Stable line is closed, branch naming may be simplified, but the same
rule remains: the last verified release baseline is never modified in place.

## Commits

- Each commit should do one coherent thing.
- Subject format: `area: summary`, for example
  `services: require fresh readiness endpoints`.
- Do not commit build output (`target/`, `out/`, images, ISOs or logs).
- Do not commit credentials, private release keys or Wi-Fi/authentication state.
- Keep generated release evidence separate from implementation changes.

## Before pushing implementation

Use the focused source/static gates that match the change:

```sh
make check
make msrv
make wayland-host-test
```

Buildroot/runtime gates are intentionally expensive and may be deferred while
implementation is still changing. The complete frozen-source pass is
`make final-validation-test`; see [FINAL-VALIDATION.md](FINAL-VALIDATION.md).

## Evidence policy

`PASS`, `VERIFIED`, `COMPLETE` and `DONE` refer to tested evidence, not
intent or code presence.

For phases 12–39, implementation may exist while the Roadmap status remains
`IN PROGRESS`. Promote a phase only after its documented exit gate actually
runs on the frozen source revision and the evidence is recorded in
`docs/test-reports/`.

Failures are recorded as failures and fixed from the first broken layer rather
than hidden by retries or by weakening expectations.

## Debugging order

Start at the lowest failing boundary and move upward:

```text
UEFI/GRUB
-> kernel
-> wana-init / mounts / udev
-> wana-services readiness
-> DRM/KMS
-> GBM/EGL/GLES
-> input
-> compositor
-> privileged shell
-> native application / compatibility layer
```

For A/B update failures, preserve the active slot and inspect staging,
update-environment verification, slot write, trial boot and confirmation in
that order.

Subsystem-tagged logs (`[INIT]`, `[DRM]`, `[COMPOSITOR]`, `[SECURITY]`,
etc.) are designed so CI and developers can identify the first broken layer.
