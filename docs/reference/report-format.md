# report.json

Machine-readable results, described by
[`schemas/report.v1.schema.json`](../../schemas/report.v1.schema.json)
(JSON Schema 2020-12, generated from the Rust types; a test fails when the
committed schema and the code disagree).

```json
{
  "schema_version": 1,
  "tool": { "name": "steamos-preflight", "version": "0.1.0" },
  "generated_at": "2026-11-04T18:40:12Z",
  "target": { "exe": "Game.exe", "args": [] },
  "environment": { "os": "Ubuntu 24.04.5 LTS", "vulkan_device": "llvmpipe (LLVM 20.1.2, 256 bits)", "display": "Xorg dummy 1280x800_60.00 59.81*+" },
  "runs": [
    { "runtime": "ge-proton11-12", "runtime_version": "GE-Proton11-12", "outcome": "crashed", "exit_code": 148, "duration_ms": 4100, "capture_verified": true }
  ],
  "checks": [
    {
      "id": "SP-RUN-002", "title": "Game keeps running during the observation window",
      "category": "runtime", "status": "FAIL", "evidence": "observed", "runtime": "ge-proton11-12",
      "summary": "Crashed after 4.1 s.",
      "details": ["wine: Unhandled division by zero at address 00006FFFFD7B18C0 (thread 014c), starting debugger..."],
      "artifacts": ["runs/ge-proton11-12/runtime.log"]
    }
  ],
  "summary": { "verdict": "fail", "counts": { "pass": 10, "fail": 2, "warning": 0, "unknown": 2, "not_tested": 5 } },
  "limitations": ["This is not a Valve review and cannot predict a Steam Deck Verified or Steam Machine Verified rating.", "…"]
}
```

| Field | Notes |
|---|---|
| `runs[].outcome` | `survived`, `exited_cleanly`, `exited_with_error`, `crashed`, `launch_failed` |
| `runs[].capture_verified` | Whether the control frame was captured; screenshot checks are only trusted when `true` |
| `checks[].status` | `PASS`, `FAIL`, `WARNING`, `UNKNOWN`, `NOT_TESTED` |
| `checks[].evidence` | `static`, `observed`, `heuristic`, `none` |
| `checks[].artifacts` | Paths relative to the results directory |
| `summary.verdict` | `fail` if any check fails the configured `fail_on` |

## Versioning

- Adding a field or a check id keeps `schema_version` at 1. Consumers must
  ignore fields and ids they do not know.
- Removing or renaming a field, or changing a field's meaning, bumps
  `schema_version`. `steamos-preflight report` refuses versions it does not
  know.
- Check ids follow the rules in [checks.md](checks.md).

Strings in a report can come from the build under test (file names, log
lines). Treat them as untrusted when you render them yourself; the built-in
renderers escape them.
