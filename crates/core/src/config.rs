use std::fmt;
use std::path::{Component, Path, PathBuf};
use std::str::FromStr;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::model::FailOn;

pub const CONFIG_SCHEMA_VERSION: u32 = 1;
pub const DEFAULT_CONFIG_FILE: &str = "protonaut.toml";
pub const MIN_OBSERVE_SECS: u64 = 3;
pub const MAX_OBSERVE_SECS: u64 = 600;
const MAX_ARGS: usize = 32;
const MAX_ARG_LEN: usize = 1024;

#[derive(Debug, thiserror::Error, PartialEq)]
pub enum ConfigError {
    #[error("cannot read {path}: {message}")]
    Read { path: String, message: String },
    #[error("invalid {path}: {message}")]
    Parse { path: String, message: String },
    #[error(
        "unsupported schema_version {0}; this version of protonaut reads version {CONFIG_SCHEMA_VERSION}"
    )]
    SchemaVersion(u32),
    #[error("{field}: {message}")]
    Invalid {
        field: &'static str,
        message: String,
    },
}

fn invalid(field: &'static str, message: impl Into<String>) -> ConfigError {
    ConfigError::Invalid {
        field,
        message: message.into(),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(try_from = "String", into = "String")]
#[schemars(with = "String")]
pub struct Resolution {
    pub width: u32,
    pub height: u32,
}

impl Resolution {
    pub const DECK: Resolution = Resolution {
        width: 1280,
        height: 800,
    };
    pub const SUPPORTED: [Resolution; 3] = [
        Resolution {
            width: 1280,
            height: 800,
        },
        Resolution {
            width: 1280,
            height: 720,
        },
        Resolution {
            width: 1920,
            height: 1080,
        },
    ];
}

impl fmt::Display for Resolution {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}x{}", self.width, self.height)
    }
}

impl FromStr for Resolution {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let (w, h) = s
            .split_once('x')
            .ok_or_else(|| format!("expected WIDTHxHEIGHT, got {s:?}"))?;
        let r = Resolution {
            width: w.parse().map_err(|_| format!("bad width in {s:?}"))?,
            height: h.parse().map_err(|_| format!("bad height in {s:?}"))?,
        };
        if Resolution::SUPPORTED.contains(&r) {
            Ok(r)
        } else {
            Err(format!(
                "{r} is not supported; use one of 1280x800, 1280x720, 1920x1080"
            ))
        }
    }
}

impl TryFrom<String> for Resolution {
    type Error = String;
    fn try_from(s: String) -> Result<Self, Self::Error> {
        s.parse()
    }
}

impl From<Resolution> for String {
    fn from(r: Resolution) -> Self {
        r.to_string()
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct BuildSection {
    /// Directory holding the Windows build, relative to the config file.
    pub dir: Option<PathBuf>,
    /// Executable path relative to `dir`.
    pub exe: Option<String>,
    #[serde(default)]
    pub args: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields, default)]
pub struct RunSection {
    /// Runtime ids or aliases, for example `stable` or `ge-proton10-34`.
    pub runtimes: Vec<String>,
    /// How long to watch the game after launch, in seconds.
    pub observe_secs: u64,
    pub resolution: Resolution,
    pub fail_on: FailOn,
}

impl Default for RunSection {
    fn default() -> Self {
        Self {
            runtimes: vec!["stable".to_owned()],
            observe_secs: 20,
            resolution: Resolution::DECK,
            fail_on: FailOn::Fail,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ChecksSection {
    /// Check ids to leave out of the report, for example `PN-MEDIA-001`.
    #[serde(default)]
    pub skip: Vec<String>,
}

/// The file as written by the user.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ConfigFile {
    #[serde(default = "default_schema_version")]
    pub schema_version: u32,
    #[serde(default)]
    pub build: BuildSection,
    #[serde(default)]
    pub run: RunSection,
    #[serde(default)]
    pub checks: ChecksSection,
}

fn default_schema_version() -> u32 {
    CONFIG_SCHEMA_VERSION
}

impl Default for ConfigFile {
    fn default() -> Self {
        Self {
            schema_version: CONFIG_SCHEMA_VERSION,
            build: BuildSection::default(),
            run: RunSection::default(),
            checks: ChecksSection::default(),
        }
    }
}

impl ConfigFile {
    pub fn parse(text: &str, origin: &str) -> Result<Self, ConfigError> {
        let file: ConfigFile = toml::from_str(text).map_err(|e| ConfigError::Parse {
            path: origin.to_owned(),
            message: e.message().to_owned(),
        })?;
        if file.schema_version != CONFIG_SCHEMA_VERSION {
            return Err(ConfigError::SchemaVersion(file.schema_version));
        }
        Ok(file)
    }

    pub fn load(path: &Path) -> Result<Self, ConfigError> {
        let text = std::fs::read_to_string(path).map_err(|e| ConfigError::Read {
            path: path.display().to_string(),
            message: e.to_string(),
        })?;
        Self::parse(&text, &path.display().to_string())
    }
}

/// Command-line values that take precedence over the file.
#[derive(Debug, Clone, Default)]
pub struct Overrides {
    pub build_dir: Option<PathBuf>,
    pub exe: Option<String>,
    pub args: Option<Vec<String>>,
    pub runtimes: Option<Vec<String>>,
    pub observe_secs: Option<u64>,
    pub resolution: Option<Resolution>,
    pub fail_on: Option<FailOn>,
}

/// A validated configuration ready to run.
#[derive(Debug, Clone, PartialEq)]
pub struct Config {
    pub build_dir: PathBuf,
    pub exe: String,
    pub args: Vec<String>,
    pub runtimes: Vec<String>,
    pub observe_secs: u64,
    pub resolution: Resolution,
    pub fail_on: FailOn,
    pub skip: Vec<String>,
}

impl Config {
    /// `base` is the directory relative paths in the file are resolved from.
    pub fn resolve(
        file: ConfigFile,
        overrides: Overrides,
        base: &Path,
    ) -> Result<Self, ConfigError> {
        let build_dir = overrides
            .build_dir
            .or_else(|| file.build.dir.map(|d| base.join(d)))
            .ok_or_else(|| {
                invalid(
                    "build.dir",
                    "no build directory given (pass a path or set build.dir)",
                )
            })?;
        let exe = overrides.exe.or(file.build.exe);
        let exe = match exe {
            Some(e) => e,
            None => {
                return Err(invalid(
                    "build.exe",
                    "no executable given (pass --exe or set build.exe)",
                ));
            }
        };
        let config = Config {
            build_dir,
            exe,
            args: overrides.args.unwrap_or(file.build.args),
            runtimes: overrides.runtimes.unwrap_or(file.run.runtimes),
            observe_secs: overrides.observe_secs.unwrap_or(file.run.observe_secs),
            resolution: overrides.resolution.unwrap_or(file.run.resolution),
            fail_on: overrides.fail_on.unwrap_or(file.run.fail_on),
            skip: file.checks.skip,
        };
        config.validate()?;
        Ok(config)
    }

    fn validate(&self) -> Result<(), ConfigError> {
        validate_exe_path(&self.exe)?;
        if self.args.len() > MAX_ARGS {
            return Err(invalid(
                "build.args",
                format!("at most {MAX_ARGS} arguments"),
            ));
        }
        if let Some(a) = self
            .args
            .iter()
            .find(|a| a.len() > MAX_ARG_LEN || a.contains('\0'))
        {
            return Err(invalid(
                "build.args",
                format!("argument too long or contains NUL: {:.40}", a),
            ));
        }
        if !(MIN_OBSERVE_SECS..=MAX_OBSERVE_SECS).contains(&self.observe_secs) {
            return Err(invalid(
                "run.observe_secs",
                format!("must be between {MIN_OBSERVE_SECS} and {MAX_OBSERVE_SECS}"),
            ));
        }
        if self.runtimes.is_empty() {
            return Err(invalid("run.runtimes", "list at least one runtime"));
        }
        if let Some(r) = self.runtimes.iter().find(|r| !is_identifier(r)) {
            return Err(invalid(
                "run.runtimes",
                format!("{r:?} is not a runtime id"),
            ));
        }
        if let Some(s) = self.skip.iter().find(|s| !is_check_id(s)) {
            return Err(invalid(
                "checks.skip",
                format!("{s:?} is not a check id like PN-PE-001"),
            ));
        }
        Ok(())
    }

    /// The executable as a path inside the build directory.
    pub fn exe_path(&self) -> PathBuf {
        self.build_dir.join(&self.exe)
    }
}

/// The executable must stay inside the build directory: relative, no `..`,
/// and an `.exe`.
pub fn validate_exe_path(exe: &str) -> Result<(), ConfigError> {
    if exe.is_empty() || exe.contains('\0') {
        return Err(invalid("build.exe", "must not be empty"));
    }
    let normalized = exe.replace('\\', "/");
    let path = Path::new(&normalized);
    if path.is_absolute() || normalized.contains(':') {
        return Err(invalid(
            "build.exe",
            "must be relative to the build directory",
        ));
    }
    if path
        .components()
        .any(|c| !matches!(c, Component::Normal(_) | Component::CurDir))
    {
        return Err(invalid("build.exe", "must not contain '..'"));
    }
    if !normalized.to_ascii_lowercase().ends_with(".exe") {
        return Err(invalid("build.exe", "must be a .exe file"));
    }
    Ok(())
}

fn is_identifier(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 64
        && s.bytes().all(|b| {
            b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'.' || b == b'_'
        })
}

fn is_check_id(s: &str) -> bool {
    s.starts_with("PN-")
        && s.len() <= 32
        && s.bytes()
            .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'-')
}

pub const STARTER_CONFIG: &str = r#"# Protonaut configuration. Every key is optional except where noted;
# command-line flags override these values.
schema_version = 1

[build]
# Directory with your packaged Windows build, relative to this file. Required
# here or as the path argument.
dir = "dist/Windows"
# Executable inside that directory. Required here or as --exe.
exe = "MyGame.exe"
args = []

[run]
# "stable" is the newest runtime this release has verified.
runtimes = ["stable"]
# Seconds to watch the game after launch.
observe_secs = 20
# 1280x800 (Steam Deck), 1280x720 or 1920x1080.
resolution = "1280x800"
# "fail" fails the run on FAIL results only; "warning" also on WARNING.
fail_on = "fail"

[checks]
skip = []
"#;

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn resolve(text: &str) -> Result<Config, ConfigError> {
        Config::resolve(
            ConfigFile::parse(text, "t.toml")?,
            Overrides::default(),
            Path::new("/base"),
        )
    }

    #[test]
    fn starter_config_is_valid() {
        let c = resolve(STARTER_CONFIG).unwrap();
        assert_eq!(c.build_dir, PathBuf::from("/base/dist/Windows"));
        assert_eq!(c.exe, "MyGame.exe");
        assert_eq!(c.resolution, Resolution::DECK);
        assert_eq!(c.observe_secs, 20);
    }

    #[test]
    fn minimal_file_uses_defaults() {
        let c = resolve("[build]\ndir='b'\nexe='g.exe'\n").unwrap();
        assert_eq!(c.runtimes, vec!["stable"]);
        assert_eq!(c.fail_on, FailOn::Fail);
    }

    #[test]
    fn overrides_win_over_the_file() {
        let file = ConfigFile::parse(
            "[build]\ndir='b'\nexe='g.exe'\n[run]\nobserve_secs=30\n",
            "t",
        )
        .unwrap();
        let o = Overrides {
            observe_secs: Some(5),
            exe: Some("other.exe".into()),
            ..Default::default()
        };
        let c = Config::resolve(file, o, Path::new("/x")).unwrap();
        assert_eq!((c.observe_secs, c.exe.as_str()), (5, "other.exe"));
    }

    #[test]
    fn unknown_keys_are_rejected() {
        let err = ConfigFile::parse("[run]\nobserve_sec = 5\n", "t").unwrap_err();
        assert!(matches!(err, ConfigError::Parse { .. }), "{err:?}");
    }

    #[test]
    fn future_schema_version_is_rejected() {
        assert_eq!(
            ConfigFile::parse("schema_version = 2\n", "t").unwrap_err(),
            ConfigError::SchemaVersion(2)
        );
    }

    #[test]
    fn exe_must_stay_inside_build_dir() {
        for bad in [
            "../evil.exe",
            "/abs/game.exe",
            "C:\\game.exe",
            "a/../../b.exe",
            "game.bat",
            "",
            "x\\..\\..\\y.exe",
        ] {
            assert!(
                validate_exe_path(bad).is_err(),
                "{bad:?} should be rejected"
            );
        }
        for good in [
            "Game.exe",
            "bin/x64/Game.EXE",
            "./Game.exe",
            "bin\\Game.exe",
        ] {
            assert!(
                validate_exe_path(good).is_ok(),
                "{good:?} should be accepted"
            );
        }
    }

    #[test]
    fn observe_window_is_bounded() {
        assert!(resolve("[build]\ndir='b'\nexe='g.exe'\n[run]\nobserve_secs=2\n").is_err());
        assert!(resolve("[build]\ndir='b'\nexe='g.exe'\n[run]\nobserve_secs=601\n").is_err());
        assert!(resolve("[build]\ndir='b'\nexe='g.exe'\n[run]\nobserve_secs=3\n").is_ok());
    }

    #[test]
    fn resolution_accepts_only_supported_modes() {
        assert_eq!("1920x1080".parse::<Resolution>().unwrap().width, 1920);
        assert!("800x600".parse::<Resolution>().is_err());
        assert!("1280".parse::<Resolution>().is_err());
    }

    #[test]
    fn runtime_ids_and_skip_ids_are_validated() {
        assert!(resolve("[build]\ndir='b'\nexe='g.exe'\n[run]\nruntimes=['Bad Id']\n").is_err());
        assert!(resolve("[build]\ndir='b'\nexe='g.exe'\n[run]\nruntimes=[]\n").is_err());
        assert!(resolve("[build]\ndir='b'\nexe='g.exe'\n[checks]\nskip=['rm -rf']\n").is_err());
    }

    #[test]
    fn missing_build_or_exe_is_a_clear_error() {
        let e = resolve("").unwrap_err();
        assert!(e.to_string().contains("build directory"), "{e}");
        let e = resolve("[build]\ndir='b'\n").unwrap_err();
        assert!(e.to_string().contains("--exe"), "{e}");
    }
}
