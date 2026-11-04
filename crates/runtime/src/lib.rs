//! SteamOS Preflight runtime layer: installs pinned Proton builds, runs a
//! headless Xorg display and supervises the build under test.
//!
//! Only `ProtonBackend` exists today. A future backend (plain Wine, a real
//! Steam Deck over SSH) produces the same `RunObservation`, so the rules in
//! `preflight_core::runtime_checks` and the reports stay unchanged.

pub mod capture;
pub mod display;
pub mod install;
pub mod proton;
pub mod supervise;

use std::path::Path;

#[derive(Debug, thiserror::Error)]
pub enum RuntimeError {
    #[error("{path}: {source}")]
    Io {
        path: String,
        source: std::io::Error,
    },
    #[error("runtime manifest: {0}")]
    Manifest(String),
    #[error("unknown runtime {0:?}; known runtimes: {1} (or `stable`)")]
    UnknownRuntime(String, String),
    #[error(
        "checksum mismatch for {id}: expected {expected}, got {actual}; the download was rejected"
    )]
    Checksum {
        id: String,
        expected: String,
        actual: String,
    },
    #[error("{tool} failed: {message}")]
    Tool { tool: String, message: String },
    #[error("could not start {program}: {message}")]
    Spawn { program: String, message: String },
    #[error("environment: {0}")]
    Environment(String),
    #[error("image: {0}")]
    Image(String),
}

impl RuntimeError {
    pub fn io(path: &Path, source: std::io::Error) -> Self {
        RuntimeError::Io {
            path: path.display().to_string(),
            source,
        }
    }
}
