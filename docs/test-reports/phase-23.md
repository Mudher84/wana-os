# Phase 23 — Security hardening

Status: **IN PROGRESS**

Phase 23 is not considered complete merely because hardening options exist in
source. The exit gate must prove the security posture in a booted Wana OS guest.

## Implemented hardening

- Debug root console shell is disabled by default. It is available only when
  the kernel command line explicitly contains `wana.shell=1`.
- Buildroot root password login is disabled, so the default empty root
  password cannot be used as an authentication path.
- `/proc` and `/sys` are mounted `nosuid,nodev,noexec`.
- Volatile tmpfs mounts `/dev/shm`, `/run`, and `/tmp` are mounted
  `nosuid,nodev,noexec`.
- Live ISO media is mounted read-only with `nosuid,nodev,noexec`.
- PID 1 reads `/proc/self/mountinfo` after early mounts and fails the secure
  boot state if any required runtime mount flag is missing.
- Existing kernel isolation primitives remain enabled: namespaces, cgroups,
  seccomp/filtering, and Landlock.
- Buildroot's resolved configuration is gated for disabled root password login,
  PIE, strong stack protector,
  full RELRO, and FORTIFY_SOURCE so a future default change cannot silently
  weaken userspace binaries.
- Permission policy and audit stores reject symlinks, insecure modes, and
  ownership that does not match the effective UID.

## Exit test

`make security-hardening-boot-test` boots the real Buildroot kernel and
initramfs under QEMU + OVMF and requires all of the following:

1. all seven early mounts succeed;
2. PID 1 reports 5/5 hardened runtime mounts;
3. the debug console shell reports disabled without `wana.shell=1`;
4. the system reaches `[INIT] info: ready`;
5. the guest powers off cleanly.

The Buildroot workflow runs this gate after the Phase 22 permissions boot test.

The workflow's config job also runs `make security-config-check` before the
full image build.

## Evidence

Pending a green Buildroot run on the Phase 23 runtime-verification branch.
Do not mark this phase PASS until that run completes successfully and its run
ID is recorded here.
