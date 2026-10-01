# Phase 34 — Android compatibility and elastic installed storage

Status: **IN PROGRESS**

Phase 34 integrates Waydroid with Wana's own init/service model. Wana does not
require systemd for Android support.

## Implemented

- Kernel Binder IPC/BinderFS with binder, hwbinder and vndbinder devices.
- cgroup v2, namespaces/network primitives and LXC support.
- Pinned external Buildroot packages for:
  - libglibutil;
  - libgbinder;
  - gbinder-python;
  - Waydroid 1.6.3.
- Waydroid is installed with `USE_SYSTEMD=0` and
  `USE_DBUS_ACTIVATION=0`; the privileged container manager is supervised by
  `wana-services` on the system D-Bus.
- Waydroid bridge networking includes iproute2, iptables and dnsmasq.
- Android audio is routed through Wana's PipeWire PulseAudio compatibility
  service.
- `wana-android` provides initialization, session start/stop, full UI, APK
  install, package launch, shell and log commands.
- Android appears in the Wana launcher and user sessions reuse the native
  `/run/user/1000/wayland-0` display.
- The release image does not embed a multi-gigabyte Android system image.
  `embiggen-disk /` runs as an idempotent Wana service so an installed Wana
  system expands its final ext4 root partition to the actual target disk and
  has room for Android images/app data.
- The reproducible release gate validates the Waydroid runtime itself; the full
  Android UI gate is separate and consumes a pre-provisioned Android test disk
  so release reproducibility never depends on a live OTA download.

## Exit gate

1. Build the pinned Binder/Waydroid dependency chain.
2. Boot and report Waydroid 1.6.3 from the Wana image.
3. Install/provision an official Android image on an expanded test disk.
4. Start the Android session as uid/gid 1000 through Wana's Wayland socket.
5. Launch the full Android UI and at least one installed APK without Wana
   compositor/service errors.

## Evidence

Pending final consolidated validation. No PASS is claimed yet.
