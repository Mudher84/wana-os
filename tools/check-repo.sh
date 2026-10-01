#!/bin/sh
# Repository hygiene checks. Run by `make check` and CI.
# Fails if build output is tracked, a required doc is missing, the Cargo
# lockfile is missing, or a tracked file exceeds the size limit.
set -eu

cd "$(dirname "$0")/.."
fail=0
err() { echo "[CHECK] error: $*" >&2; fail=1; }

for f in README.md docs/ARCHITECTURE.md docs/ROADMAP.md docs/BUILDING.md \
         docs/CONTRIBUTING.md Cargo.toml Cargo.lock rust-toolchain.toml \
         platform/buildroot.env platform/external.desc platform/configs/wana_x86_64_defconfig; do
    [ -f "$f" ] || err "required file missing: $f"
done

# Build output must never be committed.
tracked_artifacts=$(git ls-files | grep -E '(^|/)(target|out|dl)/|\.(iso|img|qcow2)$' || true)
[ -z "$tracked_artifacts" ] || err "build artifacts are tracked:
$tracked_artifacts"

# Large binaries belong in release assets, not git.
big=$(git ls-files -z | xargs -0 -r stat -c '%s %n' 2>/dev/null | awk '$1 > 1048576')
[ -z "$big" ] || err "tracked files over 1 MiB:
$big"

# The ext4 identifiers used by the defconfig must match disk.env.
. platform/board/x86_64/disk.env
for id in "-U $WANA_ROOT_FS_UUID" "hash_seed=$WANA_ROOT_FS_HASH_SEED"; do
    grep -q -- "BR2_TARGET_ROOTFS_EXT2_MKFS_OPTIONS=.*$id" platform/configs/wana_x86_64_defconfig ||
        err "defconfig ext4 options do not contain '$id' from disk.env"
done

# Shell scripts must be executable.
for f in $(git ls-files 'tools/*.sh'); do
    [ -x "$f" ] || err "not executable: $f"
done

# The boot-test console line repair must keep behaving as specified.
python3 tools/console_lines.py --self-test >/dev/null || err "tools/console_lines.py --self-test failed"

if [ "$fail" -ne 0 ]; then
    echo "[CHECK] FAIL" >&2
    exit 1
fi
echo "[CHECK] repository checks: PASS"
