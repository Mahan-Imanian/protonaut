# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Security

- The action kills every process of the sandbox user before reading results, so a build cannot leave a detached process running into later steps.
- Runtimes are installed by root into a read-only cache, and the sandbox home is recreated for each use.
- A hostile test build checks in CI that the sandbox blocks the network, the runner's files, CI tokens in `/proc`, `sudo` and escaped processes.

### Changed

- Licensed under MIT OR Apache-2.0.
- The action installs a release binary for `vX.Y.Z` refs, checked against `SHA256SUMS` and its build provenance attestation, instead of building the CLI on every first use.
- OpenTTD screenshots (GPL-2.0 artwork) were removed from the documentation; image licences are listed in `docs/images/ATTRIBUTION.md`.

### Fixed

- 32-bit games failed to start because Wine's 32-bit side had no i386 host libraries. The action installs them, `diagnose` reports when they are missing, and a 32-bit fixture runs in the end-to-end suite.

### Added

- SP-DISP-005 reads the screen with OCR (Tesseract) and warns about error messages a game draws inside its own window. Summaries with warnings read "No failures, N warnings to review" instead of a plain pass.
- Real-game tests in CI: OpenTTD 15.3, a Godot 4.7.2 3D game exported with Vulkan and with Direct3D 12, and Chocolate Doom 3.0.1 (32-bit, Direct3D 9 through SDL2) with Freedoom.
- `docs/roadmap.md`: coverage of Valve's review criteria and the roadmap.
- `steamos-preflight` CLI: `test` runs a Windows build under pinned GE-Proton builds on a headless Xorg display with software Vulkan, `inspect` runs static checks only, plus `proton`, `report`, `diagnose`, `setup`, `init` and `schema` commands.
- Static checks for PE architecture, unresolved DLL imports, Steam Input and on-screen keyboard API use, case-colliding paths, anti-cheat files, launchers and video formats.
- Runtime checks for launch, crashes, early exits, window creation, black or flat frames, letterboxing and extra dialog windows, with a control fixture that verifies screen capture per runtime.
- Versioned `report.json` (schema v1) with Markdown, HTML and JUnit renderings; JSON Schemas for the report and `preflight.toml`.
- GitHub Action (`uses: Mahan-Imanian/steamos-preflight@<ref>`) that runs the build as an unprivileged user with no network and no CI token, uploads results and writes a job summary (`fail-job: "false"` keeps its outputs available), plus a `comment` action that posts or updates one PR comment from `report.json`.
- Reproducible Windows test fixtures and an end-to-end suite covering crash, startup failure, missing asset, missing DLL, black screen, letterbox and dialog cases.
