#!/usr/bin/env bash
# APP-64: the home-screen icons — the menger mark (web/favicon.svg: the
# Sierpinski carpet, level 2, 27 units square) on an opaque tile of the
# paper colour (tokens.toml's --bg), as PNG, because iOS takes no SVG for
# Add to Home Screen and draws a letter without one.
#
#   scripts/icons.sh      writes frontend/icons/ (the app's build copies them
#                         to its root, src/shell.rs serves them at /) and
#                         landing/icons/apple-touch-icon.png (menger.sh)
#
# Each mark is a whole number of pixels per unit, centred, so its edges are
# crisp: apple-touch-icon 180 (the mark 60%: iOS rounds the corners and wants
# a margin), 192 and 512 for the manifest, and a maskable 512 whose mark fits
# the safe zone (a circle of 80% of the side). Needs rsvg-convert.
set -euo pipefail
cd "$(dirname "$0")/.."

PAPER='#fbfbf9'  # --bg, light
MARK='#1a5fb4'   # the favicon's fill, light
PATH_D=$(sed -n 's/.*<path d="\([^"]*\)".*/\1/p' web/favicon.svg)
[[ -n $PATH_D ]] || { echo "no mark in web/favicon.svg" >&2; exit 1; }

tile() { # SIDE UNIT_PX OUT
  local side=$1 px=$2 out=$3 mark off
  mark=$((27 * px)); off=$(((side - mark) / 2))
  printf '<svg xmlns="http://www.w3.org/2000/svg" width="%d" height="%d" viewBox="0 0 %d %d" shape-rendering="crispEdges"><rect width="%d" height="%d" fill="%s"/><g transform="translate(%d %d) scale(%d)"><path fill="%s" d="%s"/></g></svg>' \
    "$side" "$side" "$side" "$side" "$side" "$side" "$PAPER" "$off" "$off" "$px" "$MARK" "$PATH_D" \
    | rsvg-convert --format png --width "$side" --height "$side" > "$out"
}

mkdir -p frontend/icons landing/icons
tile 180 4 frontend/icons/apple-touch-icon.png
tile 192 4 frontend/icons/icon-192.png
tile 512 12 frontend/icons/icon-512.png
tile 512 10 frontend/icons/icon-512-maskable.png
# menger.sh is a page, not an app: the home-screen icon alone, no manifest.
cp frontend/icons/apple-touch-icon.png landing/icons/
echo "wrote frontend/icons/ and landing/icons/" >&2
