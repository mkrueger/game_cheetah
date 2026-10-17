#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."

for tool in inkscape magick; do
    if ! command -v "$tool" >/dev/null 2>&1; then
        printf 'Required icon generation tool is missing: %s\n' "$tool" >&2
        exit 1
    fi
done

rendered="$(mktemp --suffix=.png)"
trap 'rm -f -- "$rendered"' EXIT

inkscape assets/cheetah.svg --export-type=png --export-width=1024 --export-filename="$rendered"
magick "$rendered" -resize 512x512 PNG32:assets/cheetah.png
magick "$rendered" -resize 128x128 PNG32:build/linux/128x128.png
magick "$rendered" -resize 256x256 PNG32:build/linux/256x256.png
cp build/linux/256x256.png build/mac/128x128@2x.png
magick "$rendered" -define icon:auto-resize=256,128,64,48,32,24,16 build/windows/app.ico

inkscape assets/cheetah.svg --export-id=mascot --export-id-only --export-background-opacity=0 \
    --export-type=png --export-width=1024 --export-filename="$rendered"
magick "$rendered" -resize 480x480 -background none -gravity center -extent 512x512 PNG32:assets/cheetah-menu.png
