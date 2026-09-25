# Wana OS

Wana OS is an independent Linux distribution. It is built from source with
Buildroot on the Linux kernel. Its own components are written in Rust: init,
system services, a native DRM/KMS + GBM/EGL/GLES graphics stack, a compositor,
a desktop shell, and the installer.

It is not a themed Ubuntu, Debian or Arch.

## Status

Early bring-up. See [docs/ROADMAP.md](docs/ROADMAP.md) for each phase's
status and the evidence behind it.

## Quick start

```sh
make help     # list targets
make check    # format, lint, unit tests, repository checks
```

## Documentation

- [Architecture](docs/ARCHITECTURE.md)
- [Roadmap](docs/ROADMAP.md)
- [Building](docs/BUILDING.md)
- [Contributing](docs/CONTRIBUTING.md)
- [Audits](docs/audit/) and [test reports](docs/test-reports/)
