# Coverage and roadmap

How SteamOS Preflight's checks line up with what Valve reviews, what is
verified, what is only a heuristic, what is missing, and what can never run
on a CI runner. Valve's criteria are taken from the Steamworks
[compatibility review documentation](https://partner.steamgames.com/doc/steamhardware/compat).

**Status words used below.** *Verified*: implemented and proven by the
fixtures or real games in CI to detect the problem. *Heuristic*: implemented,
but can be wrong in either direction, and reported as such. *Missing*: could
be automated on a CI runner, not built yet. *Hardware*: needs a real device.

## Valve's criteria

| Valve criterion | What SteamOS Preflight does | Status | Evidence / gap |
|---|---|---|---|
| **Proton: no blocking bugs** (game starts and runs) | SP-RUN-001/002/003: launch, stays up for the observation window, no crash signatures | Verified | Crash, startup failure, missing asset and missing DLL fixtures; OpenTTD 15.3 runs 40 s under two runtimes |
| Proton: middleware it does not support (anti-cheat) | SP-AC-001 flags known anti-cheat files | Heuristic | Detection only; cannot know whether the vendor's Proton support is enabled |
| Proton: middleware (media codecs) | SP-MEDIA-001 flags formats usually decoded by Media Foundation | Heuristic | Static only; playback is not verified at runtime |
| **Display: supported resolution (1280x800 or 1280x720)** | Runs at 1280x800; SP-DISP-003 reports a game window smaller than the display (observed) or black bars (heuristic) | Partly verified | Window size verified with OpenTTD (960x600 window reported). Whether the game's own resolution setting defaults correctly is not read |
| Display: smallest text ≥ 9 px at 1280x800 | SP-HW-003, always NOT_TESTED | Missing | Needs OCR on the screenshots |
| **Input: full controller support in the default configuration** | SP-HW-002, always NOT_TESTED | Missing | Needs injected gamepad input and a check that the game reacts |
| Input: controller glyphs match the device | SP-STEAM-001 reports Steam Input flat-API imports | Heuristic | `UNKNOWN` for interface-based Steam Input and for engines that load it from a plugin. Glyphs on screen are not read |
| Input: text entry opens the on-screen keyboard | SP-STEAM-002 reports the Steamworks text-input imports | Heuristic | Same limits as above |
| **Seamlessness: no "unsupported OS/GPU" warnings** | SP-DISP-004 flags small OS-level windows | Heuristic, partly verified | Caught a real "Fatal Application Failure" window (OpenTTD under GE-Proton10-34). **Misses messages a game draws inside its own window**, such as OpenTTD's in-game "no compatible GPU found" (shown because of software rendering) |
| Seamlessness: launchers are controller-navigable | SP-LAUNCH-001 flags launcher-like executables | Heuristic | Launcher navigation is not tested |
| **Performance: 30 fps at default settings** | SP-HW-001, always NOT_TESTED | Hardware | CPU software rendering says nothing about Deck or Steam Machine frame rates |

Beyond Valve's list, SteamOS Preflight also checks PE architecture
(SP-PE-002), unresolved DLL imports (SP-PE-003, verified exactly against
`objdump` for OpenTTD's 14 imports) and case-colliding files (SP-FS-001).

## Known false negative found by real-game testing

OpenTTD without its base graphics set shows "Missing graphics: OpenTTD
requires graphics to function" **inside its own window** and keeps running.
SteamOS Preflight reported the run as passing: every current runtime check
is satisfied (process alive, window present, frame not black). Any game
that reports a fatal problem in its own UI instead of crashing or opening an
OS dialog is missed the same way. This is the main reason for items 1 and 2
below.

## Roadmap, in priority order

1. **Baseline screenshot comparison.** Compare each run's frames with the
   last accepted run on the main branch and warn on large differences. It
   would have caught the missing-graphics build, works for any engine, and
   needs no game-specific knowledge. Open question: animated title screens
   need a tolerant comparison or a fixed capture time.
2. **OCR on screenshots.** Measure the smallest text height (Valve's 9 px
   rule) and flag words such as "error", "missing", "unsupported" or "GPU".
   Heuristic by nature; would have caught both OpenTTD messages above.
3. **Game arguments in the action** (already in the CLI), so studios can
   start a test map or a benchmark mode directly.
4. **Scripted controller input.** A virtual gamepad (Linux `uinput`) driven
   by a short script, with "did the screen change" as the observation.
   Needs a design for giving the sandbox user access to one virtual device
   without broader device access.
5. **Prebuilt release binaries for the action**, removing the 75-101 s
   `cargo build` from every first use.
6. **gamescope as the display**, closer to SteamOS's compositor (it would
   also scale windowed games the way the Deck does).
7. **More runtimes** as GE-Proton releases, each added only after the
   end-to-end suite passes.

## Future integrations (not planned until there is demand)

- **Real hardware backend**: run the same build on a Steam Deck or Steam
  Machine dev kit (for example over SSH or Valve's devkit service) and
  produce the same `RunObservation`, so performance and input checks can run
  for real. The architecture already separates evidence gathering from the
  rules for this (docs/architecture.md).
- **Valve's own Proton builds**, which are only distributed through Steam
  and need a Steam login.
- **Steamworks without a Steam client.** Games that require the Steam API
  to initialise cannot be tested today. Emulating the API raises licensing
  and terms-of-service questions that need an answer before any work.

## Hardware only

Frame rate and frame pacing, GPU driver behaviour, thermal throttling,
battery use, suspend and resume, touchscreen input, the physical feel of the
controls, and anything that depends on the Steam client overlay. A software
renderer on a CI runner cannot stand in for these, and SteamOS Preflight
reports them as `NOT_TESTED` rather than guessing.
