# Test report: Phase 3 (Linux kernel)

- Kernel: Linux 6.18.33 (the tarball SHA-256 is pinned by Buildroot: `6f16ff30…fb782`)
- Base config: `x86_64_defconfig`, plus `platform/board/x86_64/linux.fragment` (49 options)
- Commit under test: `b2710d6`

## T1: Fragment options reach the final kernel config (local)

- Source: `v6.18.33` from `github.com/gregkh/linux` (same release as the Buildroot tarball)
- Command: `make x86_64_defconfig`, `merge_config.sh -m .config linux.fragment`, `make olddefconfig`, `tools/check-kernel-config.sh`
- Expected: all fragment options present with the requested values
- Actual: `[KERNEL] kernel config check: 49 options verified: PASS`
- Result: **PASS**

## T2: Config check detects a dropped option (local)

- Command: the same, with `CONFIG_DRM_AMDGPU_SI=y` added (its dependency `DRM_AMDGPU` is not enabled)
- Expected: FAIL naming the option
- Actual: `wanted 'CONFIG_DRM_AMDGPU_SI=y', got '(absent)'`, `FAIL`, exit 1
- Result: **PASS**

## T3: Kernel build (local, host gcc 13.3)

- Command: `make -j4 bzImage`
- Expected: `bzImage` produced
- Actual: `Kernel: arch/x86/boot/bzImage is ready`, 8m56s. Output of `file`:
  `Linux kernel x86 boot executable bzImage, version 6.18.33-wana`.
  sha256 `1579f6d8b615e1fe0e947bc893ee80ada6b3ce51386d662c0672b8c2061458bd` (not reproducible: embeds the build host and time)
- Result: **PASS**

## T4: UEFI boot under QEMU + OVMF (local)

- Command: `tools/qemu-boot-test.sh --kernel bzImage --expect 'Linux version 6\.18\.33-wana' --expect 'efi: EFI v[0-9]' --expect 'VFS: Unable to mount root fs'`
- Environment: QEMU 8.2.2 (q35, TCG, no KVM), OVMF `OVMF_CODE_4M.fd` (EDK II 2024.02)
- Expected: kernel boots through UEFI and stops at the root filesystem mount (Phase 3 has no rootfs)
- Actual: all three patterns found. Log excerpts:
  - `[0.000000] efi: EFI v2.7 by Ubuntu distribution of EDK II`
  - `[0.485252] LSM: initializing lsm=capability,landlock,selinux`
  - `[2.041056] [drm] Initialized bochs-drm 1.0.0 for 0000:00:01.0 on minor 0`
  - `[2.874906] Kernel panic - not syncing: VFS: Unable to mount root fs on unknown-block(0,0)`
- Result: **PASS**

## T5: Boot test detects a missing stage (local)

- Command: the same boot, with `--expect 'Run /sbin/init as init process'`
- Expected: FAIL (there is no init yet)
- Actual: `missing: Run /sbin/init as init process`, the last 20 console lines printed, `FAIL`, exit 1
- Result: **PASS**

## T6: Buildroot kernel build, config check, and UEFI boot in CI

- Run: [36105396143](https://github.com/Mudher84/wana-os/actions/runs/36105396143) (PR Mudher84/wana-os#2, commit `b2710d6`), job `toolchain, kernel, UEFI boot`
- Commands: `make kernel`, `make kernel-config-check`, `make kernel-boot-test`
- Expected: Buildroot builds bzImage with its own cross toolchain; all fragment options present; kernel boots under OVMF and reaches the root mount
- Actual:
  - `Build kernel` succeeded in 12m08s (07:21:53 to 07:34:01)
  - `Check kernel config` succeeded (the script exits 1 on any missing option)
  - Boot, with KVM on the runner:
    ```
    [BOOT] info: qemu accel=kvm firmware=/usr/share/OVMF/OVMF_CODE_4M.fd kernel=.../images/bzImage timeout=180s
    [BOOT] info: found: Linux version 6\.18\.33-wana
    [BOOT] info: found: efi: EFI v[0-9]
    [BOOT] info: found: VFS: Unable to mount root fs
    [BOOT] boot test: PASS (log: out/logs/kernel-boot.log)
    ```
- Artifact: `wana-kernel-<sha>` (bzImage, kernel `.config`, Buildroot `.config`, build and boot logs), retained 14 days
- Result: **PASS**

## Phase 3 status

**PASS.** T1-T6 all pass. The kernel boots through UEFI and stops exactly where
Phase 4 has to supply a root filesystem and `init`.
