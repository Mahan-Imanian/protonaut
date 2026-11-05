# GitHub Action

```yaml
- uses: Mahan-Imanian/protonaut@v0
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
| `artifact-name` | `protonaut-results` | Results artifact name; empty skips the upload |
| `fail-job` | `true` | `false` never fails the step on compatibility failures, so the outputs below stay available |

## Outputs

| Output | Meaning |
|---|---|
| `verdict` | `pass`, `fail`, or `error` when the test could not run |
| `exit-code` | `protonaut test` exit code (see [cli.md](cli.md)) |
| `results` | Absolute path of this invocation's results directory (unique per use, so several uses in one job do not overwrite each other) |

The step fails when the verdict is `fail` or the test could not run. GitHub
drops the outputs of a composite action that fails, so when a later step
needs `verdict` or `results` (to upload the report or post a comment), set
`fail-job: "false"` and fail the job yourself afterwards, as the example
below does. An environment error still fails the step.

## What it does

1. Validates the inputs and the runner.
2. Installs Xorg (dummy driver), Mesa lavapipe, ImageMagick, Tesseract OCR
   and x11 tools.
3. Installs the CLI: for a `vX.Y.Z` tag, the release binary after checking
   its SHA-256 and build provenance attestation (see [Pinning](#pinning));
   for any other ref, a `cargo build --locked` of the action's own source.
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
  protonaut:
    runs-on: ubuntu-24.04
    permissions:
      contents: read
    steps:
      - uses: actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1 # v7.0.1
        with:
          persist-credentials: false
      - run: ./build-windows.sh            # your build
      - id: protonaut
        uses: Mahan-Imanian/protonaut@v0
        with:
          fail-job: "false"
          build: dist/Windows
          exe: MyGame.exe
      - uses: actions/upload-artifact@cf430e030ddbb5b0abf93d22962f4752f3646cd9 # v7.0.2
        if: always()
        with:
          name: protonaut-report
          path: ${{ steps.protonaut.outputs.results }}/report.json
      - if: steps.protonaut.outputs.verdict != 'pass'
        run: exit 1

  comment:
    needs: protonaut
    if: github.event_name == 'pull_request' && github.event.pull_request.head.repo.full_name == github.repository
    runs-on: ubuntu-24.04
    permissions:
      contents: read
      pull-requests: write # update the report comment
    steps:
      - uses: actions/download-artifact@9000827ccba6bdab643e8b6fd33ac0654aef8333 # v8.0.2
        with:
          name: protonaut-report
      - uses: Mahan-Imanian/protonaut/comment@v0
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

`@v0` follows the latest 0.x release, and `@v0.2.1` pins one version. Both install the
release binary. Before running it, the action checks it against the release's
`SHA256SUMS` and verifies its GitHub build provenance attestation: it must have
been built by this repository's `release.yml` workflow from the matching
`refs/tags/vX.Y.Z` ref on a GitHub-hosted runner. Any mismatch stops the job.

Tags can be moved. For an immutable pin, use a full commit SHA. With a commit
SHA or a branch, the action builds the CLI from source with
`cargo build --locked`, which adds about three minutes to the first use in a job.


## Cost

GitHub-hosted runner minutes are free for public repositories and billed or
quota-limited for private ones. Measured on `ubuntu-24.04`: the first use in
a job took 235 s when it built the CLI from source (packages, the build,
downloading the runtime, one test with `observe-secs: 12`); a second use in
the same job took 31 s. A release tag skips the build. Each
runtime adds roughly 10 s plus `observe-secs`. Numbers per step are in
[runtimes.md](../compatibility/runtimes.md#timing-observed-on-github-hosted-ubuntu-2404-november-2026).
