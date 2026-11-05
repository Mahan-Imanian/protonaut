use std::collections::BTreeSet;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

use serde::Deserialize;
use sha2::{Digest, Sha256};

use crate::RuntimeError;
use crate::supervise::run_tool;

const MANIFEST: &str = include_str!("../runtimes.toml");

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct RuntimeSpec {
    pub id: String,
    pub name: String,
    pub url: String,
    pub sha256: String,
    pub size: u64,
    pub archive_dir: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ManifestFile {
    stable: String,
    runtime: Vec<RuntimeSpec>,
}

#[derive(Debug)]
pub struct Manifest {
    pub stable: String,
    pub runtimes: Vec<RuntimeSpec>,
}

impl Manifest {
    pub fn builtin() -> Result<Self, RuntimeError> {
        Self::parse(MANIFEST)
    }

    pub fn parse(text: &str) -> Result<Self, RuntimeError> {
        let file: ManifestFile =
            toml::from_str(text).map_err(|e| RuntimeError::Manifest(e.message().to_owned()))?;
        let m = Manifest {
            stable: file.stable,
            runtimes: file.runtime,
        };
        m.validate()?;
        Ok(m)
    }

    fn validate(&self) -> Result<(), RuntimeError> {
        let mut ids = BTreeSet::new();
        for r in &self.runtimes {
            if !ids.insert(&r.id) {
                return Err(RuntimeError::Manifest(format!(
                    "duplicate runtime id {}",
                    r.id
                )));
            }
            if r.sha256.len() != 64
                || !r
                    .sha256
                    .bytes()
                    .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
            {
                return Err(RuntimeError::Manifest(format!(
                    "{}: sha256 must be 64 lowercase hex digits",
                    r.id
                )));
            }
            if !r.url.starts_with("https://") {
                return Err(RuntimeError::Manifest(format!(
                    "{}: url must be https",
                    r.id
                )));
            }
            if r.archive_dir.is_empty()
                || r.archive_dir.contains('/')
                || r.archive_dir.contains("..")
            {
                return Err(RuntimeError::Manifest(format!(
                    "{}: archive_dir must be a single directory name",
                    r.id
                )));
            }
        }
        if !ids.contains(&self.stable) {
            return Err(RuntimeError::Manifest(format!(
                "stable points at unknown runtime {}",
                self.stable
            )));
        }
        Ok(())
    }

    /// Resolves an id or the `stable` alias.
    pub fn resolve(&self, id: &str) -> Result<&RuntimeSpec, RuntimeError> {
        let id = if id == "stable" {
            self.stable.as_str()
        } else {
            id
        };
        self.runtimes.iter().find(|r| r.id == id).ok_or_else(|| {
            let known: Vec<_> = self.runtimes.iter().map(|r| r.id.as_str()).collect();
            RuntimeError::UnknownRuntime(id.to_owned(), known.join(", "))
        })
    }
}

/// `$PROTONAUT_CACHE`, else `$XDG_CACHE_HOME/protonaut`,
/// else `~/.cache/protonaut`.
pub fn default_cache_dir() -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("PROTONAUT_CACHE").filter(|p| !p.is_empty()) {
        return Some(PathBuf::from(p));
    }
    if let Some(p) = std::env::var_os("XDG_CACHE_HOME").filter(|p| !p.is_empty()) {
        return Some(PathBuf::from(p).join("protonaut"));
    }
    std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".cache/protonaut"))
}

#[derive(Debug, Clone)]
pub struct InstalledRuntime {
    pub spec: RuntimeSpec,
    /// Directory containing the `proton` script.
    pub root: PathBuf,
}

impl InstalledRuntime {
    /// Lowercased names of every Windows DLL the runtime ships as a builtin.
    pub fn builtin_dlls(&self) -> BTreeSet<String> {
        let mut out = BTreeSet::new();
        for sub in [
            "files/lib/wine/x86_64-windows",
            "files/lib/wine/i386-windows",
            "files/lib64/wine/x86_64-windows",
        ] {
            if let Ok(entries) = fs::read_dir(self.root.join(sub)) {
                for e in entries.flatten() {
                    let name = e.file_name().to_string_lossy().to_lowercase();
                    if name.ends_with(".dll") {
                        out.insert(name);
                    }
                }
            }
        }
        out
    }

    /// The version string from the runtime's `version` file.
    pub fn version(&self) -> String {
        fs::read_to_string(self.root.join("version"))
            .ok()
            .and_then(|v| v.split_whitespace().nth(1).map(str::to_owned))
            .unwrap_or_else(|| self.spec.name.clone())
    }
}

pub struct Cache {
    pub dir: PathBuf,
}

impl Cache {
    pub fn new(dir: PathBuf) -> Self {
        Self { dir }
    }

    fn runtime_dir(&self, spec: &RuntimeSpec) -> PathBuf {
        self.dir.join("runtimes").join(&spec.id)
    }

    /// Returns the runtime if it is fully installed. Installation writes a
    /// marker last, so a half-extracted directory is never used.
    pub fn installed(&self, spec: &RuntimeSpec) -> Option<InstalledRuntime> {
        let dir = self.runtime_dir(spec);
        let marker = fs::read_to_string(dir.join(".protonaut-verified")).ok()?;
        let root = dir.join(&spec.archive_dir);
        (marker.trim() == spec.sha256 && root.join("proton").is_file()).then(|| InstalledRuntime {
            spec: spec.clone(),
            root,
        })
    }

    /// Downloads, verifies and extracts a runtime. `archive` lets callers use
    /// a tarball fetched by other means (it is verified all the same).
    pub async fn install(
        &self,
        spec: &RuntimeSpec,
        archive: Option<&Path>,
    ) -> Result<InstalledRuntime, RuntimeError> {
        if let Some(rt) = self.installed(spec) {
            return Ok(rt);
        }
        let downloads = self.dir.join("downloads");
        fs::create_dir_all(&downloads).map_err(|e| RuntimeError::io(&downloads, e))?;
        let tarball = match archive {
            Some(p) => p.to_path_buf(),
            None => {
                let dest = downloads.join(format!("{}.tar.gz", spec.id));
                if !dest.is_file() || sha256_file(&dest)? != spec.sha256 {
                    let partial = downloads.join(format!("{}.tar.gz.part", spec.id));
                    tracing::info!(runtime = %spec.id, url = %spec.url, "downloading runtime");
                    run_tool(
                        "curl",
                        &[
                            "--fail",
                            "--location",
                            "--silent",
                            "--show-error",
                            "--proto",
                            "=https",
                            "--tlsv1.2",
                            "--retry",
                            "3",
                            "--output",
                        ],
                        &[partial.as_os_str(), spec.url.as_ref()],
                        std::time::Duration::from_secs(1800),
                    )
                    .await?;
                    fs::rename(&partial, &dest).map_err(|e| RuntimeError::io(&dest, e))?;
                }
                dest
            }
        };
        let actual = sha256_file(&tarball)?;
        if actual != spec.sha256 {
            return Err(RuntimeError::Checksum {
                id: spec.id.clone(),
                expected: spec.sha256.clone(),
                actual,
            });
        }
        let final_dir = self.runtime_dir(spec);
        let staging =
            self.dir
                .join("runtimes")
                .join(format!(".{}.staging-{}", spec.id, std::process::id()));
        if staging.exists() {
            fs::remove_dir_all(&staging).map_err(|e| RuntimeError::io(&staging, e))?;
        }
        fs::create_dir_all(&staging).map_err(|e| RuntimeError::io(&staging, e))?;
        tracing::info!(runtime = %spec.id, "extracting runtime");
        // GNU tar strips leading '/' and refuses members that climb out with
        // '..' by default; the archive is also pinned by checksum.
        run_tool(
            "tar",
            &[
                "--extract",
                "--gzip",
                "--no-same-owner",
                "--no-same-permissions",
                "--file",
            ],
            &[
                tarball.as_os_str(),
                "--directory".as_ref(),
                staging.as_os_str(),
            ],
            std::time::Duration::from_secs(900),
        )
        .await?;
        if !staging.join(&spec.archive_dir).join("proton").is_file() {
            let _ = fs::remove_dir_all(&staging);
            return Err(RuntimeError::Manifest(format!(
                "{}: archive has no {}/proton",
                spec.id, spec.archive_dir
            )));
        }
        fs::write(staging.join(".protonaut-verified"), &spec.sha256)
            .map_err(|e| RuntimeError::io(&staging, e))?;
        if final_dir.exists() {
            fs::remove_dir_all(&final_dir).map_err(|e| RuntimeError::io(&final_dir, e))?;
        }
        fs::rename(&staging, &final_dir).map_err(|e| RuntimeError::io(&final_dir, e))?;
        self.installed(spec)
            .ok_or_else(|| RuntimeError::Manifest(format!("{} did not install cleanly", spec.id)))
    }

    pub fn list(&self, manifest: &Manifest) -> Vec<(RuntimeSpec, bool)> {
        manifest
            .runtimes
            .iter()
            .map(|s| (s.clone(), self.installed(s).is_some()))
            .collect()
    }
}

pub fn sha256_file(path: &Path) -> Result<String, RuntimeError> {
    let mut f = fs::File::open(path).map_err(|e| RuntimeError::io(path, e))?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 1 << 20];
    loop {
        let n = f.read(&mut buf).map_err(|e| RuntimeError::io(path, e))?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hex::encode(hasher.finalize()))
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn builtin_manifest_is_valid_and_stable_resolves() {
        let m = Manifest::builtin().unwrap();
        assert_eq!(m.resolve("stable").unwrap().id, m.stable);
        assert!(m.resolve("ge-proton10-34").is_ok());
        assert!(matches!(
            m.resolve("proton-9000"),
            Err(RuntimeError::UnknownRuntime(..))
        ));
    }

    #[test]
    fn manifest_validation_rejects_bad_entries() {
        let entry = |id: &str, sha: &str, url: &str, dir: &str| {
            format!(
                "[[runtime]]\nid=\"{id}\"\nname=\"n\"\nurl=\"{url}\"\nsha256=\"{sha}\"\nsize=1\narchive_dir=\"{dir}\"\n"
            )
        };
        let good = "a".repeat(64);
        assert!(
            Manifest::parse(&format!(
                "stable=\"x\"\n{}",
                entry("x", &good, "https://e", "d")
            ))
            .is_ok()
        );
        for bad in [
            format!("stable=\"y\"\n{}", entry("x", &good, "https://e", "d")),
            format!("stable=\"x\"\n{}", entry("x", "abc", "https://e", "d")),
            format!("stable=\"x\"\n{}", entry("x", &good, "http://e", "d")),
            format!("stable=\"x\"\n{}", entry("x", &good, "https://e", "../d")),
            format!(
                "stable=\"x\"\n{}{}",
                entry("x", &good, "https://e", "d"),
                entry("x", &good, "https://e", "d")
            ),
        ] {
            assert!(Manifest::parse(&bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn sha256_matches_known_vector() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("f");
        fs::write(&p, b"abc").unwrap();
        assert_eq!(
            sha256_file(&p).unwrap(),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn half_installed_runtime_is_ignored() {
        let dir = tempfile::tempdir().unwrap();
        let cache = Cache::new(dir.path().to_path_buf());
        let m = Manifest::builtin().unwrap();
        let spec = m.resolve("stable").unwrap();
        let root = dir
            .path()
            .join("runtimes")
            .join(&spec.id)
            .join(&spec.archive_dir);
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join("proton"), b"").unwrap();
        assert!(cache.installed(spec).is_none(), "no marker yet");
        fs::write(
            dir.path()
                .join("runtimes")
                .join(&spec.id)
                .join(".protonaut-verified"),
            "0".repeat(64),
        )
        .unwrap();
        assert!(
            cache.installed(spec).is_none(),
            "marker for another checksum"
        );
        fs::write(
            dir.path()
                .join("runtimes")
                .join(&spec.id)
                .join(".protonaut-verified"),
            &spec.sha256,
        )
        .unwrap();
        assert!(cache.installed(spec).is_some());
    }

    #[tokio::test]
    async fn tampered_archive_is_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let cache = Cache::new(dir.path().to_path_buf());
        let m = Manifest::builtin().unwrap();
        let fake = dir.path().join("fake.tar.gz");
        fs::write(&fake, b"not the real archive").unwrap();
        let err = cache
            .install(m.resolve("stable").unwrap(), Some(&fake))
            .await
            .unwrap_err();
        assert!(matches!(err, RuntimeError::Checksum { .. }), "{err}");
    }
}
