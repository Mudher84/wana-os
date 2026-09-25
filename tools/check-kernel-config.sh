#!/bin/sh
# Verify that every option in a kernel config fragment survived into the
# final kernel .config with the requested value. Kconfig drops options whose
# dependencies are unmet without failing the build; this makes that loud.
# Usage: tools/check-kernel-config.sh <fragment> <final-.config>
set -eu

fragment=$1
config=$2
[ -f "$config" ] || { echo "[KERNEL] error: no kernel config at $config" >&2; exit 1; }

fail=0
checked=0
while IFS= read -r line; do
    case "$line" in
        CONFIG_*=*)
            want=$line ;;
        "# CONFIG_"*" is not set")
            want=$line ;;
        *) continue ;;
    esac
    checked=$((checked + 1))
    if ! grep -qxF -- "$want" "$config"; then
        name=$(printf '%s\n' "$want" | sed -E 's/^# (CONFIG_[A-Za-z0-9_]+) is not set$/\1/; s/=.*//')
        have=$(grep -E "^$name=|^# $name is not set" "$config" || echo "(absent)")
        echo "[KERNEL] error: wanted '$want', got '$have'" >&2
        fail=1
    fi
done < "$fragment"

if [ "$fail" -ne 0 ]; then
    echo "[KERNEL] kernel config check: FAIL" >&2
    exit 1
fi
echo "[KERNEL] kernel config check: $checked options verified: PASS"
