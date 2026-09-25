# Wana OS Architecture

This document records the architecture decisions that are in force now, and
the ones that are proposed but not yet validated. A proposed decision becomes
final only after the phase that implements it has been built and tested (see
[ROADMAP.md](ROADMAP.md)).

Status legend: **Decided**: in force. **Proposed**: planned, to be confirmed by
the phase that implements it.

## 1. System layers and boot flow

```
Firmware (UEFI)
  -> Bootloader (GRUB, x86_64-efi, /EFI/BOOT/bootx64.efi on the ESP)
                                                   platform/board/x86_64/grub.cfg
  -> Linux kernel (+ initramfs for live ISO)       platform/board/x86_64/linux.fragment
  -> wana-init (PID 1)                             crates/wana-init
       -> early mounts (proc, sys, devtmpfs, devpts, shm, run, tmp), hostname
       -> reaps orphans; supervises the console debug shell (Phase 4)
       -> device manager (udev)                    eudev, from Buildroot
       -> system services (supervised by wana-init)
            display/session   -> wana-compositor   crates/wana-compositor
            input             -> inside the compositor's seat, via wana-input
            permissions       -> wana-permd        crates/wana-permd
            network, power, notifications (later)
  -> wana-compositor (owns DRM master + seat)
       -> wana-shell (privileged Wayland client: desktop, dock, launcher, status)
       -> applications (unprivileged Wayland clients, sandboxed)
```

Each arrow is a process or ownership boundary. No single desktop program owns
the whole system. When a layer fails, the ones below it keep running and keep
logging, so the first broken layer can be identified (see §9).

## 2. Build system: Buildroot (**Decided**)

We use Buildroot with a `BR2_EXTERNAL` tree (`platform/`). The Buildroot
version is pinned by git tag and fetched from `github.com/buildroot/buildroot`.

Why Buildroot:

- It builds the whole system from source (toolchain, kernel, libraries,
  rootfs) with no upstream distribution underneath. That matches the goal of
  a distribution that is not a derivative of Ubuntu, Debian, or Arch.
- Configuration is Kconfig `defconfig` files plus a kernel config fragment.
  All of it lives in this repository and is diffable.
- `BR2_EXTERNAL` keeps every Wana-specific file outside the Buildroot tree, so
  moving to a newer Buildroot means changing one pinned tag.
- It has native Cargo package support, so our Rust crates are built by the
  same reproducible pipeline as everything else.
- It can produce the kernel, a rootfs image, and an EFI GRUB setup in one
  documented command, which is requirement 10.

Alternatives considered:

| Option | Why not (now) |
|--------|---------------|
| Yocto/OpenEmbedded | Solves the same problem with much more machinery (layers, BitBake, sstate). Its strengths, such as many machines and SDK generation, are not our bottleneck. Build times and the learning curve are several times higher. |
| Debian/Ubuntu bootstrap (debootstrap, live-build) | Would make Wana a Debian derivative, which the project explicitly rejects. |
| Hand-written LFS-style scripts | No dependency tracking, no license or hash checking, poor reproducibility. We would be re-implementing Buildroot badly. |

Known Buildroot limitation: it produces an image, not a package-managed
system. Proposed answer: Wana ships as an **immutable, image-based system**
(read-only root, A/B root partitions for updates, writable `/home` and
`/var`). This is a feature for stability and security. User-installable
applications will need their own mechanism, which is decided in a later phase.

## 2a. Disk layout (**Decided**, Phase 5)

GPT. Partition 1 is the EFI System Partition (vfat, `WANA-ESP`): GRUB at
`/EFI/BOOT/bootx64.efi` (the removable-media path, so no NVRAM boot entry
is needed), `grub.cfg` next to it, and the kernel at `/wana/bzImage`.
Partition 2 is the root filesystem (ext4, GPT type
`4f68bce3-e8cd-4db1-96e7-fbcaf984b709`, "Linux root x86-64"). The kernel
finds it by `root=PARTUUID=`, mounted read-only. GRUB sources an optional
`/wana/test.cfg` so automated tests can add kernel arguments to a copy of
the image without rebuilding it. GRUB stays replaceable: only `grub.cfg`,
the `BR2_TARGET_GRUB2*` symbols and `post-image.sh` know about it.

## 3. C library: glibc (**Proposed**, Phase 2)

glibc rather than musl. Mesa, libinput, and future compatibility layers
(ordinary Linux binaries, Wine) are developed and tested against glibc first,
and requirement 35 says Wana must be able to run normal Linux applications.

## 4. Languages

- **Rust** for every new Wana component: init, services, graphics backend,
  compositor, shell, settings, installer logic, the permission broker, and
  utilities. All crates share one Cargo workspace (`Cargo.toml`) with a single
  lockfile and a pinned toolchain (`rust-toolchain.toml`).
- **C** only where we consume existing system libraries (Mesa, libinput,
  libdrm, eudev). We call them through Rust bindings and do not fork them.
- **Shell / Make / Kconfig** for build glue: Buildroot configuration,
  post-build and post-image scripts, and the top-level `Makefile`.

## 5. Graphics stack (DRM/KMS layer **Decided**, Phase 7; GBM/EGL/GLES **Proposed**, Phase 8)

```
Linux DRM/KMS  ->  GBM  ->  EGL  ->  OpenGL ES 3  ->  wana-compositor  ->  wana-shell
```

- Device discovery enumerates the `drm` subsystem through udev (sysfs
  fallback) and picks the card with a connected connector. It never
  hard-codes `/dev/dri/card0`.
- `wana-drm` (implemented): uses the kernel DRM uAPI directly, with no libdrm. `repr(C)`
  structs and ioctl numbers are checked against `<drm/drm_mode.h>` in unit tests. It opens the device and becomes DRM master. It enumerates
  connectors, encoders, CRTCs and modes, picks a mode (preferred first, then
  highest refresh at native resolution), does atomic modesetting (legacy as
  fallback), and page flipping driven by vblank events. It never uses a fixed
  60 Hz timer, so 90 Hz and 120 Hz panels work without code changes.
- `wana-render`: GBM surfaces, an EGL context with
  `EGL_PLATFORM_GBM_KHR`, and a GLES 3 renderer.
- Mesa provides GBM, EGL and GLES. Under QEMU we use `virtio-gpu` for KMS.
  Rendering there is Mesa software (llvmpipe) at first, so CI can test it
  without a GPU.
- Weston is not used, not even for bring-up. The first graphical milestone
  draws directly through our own stack.

## 6. Input (**Proposed**, Phase 9)

udev -> libinput -> `wana-input` -> compositor seat -> focused client.
`wana-input` turns libinput events into Wana input events. Pointer, keyboard
and touchpad come first. Touch, gesture and tablet event types are in the
event model from the start. No component other than `wana-input` touches
`/dev/input/event*`. The shell and applications receive input only from the
compositor.

## 7. Compositor, shell, and applications (**Proposed**, Phases 10-13)

- The client protocol is **Wayland**, so ordinary Linux applications (GTK,
  Qt, SDL, Chromium, and later Xwayland and Wine) can run without dictating
  our desktop. The implementation is our own Rust code. Whether we build on
  the Smithay protocol library or on bare `wayland-server` is decided at the
  start of Phase 10, with written reasons.
- `wana-shell` is a separate process: a privileged client that uses
  Wana-specific protocols for its surfaces (desktop, dock, launcher, status
  area, control center). A shell crash must not take down the compositor or
  the applications.
- A design system (`design/`: tokens for color, radius, spacing, type, and
  motion curves) is shared by the shell and the apps, so screens are not
  styled one at a time.

## 8. Services, IPC, and security (**Proposed**, Phases 17, 22-23)

- `wana-init` supervises services. Each service has a declared owner (its
  own UID where possible), a restart policy, and a dependency list.
- IPC is Unix domain sockets under `/run/wana/`. Peers are authenticated with
  `SO_PEERCRED`, and messages use a versioned format (`wana-ipc`). D-Bus is
  added only when application compatibility needs it (XDG portals,
  notifications).
- Permissions: applications start with no user-level capabilities. They run
  sandboxed (mount/user/pid namespaces, seccomp, Landlock). Access to files,
  camera, microphone, location, network, notifications, clipboard, USB, and
  background activity is granted by `wana-permd`.
- Permission audit: `wana-permd` records (app, permission, action, decision,
  timestamp) in a bounded ring buffer that only the `wana-permd` user can
  write. Applications have no write path to it.

## 8a. wana-init (**Decided**, Phase 4)

- PID 1 is our own Rust program, not BusyBox init or systemd. It has no
  crates.io dependencies: PID 1 keeps a small, auditable surface, and
  Buildroot builds local Cargo packages with `--offline`. The few libc
  calls std lacks (`mount`, `sethostname`, `reboot`, `waitpid`) are wrapped
  in `crates/wana-init/src/sys.rs`.
- It is `/init` in the initramfs and `/sbin/init` on disk (same binary).
- It never exits. Failures are logged as `[INIT] error` and the boot
  continues degraded, so the console shows the first broken step.
- Kernel command line options: `wana.log=`, `wana.test=poweroff|reboot`
  (automated test boots), `wana.shell=0`.
- MSRV = the Rust version shipped by the pinned Buildroot (1.88 for
  2026.02.3). CI checks it.

## 9. Logging (**Decided**)

Every component logs through `wana-log` with a subsystem tag:
`[BOOT] [KERNEL] [INIT] [DRM] [GBM] [EGL] [RENDER] [INPUT] [COMPOSITOR]
[SHELL] [NETWORK] [INSTALLER] [SECURITY]`. Each line has the form
`[TAG] level: message`, so CI and developers can grep a serial console log
and find the first failing layer.

## 10. Repository layout

Directories are created when their first component lands. There are no empty
placeholder trees.

```
wana-os/
├── Makefile              single entry point (make help)
├── Cargo.toml            Rust workspace for all Wana userspace components
├── rust-toolchain.toml   pinned Rust toolchain
├── crates/               one crate per Wana component
│   └── wana-log/         [present] tagged logging
├── platform/             Buildroot BR2_EXTERNAL tree            (Phase 2)
│   ├── configs/          wana_x86_64_defconfig
│   ├── board/x86_64/     kernel fragment, grub.cfg, rootfs overlay, post-image
│   └── package/          Buildroot packages for our crates
├── tests/                QEMU boot and graphics tests            (Phase 5+)
├── design/               design system tokens, icons, fonts     (Phase 11+)
├── tools/                developer and CI scripts
├── .github/workflows/    CI
└── docs/                 architecture, roadmap, audits, test reports
```

Where the directories from the original project brief live:

| Requested | Location | Reason |
|-----------|----------|--------|
| `kernel/`, `boot/`, `buildroot/` | `platform/` | In Buildroot these are all board configuration consumed by one defconfig. Keeping them together avoids cross-tree path glue. |
| `init/`, `services/`, `compositor/`, `shell/`, `settings/`, `installer/`, `security/`, `permissions/`, `graphics/` | `crates/wana-*` | One Cargo workspace gives a shared lockfile, one lint policy, and one `cargo test`. |
| `apps/` | `crates/wana-app-*` | Same reason. |
| `assets/` | `design/` | Assets belong to the design system. |
| `ci/` | `.github/workflows/` + `tools/` | GitHub reads workflows only from `.github/`. The scripts they call live in `tools/`. |
