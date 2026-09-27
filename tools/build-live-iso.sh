#!/bin/sh
# Build the reproducible Wana OS UEFI Live ISO from Buildroot artifacts.
set -eu

out=${BINARIES_DIR:?BINARIES_DIR is required}
host=${HOST_DIR:?HOST_DIR is required}
board=${BOARD_DIR:-$(cd "$(dirname "$0")/../platform/board/x86_64" && pwd)}

for f in bzImage rootfs.cpio.zst efi-part/EFI/BOOT/bootx64.efi; do
    [ -f "$out/$f" ] || { echo "[BOOT] error: Live ISO missing $out/$f" >&2; exit 1; }
done

mkfs="$host/sbin/mkfs.fat"
[ -x "$mkfs" ] || mkfs="$host/sbin/mkfs.vfat"
mcopy="$host/bin/mcopy"
mmd="$host/bin/mmd"
xorriso="$host/bin/xorriso"
for p in "$mkfs" "$mcopy" "$mmd" "$xorriso"; do
    [ -x "$p" ] || { echo "[BOOT] error: Live ISO tool missing: $p" >&2; exit 1; }
done

tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
root="$tmp/iso"
mkdir -p "$root/live" "$root/boot"
cp "$out/bzImage" "$root/live/bzImage"
cp "$out/rootfs.cpio.zst" "$root/live/rootfs.cpio.zst"

# UEFI El Torito image: only GRUB + its Live config. Kernel/initramfs stay on
# ISO9660 so the EFI image remains small.
efi="$root/boot/efi.img"
truncate -s 8M "$efi"
"$mkfs" --invariant -F 32 -n WANA-LIVE -i 574C4956 "$efi" >/dev/null
"$mmd" -i "$efi" ::/EFI ::/EFI/BOOT
"$mcopy" -o -i "$efi" "$out/efi-part/EFI/BOOT/bootx64.efi" ::/EFI/BOOT/bootx64.efi
"$mcopy" -o -i "$efi" "$board/grub-live.cfg" ::/EFI/BOOT/grub.cfg

if [ -n "${SOURCE_DATE_EPOCH:-}" ]; then
    find "$root" -exec touch -h -d "@$SOURCE_DATE_EPOCH" {} +
    iso_date=$(date -u -d "@$SOURCE_DATE_EPOCH" +%Y%m%d%H%M%S00)
    date_opt="--modification-date=$iso_date"
else
    date_opt=
fi

iso="$out/Wana-OS-Live.iso"
rm -f "$iso"
"$xorriso" -as mkisofs -quiet     -iso-level 3 -full-iso9660-filenames     -volid WANA-LIVE     $date_opt     -e boot/efi.img -no-emul-boot     -o "$iso" "$root"

echo "[BOOT] info: Live ISO $iso"
