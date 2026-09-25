# Test report: Phase 4 (Minimal root filesystem, wana-init as PID 1)

- Component: `crates/wana-init` 0.1.0 (Rust, no crates.io dependencies, libc called directly)
- Buildroot integration: `platform/package/wana-init` (Cargo package, local source),
  `BR2_INIT_NONE`, initramfs `rootfs.cpio.zst`, post-build script links `/init` and
  `/sbin/init` to `/usr/sbin/wana-init`
- MSRV: Rust 1.88 (the version Buildroot 2026.02.3 ships as `host-rust-bin`)

## T1: Unit tests, lint, MSRV (local)

- Commands: `make check`, `make msrv`
- Expected: all pass on 1.94.1 (pinned dev toolchain) and on 1.88 (Buildroot's rustc)
- Actual: wana-init 8 tests passed, wana-log 6 passed, clippy clean with `-D warnings`, `[CHECK] repository checks: PASS`. The same 14 tests pass on 1.88.
- Result: **PASS**

## T2: Refuses to run when it is not PID 1 (local)

- Command: `./target/debug/wana-init`
- Expected: error and non-zero exit, nothing mounted
- Actual: `[INIT] error: wana-init must run as PID 1 (running as pid 1069)`, exit 1
- Result: **PASS**

## T3: Boot to ready under UEFI, then power off (local)

- Build: `cargo build --release -p wana-init --target x86_64-unknown-linux-musl` (static,
  619 KB). Hand-made test initramfs: `/init` -> wana-init, `/etc/hostname`, `/dev/console`.
  Kernel: the Phase 3 bzImage.
- Command: `tools/qemu-boot-test.sh ... --append "wana.test=poweroff wana.log=debug"`
- Expected: kernel runs /init; early mounts succeed; hostname set; `ready`; clean power off
- Actual (QEMU TCG + OVMF):
  ```
  [INIT] info: wana-init 0.1.0 starting
  [INIT] info: early mounts: 7 ok, 0 failed
  [INIT] info: hostname: wana
  [INIT] info: kernel: Linux 6.18.33-wana
  [INIT] info: ready (3.10s after kernel start)
  [INIT] info: test boot: PowerOff
  [    3.191062] reboot: Power down
  ```
- Result: **PASS**

## T4: Console shell supervision and restart (local)

- Initramfs as in T3, plus a static busybox as `/bin/sh`
- Command: boot without `wana.test`, then type `exit` on the serial console after 12 s (`--input-after 12 'exit\n'`)
- Expected: shell started; after `exit`, init reaps it and starts a new one
- Actual:
  ```
  [INIT] info: console shell started (pid 76)
  [INIT] info: console shell exited (wait status 0), restarting
  [INIT] info: console shell started (pid 77)
  ```
- Result: **PASS**

## T5: Orphan reaping (local)

- Command: boot with `wana.log=debug`, type `sleep 3 &` then `exit`: the background `sleep`
  outlives its parent and is re-parented to PID 1
- Expected: init reaps the orphan
- Actual: `[INIT] debug: reaped pid 76 (wait status 0)`
- Note: the first attempt reported FAIL because the test pattern was anchored with `^`, and
  the log line followed the shell prompt on the same console line. Fixed the pattern (not
  the code) and re-ran.
- Result: **PASS**

## T6: Buildroot package source copy (local)

- Command: `make br-wana-init-rsync`
- Expected: only repository sources copied (no `out/`, `target/`, `dl/`, `.git`)
- Actual: build dir holds `Cargo.toml Cargo.lock crates platform tools docs ...`, 256 KB total
- Result: **PASS**

## T7: `make system-boot-test` target (local, with T3 artifacts in place of Buildroot's)

- Expected: all 7 patterns found
- Actual: `[BOOT] boot test: PASS`
- Result: **PASS**

## T8: Full Buildroot image and system boot in CI

- Run: [36110220726](https://github.com/Mudher84/wana-os/actions/runs/36110220726) (PR Mudher84/wana-os#2, commit `8d3d7e7`), job `toolchain, image, UEFI boot`, 19m07s with warm caches
- Build order from the log: `linux 6.18.33` → `host-rustc` (rust-bin 1.88) → `wana-init 0.1.0` Syncing/Building/Installing → post-build script → `rootfs.cpio` → `rootfs.tar`
- `make kernel-config-check`: `[KERNEL] kernel config check: 49 options verified: PASS`
- `make kernel-boot-test`: PASS (unchanged Phase 3 test)
- `make system-boot-test` (QEMU accel=kvm, OVMF, Buildroot `bzImage` + `rootfs.cpio.zst`, `wana.test=poweroff`):
  ```
  [BOOT] info: found: Linux version 6\.18\.33-wana
  [BOOT] info: found: Run /init as init process
  [BOOT] info: found: \[INIT\] info: wana-init [0-9.]+ starting
  [BOOT] info: found: \[INIT\] info: early mounts: 7 ok, 0 failed
  [BOOT] info: found: \[INIT\] info: hostname: wana
  [BOOT] info: found: \[INIT\] info: ready
  [BOOT] info: found: reboot: Power down
  [BOOT] boot test: PASS (log: out/logs/system-boot.log)
  ```
- MSRV job (`make msrv` on Rust 1.88): success
- Artifact: `wana-image-<sha>` (bzImage, rootfs.cpio.zst, configs, build and boot logs), 14 days
- Result: **PASS**

## Phase 4 status

**PASS.** T1-T8 all pass. The Buildroot-built system boots under UEFI into
`wana-init`, which reaches `ready` in the initramfs.

Known limits, deliberately outside Phase 4:
- The kernel is still loaded with QEMU `-kernel`, not by a bootloader. Phase 5 adds GRUB on an ESP.
- The root filesystem is an initramfs only. A disk root filesystem comes with the installer (Phase 19).
- The debug shell is a root shell on the console, with no login. It is a bring-up tool, and it must be gated before any release (Phase 23).
