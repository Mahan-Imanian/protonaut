#!/usr/bin/env bash
# Usage: render.sh <page.html> <out.png> <width> <height>
# Headless Edge adds about 25 x 95 px of chrome to the window, so pad, then crop.
set -euo pipefail
edge="/c/Program Files (x86)/Microsoft/Edge/Application/msedge.exe"
profile=$(mktemp -d)
page=$(cygpath -m "$(realpath "$1")")
raw=$(mktemp -u).png
"$edge" --headless=new --disable-gpu --no-proxy-server --hide-scrollbars --force-device-scale-factor=1 \
  --user-data-dir="$(cygpath -w "$profile")" --window-size="$(($3 + 25)),$(($4 + 95))" \
  --default-background-color=00000000 --virtual-time-budget=4000 \
  --screenshot="$(cygpath -w "$raw")" "file:///$page" >/dev/null 2>&1
ffmpeg -y -loglevel error -i "$raw" -vf "crop=$3:$4:0:0" "$2"
rm -rf "$raw" "$profile"
