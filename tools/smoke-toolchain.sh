#!/bin/sh
# Smoke-test the Buildroot cross toolchain: compile C and C++ programs and
# check the output is an x86-64 ELF linked against the Wana glibc loader.
# Usage: tools/smoke-toolchain.sh <buildroot-output-dir>
set -eu

host="$1/host"
cc=$(ls "$host"/bin/x86_64-*-linux-gnu-gcc 2>/dev/null | head -n1)
[ -n "$cc" ] || { echo "[CHECK] error: no cross gcc in $host/bin" >&2; exit 1; }
cxx=${cc%gcc}g++
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT

echo "[CHECK] info: $("$cc" --version | head -n1)"

printf '#include <stdio.h>\nint main(void){puts("wana");return 0;}\n' > "$tmp/t.c"
printf '#include <iostream>\nint main(){std::cout<<"wana\\n";}\n' > "$tmp/t.cpp"
"$cc" -O2 -o "$tmp/t-c" "$tmp/t.c"
"$cxx" -O2 -o "$tmp/t-cxx" "$tmp/t.cpp"

fail=0
for bin in "$tmp/t-c" "$tmp/t-cxx"; do
    desc=$(file -b "$bin")
    echo "[CHECK] info: $(basename "$bin"): $desc"
    case "$desc" in
        *"ELF 64-bit LSB"*"x86-64"*"interpreter /lib64/ld-linux-x86-64.so.2"*) ;;
        *) echo "[CHECK] error: unexpected binary type for $(basename "$bin")" >&2; fail=1 ;;
    esac
done
[ "$fail" -eq 0 ] || { echo "[CHECK] toolchain smoke test: FAIL" >&2; exit 1; }
echo "[CHECK] toolchain smoke test: PASS"
