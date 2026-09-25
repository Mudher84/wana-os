# Test report: Phase 5 (UEFI boot through a bootloader, from a disk image)

- Boot chain: OVMF (UEFI) -> GRUB 2 `x86_64-efi` (`/EFI/BOOT/bootx64.efi` on the ESP) ->
  `/wana/bzImage` (ESP) -> ext4 root partition found by PARTUUID -> `/sbin/init` = wana-init
- Image: `images/disk.img`, GPT: `esp` 64 MiB vfat (type EF00) + `root` ext4 (type 8304,
  Linux root x86-64 per the Discoverable Partitions Spec, fixed PARTUUID from
  `platform/board/x86_64/disk.env`)
- Built by: `platform/board/x86_64/post-image.sh` + `genimage.cfg`, with `grub.cfg` templated
- Test hook: `tools/mk-test-disk.sh` copies the image and writes `/wana/test.cfg` onto the ESP
  copy (`set timeout=0`, `wana_args="wana.test=poweroff"`). The original image is never modified.

## T1: Disk image assembly (local)

- Inputs: Phase 3 bzImage; `mkfs.ext4 -d` of the Phase 4 test root; GRUB from Ubuntu's
  `grub-mkimage -O x86_64-efi` with the exact module list of the defconfig
  (`boot linux ext2 fat part_gpt normal efi_gop serial terminal test configfile echo`)
- Command: `BINARIES_DIR=... GENIMAGE=genimage platform/board/x86_64/post-image.sh`
- Expected: GPT with ESP + root, root PARTUUID as configured
- Actual (`sgdisk -p` / `-i 2`):
  ```
  1   2048  133119  64.0 MiB  EF00  esp
  2 133120  264191  64.0 MiB  8304  root
  Partition unique GUID: 5D9F6A52-7E1B-4C3A-9B8E-0A1A57A0A001
  ```
- Result: **PASS**

## T2: Firmware -> GRUB -> kernel -> ext4 root -> wana-init (local)

- Command: `make disk-boot-test` (test copy with `wana.test=poweroff`, QEMU TCG + OVMF, virtio disk, snapshot)
- Actual:
  ```
  BdsDxe: starting Boot0001 "UEFI Misc Device" from PciRoot(0x0)/Pci(0x3,0x0)
  [BOOT] info: loading Wana OS kernel
  Command line: BOOT_IMAGE=/wana/bzImage root=PARTUUID=5d9f6a52-7e1b-4c3a-9b8e-0a1a57a0a001 rootwait ro console=tty0 console=ttyS0,115200 wana.test=poweroff
  EXT4-fs (vda2): mounted filesystem ... ro with ordered data mode
  Run /sbin/init as init process
  [INIT] info: early mounts: 7 ok, 0 failed
  [INIT] info: ready (3.67s after kernel start)
  reboot: Power down
  ```
  On a disk root the kernel has already mounted devtmpfs, so wana-init takes its "already mounted" path. That path still counts as success: 7 ok.
- Result: **PASS**

## T3: Default boot without the test hook (local)

- Command: boot the original `disk.img` (no `/wana/test.cfg`), 45 s timeout
- Expected: GRUB menu "Wana OS", 3 s timeout, boots to `ready`, console shell starts, no power off
- Actual: all found; `Power down` count 0; the source image's ESP `/wana` still holds only `bzImage`
- Result: **PASS**

## T4: Buildroot-built disk image in CI

- Run: [36115622561](https://github.com/Mudher84/wana-os/actions/runs/36115622561) (PR Mudher84/wana-os#2, commit `a2bfbd3`), job `toolchain, image, UEFI boot`, 21m50s
- `make image` now also builds GRUB 2 (x86_64-efi), `rootfs.ext4`, host genimage/mtools/dosfstools, and runs `post-image.sh`: success
- The earlier boot tests still pass: kernel config (49 options), kernel alone, kernel + initramfs
- `make disk-boot-test` (QEMU accel=kvm, OVMF, Buildroot `disk.img`, test copy with `wana.test=poweroff`):
  ```
  [BOOT] info: out/test/disk-test.img: /wana/test.cfg -> wana_args="wana.test=poweroff"
  [BOOT] info: found: BdsDxe: starting Boot
  [BOOT] info: found: \[BOOT\] info: loading Wana OS kernel
  [BOOT] info: found: Linux version 6\.18\.33-wana
  [BOOT] info: found: root=PARTUUID=5d9f6a52-7e1b-4c3a-9b8e-0a1a57a0a001
  [BOOT] info: found: Run /sbin/init as init process
  [BOOT] info: found: \[INIT\] info: early mounts: 7 ok, 0 failed
  [BOOT] info: found: \[INIT\] info: ready
  [BOOT] info: found: reboot: Power down
  [BOOT] boot test: PASS (log: out/logs/disk-boot.log)
  ```
- Artifact: `wana-image-<sha>` now includes `disk.img` (37.6 MB zipped), 14 days
- Result: **PASS**

## Phase 5 status

**PASS.** T1-T4 all pass. Wana OS boots from a disk the way real hardware
does: UEFI firmware, then GRUB on the ESP, then the kernel, then the ext4 root, then wana-init.

Not covered yet: real hardware (Phase 28), Secure Boot (Phase 23), a
writable root / A-B updates (later), the ISO (Phase 18).
