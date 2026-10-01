#!/bin/sh
# Fetch the pinned Buildroot release into $1 and verify its commit.
# Usage: tools/fetch-buildroot.sh <dest-dir>
set -eu

root=$(cd "$(dirname "$0")/.." && pwd)
. "$root/platform/buildroot.env"
dest=$1

if [ -d "$dest/.git" ]; then
    have=$(git -C "$dest" rev-parse HEAD)
    if [ "$have" = "$BUILDROOT_COMMIT" ]; then
        echo "[BOOT] info: buildroot $BUILDROOT_VERSION already present at $dest"
        exit 0
    fi
    echo "[BOOT] error: $dest is at $have, expected $BUILDROOT_COMMIT ($BUILDROOT_VERSION)" >&2
    echo "[BOOT] error: remove $dest and retry" >&2
    exit 1
fi

echo "[BOOT] info: fetching buildroot $BUILDROOT_VERSION from $BUILDROOT_URL"
git -c advice.detachedHead=false clone --quiet --depth 1 \
    --branch "$BUILDROOT_VERSION" "$BUILDROOT_URL" "$dest"

have=$(git -C "$dest" rev-parse HEAD)
if [ "$have" != "$BUILDROOT_COMMIT" ]; then
    echo "[BOOT] error: tag $BUILDROOT_VERSION resolved to $have, expected $BUILDROOT_COMMIT" >&2
    rm -rf "$dest"
    exit 1
fi
echo "[BOOT] info: buildroot $BUILDROOT_VERSION verified at $have"
