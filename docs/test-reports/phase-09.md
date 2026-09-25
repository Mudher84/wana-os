# Test report: Phase 9 (input stack: udev + libinput + xkbcommon -> wana-input)

Components:
- `wana-init` (`crates/wana-init/src/udev.rs`): device manager bring-up before `ready`.
  - Finds `udevd` (eudev `/sbin/udevd`, or `systemd-udevd` in host-built test images) and `udevadm`.
  - Turns off the kernel hotplug helper, starts `udevd` as a direct child and waits for `/run/udev/control`.
  - Runs `udevadm trigger` (subsystems, then devices) and `udevadm settle`.
  - Logs how many devices the udev database holds and how many are input (`E:ID_INPUT=1`).
  - Restarts `udevd` if it dies.
  - Failures give `degraded: ...` instead of `ready`.
  - New kernel option `wana.udev=0`.
- `crates/wana-input`: small FFI declarations for libudev, libinput and libxkbcommon, with no crates, plus safe wrappers:
  - `libinput.rs`: udev seat context and owned events (device, key, relative and absolute motion, button, wheel);
    touch, tablet, gesture and switch events stay in the model as `Other`.
  - `keyboard.rs`: xkbcommon keymap (`evdev`/`pc105`/layout) and state; evdev code becomes keysym and text.
  - `names.rs`: button names and typed-text matching.
- `wana-input` (`/usr/bin/wana-input`): bring-up tool and test.
  - Logs every device and event as an `[INPUT]` line, including the source event node.
  - `--expect-text/--expect-pointer/--expect-button` make it exit 0 once that input arrived, and 1 on timeout.
- Buildroot:
  - `/dev` management is now devtmpfs + eudev 3.2.14, without module loading or hwdb.
  - libinput 1.31.1, libxkbcommon 1.9.2 and xkeyboard-config 2.38 are in the image.
  - The new `wana-input` package pulls them in.
  - `board/x86_64/users.table` adds the groups eudev's rules name: `input`, `kvm`, `sgx`, with fixed GIDs.
- `tools/qemu-graphics-test.py`:
  - `--input virtio` adds a virtio keyboard and tablet.
  - `--send-on REGEX --send CMD...` types and clicks through the QEMU monitor when a log line appears.
- `make input-boot-test` and a CI step.

The kernel side was already in place: `CONFIG_INPUT_EVDEV`, `CONFIG_VIRTIO_INPUT`, USB HID and xHCI are in the fragment
since Phase 3; PS/2 keyboard and mouse come from `x86_64_defconfig`.

## T1: Unit tests, lint, MSRV (local)

- `make check`: 44 tests:
  - wana-init 14 (new: udev database counting, input record detection, `wana.udev`);
  - wana-input 7;
  - wana-drm 13, wana-log 6, wana-render 4.
- The wana-input keyboard tests run the real xkbcommon with the host's xkeyboard-config:
  - US layout: `a`, `Shift` + `w` = `W`, `Return`;
  - French AZERTY: the QWERTY `a` position types `q`;
  - an unknown layout is an error.
- clippy `-D warnings` is clean.
- `make msrv` (Rust 1.88) passes.
- Result: **PASS**

## T2: udev coldplug by wana-init (local, initramfs, host systemd-udevd 255)

- Setup:
  - kernel 6.18.33-wana with the Wana fragment;
  - initramfs with `wana-init`, the host's `systemd-udevd`/`udevadm`, udev rules and glibc;
  - QEMU TCG + OVMF.
- Log (`wana.test=poweroff`):
  ```
  [INIT] info: udev: /usr/lib/systemd/systemd-udevd started (pid 76)
  [INIT] info: udev: coldplug done in 4209 ms: 179 devices initialized, 6 input
  [INIT] info: ready (8.01s after kernel start)
  [INIT] info: test boot: PowerOff
  ```
- The test initramfs had a minimal `/etc/group`, and udevd logged `Unknown group 'dialout'` (and `kmem`, `lp`,
  `cdrom`, `tape`) for each rule that names them. That is why the Buildroot image gets `board/x86_64/users.table`:
  Buildroot's skeleton lacks `input`, `kvm` and `sgx`, which eudev's rules use.
- Result: **PASS**

## T3: Injected keys, pointer and click reach wana-input (local, initramfs, host libinput 1.25 / xkbcommon 1.6)

- Setup: as T2, plus libinput, xkbcommon, the xkeyboard-config data and `wana-input`. QEMU runs with
  `--input virtio`: virtio keyboard and tablet, plus the q35 PS/2 keyboard and mouse.
- Command: `wana.run=/usr/bin/wana-input,--timeout,60,--expect-text,wana,--expect-pointer,--expect-button`.
  When `waiting for input` appears, the harness sends `sendkey w`, `a`, `n`, `a`, `mouse_move 40 30`,
  `mouse_button 1` and `mouse_button 0`.
- Log:
  ```
  [INIT] info: udev: coldplug done in 4485 ms: 189 devices initialized, 10 input
  [INPUT] info: wana-input 0.1.0: seat seat0, layout us
  [INPUT] info: keymap compiled: English (US) (xkbcommon)
  [INPUT] info: device added: Power Button (event0) [keyboard]
  [INPUT] info: device added: QEMU Virtio Keyboard (event1) [keyboard]
  [INPUT] info: device added: QEMU Virtio Tablet (event2) [pointer]
  [INPUT] info: device added: AT Translated Set 2 keyboard (event3) [keyboard]
  [INPUT] info: device added: ImExPS/2 Generic Explorer Mouse (event4) [pointer]
  [INPUT] info: seat0: 5 devices, 3 with keyboard, 2 with pointer
  [INPUT] info: waiting for input (timeout 60s)
  [INPUT] info: event1: key 17 pressed: w text "w"
  [INPUT] info: event1: key 30 pressed: a text "a"
  [INPUT] info: event1: key 49 pressed: n text "n"
  [INPUT] info: event1: key 30 pressed: a text "a"
  [INPUT] info: event4: pointer motion dx 22.00 dy 16.50
  [INPUT] info: event2: button BTN_LEFT pressed
  [INPUT] info: event2: button BTN_LEFT released
  [INPUT] info: typed text "wana" matches "wana"
  [INPUT] info: done: 4 key presses, 1 pointer events, 1 left clicks
  [INIT] info: /usr/bin/wana-input exited successfully
  ```
- QEMU sends the keys to the virtio keyboard, the relative motion to the PS/2 mouse and the click to the virtio
  tablet. libinput handles all three the same way, and each line names its source node. The motion is 22/16.5
  for a 40/30 move because libinput's pointer acceleration applies.
- First attempt: the tool exited on the button *press*. The guest powered off before QEMU got `mouse_button 0`,
  and the monitor socket closed (broken pipe). `--expect-button` now waits for a full click, and the harness
  reports a monitor failure instead of crashing.
- Result: **PASS**

## T4: A broken layer is reported at that layer (local)

- No udev (`wana.udev=0`): the device list is empty, and `wana-input` stops at `[INPUT]`, naming udev:
  ```
  [INIT] info: udev: disabled (wana.udev=0)
  [INPUT] error: no input devices on seat0 (is udevd running and are devices tagged ID_INPUT?)
  [INIT] error: /usr/bin/wana-input failed: exit status: 1
  ```
- No keymap data (xkeyboard-config removed from the image):
  ```
  xkbcommon: ERROR: failed to add default include path /usr/share/X11/xkb
  [INPUT] error: keymap: xkb_context_new failed
  ```
  After this run the message was extended to name the missing package
  (`is xkeyboard-config installed in /usr/share/X11/xkb?`).
- Both test runs exit 1.
- Result: **PASS**

## T5: Buildroot configuration

- `make config-check`: the defconfig loads and round-trips unchanged. The resolved `.config` has:
  - `BR2_ROOTFS_DEVICE_CREATION_DYNAMIC_EUDEV`, eudev with module loading and hwdb off;
  - libinput, libevdev, libxkbcommon, xkeyboard-config;
  - util-linux libs and libblkid (eudev dependencies);
  - `BR2_PACKAGE_WANA_INPUT`, and the users table.
- Result: **PASS**

## T6: Buildroot image + `make input-boot-test` in CI

- Run [36174232427](https://github.com/Mudher84/wana-os/actions/runs/36174232427), commit `d73a8e9`, job
  `toolchain, image, UEFI boot`, 21 min:
  - `Build image` took 10m31s: eudev, util-linux libs, libevdev, libinput, libxkbcommon, xkeyboard-config (with its host
    X tools) and `wana-input`, built from source for the first time.
- All earlier tests still pass with eudev in the image:
  - kernel config (50 options);
  - kernel alone, initramfs, disk through GRUB;
  - graphics (`wana-kms`, 3/3 pixels) and GPU rendering (`wana-gl`, 3/3 pixels).
  Every boot now shows `[INIT] info: udev: /sbin/udevd started` and a coldplug line before `ready`.
  `ready` comes 1.52-1.56 s after kernel start, against about 1.3 s before udev.
- `make input-boot-test` (KVM, OVMF → GRUB → kernel → ext4 → wana-init → eudev → wana-input), 7 s:
  ```
  [INIT] info: udev: /sbin/udevd started (pid 79)
  [INIT] info: udev: coldplug done in 342 ms: 136 devices initialized, 10 input
  [INIT] info: ready (1.52s after kernel start)
  [INPUT] info: wana-input 0.1.0: seat seat0, layout us
  [INPUT] info: keymap compiled: English (US) (xkbcommon)
  [INPUT] info: device added: Power Button (event0) [keyboard]
  [INPUT] info: device added: QEMU Virtio Keyboard (event1) [keyboard]
  [INPUT] info: device added: QEMU Virtio Tablet (event2) [pointer]
  [INPUT] info: device added: AT Translated Set 2 keyboard (event3) [keyboard]
  [INPUT] info: device added: ImExPS/2 Generic Explorer Mouse (event4) [pointer]
  [INPUT] info: seat0: 5 devices, 3 with keyboard, 2 with pointer
  [INPUT] info: waiting for input (timeout 60s)
  [INPUT] info: event1: key 17 pressed: w text "w"
  [INPUT] info: event1: key 30 pressed: a text "a"
  [INPUT] info: event1: key 49 pressed: n text "n"
  [INPUT] info: event1: key 30 pressed: a text "a"
  [INPUT] info: event4: pointer motion dx 22.00 dy 16.50
  [INPUT] info: event2: button BTN_LEFT pressed
  [INPUT] info: event2: button BTN_LEFT released
  [INPUT] info: typed text "wana" matches "wana"
  [INPUT] info: done: 4 key presses, 1 pointer events, 1 left clicks
  [INIT] info: /usr/bin/wana-input exited successfully
  [BOOT] graphics test: PASS
  ```
  All 9 expectations were found. The CI result matches the local run, event routing included, with the Buildroot
  versions: eudev 3.2.14, libinput 1.31.1, xkbcommon 1.9.2 and xkeyboard-config 2.38.
- `ci` run 36174267098: fmt, clippy, 44 unit tests including the real-keymap tests, repo checks, MSRV 1.88: success.
- Result: **PASS**
- Gap in the evidence, and its fix: the CI console echoes only tagged lines. An untagged eudev error, such as
  `Unknown group`, would not show. `qemu-graphics-test.py` now has `--reject REGEX`, and `input-boot-test` fails
  if `Unknown (group|user)` or an `[INIT]`/`[INPUT]` error appears anywhere in the console log. The check was
  proven locally: the minimal-`/etc/group` test image fails it (`rejected pattern present: Unknown (group|user)`).
  The next CI run applies it to the Buildroot image.

## Phase 9 status

**PASS.** T1-T6 pass. The Buildroot image, booted the way hardware boots it, starts udev from `wana-init`, and
input injected through QEMU travels evdev → udev → libinput → xkbcommon → `wana-input`: text, pointer motion and a
click, each attributed to its device.

Not done yet:
- eudev's hwdb is disabled (Phase 27, real laptops);
- touch, tablet and gesture events are recognized but not handled yet;
- key repeat and seat and device permission handling belong to the compositor (Phase 10). `wana-input` runs as
  root and opens devices directly.
