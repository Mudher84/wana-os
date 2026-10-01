#!/bin/sh
# Build a deterministic A/B update test disk from an already-built Wana image.
# No network access is used. The staged payload is the exact current build and
# is injected into the persistent Data filesystem; GRUB will enter update mode
# automatically because /var/lib/wana/update/pending/update.txt exists.
set -eu

if [ "$#" -ne 5 ]; then
    echo "usage: $0 IMAGES_DIR HOST_GENIMAGE OUT_DISK VERSION COMMIT" >&2
    exit 2
fi

images=$1
genimage=$2
out_disk=$3
version=$4
commit=$5

case "$version" in
    ''|*[!0-9A-Za-z._+-]*) echo "[UPDATE-TEST] invalid version" >&2; exit 2 ;;
esac
case "$commit" in
    ???????*) ;;
    *) echo "[UPDATE-TEST] invalid commit" >&2; exit 2 ;;
esac

for f in rootfs.ext4 rootfs.ext4.zst rootfs.cpio.zst bzImage data.ext4 genimage.cfg; do
    [ -s "$images/$f" ] || { echo "[UPDATE-TEST] missing $images/$f" >&2; exit 1; }
done
[ -d "$images/efi-part" ] || { echo "[UPDATE-TEST] missing $images/efi-part" >&2; exit 1; }
[ -x "$genimage" ] || { echo "[UPDATE-TEST] missing genimage $genimage" >&2; exit 1; }
command -v debugfs >/dev/null 2>&1 || { echo "[UPDATE-TEST] debugfs is required" >&2; exit 1; }

tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
work="$tmp/images"
mkdir -p "$work"
cp "$images/rootfs.ext4" "$work/rootfs.ext4"
cp "$images/rootfs.ext4.zst" "$work/rootfs.ext4.zst"
cp "$images/rootfs.cpio.zst" "$work/rootfs.cpio.zst"
cp "$images/bzImage" "$work/bzImage"
cp "$images/data.ext4" "$work/data.ext4"
cp "$images/genimage.cfg" "$work/genimage.cfg"
cp -a "$images/efi-part" "$work/efi-part"

sha() { sha256sum "$1" | awk '{print $1}'; }
size() { stat -c%s "$1"; }

cat > "$tmp/update.txt" <<EOF
WANA-UPDATE-1
version=$version
commit=$commit
rootfs=rootfs.ext4.zst
rootfs_size=$(size "$work/rootfs.ext4.zst")
rootfs_sha256=$(sha "$work/rootfs.ext4.zst")
kernel=bzImage
kernel_size=$(size "$work/bzImage")
kernel_sha256=$(sha "$work/bzImage")
initrd=rootfs.cpio.zst
initrd_size=$(size "$work/rootfs.cpio.zst")
initrd_sha256=$(sha "$work/rootfs.cpio.zst")
EOF

mkdir_ext4() {
    path=$1
    if ! debugfs -R "stat $path" "$work/data.ext4" >/dev/null 2>&1; then
        debugfs -w -R "mkdir $path" "$work/data.ext4" >/dev/null 2>&1
    fi
}
mkdir_ext4 /var
mkdir_ext4 /var/lib
mkdir_ext4 /var/lib/wana
mkdir_ext4 /var/lib/wana/update
mkdir_ext4 /var/lib/wana/update/pending

for pair in \
    "$tmp/update.txt:update.txt" \
    "$work/rootfs.ext4.zst:rootfs.ext4.zst" \
    "$work/bzImage:bzImage" \
    "$work/rootfs.cpio.zst:rootfs.cpio.zst"
do
    src=$(printf "%s" "$pair" | cut -d: -f1)
    name=$(printf "%s" "$pair" | cut -d: -f2)
    debugfs -w -R "write $src /var/lib/wana/update/pending/$name" "$work/data.ext4" >/dev/null 2>&1
done

out="$tmp/out"
root="$tmp/root"
gwork="$tmp/genwork"
mkdir -p "$out" "$root" "$gwork"
"$genimage" \
    --rootpath "$root" \
    --tmppath "$gwork" \
    --inputpath "$work" \
    --outputpath "$out" \
    --config "$work/genimage.cfg"

[ -s "$out/disk.img" ] || { echo "[UPDATE-TEST] genimage did not produce disk.img" >&2; exit 1; }
mkdir -p "$(dirname "$out_disk")"
cp --sparse=always "$out/disk.img" "$out_disk"
echo "[UPDATE-TEST] staged disk ready: $out_disk version=$version commit=$commit"
