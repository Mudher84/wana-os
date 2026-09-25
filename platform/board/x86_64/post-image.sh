#!/bin/sh
# Buildroot post-image script: assemble the UEFI-bootable disk image.
# Environment from Buildroot: BINARIES_DIR, BUILD_DIR, HOST_DIR.
# Usage outside Buildroot (tests): BINARIES_DIR=... GENIMAGE=genimage post-image.sh
set -eu

board=$(cd "$(dirname "$0")" && pwd)
. "$board/disk.env"
out=$BINARIES_DIR

for f in bzImage rootfs.ext4 efi-part/EFI/BOOT/bootx64.efi; do
    [ -f "$out/$f" ] || { echo "[BOOT] error: missing $out/$f" >&2; exit 1; }
done

# ESP contents: GRUB (installed by Buildroot), our grub.cfg, the kernel.
mkdir -p "$out/efi-part/wana"
cp "$out/bzImage" "$out/efi-part/wana/bzImage"
sed "s/__ROOT_PARTUUID__/$WANA_ROOT_PARTUUID/g" "$board/grub.cfg" > "$out/efi-part/EFI/BOOT/grub.cfg"
sed "s/__ROOT_PARTUUID__/$WANA_ROOT_PARTUUID/g" "$board/genimage.cfg" > "$out/genimage.cfg"

tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
mkdir -p "$tmp/root"
${GENIMAGE:-$HOST_DIR/bin/genimage} \
    --rootpath "$tmp/root" --tmppath "$tmp/work" \
    --inputpath "$out" --outputpath "$out" \
    --config "$out/genimage.cfg"
echo "[BOOT] info: disk image $out/disk.img (root PARTUUID $WANA_ROOT_PARTUUID)"
