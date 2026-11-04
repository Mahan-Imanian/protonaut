# SteamOS Preflight

**Test your Windows game build under Proton in CI, on every pull request,
without a Steam Deck.**

SteamOS Preflight is for developers who ship a Windows build on Steam and
want to know when a change breaks it on SteamOS. Valve's compatibility
review only happens after you submit a build and takes about a week, and it
is repeated after Proton updates; until now, catching a regression earlier
meant testing by hand on Linux or a Deck. SteamOS Preflight runs your build
under pinned Proton releases on an ordinary GitHub runner, watches it start,
render and keep running, and reports what broke with screenshots and logs.

Questions and bug reports: [open an issue](https://github.com/Mahan-Imanian/steamos-preflight/issues/new/choose).
Security problems: see [SECURITY.md](SECURITY.md).

## Quick start

In a GitHub workflow on `ubuntu-24.04`, after your build step:

```yaml
- uses: Mahan-Imanian/steamos-preflight@<commit SHA>
  with:
    build: dist/Windows
    exe: MyGame.exe
```

Or on your own Ubuntu 24.04 machine, after installing the system packages
listed in [Run it locally](#run-it-locally):

```bash
cargo install --locked --git https://github.com/Mahan-Imanian/steamos-preflight steamos-preflight
```

```bash
steamos-preflight test dist/Windows --exe MyGame.exe --install
```

```text
FAIL       SP-RUN-002     Game keeps running during the observation window [ge-proton11-12] — Crashed after 4.1 s.
FAILED: 10 pass, 2 fail, 0 warning, 2 unknown, 5 not tested. Report: preflight-results/report.html
```

> Independent community project, **not affiliated with Valve**. It cannot
> predict a Steam Deck Verified rating, and software rendering does not test
> GPU performance or how the game feels on a controller.

## What it finds

| | Examples |
|---|---|
| **Launch and stability** | crashes (unhandled exceptions), exits with an error, missing DLLs, a game that never opens a window |
| **Rendering** | black or empty frames, letterboxing at 1280x800, extra "unsupported hardware" style windows |
| **The build itself** | 32/64-bit and ARM64 executables, DLL imports nothing provides, files that differ only by case, anti-cheat files, launchers, Media Foundation video |
| **Steamworks hints** | whether the Steam Input and on-screen keyboard APIs are imported |

Each result is `PASS`, `FAIL`, `WARNING`, `UNKNOWN` or `NOT_TESTED`, and
says whether it was observed, read from the files, or guessed by a
heuristic. Missing evidence is never reported as a pass. The full list is in
[docs/reference/checks.md](docs/reference/checks.md).

**What it cannot tell you:** frame rate on real hardware, real GPU driver
behaviour, controller feel, touch input, suspend and resume, battery use,
and anything that needs the Steam client. Those still need a device.

![HTML report from a real run: a letterboxed frame flagged as a warning, with the captured screenshot](docs/images/report.png)

*`report.html` from the letterbox fixture under two runtimes. GE-Proton10-34
shows "Capture: not working", so its screenshot checks are `NOT_TESTED`.*

## How it works

1. Your build runs under a pinned, checksum-verified
   [GE-Proton](https://github.com/GloriousEggroll/proton-ge-custom) release
   with Mesa's software Vulkan driver on a headless Xorg display at
   1280x800, 60 Hz.
2. Before your game, a control program that draws a known colour runs under
   the same runtime. If its frame cannot be captured, screenshot checks are
   reported as `NOT_TESTED` rather than as a false "black screen".
3. Your game runs for `observe-secs` as an unprivileged user with no
   network and no CI token; windows, screenshots and the Wine log are
   recorded; every process is stopped afterwards.
4. Rules turn that evidence into results: `report.json` (versioned schema),
   an HTML report, JUnit XML, and a PR comment.

More in [docs/architecture.md](docs/architecture.md).

## Use it in GitHub Actions

```yaml
jobs:
  preflight:
    runs-on: ubuntu-24.04
    permissions:
      contents: read
    steps:
      - uses: actions/checkout@<sha> # v7
        with:
          persist-credentials: false
      - run: ./build-windows.sh # your build step
      - uses: Mahan-Imanian/steamos-preflight@<sha>
        with:
          build: dist/Windows
          exe: MyGame.exe
          runtimes: stable    # or several: "ge-proton11-12 ge-proton10-34"
          observe-secs: "20"
```

The step fails on any `FAIL`, writes the report to the job summary, and
uploads screenshots and logs as an artifact. For PR comments, add the
`comment` action in a second job that never runs the build:
[docs/reference/action.md](docs/reference/action.md) has inputs, outputs, the
comment setup and a [complete example workflow](docs/examples/github-actions.yml).

## Run it locally

On Ubuntu 24.04 (or a similar x86-64 Linux):

```bash
sudo apt-get install --no-install-recommends xserver-xorg-core xserver-xorg-video-dummy x11-xserver-utils x11-utils imagemagick mesa-vulkan-drivers python3 curl
```

```bash
cargo install --locked --git https://github.com/Mahan-Imanian/steamos-preflight steamos-preflight
```

```bash
sudo steamos-preflight setup
```

```bash
steamos-preflight proton install stable
```

```bash
steamos-preflight test dist/Windows --exe MyGame.exe
```

`steamos-preflight diagnose` explains anything missing. `steamos-preflight
inspect dist/Windows --exe MyGame.exe` runs only the checks that read the
files, in about a second, without Proton. Results land in
`preflight-results/` (open `report.html`). Exit codes: 0 no failures,
1 failures, 2 usage error, 3 environment error. Details:
[docs/reference/cli.md](docs/reference/cli.md).

On your own machine the game runs as your user; the CLI clears its
environment but is not a sandbox. Only run builds you trust.

## Example output

```text
PASS       SP-PE-003      Imported DLLs are available — All 4 imported DLLs are shipped or provided by the runtime.
FAIL       SP-RUN-002     Game keeps running during the observation window [ge-proton11-12] — Crashed after 4.1 s.
FAIL       SP-RUN-003     No crash signatures in the runtime log [ge-proton11-12] — 1 crash signature(s) in the Wine log.
NOT_TESTED SP-HW-001      Frame rate at default settings [ge-proton11-12] — Valve's 30 fps target needs real Deck or Steam Machine hardware; this run uses CPU software rendering.

FAILED: 10 pass, 2 fail, 0 warning, 2 unknown, 5 not tested. Report: preflight-results/report.html
```

An excerpt of `steamos-preflight test` on the crash fixture from [`fixtures/`](fixtures/README.md); the
end-to-end suite checks that every fixture (crash, startup failure, missing
asset, missing DLL, black screen, letterbox, extra window) is reported
correctly under each supported runtime.

## Security

Game builds are untrusted code. In the action they run as a separate user,
in a network namespace with no network by default, with an empty
environment, from a copy of the build. Runtime downloads are pinned by
SHA-256, result files are copied out without following symlinks, PR
comments are re-rendered from validated JSON, and every artifact is scrubbed
of host paths. What this does and does not protect against:
[docs/security/threat-model.md](docs/security/threat-model.md).

## Status

Version 0.1.0, **experimental**. The CLI, report schema v1 and action
inputs are documented and versioned, but may still change before 1.0; every
change is listed in the [changelog](CHANGELOG.md). Verified runtimes and
known gaps: [docs/compatibility/runtimes.md](docs/compatibility/runtimes.md).
How the checks map to Valve's review criteria, what is missing and what
needs hardware: [docs/roadmap.md](docs/roadmap.md).

Besides the synthetic fixtures, CI runs the real open-source game OpenTTD
15.3 through the action. That testing found a known blind spot: a game that
shows a fatal error **inside its own window** (OpenTTD without its graphics
set) is currently reported as passing.

## Contributing

`cargo test --workspace` runs without Proton or a Steam Deck. See
[CONTRIBUTING.md](CONTRIBUTING.md) and [docs/development.md](docs/development.md).
Reports comparing results with a real device are especially useful.

## License

Licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))
- MIT license ([LICENSE-MIT](LICENSE-MIT))

at your option.

Unless you explicitly state otherwise, any contribution intentionally
submitted for inclusion in this project by you, as defined in the Apache-2.0
license, shall be dual licensed as above, without any additional terms or
conditions.

GE-Proton, Wine, DXVK and Mesa are separate projects under their own
licenses. SteamOS Preflight downloads them at test time and does not
redistribute them.
