# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- `steamos-preflight` CLI: `test` runs a Windows build under pinned GE-Proton builds on a headless Xorg display with software Vulkan, `inspect` runs static checks only, plus `proton`, `report`, `diagnose`, `setup`, `init` and `schema` commands.
- Static checks for PE architecture, unresolved DLL imports, Steam Input and on-screen keyboard API use, case-colliding paths, anti-cheat files, launchers and video formats.
- Runtime checks for launch, crashes, early exits, window creation, black or flat frames, letterboxing and extra dialog windows, with a control fixture that verifies screen capture per runtime.
- Versioned `report.json` (schema v1) with Markdown, HTML and JUnit renderings; JSON Schemas for the report and `preflight.toml`.
- GitHub Action (`uses: Mahan-Imanian/steamos-preflight@<ref>`) that runs the build as an unprivileged user with no network and no CI token, uploads results and writes a job summary, plus a `comment` action that posts or updates one PR comment from `report.json`.
- Reproducible Windows test fixtures and an end-to-end suite covering crash, startup failure, missing asset, missing DLL, black screen, letterbox and dialog cases.
