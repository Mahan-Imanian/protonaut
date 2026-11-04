# Security policy

## Supported versions

SteamOS Preflight is pre-1.0. Security fixes go into the latest release
and `main` only.

| Version | Supported |
|---|---|
| latest 0.x release | yes |
| older 0.x releases | no |

## Reporting a vulnerability

Please do not open a public issue. Use GitHub's private vulnerability
reporting: the **Security** tab of this repository, then **Report a
vulnerability**. Include the version or commit, what an attacker controls
(for example, the game build, an action input, a `report.json`), and what
they gain.

You can expect an acknowledgement within 7 days and a fix or a plan within
30 days for confirmed issues. Reporters are credited in the release notes
unless they prefer not to be.

## Scope

In scope: anything that lets a game build or a crafted report read secrets
or tokens, escape the sandbox user or network namespace, write outside the
results directory, inject content into PR comments or reports, or tamper
with runtime downloads.

Known limits that are not vulnerabilities are listed in the
[threat model](docs/security/threat-model.md#what-is-not-protected), for
example that a malicious build can forge its own report, or that the
sandbox is not a VM.
