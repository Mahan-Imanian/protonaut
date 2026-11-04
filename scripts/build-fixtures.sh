#!/usr/bin/env bash
# Builds the Windows test fixtures in fixtures/bin from fixtures/src.
# Requires mingw-w64 (x86_64-w64-mingw32-g++ and dlltool). The flags remove
# timestamps and build ids so the output is reproducible; CI rebuilds the
# fixtures and fails if the committed binaries differ.
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
src="$root/fixtures/src"
out="$root/fixtures/bin"
tmp="$(mktemp -d)"
trap 'rm -rf -- "$tmp"' EXIT

cxx="${CXX:-x86_64-w64-mingw32-g++}"
dlltool="${DLLTOOL:-x86_64-w64-mingw32-dlltool}"
flags=(-O2 -static -s -std=c++17 -mwindows -fno-ident -ffile-prefix-map="$root"=. -Wl,--no-insert-timestamp -Wl,--build-id=none)
libs=(-ld3d11 -ldxgi)

mkdir -p "$out"
"$dlltool" -d "$src/fx_absent.def" -l "$tmp/libfx_absent.a"

build() {
  local name="$1" mode="$2"; shift 2
  "$cxx" "${flags[@]}" -DFX_MODE="$mode" -o "$out/$name.exe" "$src/fixture.cpp" "$@" "${libs[@]}"
}

build fx_render_ok 1
build fx_black 2
build fx_crash 3
build fx_exit_fail 4
build fx_missing_asset 5
build fx_letterbox 6
build fx_dialog 7
build fx_missing_dll 8 "$tmp/libfx_absent.a"
build fx_scene 9

( cd "$out" && sha256sum ./*.exe > SHA256SUMS )
echo "built $(ls "$out"/*.exe | wc -l) fixtures into fixtures/bin"
