# Test fixtures

Small Windows programs with one known behaviour each. The test suite runs
them through `steamos-preflight` and checks that each behaviour is reported
correctly (`tests/e2e/expectations.toml`). No commercial game binaries or
assets are used anywhere in the tests.

| Binary | Behaviour | Used for |
|---|---|---|
| `fx_render_ok.exe` | Clears every frame to sRGB (26, 153, 51) | Capture control run (embedded in the CLI) |
| `fx_scene.exe` | Draws a grid of coloured tiles | A healthy game |
| `fx_scene32.exe` | `fx_scene` built as a 32-bit executable | 32-bit games (D3D11 through 32-bit DXVK) |
| `fx_black.exe` | Draws black frames | Black-screen warning |
| `fx_crash.exe` | Divides by zero after 180 frames (about 3 s) | Crash detection |
| `fx_exit_fail.exe` | Exits with code 3 before opening a window | Startup failure |
| `fx_missing_asset.exe` | Exits with code 4 when `assets/level0.dat` is missing | Missing game data |
| `fx_letterbox.exe` | Draws a 16:9 picture inside the screen, black bars above and below | Letterbox warning |
| `fx_dialog.exe` | Opens a small extra window next to the game | Dialog warning |
| `fx_text_error.exe` | Keeps running and draws "Fatal error: required data files are missing" in its window with GDI | In-window error text (SP-DISP-005) |
| `fx_missing_dll.exe` | Imports `fx_absent.dll`, which does not exist | Missing DLL, static and at runtime |
| `fx_probe.exe` | Acts like a hostile build: tries the network, the runner's files, CI tokens in `/proc`, `sudo`, and leaves a detached Linux process behind. Writes BLOCKED/OPEN per attempt to `probe-results.txt`, never file contents | Sandbox test in the end-to-end workflow |

All except the probe come from [`src/fixture.cpp`](src/fixture.cpp), one `FX_MODE` each; the probe is [`src/probe.c`](src/probe.c).
Every program also accepts `--seconds=N` (exit after N seconds) and
`--report` (write the D3D11 feature level and adapter name to `result.txt`).

## Rebuilding

```bash
sudo apt-get install --no-install-recommends g++-mingw-w64-x86-64 binutils-mingw-w64-x86-64 g++-mingw-w64-i686
```

```bash
scripts/build-fixtures.sh
```

The build strips timestamps, build ids and source paths, so two builds with
the same MinGW version produce identical files. `bin/SHA256SUMS` records
the hashes, and the `fixtures` CI job rebuilds on Ubuntu 24.04 and fails if
the committed binaries differ from the source.

## Licensing

The fixture sources are part of SteamOS Preflight (MIT OR Apache-2.0). The
binaries are statically linked with the MinGW-w64 runtime and GCC's
libstdc++, which the GCC Runtime Library Exception and the MinGW-w64
runtime licences allow to be distributed in compiled programs.
