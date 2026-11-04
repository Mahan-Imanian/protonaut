//! Deterministic inspection of a build directory and its main executable.
//! Nothing here executes or loads the build.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use object::{Architecture, Object};

/// Upper bound on files walked, so a pathological tree cannot stall CI.
pub const MAX_FILES: usize = 250_000;

#[derive(Debug, thiserror::Error)]
pub enum InspectError {
    #[error("build directory {0} does not exist or is not a directory")]
    NoBuildDir(String),
    #[error("executable {0} not found in the build directory")]
    NoExe(String),
    #[error("cannot read {path}: {message}")]
    Io { path: String, message: String },
    #[error("build directory has more than {MAX_FILES} files")]
    TooManyFiles,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuildTree {
    /// Paths relative to the build directory, `/`-separated, sorted.
    pub files: Vec<String>,
    /// Symbolic links found (never followed).
    pub symlinks: Vec<String>,
}

impl BuildTree {
    pub fn scan(root: &Path) -> Result<Self, InspectError> {
        if !root.is_dir() {
            return Err(InspectError::NoBuildDir(root.display().to_string()));
        }
        let mut files = Vec::new();
        let mut symlinks = Vec::new();
        let mut stack = vec![PathBuf::new()];
        while let Some(rel) = stack.pop() {
            let dir = root.join(&rel);
            let entries = fs::read_dir(&dir).map_err(|e| io_err(&dir, e))?;
            for entry in entries {
                let entry = entry.map_err(|e| io_err(&dir, e))?;
                let child_rel = rel.join(entry.file_name());
                let kind = entry.file_type().map_err(|e| io_err(&entry.path(), e))?;
                let display = to_slash(&child_rel);
                if kind.is_symlink() {
                    symlinks.push(display);
                } else if kind.is_dir() {
                    stack.push(child_rel);
                } else if kind.is_file() {
                    files.push(display);
                    if files.len() > MAX_FILES {
                        return Err(InspectError::TooManyFiles);
                    }
                }
            }
        }
        files.sort();
        symlinks.sort();
        Ok(Self { files, symlinks })
    }

    /// Groups of paths that differ only by letter case.
    pub fn case_collisions(&self) -> Vec<Vec<String>> {
        let mut by_lower: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for f in &self.files {
            by_lower
                .entry(f.to_lowercase())
                .or_default()
                .push(f.clone());
        }
        by_lower.into_values().filter(|v| v.len() > 1).collect()
    }

    /// Case-insensitive lookup, the way Windows and Wine resolve names.
    pub fn find_ci(&self, rel: &str) -> Option<&str> {
        let wanted = rel
            .replace('\\', "/")
            .trim_start_matches("./")
            .to_lowercase();
        self.files
            .iter()
            .find(|f| f.to_lowercase() == wanted)
            .map(String::as_str)
    }

    /// File names (last component) in the given directory, lowercased.
    pub fn names_in_dir(&self, dir: &str) -> Vec<String> {
        let dir = dir.to_lowercase();
        self.files
            .iter()
            .filter_map(|f| {
                let lower = f.to_lowercase();
                let (parent, name) = match lower.rsplit_once('/') {
                    Some((p, n)) => (p.to_owned(), n.to_owned()),
                    None => (String::new(), lower.clone()),
                };
                (parent == dir).then_some(name)
            })
            .collect()
    }
}

fn to_slash(p: &Path) -> String {
    p.components()
        .map(|c| c.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
}

fn io_err(path: &Path, e: std::io::Error) -> InspectError {
    InspectError::Io {
        path: path.display().to_string(),
        message: e.to_string(),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PeArch {
    X86,
    X64,
    Arm64,
    Other,
}

impl PeArch {
    pub fn as_str(self) -> &'static str {
        match self {
            PeArch::X86 => "x86 (32-bit)",
            PeArch::X64 => "x86-64",
            PeArch::Arm64 => "ARM64",
            PeArch::Other => "unknown",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PeInfo {
    pub arch: PeArch,
    /// Imported functions grouped by DLL; DLL names lowercased.
    pub imports: BTreeMap<String, Vec<String>>,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum PeError {
    #[error("not a Windows PE executable ({0})")]
    NotPe(String),
    #[error("malformed PE file: {0}")]
    Malformed(String),
}

impl PeInfo {
    pub fn parse(data: &[u8]) -> Result<Self, PeError> {
        let kind = object::FileKind::parse(data).map_err(|e| PeError::NotPe(e.to_string()))?;
        if !matches!(kind, object::FileKind::Pe32 | object::FileKind::Pe64) {
            return Err(PeError::NotPe(format!("{kind:?}")));
        }
        let file = object::File::parse(data).map_err(|e| PeError::Malformed(e.to_string()))?;
        let arch = match file.architecture() {
            Architecture::I386 => PeArch::X86,
            Architecture::X86_64 => PeArch::X64,
            Architecture::Aarch64 => PeArch::Arm64,
            _ => PeArch::Other,
        };
        let mut imports: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for import in file
            .imports()
            .map_err(|e| PeError::Malformed(e.to_string()))?
        {
            let import = import.map_err(|e| PeError::Malformed(e.to_string()))?;
            let lib = String::from_utf8_lossy(import.library()).to_lowercase();
            let name = match import.name() {
                object::read::NameOrOrdinal::Name(n) => String::from_utf8_lossy(n).into_owned(),
                object::read::NameOrOrdinal::Ordinal(o) => format!("#{o}"),
            };
            imports.entry(lib).or_default().push(name);
        }
        for names in imports.values_mut() {
            names.sort();
            names.dedup();
        }
        Ok(Self { arch, imports })
    }

    pub fn load(path: &Path) -> Result<Result<Self, PeError>, InspectError> {
        let data = fs::read(path).map_err(|e| io_err(path, e))?;
        Ok(Self::parse(&data))
    }

    pub fn imported_functions<'a>(
        &'a self,
        dll_prefix: &'a str,
    ) -> impl Iterator<Item = &'a str> + 'a {
        self.imports
            .iter()
            .filter(move |(lib, _)| lib.starts_with(dll_prefix))
            .flat_map(|(_, names)| names.iter().map(String::as_str))
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn tree(files: &[&str]) -> BuildTree {
        BuildTree {
            files: files.iter().map(|s| s.to_string()).collect(),
            symlinks: vec![],
        }
    }

    #[test]
    fn scan_lists_files_sorted_with_slashes_and_skips_symlinks() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join("data/sub")).unwrap();
        fs::write(dir.path().join("Game.exe"), b"x").unwrap();
        fs::write(dir.path().join("data/sub/a.pak"), b"x").unwrap();
        std::os::unix::fs::symlink("/etc/passwd", dir.path().join("link")).unwrap();
        let t = BuildTree::scan(dir.path()).unwrap();
        assert_eq!(t.files, vec!["Game.exe", "data/sub/a.pak"]);
        assert_eq!(t.symlinks, vec!["link"]);
    }

    #[test]
    fn scan_rejects_missing_dir() {
        assert!(matches!(
            BuildTree::scan(Path::new("/nonexistent/x")),
            Err(InspectError::NoBuildDir(_))
        ));
    }

    #[test]
    fn case_collisions_are_grouped() {
        let t = tree(&["Data/A.pak", "data/a.pak", "b.txt"]);
        assert_eq!(
            t.case_collisions(),
            vec![vec!["Data/A.pak".to_string(), "data/a.pak".to_string()]]
        );
        assert!(tree(&["a", "b"]).case_collisions().is_empty());
    }

    #[test]
    fn find_ci_matches_like_windows() {
        let t = tree(&["Bin/Game.EXE"]);
        assert_eq!(t.find_ci("bin\\game.exe"), Some("Bin/Game.EXE"));
        assert_eq!(t.find_ci("./BIN/GAME.exe"), Some("Bin/Game.EXE"));
        assert_eq!(t.find_ci("game.exe"), None);
    }

    #[test]
    fn names_in_dir_is_case_insensitive() {
        let t = tree(&["Bin/A.dll", "Bin/sub/B.dll", "c.dll"]);
        assert_eq!(t.names_in_dir("bin"), vec!["a.dll"]);
        assert_eq!(t.names_in_dir(""), vec!["c.dll"]);
    }

    #[test]
    fn non_pe_input_is_rejected_without_panicking() {
        assert!(PeInfo::parse(b"MZ").is_err());
        assert!(matches!(
            PeInfo::parse(b"\x7fELF\x02\x01\x01\0\0\0\0\0\0\0\0\0"),
            Err(PeError::NotPe(_)) | Err(PeError::Malformed(_))
        ));
        assert!(PeInfo::parse(&[]).is_err());
    }
}
