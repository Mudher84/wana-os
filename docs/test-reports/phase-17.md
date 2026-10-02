# Phase 17 — System services

Status: **IN PROGRESS**

## Implemented

- PID1-started native service supervisor.
- Dependency graph validation and restart policies.
- Secure service configuration ownership/mode/symlink checks.
- Clean service execution environment with explicit UID/GID and cleared
  supplementary groups.
- Services may declare `ready_path=` only under transient `/run`; startup waits
  for that runtime endpoint before launching dependent services.
- Before every initial start or restart, the supervisor removes a stale
  non-directory readiness endpoint and refuses symlink/directory replacements,
  so a socket left behind by a crashed process cannot create a false-ready.
- System D-Bus, user D-Bus, PipeWire and PipeWire Pulse compatibility use
  readiness paths, so dependent services no longer rely on process-start
  ordering alone.
- PID 1 prepares both `/run/dbus` and the private `/run/user/1000` runtime
  directory before starting the service graph.

## Exit gate

`make services-boot-test` must start the supervisor, validate the canonical
service directory, observe the declared readiness paths and reach a clean
ready/poweroff cycle.

## Evidence

Implementation and the boot gate exist. Final consolidated Buildroot evidence
is pending.
