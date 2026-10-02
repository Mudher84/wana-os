#!/bin/sh
# Buildroot post-build script (runs after all packages are installed).
# $1 = target directory.
set -eu
target=$1

# wana-init is PID 1 for both boot paths:
#   /init       initramfs (the kernel runs /init first)
#   /sbin/init  disk root filesystem (kernel default)
if [ ! -x "$target/usr/sbin/wana-init" ]; then
    echo "[INIT] error: $target/usr/sbin/wana-init missing" >&2
    exit 1
fi
ln -sfn usr/sbin/wana-init "$target/init"
ln -sfn ../usr/sbin/wana-init "$target/sbin/init"
echo "[INIT] info: /init and /sbin/init -> /usr/sbin/wana-init"

# eudev: log errors only. At the default level ("info") udevd writes its
# notices to /dev/kmsg, which the console prints in the middle of other
# programs' lines (CI run 36191209030). Errors are still logged. eudev
# also writes one unconditional "starting eudev-<version>" line; the boot
# test harness repairs such splits (tools/console_lines.py).
conf="$target/etc/udev/udev.conf"
if [ -f "$conf" ]; then
    sed -i -e '/^#\{0,1\} *udev_log=/d' "$conf"
    echo 'udev_log="err"' >> "$conf"
    echo "[INIT] info: $conf: udev_log=\"err\""
fi

# Installed release identity. This is deterministic source metadata used by the
# updater to avoid re-downloading the release that is already installed.
repo=$(cd "$(dirname "$0")/../../.." && pwd)
version=$(sed -n '/^\[workspace\.package\]/,/^\[/ s/^version = "\([^"]*\)"/\1/p' "$repo/Cargo.toml" | head -n1)
commit=$(git -C "$repo" rev-parse HEAD)
mkdir -p "$target/etc"
cat > "$target/etc/wana-release" <<EOF
WANA-RELEASE-1
version=$version
commit=$commit
EOF
chmod 0644 "$target/etc/wana-release"

# Bind-mount targets for persistent Data must exist in the immutable system
# image before PID 1 mounts the Data volume over them.
mkdir -p "$target/data" "$target/home/wana" "$target/var/lib/wana" \
    "$target/var/lib/waydroid" "$target/var/lib/bluetooth"
echo "[BUILD] info: /etc/wana-release version=$version commit=$commit"
