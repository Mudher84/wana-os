#!/bin/sh
# Boot a kernel under QEMU + OVMF (UEFI) with a serial console and check the
# console log for expected lines. Exits 0 only if every --expect pattern
# (extended regex) appears before QEMU exits or the timeout hits.
#
# Usage:
#   tools/qemu-boot-test.sh --kernel bzImage [--initrd file] [--append "args"]
#                           [--timeout secs] [--log file] --expect REGEX...
set -eu

kernel= initrd= append= timeout=120 log=qemu-serial.log
expects=$(mktemp)
trap 'rm -f "$expects" "${vars:-}"' EXIT

while [ $# -gt 0 ]; do
    case "$1" in
        --kernel) kernel=$2; shift 2 ;;
        --initrd) initrd=$2; shift 2 ;;
        --append) append=$2; shift 2 ;;
        --timeout) timeout=$2; shift 2 ;;
        --log) log=$2; shift 2 ;;
        --expect) printf '%s\n' "$2" >> "$expects"; shift 2 ;;
        *) echo "[BOOT] error: unknown argument $1" >&2; exit 2 ;;
    esac
done
[ -f "$kernel" ] || { echo "[BOOT] error: kernel not found: $kernel" >&2; exit 2; }
[ -s "$expects" ] || { echo "[BOOT] error: at least one --expect is required" >&2; exit 2; }

ovmf_code=
for f in /usr/share/OVMF/OVMF_CODE_4M.fd /usr/share/OVMF/OVMF_CODE.fd /usr/share/edk2/x64/OVMF_CODE.4m.fd; do
    [ -f "$f" ] && { ovmf_code=$f; break; }
done
[ -n "$ovmf_code" ] || { echo "[BOOT] error: OVMF firmware not found (install ovmf)" >&2; exit 2; }
ovmf_vars_src=$(dirname "$ovmf_code")/$(basename "$ovmf_code" | sed 's/CODE/VARS/')
vars=$(mktemp)
cp "$ovmf_vars_src" "$vars"

accel=tcg
[ -w /dev/kvm ] && accel=kvm

echo "[BOOT] info: qemu accel=$accel firmware=$ovmf_code kernel=$kernel timeout=${timeout}s"
set -- -machine q35,accel=$accel -m 1024 -smp 2 -nographic -no-reboot \
    -drive if=pflash,format=raw,readonly=on,file="$ovmf_code" \
    -drive if=pflash,format=raw,file="$vars" \
    -kernel "$kernel" -append "console=ttyS0 panic=-1 $append"
[ -n "$initrd" ] && set -- "$@" -initrd "$initrd"

rc=0
timeout "$timeout" qemu-system-x86_64 "$@" > "$log" 2>&1 < /dev/null || rc=$?
[ "$rc" -eq 124 ] && echo "[BOOT] info: timeout reached after ${timeout}s"

fail=0
while IFS= read -r pattern; do
    if grep -Eq -- "$pattern" "$log"; then
        echo "[BOOT] info: found: $pattern"
    else
        echo "[BOOT] error: missing: $pattern" >&2
        fail=1
    fi
done < "$expects"

if [ "$fail" -ne 0 ]; then
    echo "[BOOT] last 20 console lines:" >&2
    tail -n 20 "$log" >&2
    echo "[BOOT] boot test: FAIL (log: $log)" >&2
    exit 1
fi
echo "[BOOT] boot test: PASS (log: $log)"
