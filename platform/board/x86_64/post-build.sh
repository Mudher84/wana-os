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
