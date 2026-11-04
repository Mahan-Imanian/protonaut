# Check catalogue

Every result in `report.json` has a stable `id`. An id is never reused for a
different meaning; a check that changes meaning gets a new id.

**Statuses.** `PASS` and `FAIL` are directly observed. `WARNING` needs a
human to look. `UNKNOWN` means the check ran but the evidence was not enough
for a verdict. `NOT_TESTED` means the check could not run here. Only `FAIL`
fails the run by default (`fail_on = "warning"` adds `WARNING`); `UNKNOWN`
and `NOT_TESTED` never count as a pass in any output format.

**Evidence.** `static`: deterministic inspection of the files. `observed`:
seen while the build ran. `heuristic`: pattern matching or image analysis
that can be wrong. `none`: nothing was measured.

## Static checks (no execution)

| Id | Checks | FAIL when | Other outcomes |
|---|---|---|---|
| SP-PE-001 | The executable is a Windows PE file | Not a PE file, or malformed | |
| SP-PE-002 | Architecture | ARM64 or unknown machine type | x86-64 and 32-bit x86 pass (32-bit runs through WoW64) |
| SP-PE-003 | Every imported DLL is shipped next to the executable or provided by the runtime | An import is in neither place | `UNKNOWN` when no runtime is installed to compare with. API sets (`api-ms-win-*`) are skipped. Delay-loaded DLLs are not checked |
| SP-STEAM-001 | Steam Input API is imported by name | never | `PASS` when a `SteamInput` flat-API function is imported; `UNKNOWN` otherwise, because C++ interface calls are invisible to import tables |
| SP-STEAM-002 | Steamworks on-screen keyboard API is imported | never | As above, for `ShowGamepadTextInput` / `ShowFloatingGamepadTextInput` |
| SP-FS-001 | No two paths differ only by letter case | Such a pair exists (only one is reachable on Windows and under Wine) | |
| SP-AC-001 | Anti-cheat files | never | `WARNING` when Easy Anti-Cheat, BattlEye, nProtect, XIGNCODE3, PunkBuster, Vanguard, mhyprot or ACE files are present. Finding them says nothing about whether they work under Proton |
| SP-LAUNCH-001 | Launcher (heuristic) | never | `WARNING` when the executable or another `.exe` is named like a launcher |
| SP-MEDIA-001 | Video formats (heuristic) | never | `WARNING` for formats usually decoded through Media Foundation or DirectShow (`.wmv`, `.asf`, `.mp4`, `.m4v`, `.mov`, `.avi`, `.mkv`) |

## Runtime checks (one result per runtime)

| Id | Checks | FAIL when | Other outcomes |
|---|---|---|---|
| SP-RUN-001 | The process starts | The runtime could not start it, or Wine's loader logged a missing DLL | |
| SP-RUN-002 | It keeps running for `observe_secs` | Nonzero exit or crash | `WARNING` on exit code 0 before the window ends |
| SP-RUN-003 | No crash signatures in the Wine log | An unhandled exception was logged | |
| SP-DISP-001 | A window appears | None appeared during the window | |
| SP-DISP-002 | Visible content (heuristic) | never | `WARNING` when every frame is black or one flat colour |
| SP-DISP-003 | Content fills the screen (heuristic) | never | `WARNING` on black bars over 5% of the width or height; `UNKNOWN` without content |
| SP-DISP-004 | No extra small window (heuristic) | never | `WARNING` when a window smaller than half the screen appears, for example an error or "unsupported GPU" dialog |

`SP-DISP-002` and `SP-DISP-003` read screenshots. Before the game runs, a
control program that draws a known colour runs under the same runtime and
display; if its colour is not captured, both checks are `NOT_TESTED` for
that runtime, never `WARNING` or `PASS`. See
[runtimes](../compatibility/runtimes.md) for which runtimes capture correctly.

## Hardware checks (always NOT_TESTED in this version)

| Id | Why not |
|---|---|
| SP-HW-001 | Frame rate needs real Deck or Steam Machine hardware; rendering here is CPU software Vulkan |
| SP-HW-002 | Controller-only play needs scripted input, not implemented |
| SP-HW-003 | Text size measurement (OCR) is not implemented |
