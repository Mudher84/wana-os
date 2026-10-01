# Phase 31 — Audio

Status: **IN PROGRESS**

Phase 31 makes audio a normal Wana desktop service rather than a future
integration placeholder.

## Implemented

- ALSA kernel coverage for HDA/HDMI/Realtek and USB audio.
- PipeWire runs as the unprivileged `wana` desktop user.
- `pipewire-pulse` provides PulseAudio-protocol compatibility on the same graph.
- WirePlumber is the policy/session manager and starts after PipeWire, Pulse
  compatibility and BlueZ.
- The desktop user receives access to `/dev/snd` through the Wana udev policy.
- `wana-audio` exposes status, node listing, playback and recording helpers.
- `make audio-compatibility-boot-test` is staged for the final validation run.

## Exit gate

The final validation must build the image, boot the normal service graph and
prove that the uid/gid 1000 PipeWire graph is reachable at
`/run/user/1000/pipewire-0` without INIT/service errors.

## Evidence

Pending final consolidated validation. No PASS is claimed yet.
