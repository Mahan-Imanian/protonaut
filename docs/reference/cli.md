# Command line

```text
steamos-preflight [-v...] [--log-format text|json] <command>
```

Logs go to stderr; command output (summaries, `--json`) goes to stdout.

| Command | What it does |
|---|---|
| `init [--force]` | Writes a starter `preflight.toml` |
| `inspect [BUILD] --exe EXE` | Static checks only; never runs the build |
| `test [BUILD] --exe EXE` | Static and runtime checks under each runtime |
| `proton list` / `proton install ID [--archive FILE]` | Pinned runtimes: list, download and verify |
| `report REPORT.json --format markdown\|html\|junit\|json` | Re-renders a report after validating it |
| `diagnose [--json]` | Checks the machine and prints how to fix what is missing |
| `setup [--print]` | Installs the Xorg dummy config in `/etc/X11` (root) |
| `schema [--write]` | Prints or writes the JSON Schemas |

Options shared by `inspect` and `test`:

| Option | Default | Meaning |
|---|---|---|
| `BUILD` | `build.dir` | Directory with the packaged Windows build |
| `--exe` | `build.exe` | Executable relative to `BUILD` (matched case-insensitively) |
| `--config` | `./preflight.toml` if present | Configuration file |
| `--results` | `preflight-results` | Output directory |
| `--json` | off | Print `report.json` on stdout |
| `--cache` | see below | Runtime cache directory |

`test` adds `--runtime ID` (repeatable, `stable` allowed), `--observe-secs`,
`--resolution`, `--fail-on fail|warning`, `--install` (download missing
runtimes), `--timeout SECS` (whole command, default 1800) and
`-- ARGS...` (passed to the game).

## Exit codes

| Code | Meaning |
|---|---|
| 0 | No failures (`test`, `inspect`), or the command succeeded |
| 1 | At least one `FAIL` (or `WARNING` with `--fail-on warning`) |
| 2 | Usage or configuration error |
| 3 | Environment or runtime error: the tests could not run |
| 130 | Cancelled by SIGINT or SIGTERM; all game processes were stopped |

## Environment variables

| Variable | Meaning |
|---|---|
| `STEAMOS_PREFLIGHT_CACHE` | Runtime cache. Otherwise `$XDG_CACHE_HOME/steamos-preflight`, then `~/.cache/steamos-preflight` |
| `STEAMOS_PREFLIGHT_CONFIG` | Default for `--config` |
| `STEAMOS_PREFLIGHT_LOG` | `tracing` filter, for example `debug` |
| `STEAMOS_PREFLIGHT_LOG_FORMAT` | `text` or `json` |

## Output files

```text
preflight-results/
├── report.json          schema v1, see report-format.md
├── report.md            Markdown summary (the PR comment body)
├── report.html          self-contained page with screenshots
├── junit.xml            FAIL → failure, UNKNOWN/NOT_TESTED → skipped
└── runs/<runtime>/
    ├── control.png      the capture control frame
    ├── screenshot-N-1280x800.png
    └── runtime.log      Wine/Proton output, host paths redacted
```

Host paths, the home directory, the user and host names are replaced with
`<build>`, `<work>`, `<results>`, `~`, `<user>` and `<host>` in every file.

## Configuration

See [`preflight.toml`](../../schemas/config.v1.schema.json) for the schema;
`steamos-preflight init` writes a commented example. Unknown keys are an
error, so a typo cannot be silently ignored.

## Stability

The commands, options, exit codes, environment variables and output layout
above are the public interface. Before 1.0 they can change in a minor
release; every change is listed in the changelog with migration notes.
