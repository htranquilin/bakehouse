#!/usr/bin/env bash
# Build the macOS .icns and a PNG set from app-icon.svg.
#
# Requires: rsvg-convert (brew install librsvg) and iconutil (ships with macOS).
# Run from this directory:  ./build-icons.sh

set -euo pipefail

SRC="app-icon.svg"
OUT="build"
ICONSET="$OUT/Bakehouse.iconset"

command -v rsvg-convert >/dev/null || { echo "rsvg-convert not found. brew install librsvg"; exit 1; }

rm -rf "$OUT"
mkdir -p "$ICONSET" "$OUT/png"

# Apple's required iconset members: name -> pixel size
render() {
  rsvg-convert -w "$2" -h "$2" "$SRC" -o "$ICONSET/$1"
}

render icon_16x16.png        16
render icon_16x16@2x.png     32
render icon_32x32.png        32
render icon_32x32@2x.png     64
render icon_128x128.png      128
render icon_128x128@2x.png   256
render icon_256x256.png      256
render icon_256x256@2x.png   512
render icon_512x512.png      512
render icon_512x512@2x.png   1024

if command -v iconutil >/dev/null; then
  iconutil -c icns "$ICONSET" -o "$OUT/Bakehouse.icns"
  echo "Wrote $OUT/Bakehouse.icns"
else
  echo "iconutil not found (not on macOS?). Iconset left at $ICONSET"
fi

# Loose PNGs for the docs site, README, and Tauri/Electron bundlers.
for size in 32 64 128 256 512 1024; do
  rsvg-convert -w "$size" -h "$size" "$SRC" -o "$OUT/png/bakehouse-${size}.png"
done

# Tauri expects these exact names in src-tauri/icons/
cp "$OUT/png/bakehouse-32.png"   "$OUT/png/32x32.png"
cp "$OUT/png/bakehouse-128.png"  "$OUT/png/128x128.png"
cp "$OUT/png/bakehouse-256.png"  "$OUT/png/128x128@2x.png"
cp "$OUT/png/bakehouse-512.png"  "$OUT/png/icon.png"

echo "Wrote PNGs to $OUT/png"
