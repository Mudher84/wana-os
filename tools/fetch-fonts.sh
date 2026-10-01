#!/bin/sh
# Fetches Wana's pinned UI fonts for host builds and tests into DIR
# (default out/fonts), under the names the image uses. URLs and commit come
# from the wana-fonts Buildroot package; each file must match the hash in
# platform/package/wana-fonts/wana-fonts.hash, the same check Buildroot
# does. Writes DIR/SHA256SUMS like the image. Idempotent.
set -eu

cd "$(dirname "$0")/.."
DIR=${1:-out/fonts}
PKG=platform/package/wana-fonts
COMMIT=$(sed -n 's/^WANA_FONTS_VERSION = //p' $PKG/wana-fonts.mk)
URL=https://raw.githubusercontent.com/google/fonts/$COMMIT/ofl
mkdir -p "$DIR"

# upstream path (URL-encoded as in wana-fonts.mk) -> installed name
fetch() {
    enc=$(basename "$1")
    want=$(awk -v f="$enc" '$1 == "sha256" && $3 == f { print $2 }' $PKG/wana-fonts.hash)
    [ -n "$want" ] || { echo "[RENDER] error: no hash for $enc in wana-fonts.hash" >&2; exit 1; }
    if [ -f "$DIR/$2" ] && [ "$(sha256sum "$DIR/$2" | cut -d' ' -f1)" = "$want" ]; then
        return
    fi
    curl -sSf --retry 3 -o "$DIR/$2.part" "$URL/$1"
    got=$(sha256sum "$DIR/$2.part" | cut -d' ' -f1)
    if [ "$got" != "$want" ]; then
        rm -f "$DIR/$2.part"
        echo "[RENDER] error: $enc: sha256 $got, expected $want" >&2
        exit 1
    fi
    mv "$DIR/$2.part" "$DIR/$2"
    echo "[RENDER] info: fetched $2 (sha256 $want)"
}

fetch notonaskharabic/NotoNaskhArabic%5Bwght%5D.ttf NotoNaskhArabic-VF.ttf
fetch notosansarabic/NotoSansArabic%5Bwdth%2Cwght%5D.ttf NotoSansArabic-VF.ttf
fetch notosans/NotoSans%5Bwdth%2Cwght%5D.ttf NotoSans-VF.ttf
(cd "$DIR" && sha256sum NotoNaskhArabic-VF.ttf NotoSansArabic-VF.ttf NotoSans-VF.ttf > SHA256SUMS)
