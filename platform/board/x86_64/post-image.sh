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
cp "$out/bzImage" "$out/efi-part/wana/bzImage-A"
cp "$out/bzImage" "$out/efi-part/wana/bzImage-B"
sed -e "s/__ROOT_PARTUUID__/$WANA_ROOT_PARTUUID/g" \
    -e "s/__ROOT_A_PARTUUID__/$WANA_ROOT_A_PARTUUID/g" \
    -e "s/__ROOT_B_PARTUUID__/$WANA_ROOT_B_PARTUUID/g" \
    -e "s/__DATA_PARTUUID__/$WANA_DATA_PARTUUID/g" \
    -e "s/__DATA_FS_UUID__/$WANA_DATA_FS_UUID/g" \
    "$board/grub.cfg" > "$out/efi-part/EFI/BOOT/grub.cfg"
sed -e "s/__ROOT_PARTUUID__/$WANA_ROOT_PARTUUID/g" \
    -e "s/__ROOT_A_PARTUUID__/$WANA_ROOT_A_PARTUUID/g" \
    -e "s/__ROOT_B_PARTUUID__/$WANA_ROOT_B_PARTUUID/g" \
    -e "s/__ROOT_SLOT_SIZE__/$WANA_ROOT_SLOT_SIZE/g" \
    -e "s/__DATA_PARTUUID__/$WANA_DATA_PARTUUID/g" \
    -e "s/__ESP_PARTUUID__/$WANA_ESP_PARTUUID/g" \
    -e "s/__DISK_GUID__/$WANA_DISK_GUID/g" \
    -e "s/__ESP_VOLID__/$WANA_ESP_VOLID/g" \
    "$board/genimage.cfg" > "$out/genimage.cfg"

# Small reproducible seed for persistent state. It is the final partition and
# grows to the target disk on first normal boot.
mkfs="$HOST_DIR/sbin/mkfs.ext4"
[ -x "$mkfs" ] || mkfs="$HOST_DIR/sbin/mke2fs"
[ -x "$mkfs" ] || { echo "[BOOT] error: host mke2fs missing" >&2; exit 1; }
rm -f "$out/data.ext4"
truncate -s 64M "$out/data.ext4"
E2FSPROGS_FAKE_TIME="${SOURCE_DATE_EPOCH:-0}" "$mkfs" -q -F \
    -L WANA-DATA -U "$WANA_DATA_FS_UUID" -O ^64bit \
    -E "lazy_itable_init=0,lazy_journal_init=0,hash_seed=$WANA_DATA_FS_HASH_SEED" \
    "$out/data.ext4"
echo "[BOOT] info: persistent data seed $out/data.ext4"

# Reproducibility: FAT stores file times, so pin every ESP file to
# SOURCE_DATE_EPOCH (exported by Buildroot; the build's reference time).
if [ -n "${SOURCE_DATE_EPOCH:-}" ]; then
    find "$out/efi-part" -exec touch -h -d "@$SOURCE_DATE_EPOCH" {} +
fi

tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
mkdir -p "$tmp/root"
${GENIMAGE:-$HOST_DIR/bin/genimage} \
    --rootpath "$tmp/root" --tmppath "$tmp/work" \
    --inputpath "$out" --outputpath "$out" \
    --config "$out/genimage.cfg"
echo "[BOOT] info: disk image $out/disk.img (root PARTUUID $WANA_ROOT_PARTUUID)"

# Device-update payload: the exact ext4 root filesystem, compressed with the
# pinned host zstd. Single-threaded compression keeps the byte stream stable.
zstd="$HOST_DIR/bin/zstd"
[ -x "$zstd" ] || { echo "[UPDATE] error: host zstd missing: $zstd" >&2; exit 1; }
rm -f "$out/rootfs.ext4.zst"
"$zstd" -q -19 -T1 -f "$out/rootfs.ext4" -o "$out/rootfs.ext4.zst"
echo "[UPDATE] info: rootfs payload $out/rootfs.ext4.zst"

BOARD_DIR="$board" BINARIES_DIR="$out" HOST_DIR="$HOST_DIR" \
    SOURCE_DATE_EPOCH="${SOURCE_DATE_EPOCH:-}" \
    "$board/../../../tools/build-live-iso.sh"
