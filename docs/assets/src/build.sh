#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")"
F='<style>
@font-face{font-family:"Saira Condensed";font-weight:800;src:url(../fonts/saira-condensed-latin-800-normal.woff2)}
@font-face{font-family:"Saira Condensed";font-weight:600;src:url(../fonts/saira-condensed-latin-600-normal.woff2)}
@font-face{font-family:"IBM Plex Mono";font-weight:400;src:url(../fonts/ibm-plex-mono-latin-400-normal.woff2)}
@font-face{font-family:"IBM Plex Mono";font-weight:500;src:url(../fonts/ibm-plex-mono-latin-500-normal.woff2)}
html,body{margin:0;background:transparent}
</style>'
out=$(mktemp -d -p .)
trap 'rm -rf "$out"' EXIT
page() { { echo '<!doctype html><meta charset="utf-8">'; echo "$F"; sed -e "/{{PANEL}}/{r panel.html" -e "d}" "$1"; } | sed 's/\.\.\/fonts/..\/..\/fonts/' > "$2"; }
page wordmark-dark.tpl "$out/d.html"; ./render.sh "$out/d.html" ../wordmark-dark.png 1040 220
page wordmark-light.tpl "$out/l.html"; ./render.sh "$out/l.html" ../wordmark-light.png 1040 220
page social.tpl "$out/s.html"; ./render.sh "$out/s.html" ../social-preview.png 1280 640
page card.tpl "$out/c.html"; ./render.sh "$out/c.html" ../profile-card.png 2000 620
page mark.tpl "$out/m.html"; ./render.sh "$out/m.html" "$out/m.png" 256 256; ffmpeg -y -loglevel error -i "$out/m.png" -vf scale=64:64 ../favicon.png
