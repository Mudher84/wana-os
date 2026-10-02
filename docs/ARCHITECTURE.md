# Wana OS Architecture

This document describes the implementation currently present on the pre-stable
line. Phase status and evidence remain authoritative in
[ROADMAP.md](ROADMAP.md).

## 1. Boot and storage

```text
UEFI
  -> GRUB on the EFI System Partition
     -> normal slot A or B
     -> staged update environment when a complete update is pending
     -> manual force-slot A/B recovery entries
  -> Linux 6.18.33
  -> wana-init (PID 1)
  -> wana-services
  -> unprivileged desktop session
```

The installed GPT layout is:

1. EFI System Partition;
2. fixed-size ext4 system slot A;
3. fixed-size ext4 system slot B;
4. persistent ext4 Data partition.

Data is last so `embiggen-disk` can grow the installed system to the target
device without moving either replaceable system slot.

GRUB reads the durable slot selector from Data. A staged update boots an
isolated update initramfs, writes only the inactive slot, installs the new
kernel through a verified temporary ESP file, arms a trial boot, then switches
the slot selector. A healthy service-ready boot confirms the new slot; a
degraded trial restores the previous selector and reboots.

## 2. Build system and reproducibility

Wana is a Buildroot distribution using `platform/` as a BR2_EXTERNAL tree.
Buildroot, Linux, Rust/MSRV and all Wana source are pinned by repository
configuration.

`make image` produces the kernel, initramfs, ext4 root, installed disk, Live
ISO, build manifest and checksums. The Stable workflow performs an independent
second build with ccache disabled and rejects differences in source/build
inputs or artifact SHA-256 values.

## 3. PID 1 and service model

`wana-init` is Wana's Rust PID 1. It owns early mounts, device-manager bringup,
persistent Data setup, update/trial-boot handling, process reaping and start of
`wana-services`.

`wana-services` loads root-owned service definitions from
`/etc/wana/services.d`. Each service declares:

- executable/arguments;
- uid/gid and optional HOME/environment;
- dependency ordering;
- restart policy;
- optional `ready_path` under transient `/run`.

Readiness paths are cleared before each start/restart and must be recreated by
the new service instance before dependents continue. A stale socket therefore
cannot satisfy readiness after a crash.

The production graph includes system/user D-Bus, disk growth, WPA supplicant,
DHCP, Bluetooth, Chrony, PipeWire, PipeWire Pulse compatibility, WirePlumber,
authentication, power, update, Waydroid container management and the desktop.

## 4. Desktop privilege boundary

The normal graphical session runs as `wana`, uid/gid 1000, with a private
`/run/user/1000`. PID 1/service policy grants only the device/runtime access
required by the desktop.

`wana-session` sets locale/timezone/theme environment and launches the
compositor. The compositor starts `wana-shell` over a private Wayland
connection. Privileged shell globals, including layer-shell, foreign-toplevel
and `wana_shell_control_v1`, are hidden from ordinary clients.

The global launcher key is Super. The compositor consumes it and sends
`toggle_launcher` only over the private shell-control protocol; see
[Decision 0004](decisions/0004-shell-control.md).

## 5. Graphics and input

```text
DRM/KMS -> GBM -> EGL -> GLES -> wana-compositor -> scanout
kernel evdev -> eudev -> libinput -> xkbcommon -> compositor seat
```

Wana owns DRM discovery/modesetting and its compositor path. Mesa supplies
GBM/EGL/GLES. The current compatibility matrix includes software/virtual
drivers plus the documented hardware targets in Phase 28; LLVM-backed
Iris/RadeonSI acceleration remains a non-blocking follow-up outside the current
Phase 28 exit gate.

Arabic/Latin shaping and rasterization are provided by the Wana text stack.
Keyboard text comes through xkbcommon and is routed only to the focused
surface.

## 6. Shell and native applications

The shell provides desktop surfaces, top bar, Dock, launcher and window
management. Native Wana applications include Settings, Files, Permissions,
Control Center, Power, Update and diagnostics; the launcher also exposes
screen lock and Android integration.

Persistent Settings/Permissions/Notifications stores use bounded, atomic
writes and validate ownership/mode. Notification and permission-audit sequence
exhaustion is rejected rather than silently wrapping or duplicating IDs.

## 7. Authentication and privileged brokers

The desktop does not expose the launcher until local authentication succeeds.

`wana-authd`, `wana-powerd` and `wana-updated` are root brokers under
`/run/wana`. Desktop access is authenticated with Linux `SO_PEERCRED` and
limited to uid 1000 (plus root).

Credentials are stored as a salted PBKDF2-HMAC-SHA256 verifier with 200,000
iterations under root-owned persistent state. Credential reads bind the opened
file back to the checked inode/device and revalidate ownership/mode/size before
reading.

The Power UI requires an explicit second confirmation before a destructive
operation.

## 8. Networking, time, audio and Bluetooth

Networking uses supervised WPA supplicant + DHCP. Wi-Fi credentials stay in a
root-managed configuration while the desktop uses the controlled WPA socket.

BlueZ provides Bluetooth Classic/LE and desktop audio/HID integration. PipeWire
and WirePlumber run in the uid-1000 session; `pipewire-pulse` provides the
PulseAudio protocol used by compatibility clients. Chrony provides supervised
network time synchronization.

## 9. Windows and Android compatibility

Wine 11.0 is built as an x86_64 native-Wayland runtime with ALSA and Pulse
client support; Pulse traffic reaches the normal PipeWire graph. The Wine
source and license files have fixed hashes in the Wana Buildroot package.

Waydroid 1.6.3 is integrated through Binder/BinderFS, LXC and Wana's own service
model without systemd. Its privileged container helper waits for Waydroid's
upstream initialized state before starting, so first boot does not restart-loop
while Android images are absent.

Android UI/session evidence uses a separate pre-provisioned test disk; Android
OTA images are not embedded into the reproducible Wana release.

## 10. Installer and update security

Installer validation is bound to opened source/target objects and rejects
symlink/hard-link/block-device alias races before destructive writes.

Stable online updates:

- use strict `X.Y.Z` versions;
- discover `latest` only to choose the stable version;
- re-fetch metadata/checksums/payload from the exact `vX.Y.Z` tag;
- enforce download size ceilings;
- verify metadata and payload SHA-256 values;
- reject downgrades;
- reject one version number being reused for a different commit;
- stage atomically on persistent Data;
- write only the inactive root slot.

Current online release-origin trust is GitHub HTTPS and repository/release
control. Closed hashes protect artifact integrity. Detached offline release
signatures are not part of the current Phase 36 exit gate.

## 11. Diagnostics and validation

`wana-diagnostics` collects a deliberately bounded support summary as the
desktop user. It does not copy arbitrary home/state trees, Wi-Fi credentials,
the authentication verifier or notification history.

`make final-validation-test` is the consolidated end-of-implementation gate.
It is run only after the source revision is frozen. Phase reports remain
IN PROGRESS until their actual final evidence is recorded.
