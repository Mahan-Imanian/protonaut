# Development

## Without Proton (any Linux, minutes)

Everything except the runtime runs anywhere Rust does:

```bash
cargo test --workspace
```

```bash
cargo clippy --workspace --all-targets -- -D warnings
```

The CLI integration tests (`crates/cli/tests/cli.rs`) run `inspect`,
`report`, `init` and friends on the committed fixtures; the runtime crate's
tests cover process supervision with plain shell scripts. Most bugs can be
reproduced and fixed at this level, because every verdict is a pure
function over a `RunObservation` (`crates/core/src/runtime_checks.rs`).

## With Proton (Ubuntu 24.04 or similar, x86-64)

```bash
sudo apt-get install --no-install-recommends xserver-xorg-core xserver-xorg-video-dummy x11-xserver-utils x11-utils imagemagick mesa-vulkan-drivers python3 curl
```

```bash
cargo build --release
```

```bash
sudo target/release/steamos-preflight setup
```

```bash
target/release/steamos-preflight proton install stable
```

```bash
target/release/steamos-preflight diagnose
```

```bash
python3 scripts/e2e.py --bin target/release/steamos-preflight --runtime ge-proton11-12
```

`scripts/e2e.py` runs every fixture and compares the results with
`tests/e2e/expectations.toml`. It also fails if a host path appears in any
artifact. No Steam Deck or GPU is needed.

## Changing things

| Change | Also update |
|---|---|
| A check's meaning | Give it a new id; `docs/reference/checks.md` |
| The report model | `cargo run -p steamos-preflight -- schema --write`, `docs/reference/report-format.md` |
| `preflight.toml` | The same schema command, `STARTER_CONFIG` in `crates/core/src/config.rs` |
| A fixture | `scripts/build-fixtures.sh`, `fixtures/README.md`, expectations |
| A runtime | `crates/runtime/runtimes.toml`, `docs/compatibility/runtimes.md` |

Every bug fix comes with a test that fails before the fix.

## Releasing

Maintainers only. Bump the workspace version in `Cargo.toml`, move the
`[Unreleased]` changelog section under the new version, commit, tag
`vX.Y.Z` and push the tag. The release workflow builds the binary, writes
`SHA256SUMS` and creates a **draft** release; a maintainer checks the assets
and publishes it.
