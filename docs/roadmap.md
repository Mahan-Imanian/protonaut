# Coverage and roadmap

How Protonaut's checks line up with what Valve reviews, what is
verified, what is only a heuristic, what is missing, and what can never run
on a CI runner. Valve's criteria are taken from the Steamworks
[compatibility review documentation](https://partner.steamgames.com/doc/steamhardware/compat).

**Status words used below.** *Verified*: implemented and proven by the
fixtures or real games in CI to detect the problem. *Heuristic*: implemented,
but can be wrong in either direction, and reported as such. *Missing*: could
be automated on a CI runner, not built yet. *Hardware*: needs a real device.

## Valve's criteria

| Valve criterion | What Protonaut does | Status | Evidence / gap |
|---|---|---|---|
| **Proton: no blocking bugs** (game starts and runs) | PN-RUN-001/002/003: launch, stays up for the observation window, no crash signatures | Verified | Crash, startup failure, missing asset and missing DLL fixtures; OpenTTD 15.3 and a Godot 4.7.2 game (Vulkan and Direct3D 12 exports) run 40 s under two runtimes ([real-game results](compatibility/real-games.md)) |
| Proton: middleware it does not support (anti-cheat) | PN-AC-001 flags known anti-cheat files | Heuristic | Detection only; cannot know whether the vendor's Proton support is enabled |
| Proton: middleware (media codecs) | PN-MEDIA-001 flags formats usually decoded by Media Foundation | Heuristic | Static only; playback is not verified at runtime |
| **Display: supported resolution (1280x800 or 1280x720)** | Runs at 1280x800; PN-DISP-003 reports a game window smaller than the display (observed) or black bars (heuristic) | Partly verified | Window size verified with OpenTTD (960x600) and Godot (1152x648). Whether the game's own resolution setting defaults correctly is not read |
| Display: smallest text ≥ 9 px at 1280x800 | PN-HW-003, always NOT_TESTED | Missing | OCR now runs (PN-DISP-005), but text height is not measured |
| **Input: full controller support in the default configuration** | PN-HW-002, always NOT_TESTED | Missing | Needs injected gamepad input and a check that the game reacts |
| Input: controller glyphs match the device | PN-STEAM-001 reports Steam Input flat-API imports | Heuristic | `UNKNOWN` for interface-based Steam Input and for engines that load it from a plugin. Glyphs on screen are not read |
| Input: text entry opens the on-screen keyboard | PN-STEAM-002 reports the Steamworks text-input imports | Heuristic | Same limits as above |
| **Seamlessness: no "unsupported OS/GPU" warnings** | PN-DISP-004 flags small OS-level windows; PN-DISP-005 reads the screen with OCR and flags error language | Heuristic, partly verified | PN-DISP-004 caught a real "Fatal Application Failure" window (OpenTTD under GE-Proton10-34). PN-DISP-005 catches OpenTTD's in-window "Missing graphics" message and a fixture's in-window fatal error; it reads only words, so a warning with stylised text or an icon-only message can be missed |
| Seamlessness: launchers are controller-navigable | PN-LAUNCH-001 flags launcher-like executables | Heuristic | Launcher navigation is not tested |
| **Performance: 30 fps at default settings** | PN-HW-001, always NOT_TESTED | Hardware | CPU software rendering says nothing about Deck or Steam Machine frame rates |

Beyond Valve's list, Protonaut also checks PE architecture
(PN-PE-002), unresolved DLL imports (PN-PE-003, verified exactly against
`objdump` for OpenTTD's 14 imports) and case-colliding files (PN-FS-001).

## Errors a game draws inside its own window

OpenTTD without its base graphics set shows "Missing graphics: OpenTTD
requires graphics to function" **inside its own window** and keeps running,
so every process and window check passes. Before 0.1.0 this was reported as
a clean pass. PN-DISP-005 now reads the representative frame with Tesseract
OCR and reports error language as a WARNING, and the summary reads "No
failures, N warnings to review" instead of a plain pass. OCR is a
heuristic: stylised fonts, very small text or a message shown only before
the capture can still be missed, which is why a screenshot comparison is
item 1 below.

## Roadmap, in priority order

1. **Baseline screenshot comparison.** Compare each run's frames with the
   last accepted run on the main branch and warn on large differences. It
   would have caught the missing-graphics build, works for any engine, and
   needs no game-specific knowledge. Open question: animated title screens
   need a tolerant comparison or a fixed capture time.
2. **Text height from OCR.** PN-DISP-005 already runs OCR for error
   language; measuring the smallest text height would cover Valve's 9 px
   rule.
3. **Game arguments in the action** (already in the CLI), so studios can
   start a test map or a benchmark mode directly.
4. **Scripted controller input.** A virtual gamepad (Linux `uinput`) driven
   by a short script, with "did the screen change" as the observation.
   Needs a design for giving the sandbox user access to one virtual device
   without broader device access.
5. **gamescope as the display**, closer to SteamOS's compositor (it would
   also scale windowed games the way the Deck does).
6. **More runtimes** as GE-Proton releases, each added only after the
   end-to-end suite passes.

## Future integrations (not planned until there is demand)

- **Real hardware backend**: run the same build on a Steam Deck or Steam
  Machine dev kit (for example over SSH or Valve's devkit service) and
  produce the same `RunObservation`, so performance and input checks can run
  for real. The architecture already separates evidence gathering from the
  rules for this (docs/architecture.md).
- **Valve's own Proton builds**, which are only distributed through Steam
  and need a Steam login (anonymous SteamCMD downloads fail; see
  [runtimes.md](compatibility/runtimes.md#verified-runtimes)).
- **Steamworks without a Steam client.** Games that require the Steam API
  to initialise cannot be tested today. Emulating the API raises licensing
  and terms-of-service questions that need an answer before any work.

## Hardware only

Frame rate and frame pacing, GPU driver behaviour, thermal throttling,
battery use, suspend and resume, touchscreen input, the physical feel of the
controls, and anything that depends on the Steam client overlay. A software
renderer on a CI runner cannot stand in for these, and Protonaut
reports them as `NOT_TESTED` rather than guessing.
