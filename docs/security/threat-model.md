# Threat model

SteamOS Preflight runs a game build, which is arbitrary Windows code, on a CI
machine that also holds credentials. This page states what is protected,
how, what was verified, and what is **not** protected.

**The sandbox is not VM-grade isolation.** The build runs as a separate
Linux user in its own network namespace on the same kernel as the CI job.
The strongest boundary in the system is the one GitHub provides: each job on
a GitHub-hosted runner gets a fresh virtual machine that is discarded
afterwards. SteamOS Preflight's sandbox limits what a build can reach
*inside* that VM; it does not replace it.

## Scope and assumptions

- The action runs on GitHub-hosted `ubuntu-24.04` runners. Self-hosted or
  reused runners are out of scope and not recommended for untrusted builds.
- The CLI run directly on a developer machine gives only the protections
  marked "CLI" below; it is not a sandbox.
- Attacker: anyone who controls the game build under test, including a pull
  request from a fork.

## Assets

1. The job's `GITHUB_TOKEN` and the runner's internal tokens
   (`ACTIONS_RUNTIME_TOKEN`, cache and artifact credentials).
2. Repository secrets, git credentials and the workspace.
3. Later steps and jobs, the repository and its releases.
4. The integrity of the report shown to reviewers.
5. Private details of the machine (paths, user and host names) in artifacts.

## How the action runs a build

1. Runtimes are installed **by root** into `/var/tmp/steamos-preflight/cache`,
   root-owned and read-only for everyone else.
2. A fresh home, a copy of the build and an empty results directory are
   created for the system user `spf-sandbox` (no login shell, no sudo rights,
   not in the `docker` group).
3. The CLI runs as `spf-sandbox` via `setpriv` under `unshare --net` with an
   empty environment, and starts the build with a second allowlisted
   environment in its own process group.
4. Afterwards every process of `spf-sandbox` is killed, and only then are
   regular files with known extensions copied out by a trusted step.

## Verified by the hostile probe

`fixtures/src/probe.c` behaves like a malicious build. The end-to-end
workflow runs it through the action on a GitHub-hosted runner and fails if
any attempt succeeds. Observed on `ubuntu-24.04` (November 2026):

| Attempt | Result |
|---|---|
| DNS lookup of github.com, TCP to 1.1.1.1:443 | BLOCKED |
| List `/home/runner`, `/home/runner/work`, `/home/runner/work/_temp` | BLOCKED (0 entries) |
| Write a file in `/home/runner` | BLOCKED |
| Find `GITHUB_TOKEN`, `ACTIONS_RUNTIME_TOKEN` or `ACTIONS_ID_TOKEN_REQUEST_TOKEN` in any readable `/proc/*/environ` | none (16 readable, all the sandbox user's own processes) |
| `sudo -n` from inside Wine | BLOCKED |
| Leave a detached `setsid sleep` running after the game | spawned, then killed; no `spf-sandbox` process survives the action |

The same probe run without the sandbox (as root, with network) reports
network, `/proc` tokens and `sudo` as OPEN, which shows the probe can detect
access when it exists. The workflow also checks that `spf-sandbox` has no
sudo rights, is not in the `docker` group and cannot write the runtime cache.

## Review by area

### Shared kernel

| | |
|---|---|
| Mitigated | Different UID from the runner user, so no `ptrace`, no reading its `/proc/<pid>/environ` (mode 0400) or files under `/home/runner` (0750). No network namespace access to anything. |
| Remaining | A kernel or Wine-reachable kernel exploit gives root on the job VM, which exposes that job's token and any secrets present in the job. Mitigation is outside this tool: GitHub's per-job VM, a job with `permissions: contents: read` and no secrets, and never a self-hosted runner. |

### Process permissions

| | |
|---|---|
| Mitigated | `spf-sandbox` is a system user without a password, login shell, sudo rights or supplementary groups (`setpriv --init-groups`). Verified in CI. All its processes are killed before results are read, including ones that left the process group and the Wine prefix (verified with the probe). Locally, the CLI kills the process group and runs `wineserver -k`. |
| Remaining | The CLI on a developer machine runs the game as the developer's own user; a detached Linux process started by the build is not killed there (observed in local testing). Documented; run untrusted builds only in CI. |

### Filesystem access

| | |
|---|---|
| Mitigated | No access to the runner's home, workspace or temp directories (verified). The game gets a copy of the build. Runtimes are root-owned and read-only, so a build cannot tamper with what a later use of the action in the same job runs. The sandbox home is recreated for every use. |
| Remaining | World-readable system files (`/etc`, tool caches under `/opt`) are readable; they hold no secrets on GitHub-hosted runners. `/tmp` and `/var/tmp` are shared and writable, so a build could leave files there for later steps that use predictable names in those directories. The action itself uses `mktemp` in `$RUNNER_TEMP`. |

### Symlinks

| | |
|---|---|
| Mitigated | The CLI never follows symlinks when scanning a build. The collect step copies only regular files (`find -type f` does not follow links) with plain names and no `..`, into a fresh `mktemp` directory, after all sandbox processes are dead, so there is no window to swap a file for a link. The HTML report refuses artifact paths that are not plain relative paths. |
| Remaining | A symlink inside the *build* that points into the runner's home is copied as a symlink; following it fails because the sandbox user cannot read that directory. |

### Artifact extraction

| | |
|---|---|
| Mitigated | Results are capped at 20 MB per file and filtered by extension before upload. The comment job reads only `report.json`, which the CLI parses into the typed schema (unknown structure, a wrong `schema_version` or a bad status value is rejected; tested). Runtime tarballs are verified by SHA-256 before extraction, and GNU tar refuses members that climb out with `..`. |
| Remaining | Extraction of the artifact zip itself is done by `actions/download-artifact`, which this project trusts. |

### Environment sanitisation

| | |
|---|---|
| Mitigated | Two layers: `env -i` when the action starts the CLI, and `env_clear()` plus an allowlist when the CLI starts the game. A unit test proves inherited variables never reach the child. The probe found no CI token variable in any readable process environment. |
| Remaining | None known. |

### GitHub token isolation

| | |
|---|---|
| Mitigated | The token never enters the sandbox's environment, and the runner's processes and files are unreadable to the sandbox user (verified). The documented workflow uses `permissions: contents: read` for the test job and `persist-credentials: false` on checkout. The PR comment is posted from a separate job that never runs build code. |
| Remaining | Only through a kernel exploit (see Shared kernel). |

### Pull requests from forks

| | |
|---|---|
| Mitigated | Under `pull_request`, GitHub gives fork runs a read-only token and no secrets. The example workflow skips the comment job for forks. The docs say never to run the test job under `pull_request_target`. |
| Remaining | Fork PRs can still consume runner minutes. Comments on fork PRs need a `workflow_run` setup that is not provided yet; the job summary still shows the report. |

### Malicious report content

| | |
|---|---|
| Mitigated | Markdown escapes emphasis, links, HTML and tables and breaks `@mentions`; HTML is escaped and carries a restrictive Content-Security-Policy; JUnit XML is escaped. The job summary and PR comment are re-rendered from `report.json` by the trusted CLI, never taken from the sandbox. All covered by tests. |
| Remaining | **Report integrity.** The build runs as the same user as the test process inside the sandbox, so it can forge its own results (for example a passing `report.json`). A report describes how *your* build behaved; it is not evidence about a build you do not trust. |

### Supply chain

| | |
|---|---|
| Mitigated | GE-Proton pinned by URL and SHA-256 (the digests GitHub lists for the release assets). Third-party actions pinned to commit SHAs; actionlint pinned by SHA-256; Rust dependencies locked and checked by `cargo-deny` (advisories, licences, sources); Rust toolchain pinned; Dependabot with a 7-day cooldown; zizmor in CI. Test fixtures are rebuilt in CI and must match the committed binaries byte for byte. No container images are used. |
| Remaining | Trust in GE-Proton's maintainers and in what the pinned release contains (Wine, DXVK, vkd3d-proton). Ubuntu packages (Xorg, Mesa, ImageMagick) are installed at their current archive versions, signed by Ubuntu but not pinned, so a package update can change behaviour; the weekly scheduled end-to-end run is there to notice. zizmor is pinned by version, not by hash. |

## Reporting a vulnerability

See [SECURITY.md](../../SECURITY.md).
