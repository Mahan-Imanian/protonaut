//! The versioned report model. Everything here is part of the public JSON
//! report contract described in `docs/reference/report-format.md`.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Bumped only for incompatible changes to the report. Additive fields keep
/// the same version; consumers must ignore fields they do not know.
pub const REPORT_SCHEMA_VERSION: u32 = 1;

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Status {
    /// The failure was directly observed.
    Fail,
    /// A potential problem that needs a human to investigate.
    Warning,
    /// The check ran but the evidence was insufficient for a verdict.
    Unknown,
    /// The check could not run in this environment.
    NotTested,
    /// The expected behaviour was directly observed.
    Pass,
}

impl Status {
    pub fn as_str(self) -> &'static str {
        match self {
            Status::Pass => "PASS",
            Status::Fail => "FAIL",
            Status::Warning => "WARNING",
            Status::Unknown => "UNKNOWN",
            Status::NotTested => "NOT_TESTED",
        }
    }
}

/// How the result was obtained. A heuristic result is never presented as
/// deterministic validation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Evidence {
    /// Deterministic inspection of the build files.
    Static,
    /// Directly observed while running the build.
    Observed,
    /// Image analysis or pattern matching that can be wrong.
    Heuristic,
    /// No evidence was gathered.
    None,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Category {
    Static,
    Runtime,
    Display,
    Hardware,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct CheckResult {
    /// Stable identifier, for example `SP-PE-002`. Never reused for a
    /// different meaning.
    pub id: String,
    pub title: String,
    pub category: Category,
    pub status: Status,
    pub evidence: Evidence,
    /// Runtime the check ran under; absent for static checks.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runtime: Option<String>,
    pub summary: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub details: Vec<String>,
    /// Paths relative to the results directory.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub artifacts: Vec<String>,
}

impl CheckResult {
    pub fn new(
        id: &str,
        title: &str,
        category: Category,
        status: Status,
        evidence: Evidence,
        summary: impl Into<String>,
    ) -> Self {
        Self {
            id: id.to_owned(),
            title: title.to_owned(),
            category,
            status,
            evidence,
            runtime: None,
            summary: summary.into(),
            details: Vec::new(),
            artifacts: Vec::new(),
        }
    }

    pub fn with_runtime(mut self, runtime: &str) -> Self {
        self.runtime = Some(runtime.to_owned());
        self
    }

    pub fn with_details(mut self, details: Vec<String>) -> Self {
        self.details = details;
        self
    }

    pub fn with_artifact(mut self, path: impl Into<String>) -> Self {
        self.artifacts.push(path.into());
        self
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ToolInfo {
    pub name: String,
    pub version: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct TargetInfo {
    /// Executable path relative to the build directory.
    pub exe: String,
    pub args: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct EnvironmentInfo {
    pub os: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kernel: Option<String>,
    /// Vulkan device the runtime rendered on, when observed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vulkan_device: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum RunOutcome {
    /// Still running when the observation window ended.
    Survived,
    /// Exited with code 0 before the window ended.
    ExitedCleanly,
    /// Exited with a nonzero code.
    ExitedWithError,
    /// Terminated by an unhandled exception or signal.
    Crashed,
    /// The runtime itself could not start the build.
    LaunchFailed,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct RunSummary {
    pub runtime: String,
    pub runtime_version: String,
    pub outcome: RunOutcome,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exit_code: Option<i32>,
    pub duration_ms: u64,
    /// Whether the control fixture's frame was captured correctly under this
    /// runtime. Screenshot checks are only trusted when this is true.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub capture_verified: Option<bool>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct StatusCounts {
    pub pass: u32,
    pub fail: u32,
    pub warning: u32,
    pub unknown: u32,
    pub not_tested: u32,
}

impl StatusCounts {
    pub fn from_checks(checks: &[CheckResult]) -> Self {
        let mut c = Self::default();
        for check in checks {
            match check.status {
                Status::Pass => c.pass += 1,
                Status::Fail => c.fail += 1,
                Status::Warning => c.warning += 1,
                Status::Unknown => c.unknown += 1,
                Status::NotTested => c.not_tested += 1,
            }
        }
        c
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Verdict {
    Pass,
    Fail,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Summary {
    pub verdict: Verdict,
    pub counts: StatusCounts,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Report {
    pub schema_version: u32,
    pub tool: ToolInfo,
    /// RFC 3339 UTC timestamp.
    pub generated_at: String,
    pub target: TargetInfo,
    pub environment: EnvironmentInfo,
    pub runs: Vec<RunSummary>,
    pub checks: Vec<CheckResult>,
    pub summary: Summary,
    /// Things this report cannot tell you, always included.
    pub limitations: Vec<String>,
}

/// What makes the verdict `fail`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum FailOn {
    #[default]
    Fail,
    Warning,
}

pub fn verdict(checks: &[CheckResult], fail_on: FailOn) -> Verdict {
    let failing = checks.iter().any(|c| match fail_on {
        FailOn::Fail => c.status == Status::Fail,
        FailOn::Warning => matches!(c.status, Status::Fail | Status::Warning),
    });
    if failing {
        Verdict::Fail
    } else {
        Verdict::Pass
    }
}

pub const LIMITATIONS: &[&str] = &[
    "This is not a Valve review and cannot predict a Steam Deck Verified or Steam Machine Verified rating.",
    "Rendering uses Mesa lavapipe (CPU software Vulkan): real GPU driver behaviour and performance are not tested.",
    "Frame rate, controller feel, touch input, suspend/resume and battery use need physical hardware.",
    "Proton builds come from GE-Proton, not the Valve Proton build Steam ships; results can differ.",
    "Steam client features (Steamworks API calls) are unavailable because no Steam client runs.",
];

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn check(status: Status) -> CheckResult {
        CheckResult::new(
            "SP-T-001",
            "t",
            Category::Static,
            status,
            Evidence::Static,
            "s",
        )
    }

    #[test]
    fn unknown_and_not_tested_never_fail_the_verdict() {
        let checks = vec![
            check(Status::Unknown),
            check(Status::NotTested),
            check(Status::Pass),
        ];
        assert_eq!(verdict(&checks, FailOn::Fail), Verdict::Pass);
        assert_eq!(verdict(&checks, FailOn::Warning), Verdict::Pass);
    }

    #[test]
    fn warnings_fail_only_when_configured() {
        let checks = vec![check(Status::Warning)];
        assert_eq!(verdict(&checks, FailOn::Fail), Verdict::Pass);
        assert_eq!(verdict(&checks, FailOn::Warning), Verdict::Fail);
    }

    #[test]
    fn fail_always_fails() {
        assert_eq!(verdict(&[check(Status::Fail)], FailOn::Fail), Verdict::Fail);
    }

    #[test]
    fn status_serializes_in_screaming_case() {
        assert_eq!(
            serde_json::to_string(&Status::NotTested).unwrap(),
            "\"NOT_TESTED\""
        );
        assert_eq!(Status::NotTested.as_str(), "NOT_TESTED");
    }

    #[test]
    fn counts_cover_every_status() {
        let checks: Vec<_> = [
            Status::Pass,
            Status::Fail,
            Status::Warning,
            Status::Unknown,
            Status::NotTested,
        ]
        .into_iter()
        .map(check)
        .collect();
        let c = StatusCounts::from_checks(&checks);
        assert_eq!(
            (c.pass, c.fail, c.warning, c.unknown, c.not_tested),
            (1, 1, 1, 1, 1)
        );
    }
}
