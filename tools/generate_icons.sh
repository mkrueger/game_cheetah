#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."

for tool in inkscape magick python3; do
    if ! command -v "$tool" >/dev/null 2>&1; then
        printf 'Required icon generation tool is missing: %s\n' "$tool" >&2
        exit 1
    fi
done

work="$(mktemp -d)"
trap 'rm -rf -- "$work"' EXIT
rendered="$work/rendered.png"
# Omit timestamps so regenerating unchanged artwork leaves the files byte-identical.
png=(-define png:exclude-chunks=date,time)

inkscape assets/cheetah.svg --export-type=png --export-width=1024 --export-filename="$rendered"
magick "$rendered" -resize 512x512 "${png[@]}" PNG32:assets/cheetah.png
magick "$rendered" -resize 128x128 "${png[@]}" PNG32:build/linux/128x128.png
magick "$rendered" -resize 256x256 "${png[@]}" PNG32:build/linux/256x256.png
cp build/linux/256x256.png build/mac/128x128@2x.png

# ImageMagick stores every ICO entry as an uncompressed BMP. Keep the
# conventional layout instead: a PNG-compressed 256 px entry plus BMP entries
# for the small sizes, which every Windows consumer understands.
small_icons=()
for size in 128 64 48 32 24 16; do
    magick "$rendered" -resize "${size}x${size}" "$work/$size.ico"
    small_icons+=("$work/$size.ico")
done
python3 tools/pack_ico.py build/windows/app.ico build/linux/256x256.png "${small_icons[@]}"

inkscape assets/cheetah.svg --export-id=mascot --export-id-only --export-background-opacity=0 \
    --export-type=png --export-width=1024 --export-filename="$rendered"
magick "$rendered" -resize 480x480 -background none -gravity center -extent 512x512 "${png[@]}" PNG32:assets/cheetah-menu.png
