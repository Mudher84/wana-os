#!/bin/sh
# Make a throwaway copy of the Wana Live ISO with /live/test.cfg added.
# The source ISO is never modified.
# Usage: tools/mk-test-live-iso.sh <Wana-OS-Live.iso> <test-copy.iso> "<kernel args>"
set -eu

src=$1
dst=$2
args=$3
xorriso=${XORRISO:-xorriso}

[ -f "$src" ] || { echo "[BOOT] error: Live ISO not found: $src" >&2; exit 2; }
command -v "$xorriso" >/dev/null 2>&1 || [ -x "$xorriso" ] || {
    echo "[BOOT] error: xorriso not found: $xorriso" >&2
    exit 2
}

cfg=$(mktemp)
trap 'rm -f "$cfg"' EXIT
printf 'set timeout=0\nset wana_args="%s"\n' "$args" > "$cfg"

rm -f "$dst"
"$xorriso" -indev "$src" -outdev "$dst" \
    -boot_image any replay \
    -map "$cfg" /live/test.cfg \
    -commit -end >/dev/null

[ -s "$dst" ] || { echo "[BOOT] error: failed to create test Live ISO: $dst" >&2; exit 1; }
echo "[BOOT] info: $dst: /live/test.cfg -> wana_args=\"$args\""
