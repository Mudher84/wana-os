# Test report: Phase 2 (Build environment)

- Buildroot: 2026.02.3 (LTS series), commit `679b9ead7620bbf193620d1ebf56f53c1764d37a`
- Resolved toolchain: Buildroot internal, glibc, gcc 14.3.0, Linux 6.18 headers, C++ enabled

## T1: Pinned Buildroot fetch and verification (local)

- Command: `make buildroot-src`
- Expected: clone of tag `2026.02.3` whose HEAD equals `BUILDROOT_COMMIT`
- Actual: `[BOOT] info: buildroot 2026.02.3 verified at 679b9ead…`
- Result: **PASS**

## T2: Defconfig loads and round-trips (local)

- Command: `make config-check`
- Expected: `savedefconfig` output identical to `platform/configs/wana_x86_64_defconfig`
- Actual: the first run failed. The symbols were valid, but their order was not
  canonical. After `make savedefconfig` it prints
  `[CHECK] defconfig loads and round-trips unchanged: PASS`, exit 0. The resolved
  `.config` contains `BR2_TOOLCHAIN_BUILDROOT_GLIBC=y`, `BR2_GCC_VERSION="14.3.0"`,
  `BR2_KERNEL_HEADERS_6_18=y`, `BR2_REPRODUCIBLE=y`.
- Result: **PASS**

## T3: Defconfig check rejects an invalid symbol (local)

- Command: append `BR2_KERNEL_HEADERS_6_99=y` to the defconfig, run `make config-check`
- Expected: FAIL naming the dropped symbol
- Actual: diff shows `-BR2_KERNEL_HEADERS_6_99=y`, then `[CHECK] ... FAIL`, exit 1
- Result: **PASS**

## T4: Toolchain smoke script logic (local)

- Command: `tools/smoke-toolchain.sh` against a stand-in layout that uses the host gcc, and against an empty directory
- Expected: PASS for the stand-in; exit 1 for the empty directory
- Actual: as expected
- Result: **PASS**. This checks only the script's logic, not the Buildroot toolchain.

## T5: Cross toolchain build in CI

- Command: workflow `buildroot`, job `cross toolchain` (`make toolchain`, then `tools/smoke-toolchain.sh`)
- Run: [36104757968](https://github.com/Mudher84/wana-os/actions/runs/36104757968) on `develop`, commit `2a15b98`
- Expected: toolchain builds; C and C++ test programs are x86-64 ELF using `/lib64/ld-linux-x86-64.so.2`
- Actual: `Build toolchain` succeeded in 24m05s (06:51:54 to 07:15:59, cold caches). Packages built: host-gcc-initial 14.3.0, linux-headers 6.18.33, glibc 2.42-67-g4ebd33dd, host-gcc-final 14.3.0. Smoke test output:
  ```
  [CHECK] info: x86_64-buildroot-linux-gnu-gcc.br_real (Buildroot 2026.02.3) 14.3.0
  [CHECK] info: t-c: ELF 64-bit LSB pie executable, x86-64, version 1 (SYSV), dynamically linked, interpreter /lib64/ld-linux-x86-64.so.2, for GNU/Linux 6.18.0, not stripped
  [CHECK] info: t-cxx: ELF 64-bit LSB pie executable, x86-64, version 1 (SYSV), dynamically linked, interpreter /lib64/ld-linux-x86-64.so.2, for GNU/Linux 6.18.0, not stripped
  [CHECK] toolchain smoke test: PASS
  ```
- Artifacts: `toolchain-build-log` (full build log and the resolved `.config`), retained 14 days
- Result: **PASS**

## Phase 2 status

**PASS.** T1-T5 all pass.
