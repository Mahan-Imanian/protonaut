# Runtimes and the test environment

## Verified runtimes

| Id | Build | SHA-256 (tarball) | Capture works | Notes |
|---|---|---|---|---|
| `ge-proton11-12` (`stable`) | GE-Proton11-12 x86_64 | `0595159e…5d70` | yes | All checks run |
| `ge-proton10-34` | GE-Proton10-34 | `51c580b6…773d` | **no** | DXVK presents frames that X screen capture does not see; screenshot checks are `NOT_TESTED` |

"Capture works" is measured on every run by the control fixture; this table
records what the end-to-end suite observed on Ubuntu 24.04 with Mesa 25.2
(llvmpipe). With GE-Proton10-34 the same program captured correctly when
rendered through wined3d (OpenGL), so the gap is in its Vulkan presentation
path on this display, not in the game.

**GE-Proton is not Valve's Proton.** It is a community build by
GloriousEggroll that follows Valve's Proton closely but adds patches (extra
media codecs, game fixes, newer Wine and DXVK commits). A game can behave
differently under Valve's Proton on a Steam Deck, and a PASS here says
nothing about Valve's own build.

Valve's Proton builds are distributed through Steam. On 4 November 2026 a
GitHub-hosted runner tried anonymous SteamCMD downloads
(`+login anonymous +app_update`):

| Steam app | What it is | Result |
|---|---|---|
| 1493710 | Proton Experimental | `Failed to install app '1493710' (Missing configuration)` |
| 2805730 | Proton 9.0 | `Failed to install app '2805730' (No subscription)` |
| 1628350 | Steam Linux Runtime 3.0 (sniper), the container Proton runs in | Installed (797 MB) |

So Valve's Proton needs a logged-in Steam account. Storing Steam credentials
in CI for an unprivileged test tool is not something this project will ask
users to do, and Valve's builds are not redistributable through this
project's cache, so they are not supported. Building Proton from Valve's
source (BSD-3-Clause) is possible but takes hours and a container
toolchain; it is not planned for 0.1.

## Environment

| Part | Used | Why |
|---|---|---|
| Runner | GitHub-hosted `ubuntu-24.04`, x86-64 | Tested; no GPU |
| Vulkan | Mesa lavapipe (CPU), forced with `VK_DRIVER_FILES` | Same results on machines with or without a GPU |
| Display | Xorg with the dummy driver, 60 Hz modelines | Xvfb reports a 0 Hz refresh rate, which makes DXVK 2.7.1 divide by zero in `dxgi.dll` |
| Crash handling | `WINEDLLOVERRIDES=winedbg.exe=d` | Otherwise Wine starts `winedbg --auto` on a crash and the run looks like a hang |
| Prefix | Fresh per runtime, created by the control run, then copied | The game's observation window does not include ~8 s of prefix creation |

## Timing (observed on GitHub-hosted `ubuntu-24.04`, November 2026)

| Step | Time |
|---|---|
| First use of the action in a job (packages, `cargo build`, runtime download, one test) | 235 s |
| Each further use in the same job | 31 s |
| `cargo build --release` of the CLI | 75-101 s |
| Download, verify and install one runtime (~0.5 GB) | 13-14 s |
| One fixture under one runtime, `observe-secs` 10 | 12-29 s (control run ~8 s included) |

GE-Proton10-34's capture failure reproduced on the GitHub runner as well as
locally.

## Adding a runtime

1. Add an entry to `crates/runtime/runtimes.toml` with the release URL and
   the SHA-256 that the release page lists.
2. Run `scripts/e2e.py` with the new id (see [development](../development.md)).
3. Add a row to the table above with what you observed.
