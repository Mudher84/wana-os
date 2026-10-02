#!/bin/sh
# Repository hygiene checks. Run by `make check` and CI.
# Fails if build output is tracked, a required doc is missing, the Cargo
# lockfile is missing, or a tracked file exceeds the size limit.
set -eu

cd "$(dirname "$0")/.."
fail=0
err() { echo "[CHECK] error: $*" >&2; fail=1; }

for f in README.md docs/ARCHITECTURE.md docs/ROADMAP.md docs/BUILDING.md \
         docs/CONTRIBUTING.md docs/FINAL-VALIDATION.md \
         docs/decisions/0001-wayland-protocol-layer.md \
         docs/decisions/0002-text-stack.md \
         docs/decisions/0003-shell-surfaces.md \
         docs/decisions/0004-shell-control.md \
         Cargo.toml Cargo.lock rust-toolchain.toml \
         platform/buildroot.env platform/external.desc platform/configs/wana_x86_64_defconfig; do
    [ -f "$f" ] || err "required file missing: $f"
done

# Every implemented roadmap phase keeps its evidence/report slot even while the
# status remains IN PROGRESS awaiting final validation.
i=1
while [ "$i" -le 39 ]; do
    n=$(printf '%02d' "$i")
    [ -f "docs/test-reports/phase-$n.md" ] ||
        err "required phase report missing: docs/test-reports/phase-$n.md"
    i=$((i + 1))
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

defconfig_root_size=$(sed -n 's/^BR2_TARGET_ROOTFS_EXT2_SIZE="\([^"]*\)"/\1/p' \
    platform/configs/wana_x86_64_defconfig)
[ -n "$defconfig_root_size" ] || err "defconfig rootfs size is missing"
[ "$defconfig_root_size" = "$WANA_ROOT_SLOT_SIZE" ] ||
    err "rootfs size $defconfig_root_size does not match A/B slot size $WANA_ROOT_SLOT_SIZE"

# Source files must not land with explicit implementation placeholders. Keep
# historical prose/audit documents out of this check.
placeholders=$(git grep -n -E 'TODO|FIXME|todo!\(|unimplemented!\(' -- \
    '*.rs' '*.sh' '*.py' '*.mk' '*.yml' '*.service' 2>/dev/null || true)
[ -z "$placeholders" ] || err "implementation placeholders are tracked:
$placeholders"

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
