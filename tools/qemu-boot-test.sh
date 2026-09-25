#!/bin/sh
# Boot a kernel under QEMU + OVMF (UEFI) with a serial console and check the
# console log for expected lines. Exits 0 only if every --expect pattern
# (extended regex) appears before QEMU exits or the timeout hits.
#
# Usage:
#   tools/qemu-boot-test.sh (--kernel bzImage [--initrd file] [--append "args"] | --disk disk.img)
#                           [--timeout secs] [--log file]
#                           [--input-after SECS TEXT] --expect REGEX...
#
# --disk boots a whole disk image through the firmware (virtio-blk,
# snapshot mode: the image is never written).
# --input-after types TEXT (printf escapes allowed, e.g. 'exit\n') on the
# serial console SECS seconds after QEMU starts.
set -eu

kernel= disk= initrd= append= timeout=120 log=qemu-serial.log
input_delay= input_text=
expects=$(mktemp)
trap 'rm -f "$expects" "${vars:-}"' EXIT

while [ $# -gt 0 ]; do
    case "$1" in
        --kernel) kernel=$2; shift 2 ;;
        --disk) disk=$2; shift 2 ;;
        --initrd) initrd=$2; shift 2 ;;
        --append) append=$2; shift 2 ;;
        --timeout) timeout=$2; shift 2 ;;
        --log) log=$2; shift 2 ;;
        --expect) printf '%s\n' "$2" >> "$expects"; shift 2 ;;
        --input-after) input_delay=$2; input_text=$3; shift 3 ;;
        *) echo "[BOOT] error: unknown argument $1" >&2; exit 2 ;;
    esac
done
if [ -n "$disk" ]; then
    [ -f "$disk" ] || { echo "[BOOT] error: disk image not found: $disk" >&2; exit 2; }
else
    [ -f "$kernel" ] || { echo "[BOOT] error: kernel not found: $kernel" >&2; exit 2; }
fi
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

echo "[BOOT] info: qemu accel=$accel firmware=$ovmf_code ${disk:+disk=$disk}${kernel:+kernel=$kernel} timeout=${timeout}s"
set -- -machine q35,accel=$accel -m 1024 -smp 2 -nographic -no-reboot \
    -drive if=pflash,format=raw,readonly=on,file="$ovmf_code" \
    -drive if=pflash,format=raw,file="$vars"
if [ -n "$disk" ]; then
    set -- "$@" -drive file="$disk",if=virtio,format=raw,snapshot=on
else
    set -- "$@" -kernel "$kernel" -append "console=ttyS0 panic=-1 $append"
    [ -n "$initrd" ] && set -- "$@" -initrd "$initrd"
fi

rc=0
if [ -n "$input_delay" ]; then
    # Keep stdin open until the timeout so QEMU does not see EOF early.
    { sleep "$input_delay"; printf "$input_text"; sleep "$timeout"; } |
        timeout "$timeout" qemu-system-x86_64 "$@" > "$log" 2>&1 || rc=$?
else
    timeout "$timeout" qemu-system-x86_64 "$@" > "$log" 2>&1 < /dev/null || rc=$?
fi
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
