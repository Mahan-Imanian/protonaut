# Architecture

```text
            protonaut.toml / flags
                     │
              ┌──────▼──────┐      crates/cli
              │  protonaut  │      commands, exit codes, cancellation,
              │             │      redaction, writing results
              └──┬───────┬──┘
     static pass │       │ runtime pass (per runtime)
┌────────────────▼─┐   ┌─▼──────────────────────────────┐
│ crates/core      │   │ crates/runtime                  │
│ config, inspect  │   │ install: pinned + verified      │
│ static_checks    │   │ display: Xorg dummy             │
│ runtime_checks ◄─┼───┤ proton: control run, game run   │
│ observe (pure)   │   │ supervise: process groups       │
│ report renderers │   │ capture: screenshots, windows   │
└──────────────────┘   └─────────────────────────────────┘
          │  RunObservation (data only) flows up; rules live in core
          ▼
  report.json → report.md / report.html / junit.xml
```

## Boundaries

- **core** has no process execution and no network. Every rule that turns
  evidence into PASS/FAIL is a pure function here and is unit-tested
  without Proton.
- **runtime** gathers evidence: it starts processes, takes screenshots and
  reads logs, and returns a `RunObservation`. It does not decide verdicts.
- **cli** wires them together, owns the exit codes and the results layout.

## Adding a backend

A new backend (plain Wine, another Proton distribution, a real Steam Deck
reached over SSH) only has to produce a `RunObservation` for a build. The
rules, the report format and the action stay unchanged. `ProtonBackend` in
`crates/runtime/src/proton.rs` is the reference.

## Run lifecycle (per runtime)

1. **Control run.** The embedded control fixture runs in a fresh prefix and
   draws a known colour; a screenshot decides `capture_verified`.
2. **Prefix copy.** The control prefix is copied for the game, so prefix
   creation is not counted against the game.
3. **Game run.** Started as a process group with an allowlisted environment,
   observed for `observe_secs`, sampled twice for windows and screenshots.
4. **Stop.** SIGTERM, a 5 s grace period, SIGKILL for the group, then
   `wineserver -k` for the prefix.
5. **Evidence.** The log is redacted and scanned for crash and loader
   signatures; the observation goes to `runtime_checks::evaluate`.

## Decisions

| Decision | Reason |
|---|---|
| Xorg dummy, not Xvfb | Xvfb's 0 Hz refresh rate crashes DXVK 2.7.1 (observed, see runtimes.md) |
| Control fixture per runtime | Screen capture failed silently with one Proton build; without a control, a black screenshot would be a false failure |
| GE-Proton, pinned | Valve's Proton is only distributed through Steam; pinning by SHA-256 makes runs reproducible |
| Composite action, not Docker | Runs directly on the `ubuntu-24.04` host the tool was validated on; no image to build or publish |
| External tools (`import`, `xwininfo`, `xrandr`, `curl`, `tar`) | Mature, packaged on every runner; avoids X11 and HTTP client dependencies in the binary |
| Three crates | Clear trust boundary (pure rules vs. process execution) without fragmenting the code |
