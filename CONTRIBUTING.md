# Contributing

Thanks for helping. Bug reports with a reproducible case, compatibility
reports from real games, new checks and fixes are all welcome.

## First steps

1. Read [docs/development.md](docs/development.md); `cargo test --workspace`
   needs no Proton and no Steam Deck.
2. Look for issues labelled `good first issue`.
3. For a new check, open an issue first describing what it detects, the
   evidence level, and how it could be wrong.

## Pull requests

- One concern per pull request, with tests. A bug fix includes a test that
  fails before the fix.
- `cargo fmt`, `cargo clippy --workspace --all-targets -- -D warnings` and
  `cargo test --workspace` pass.
- User-visible changes get a line in `CHANGELOG.md` under `[Unreleased]`.
- Commit messages follow [Conventional Commits](https://www.conventionalcommits.org/)
  (`feat:`, `fix:`, `docs:`, `test:`, `ci:`, `chore:`), with `!` for
  breaking changes.
- New dependencies need a reason in the PR description; `cargo deny check`
  must pass.

## Rules for checks

A check must never turn missing evidence into `PASS`. Heuristic checks say
so in their `evidence` field and never `FAIL`. Check ids are permanent: a
change of meaning gets a new id. See [docs/reference/checks.md](docs/reference/checks.md).

## Compatibility reports

If Protonaut disagrees with what you see on a real Steam Deck, open
a "Compatibility report" issue with the report (redact anything private) and
what happened on the device. These reports are how the checks get better.

## Licensing of contributions

Protonaut is licensed under MIT OR Apache-2.0. Unless you explicitly
state otherwise, any contribution intentionally submitted for inclusion by
you, as defined in the Apache-2.0 license, shall be dual licensed as above,
without any additional terms or conditions.

## Conduct

This project follows the [Code of Conduct](CODE_OF_CONDUCT.md).
