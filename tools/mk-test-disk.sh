#!/bin/sh
# Make a throwaway copy of a Wana disk image whose GRUB boots immediately
# with extra kernel arguments, by writing /wana/test.cfg onto the ESP.
# The source image is never modified.
# Usage: tools/mk-test-disk.sh <disk.img> <test-copy.img> "<kernel args>"
set -eu

root=$(cd "$(dirname "$0")/.." && pwd)
. "$root/platform/board/x86_64/disk.env"
src=$1 dst=$2 args=$3

cp --sparse=always "$src" "$dst"
cfg=$(mktemp)
trap 'rm -f "$cfg"' EXIT
printf 'set timeout=0\nset wana_args="%s"\n' "$args" > "$cfg"
mcopy -o -i "$dst@@$WANA_ESP_OFFSET" "$cfg" ::/wana/test.cfg
echo "[BOOT] info: $dst: /wana/test.cfg -> wana_args=\"$args\""
