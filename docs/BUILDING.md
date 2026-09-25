# Building Wana OS

Everything goes through the top-level `Makefile`. Run `make help` for the
current list of targets.

## What builds today

| Target | What it does | Needs |
|--------|--------------|-------|
| `make check` | rustfmt check, clippy (warnings are errors), unit tests, repository layout check | Rust toolchain (installed automatically from `rust-toolchain.toml` by rustup) |
| `make fmt` | Formats all Rust code | Rust |
| `make test` | Unit tests only | Rust |

The kernel, rootfs, and ISO targets do not exist yet. They are added in
Phases 2-6 and documented here when they work.

## Host requirements

- Linux x86_64 host
- `rustup` (it installs the toolchain pinned in `rust-toolchain.toml`)
- GNU make, git

## Network note

A Buildroot build downloads upstream source tarballs from kernel.org, gnu.org
and other hosts. It needs unrestricted outbound HTTPS. GitHub Actions runners
have it. Restricted sandboxes may not (see
[audit 0000](audit/0000-repository-audit.md)).
