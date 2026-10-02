# Wana OS

Wana OS is an independent x86_64 Linux distribution built from source with
Buildroot and the Linux kernel. It is not a themed Ubuntu, Debian or Arch
system.

Wana-owned userspace is primarily Rust: PID 1, the native DRM/KMS compositor,
desktop shell and applications, installer, service supervisor, authentication,
power and update brokers.

## Current status

- Phases 1–11 have recorded PASS evidence.
- Implementation for phases 12–39 is present on the current pre-stable line.
- Phases 12–39 remain **IN PROGRESS** until the consolidated final validation
  and evidence pass; implementation alone does not promote a phase to PASS.
- The release version in the workspace is currently `0.1.0`.

The current system includes:

- UEFI/GRUB boot, reproducible Buildroot images and Live ISO;
- native DRM/KMS + GBM/EGL/GLES graphics;
- a Wana Wayland compositor and privileged shell with Arabic/Latin text;
- launcher, Dock, window management, Settings, Files, Permissions and Control
  Center;
- Wi-Fi, DHCP, Bluetooth, ALSA/PipeWire/WirePlumber and Chrony;
- an installer and installed A/B root layout with persistent Data;
- local authentication, screen lock and peer-authenticated power/update
  brokers;
- A/B updates with trial boot, rollback and manual force-slot recovery;
- Wine 11.0 on native Wayland;
- Waydroid 1.6.3 integration and elastic installed storage;
- privacy-bounded diagnostics/support bundles.

## Entry points

```sh
make help
make check
make image
```

The one-shot end-of-implementation validation is intentionally kept for the
final frozen source revision:

```sh
make final-validation-test
```

Do not interpret the existence of that target as PASS evidence; see
[docs/FINAL-VALIDATION.md](docs/FINAL-VALIDATION.md).

## Documentation

- [Architecture](docs/ARCHITECTURE.md)
- [Roadmap](docs/ROADMAP.md)
- [Building](docs/BUILDING.md)
- [Final validation](docs/FINAL-VALIDATION.md)
- [Contributing](docs/CONTRIBUTING.md)
- [Architecture decisions](docs/decisions/)
- [Audits and phase reports](docs/audit/) / [docs/test-reports/](docs/test-reports/)
