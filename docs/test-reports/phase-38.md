# Phase 38 — Network time synchronization

Status: **IN PROGRESS**

## Implemented

- Wana OS includes Buildroot's pinned Chrony 4.8 client.
- Chronyd is supervised by `wana-services` after DHCP is started.
- Chronyd starts with the privileges required to adjust the system clock and
  uses its Buildroot-created unprivileged `chrony` account for normal work.
- `/etc/wana/chrony.conf` is client-only:
  - public pool sources with `iburst`;
  - drift tracking;
  - early-boot `makestep`;
  - RTC synchronization;
  - remote chronyc UDP command port disabled;
  - client logging disabled.
- `wana-time status` waits for the local Chrony control interface and exposes
  a deterministic `WANA_TIME_READY` marker together with tracking state.
- `wana-time sources` and `wana-time wait` provide local diagnostic and
  synchronization helpers.
- The normal desktop starts after the time-sync service process is established,
  so TLS/update and user applications are not racing service creation.

## Exit gate

`make time-sync-boot-test` must boot the image with the normal service graph,
reach the supervised Chrony daemon, obtain local tracking state through
`chronyc`, and power off without init errors.

The CI gate deliberately does not require a public NTP response. Actual
synchronization against representative networks is recorded separately during
final hardware/network validation.

## Evidence

Implementation and the deterministic local gate exist. Evidence is deferred to
the consolidated final validation pass.
