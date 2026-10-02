# Phase 39 — Diagnostics and support bundle

Status: **IN PROGRESS**

## Implemented

- `wana-diagnostics` runs as the unprivileged desktop user.
- The launcher exposes “تشخيص النظام”.
- Bundles are written under `$HOME/Diagnostics` with directory mode 0700 and
  archive mode 0600.
- Collection is deliberately bounded to support-safe state:
  release identity, kernel, uptime, selected memory fields, root/Data usage,
  selected mounts, runtime endpoint presence, and status output from the auth,
  power, update, time, Waydroid and Wine helpers.
- The collector never reads the Wi-Fi credential store, authentication verifier
  or notification history.
- A self-check rejects the bundle if sensitive path/credential markers appear
  in the summary.
- The bundle contains only a generated summary, not arbitrary `/var`,
  `/home`, kernel logs or application data.
- After creation, the helper stores a desktop notification containing the
  bundle path.
- The final launcher-driven gate requires both the privacy marker and a 0600
  bundle marker.

## Exit gate

`make diagnostics-boot-test` must launch the helper from the uid-1000 desktop,
produce `WANA_DIAGNOSTICS_PRIVACY_PASS`, create a bundle under
`/home/wana/Diagnostics` with mode 0600, and return without shell/init errors.

## Evidence

Implementation and the final gate exist. Evidence is deferred to the
consolidated final validation pass.
