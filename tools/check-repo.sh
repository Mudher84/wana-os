#!/bin/sh
# Repository hygiene checks. Run by `make check` and CI.
# Fails if build output is tracked, a required doc is missing, the Cargo
# lockfile is missing, or a tracked file exceeds the size limit.
set -eu

cd "$(dirname "$0")/.."
fail=0
err() { echo "[CHECK] error: $*" >&2; fail=1; }

for f in README.md docs/ARCHITECTURE.md docs/ROADMAP.md docs/BUILDING.md \
         docs/CONTRIBUTING.md Cargo.toml Cargo.lock rust-toolchain.toml; do
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

# Shell scripts must be executable.
for f in $(git ls-files 'tools/*.sh'); do
    [ -x "$f" ] || err "not executable: $f"
done

if [ "$fail" -ne 0 ]; then
    echo "[CHECK] FAIL" >&2
    exit 1
fi
echo "[CHECK] repository checks: PASS"
