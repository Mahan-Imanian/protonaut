# GitHub Action

```yaml
- uses: Mahan-Imanian/steamos-preflight@<full commit SHA> # pin, see "Pinning"
  with:
    build: dist/Windows
    exe: MyGame.exe
```

Runs on `ubuntu-24.04` (x86-64 Linux runners only).

## Inputs

| Input | Default | Meaning |
|---|---|---|
| `build` | required | Build directory, relative to the workspace |
| `exe` | required | Executable relative to `build` |
| `runtimes` | `stable` | Space-separated runtime ids |
| `observe-secs` | `20` | Seconds to watch the game (3-600) |
| `resolution` | `1280x800` | `1280x800`, `1280x720` or `1920x1080` |
| `fail-on` | `fail` | `warning` also fails the step on WARNING |
| `network` | `off` | `on` lets the game use the network |
| `artifact-name` | `steamos-preflight-results` | Results artifact name; empty skips the upload |
| `fail-job` | `true` | `false` never fails the step on compatibility failures, so the outputs below stay available |

## Outputs

| Output | Meaning |
|---|---|
| `verdict` | `pass`, `fail`, or `error` when the test could not run |
| `exit-code` | `steamos-preflight test` exit code (see [cli.md](cli.md)) |
| `results` | Absolute path of this invocation's results directory (unique per use, so several uses in one job do not overwrite each other) |

The step fails when the verdict is `fail` or the test could not run. GitHub
drops the outputs of a composite action that fails, so when a later step
needs `verdict` or `results` (to upload the report or post a comment), set
`fail-job: "false"` and fail the job yourself afterwards, as the example
below does. An environment error still fails the step.

## What it does

1. Validates the inputs and the runner.
2. Installs Xorg (dummy driver), Mesa lavapipe, ImageMagick and x11 tools.
3. Builds the CLI from the action's own source with `cargo build --locked`
   (about 1-2 minutes; prebuilt release binaries will replace this).
4. Restores cached runtime downloads, installs and verifies the runtimes.
5. Copies the build to a directory owned by the `spf-sandbox` user and runs
   the test as that user, with an empty environment and, by default, no
   network.
6. Copies regular result files back, writes the job summary from
   `report.json`, uploads the artifact.

See the [threat model](../security/threat-model.md) for why each step exists.

## PR comments

The comment action never runs the build. Put it in a second job that
downloads only `report.json`:

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
      - run: ./build-windows.sh            # your build
      - id: preflight
        uses: Mahan-Imanian/steamos-preflight@<sha>
        with:
          fail-job: "false"
          build: dist/Windows
          exe: MyGame.exe
      - uses: actions/upload-artifact@<sha> # v7
        if: always()
        with:
          name: preflight-report
          path: ${{ steps.preflight.outputs.results }}/report.json
      - if: steps.preflight.outputs.verdict != 'pass'
        run: exit 1

  comment:
    needs: preflight
    if: github.event_name == 'pull_request' && github.event.pull_request.head.repo.full_name == github.repository
    runs-on: ubuntu-24.04
    permissions:
      contents: read
      pull-requests: write # update the report comment
    steps:
      - uses: actions/download-artifact@<sha> # v8
        with:
          name: preflight-report
      - uses: Mahan-Imanian/steamos-preflight/comment@<sha>
        with:
          report: report.json
          pr-number: ${{ github.event.pull_request.number }}
```

The comment is updated in place on later pushes (one comment per PR). For
pull requests from forks, `pull_request` workflows get a read-only token, so
the comment step logs a warning and the job summary still shows the report.
Never switch to `pull_request_target` for the test job: it would run fork
code with a write token.

A complete workflow is in [examples/github-actions.yml](../examples/github-actions.yml).

## Pinning

Pin the action to a full commit SHA, as for any third-party action. Tags
such as `v0.1.0` will exist from the first release; a `v0` major tag will
follow the 1.0 stability commitment.

## Cost

GitHub-hosted runner minutes are free for public repositories and billed or
quota-limited for private ones. Measured on `ubuntu-24.04`: the first use in
a job took 235 s (packages, building the CLI, downloading the runtime, one
test with `observe-secs: 12`); a second use in the same job took 31 s. Each
runtime adds roughly 10 s plus `observe-secs`. Numbers per step are in
[runtimes.md](../compatibility/runtimes.md#timing-observed-on-github-hosted-ubuntu-2404-november-2026).
