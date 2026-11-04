# Threat model

SteamOS Preflight runs a game build, which is arbitrary Windows code, on a
CI machine that also holds credentials. This page lists what is protected,
from what, and how. It describes the GitHub Action as shipped in this
repository; running the CLI yourself gives you only the protections in the
"CLI" column.

## Assets

1. The workflow's `GITHUB_TOKEN` and the runner's internal tokens
   (`ACTIONS_RUNTIME_TOKEN`, cache and artifact credentials).
2. Repository secrets and git credentials.
3. Other jobs, the repository contents and published releases.
4. The integrity of the report shown to reviewers.
5. Private details of the machine (paths, user and host names) in uploaded
   artifacts.

## Threats and mitigations

| Threat | Mitigation | Where |
|---|---|---|
| The build reads `GITHUB_TOKEN` or secrets from its environment | The game is started with an empty environment plus an allowlist (`PATH`, `HOME`, `DISPLAY`, locale, Proton variables). A test proves `HOME` and `CARGO_*` from the parent never reach the child | CLI |
| The build reads the runner's memory or files (`/proc/<pid>/environ`, `~/work`, `.git/config`) | The action runs the game as a separate system user, `spf-sandbox`, which cannot read the runner user's processes or home. The game gets a copy of the build, not the workspace | Action |
| The build exfiltrates data over the network | `network: off` (the default) runs the game in a new network namespace with no interfaces up | Action |
| The build leaves processes running to attack later steps | Each run is a process group that is killed on exit, timeout or cancellation, and `wineserver -k` stops every Wine process of the prefix. CI fails if `wineserver`, `winedevice` or Xorg survive a run | CLI |
| A tampered runtime download | Every runtime is pinned by URL and SHA-256 in `crates/runtime/runtimes.toml`; a mismatch is rejected before extraction. GNU tar refuses `..` members | CLI |
| Symlinks or odd files planted in the results to make the upload step read other files | The collect step copies only regular files with known extensions and plain names, never follows symlinks, and caps each file at 20 MB | Action |
| Markdown/HTML injection (mentions, links, scripts) through file names or log lines | Every renderer escapes build-controlled strings; `@` is broken with a zero-width space; the HTML report has a restrictive Content-Security-Policy and only shows images from safe relative paths | CLI |
| A forged `report.md` posted as a PR comment | The comment action re-renders the comment from `report.json` with the trusted CLI after validating the schema; the Markdown file from the test job is never posted | Action |
| A pull request from a fork runs with write permissions | The comment job is restricted to same-repository PRs in the example workflow and never runs build code. Never run the test job under `pull_request_target` | Docs, examples |
| Shell injection through action inputs | Inputs are passed through environment variables, validated against strict patterns, and arguments are passed as arrays. The CLI never invokes a shell | Action, CLI |
| Host paths in artifacts | Build, work and results paths, `$HOME`, the user and host names are redacted in every artifact, in Linux and Wine (`Z:\…`) forms. The end-to-end suite fails if any host path appears | CLI |
| Vulnerable or malicious dependencies | `cargo-deny` checks advisories, licences and sources; Dependabot updates with a 7-day cooldown; actions are pinned to commit SHAs | Repository |

## What is not protected

- **Report integrity against a malicious build.** The build runs as the same
  user as the test process inside the sandbox and can forge its own results.
  The report tells you how your build behaved; it is not evidence about an
  untrusted third party's build.
- **Kernel or Wine exploits.** The sandbox is an unprivileged user and a
  network namespace, not a VM. A build that exploits the kernel can reach
  the runner. Use ephemeral GitHub-hosted runners; never run untrusted builds
  on a self-hosted runner that is reused.
- **Builds that need the network** (`network: on`) can send data anywhere.
- **The CLI on your own machine** runs the build as your user with your
  files readable. It scrubs the environment but is not a sandbox.

## Reporting a vulnerability

See [SECURITY.md](../../SECURITY.md).
