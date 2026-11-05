#!/bin/sh
# Installs the latest protonaut release after checking its SHA-256, and its
# build provenance when a signed-in gh CLI is available. --with-deps also
# installs the apt packages a test run needs.
set -eu

repo=Mahan-Imanian/protonaut
asset=protonaut-x86_64-unknown-linux-gnu
base="https://github.com/$repo/releases/latest/download"
install_dir="${PROTONAUT_INSTALL_DIR:-/usr/local/bin}"
with_deps=false

for arg in "$@"; do
  case "$arg" in
    --with-deps) with_deps=true ;;
    *) echo "unknown option: $arg" >&2; exit 2 ;;
  esac
done

[ "$(uname -s)-$(uname -m)" = "Linux-x86_64" ] || { echo "Protonaut needs x86-64 Linux." >&2; exit 1; }

sudo_cmd=""
[ "$(id -u)" -eq 0 ] || sudo_cmd=sudo

if [ "$with_deps" = true ]; then
  command -v apt-get >/dev/null || { echo "--with-deps needs apt-get. See docs/reference/cli.md for the package list." >&2; exit 1; }
  $sudo_cmd dpkg --add-architecture i386
  $sudo_cmd apt-get update
  $sudo_cmd apt-get install -y --no-install-recommends \
    xserver-xorg-core xserver-xorg-video-dummy x11-xserver-utils x11-utils imagemagick \
    mesa-vulkan-drivers python3 curl tesseract-ocr tesseract-ocr-eng \
    libc6:i386 libx11-6:i386 libxext6:i386 libxrandr2:i386 libxi6:i386 libxcursor1:i386 \
    libxinerama1:i386 libxcomposite1:i386 libxrender1:i386 libxfixes3:i386 libfreetype6:i386 \
    libfontconfig1:i386 libvulkan1:i386 mesa-vulkan-drivers:i386 libgl1:i386 libgl1-mesa-dri:i386
fi

dir=$(mktemp -d)
trap 'rm -rf "$dir"' EXIT
cd "$dir"

curl -fsSL --proto '=https' -o "$asset" "$base/$asset"
curl -fsSL --proto '=https' -o SHA256SUMS "$base/SHA256SUMS"
grep " $asset\$" SHA256SUMS | sha256sum -c -

if command -v gh >/dev/null && gh auth status >/dev/null 2>&1; then
  gh attestation verify "$asset" --repo "$repo"
else
  echo "Skipped the provenance check, which needs a signed-in gh CLI. The checksum passed."
fi

$sudo_cmd install -m 0755 "$asset" "$install_dir/protonaut"
protonaut --version
echo "Next: sudo protonaut setup && protonaut proton install stable"
