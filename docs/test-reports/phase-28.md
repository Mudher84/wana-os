# Phase 28 — Hardware compatibility

Status: **IN PROGRESS**

Phase 28 expands Wana OS beyond the VirtIO-only validation path and requires
real alternate device classes to boot and operate through the same native
userspace stack.

## Implemented

### Network
- Kernel: Intel e1000/e1000e, Realtek 8139CP/8139TOO and R8169 enabled.
- QEMU harness: selectable virtio, e1000 and rtl8139 network devices.
- Runtime gates use `wana-network` and udev for all three device families.

### Storage
- NVMe remains built into the kernel and now has an explicit boot test.
- The QEMU serial boot harness can attach the installed disk as virtio-blk or
  NVMe.
- The NVMe gate boots OVMF -> GRUB -> kernel -> ext4 using PARTUUID, proving
  the installed image is not tied to a virtio block-device name.
- AHCI/SATA and USB mass-storage drivers remain built in.

### Input
- i8042/AT keyboard and PS/2 mouse support is explicit in the kernel fragment.
- A dedicated PS/2 gate injects keyboard, pointer movement and a click through
  the existing udev -> libinput -> xkbcommon path without virtio-input.

### Graphics
- Kernel DRM drivers: i915, Nouveau and Radeon.
- Mesa Gallium: Nouveau and R600 in addition to the existing softpipe and
  VirGL paths.
- Kernel i915 remains enabled for Intel DRM/KMS discovery, but Mesa 26.0.1's
  Gallium i915 driver requires LLVM. Wana OS therefore does not claim Intel
  accelerated userspace support in Phase 28; enabling LLVM-backed Intel/AMD
  drivers is deferred to a measured integration with an explicit firmware and
  image-size policy.

## Exit gate

`make hardware-compatibility-test` must pass all four independent runtime
tests:

1. e1000 network discovery;
2. rtl8139 network discovery;
3. PS/2 keyboard + pointer input;
4. NVMe installed-disk boot.

The standard Buildroot kernel-config check also verifies every driver requested
by the kernel fragment. The Buildroot workflow runs the matrix after the native
desktop/application gates.

## Evidence

Pending CI/Buildroot/QEMU evidence for this branch. Phase 28 stays IN PROGRESS
until the matrix is green; modern Iris/RadeonSI support remains a documented
follow-up rather than an unsupported PASS claim.
