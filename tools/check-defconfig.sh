#!/bin/sh
# Verify the Wana defconfig loads and is in canonical savedefconfig form.
# A difference means Buildroot rejected, dropped or defaulted a symbol.
# Usage: tools/check-defconfig.sh <buildroot-make-command...>
set -eu

root=$(cd "$(dirname "$0")/.." && pwd)
defconfig="$root/platform/configs/wana_x86_64_defconfig"
tmp=$(mktemp)
trap 'rm -f "$tmp"' EXIT

"$@" wana_x86_64_defconfig >/dev/null
"$@" savedefconfig BR2_DEFCONFIG="$tmp" >/dev/null

if diff -u "$defconfig" "$tmp"; then
    echo "[CHECK] defconfig loads and round-trips unchanged: PASS"
else
    echo "[CHECK] defconfig differs from what Buildroot saved (diff above): FAIL" >&2
    exit 1
fi
