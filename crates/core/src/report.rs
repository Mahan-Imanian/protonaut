//! Assembling a report and rendering it as Markdown, JUnit XML or HTML.
//! Strings in a report can come from the build under test (file names, log
//! lines), so every renderer escapes them for its format.

use std::fmt::Write as _;

use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

use crate::model::{
    Category, CheckResult, EnvironmentInfo, FailOn, LIMITATIONS, REPORT_SCHEMA_VERSION, Report,
    RunSummary, Status, StatusCounts, Summary, TargetInfo, ToolInfo, Verdict, verdict,
};

pub const TOOL_NAME: &str = "protonaut";
/// Hidden marker used to find and update an earlier PR comment.
pub const COMMENT_MARKER: &str = "<!-- protonaut-report -->";
const MAX_TEXT: usize = 300;

pub struct ReportInput {
    pub tool_version: String,
    pub target: TargetInfo,
    pub environment: EnvironmentInfo,
    pub runs: Vec<RunSummary>,
    pub checks: Vec<CheckResult>,
    pub fail_on: FailOn,
    pub skip: Vec<String>,
}

pub fn assemble(input: ReportInput) -> Report {
    let mut checks: Vec<CheckResult> = input
        .checks
        .into_iter()
        .filter(|c| !input.skip.contains(&c.id))
        .collect();
    checks.sort_by(|a, b| {
        (category_rank(a.category), &a.runtime, &a.id).cmp(&(
            category_rank(b.category),
            &b.runtime,
            &b.id,
        ))
    });
    let summary = Summary {
        verdict: verdict(&checks, input.fail_on),
        counts: StatusCounts::from_checks(&checks),
    };
    Report {
        schema_version: REPORT_SCHEMA_VERSION,
        tool: ToolInfo {
            name: TOOL_NAME.to_owned(),
            version: input.tool_version,
        },
        generated_at: OffsetDateTime::now_utc()
            .replace_nanosecond(0)
            .unwrap_or_else(|_| OffsetDateTime::now_utc())
            .format(&Rfc3339)
            .unwrap_or_else(|_| "unknown".to_owned()),
        target: input.target,
        environment: input.environment,
        runs: input.runs,
        checks,
        summary,
        limitations: LIMITATIONS.iter().map(|s| (*s).to_owned()).collect(),
    }
}

fn category_rank(c: Category) -> u8 {
    match c {
        Category::Static => 0,
        Category::Runtime => 1,
        Category::Display => 2,
        Category::Hardware => 3,
    }
}

/// The forms in which Wine prints a Linux path: as is, and on drive `Z:`
/// with single or escaped backslashes (`Z:\\tmp\\x` in `debugstr` output).
pub fn path_variants(path: &str, placeholder: &'static str) -> Vec<(String, &'static str)> {
    let windows = format!("Z:{}", path.replace('/', "\\"));
    let escaped = format!("Z:{}", path.replace('/', "\\\\"));
    vec![
        (path.to_owned(), placeholder),
        (windows, placeholder),
        (escaped, placeholder),
    ]
}

/// Replaces each occurrence of a private value (a host path, a user name)
/// with a placeholder before the text is written to a report or artifact.
pub fn redact(text: &str, secrets: &[(String, &str)]) -> String {
    let mut out = text.to_owned();
    let mut ordered: Vec<&(String, &str)> = secrets.iter().filter(|(s, _)| s.len() >= 2).collect();
    ordered.sort_by_key(|(s, _)| std::cmp::Reverse(s.len()));
    for (value, placeholder) in ordered {
        out = out.replace(value.as_str(), placeholder);
    }
    out
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_owned()
    } else {
        let mut t: String = s.chars().take(max).collect();
        t.push('…');
        t
    }
}

/// Escapes text for GitHub-flavoured Markdown: no HTML, links, tables,
/// emphasis or @mentions can be smuggled in through build file names.
pub fn md_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in truncate(s, MAX_TEXT).chars() {
        match ch {
            '\\' | '`' | '*' | '_' | '{' | '}' | '[' | ']' | '(' | ')' | '#' | '+' | '!' | '|'
            | '~' => {
                out.push('\\');
                out.push(ch);
            }
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '@' => out.push_str("@\u{200b}"),
            '&' => out.push_str("&amp;"),
            '\n' | '\r' | '\t' => out.push(' '),
            c if c.is_control() => {}
            c => out.push(c),
        }
    }
    out
}

/// The one-line result. A run with warnings is never summarised as a plain
/// pass: warnings are things a person has to look at.
pub fn headline(r: &Report) -> String {
    let n = |s: Status| r.checks.iter().filter(|c| c.status == s).count();
    let (fails, warns) = (n(Status::Fail), n(Status::Warning));
    let plural =
        |k: usize, one: &str, many: &str| format!("{k} {}", if k == 1 { one } else { many });
    match r.summary.verdict {
        Verdict::Fail if fails > 0 => format!("{} failed", plural(fails, "check", "checks")),
        Verdict::Fail => format!("{} failed the run", plural(warns, "warning", "warnings")),
        Verdict::Pass if warns > 0 => format!(
            "No failures, {} to review",
            plural(warns, "warning", "warnings")
        ),
        Verdict::Pass => "No failures".to_owned(),
    }
}

/// Splits a summary into its first sentence and the rest, so a problem row
/// can lead with what happened.
pub fn first_sentence(text: &str) -> (String, &str) {
    let stop = [". ", "; "].iter().filter_map(|p| text.find(p)).min();
    match stop {
        Some(i) => (format!("{}.", &text[..i]), text[i + 2..].trim_start()),
        None => (text.to_owned(), ""),
    }
}

/// The check catalogue; each check id has an anchor there.
pub const CHECKS_URL: &str =
    "https://github.com/Mahan-Imanian/protonaut/blob/main/docs/reference/checks.md";

/// The catalogue entry for a check id.
pub fn check_url(id: &str) -> String {
    format!("{CHECKS_URL}#{}", id.to_ascii_lowercase())
}

/// OCR text often starts or ends with a stray curly quote mark.
pub fn clean_detail(d: &str) -> &str {
    if d.matches('“').count() == d.matches('”').count() {
        return d;
    }
    d.trim_matches(|c: char| matches!(c, '“' | '”') || c.is_whitespace())
}

/// A detail line as short outputs show it. For OCR text the line starts at
/// the first error-like word, because the text before it is usually noise;
/// the HTML report keeps the whole line.
pub fn short_detail<'a>(c: &CheckResult, d: &'a str) -> std::borrow::Cow<'a, str> {
    let d = clean_detail(d);
    if c.id != "PN-DISP-005" {
        return std::borrow::Cow::Borrowed(d);
    }
    let lower = d.to_lowercase();
    let start = [
        "error",
        "fail",
        "cannot",
        "can't",
        "not found",
        "missing",
        "requires",
    ]
    .iter()
    .filter_map(|k| lower.find(k))
    .min();
    match start {
        Some(i) if i > 0 && d.is_char_boundary(i) => {
            let head = d[..i].rfind(' ').map_or(i, |sp| sp + 1);
            std::borrow::Cow::Owned(format!("…{}", d[head..].trim_end_matches(['>', ' '])))
        }
        _ => std::borrow::Cow::Borrowed(d),
    }
}

/// Detail lines worth showing in short outputs. For OCR only the first
/// match is shown; the rest is usually noise.
pub fn detail_limit(c: &CheckResult, max: usize) -> usize {
    if c.id == "PN-DISP-005" { 1 } else { max }
}

/// What to do next for a failure or a warning, limited to what the evidence
/// supports. Runtime problems also get a command that reproduces the run.
pub fn next_step(
    c: &CheckResult,
    exe: &str,
    runtime: Option<&str>,
) -> Option<(&'static str, Option<String>)> {
    let repro = runtime.map(|rt| format!("protonaut test <build dir> --exe {exe} --runtime {rt}"));
    if c.status == Status::Warning {
        return Some(match c.id.as_str() {
            "PN-DISP-002" => (
                "Open the frame in the report: a long loading screen and a rendering failure both look black.",
                repro,
            ),
            "PN-DISP-003" => (
                "Default to the display resolution, or to 1280x800, on first launch.",
                None,
            ),
            "PN-DISP-004" => (
                "Look for an error, launcher or hardware warning window at start-up and remove it.",
                repro,
            ),
            "PN-DISP-005" => (
                "Open the frame in the report. If the text is a real error, the build starts without something it needs.",
                repro,
            ),
            "PN-AC-001" => (
                "Check with the anti-cheat vendor whether Proton support is enabled for your game.",
                None,
            ),
            "PN-LAUNCH-001" => (
                "Make sure the exe input points at the game itself, not a launcher.",
                None,
            ),
            "PN-MEDIA-001" => (
                "Test video playback under Proton, or ship the videos in a format that does not need Media Foundation.",
                None,
            ),
            _ => ("Open the evidence in the report.", None),
        });
    }
    if c.status != Status::Fail {
        return None;
    }
    Some(match c.id.as_str() {
        "PN-RUN-001" | "PN-RUN-002" | "PN-RUN-003" | "PN-DISP-001" => (
            "Read the crash line in the runtime log; the command below reproduces the run.",
            repro,
        ),
        "PN-PE-001" | "PN-PE-002" => (
            "Check that the exe input points at the Windows build you meant to test.",
            None,
        ),
        "PN-PE-003" => (
            "Ship the missing DLLs with the build, or remove the dependency.",
            None,
        ),
        _ => (
            "Open the evidence above and the runtime log for this run.",
            None,
        ),
    })
}

/// The PR comment body.
pub fn render_markdown(r: &Report, artifact_url: Option<&str>) -> String {
    crate::markdown::comment(r, artifact_url)
}

/// The GitHub Actions job summary, which has more room than a comment.
pub fn render_summary(r: &Report, artifact_url: Option<&str>) -> String {
    crate::markdown::summary(r, artifact_url)
}

pub fn xml_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            '\n' | '\t' => out.push(ch),
            c if c.is_control() => {}
            c => out.push(c),
        }
    }
    out
}

/// FAIL becomes a JUnit failure; UNKNOWN and NOT_TESTED become skipped, so CI
/// test viewers never count them as passes.
pub fn render_junit(r: &Report) -> String {
    let c = &r.summary.counts;
    let mut s = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
    let _ = writeln!(
        s,
        "<testsuites name=\"protonaut\" tests=\"{}\" failures=\"{}\" skipped=\"{}\">",
        r.checks.len(),
        c.fail,
        c.unknown + c.not_tested
    );
    let _ = writeln!(
        s,
        "  <testsuite name=\"protonaut\" tests=\"{}\" failures=\"{}\" skipped=\"{}\">",
        r.checks.len(),
        c.fail,
        c.unknown + c.not_tested
    );
    for check in &r.checks {
        let name = match &check.runtime {
            Some(rt) => format!("{} [{}] {}", check.id, rt, check.title),
            None => format!("{} {}", check.id, check.title),
        };
        let _ = write!(
            s,
            "    <testcase classname=\"protonaut.{:?}\" name=\"{}\"",
            check.category,
            xml_escape(&name)
        );
        let body = format!("{}\n{}", check.summary, check.details.join("\n"));
        match check.status {
            Status::Pass => {
                let _ = writeln!(s, "/>");
            }
            Status::Fail => {
                let _ = writeln!(
                    s,
                    ">\n      <failure message=\"{}\">{}</failure>\n    </testcase>",
                    xml_escape(&check.summary),
                    xml_escape(&body)
                );
            }
            Status::Warning => {
                let _ = writeln!(
                    s,
                    ">\n      <system-out>WARNING: {}</system-out>\n    </testcase>",
                    xml_escape(&body)
                );
            }
            Status::Unknown | Status::NotTested => {
                let _ = writeln!(
                    s,
                    ">\n      <skipped message=\"{}: {}\"/>\n    </testcase>",
                    check.status.as_str(),
                    xml_escape(&check.summary)
                );
            }
        }
    }
    s.push_str("  </testsuite>\n</testsuites>\n");
    s
}

pub fn html_escape(s: &str) -> String {
    xml_escape(s).replace('\n', "<br>")
}

/// Artifact paths are produced by this tool, but they are re-validated here
/// so a tampered report cannot point an <img> outside the results folder.
pub fn is_safe_relative_path(p: &str) -> bool {
    !p.is_empty()
        && !p.starts_with('/')
        && !p.split('/').any(|seg| seg.is_empty() || seg == "..")
        && p.bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-' | b'/'))
}

pub fn render_html(r: &Report) -> String {
    crate::html::render(r)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::model::{Evidence, RunOutcome};

    fn sample(status: Status) -> Report {
        let check = CheckResult::new(
            "PN-AC-001",
            "Anti-cheat",
            Category::Static,
            status,
            Evidence::Static,
            "found",
        )
        .with_details(vec![
            "@octocat <img src=x onerror=alert(1)> [x](javascript:1)".into(),
        ]);
        assemble(ReportInput {
            tool_version: "0.1.0".into(),
            target: TargetInfo {
                exe: "Game.exe".into(),
                args: vec![],
            },
            environment: EnvironmentInfo::default(),
            runs: vec![RunSummary {
                runtime: "ge-proton11-12".into(),
                runtime_version: "GE-Proton11-12".into(),
                outcome: RunOutcome::Survived,
                exit_code: None,
                duration_ms: 20_000,
                capture_verified: Some(true),
            }],
            checks: vec![check],
            fail_on: FailOn::Fail,
            skip: vec![],
        })
    }

    #[test]
    fn markdown_neutralises_mentions_links_and_html() {
        let md = render_markdown(&sample(Status::Warning), None);
        assert!(!md.contains("@octocat"), "{md}");
        assert!(!md.contains("<img"), "{md}");
        assert!(!md.contains("](javascript"), "{md}");
        assert!(md.starts_with(COMMENT_MARKER));
    }

    #[test]
    fn warnings_are_never_summarised_as_a_plain_pass() {
        assert_eq!(headline(&sample(Status::Pass)), "No failures");
        let w = sample(Status::Warning);
        assert_eq!(headline(&w), "No failures, 1 warning to review");
        assert!(render_markdown(&w, None).contains("Protonaut: No failures, 1 warning to review"));
        assert!(render_html(&w).contains("head s-warning"));
        assert_eq!(headline(&sample(Status::Fail)), "1 check failed");
    }

    #[test]
    fn markdown_only_links_https_artifacts() {
        assert!(
            !render_markdown(&sample(Status::Pass), Some("javascript:alert(1)"))
                .contains("](javascript")
        );
        assert!(
            !render_markdown(&sample(Status::Pass), Some("javascript:alert(1)"))
                .contains("artifacts of this run")
        );
        assert!(
            render_markdown(
                &sample(Status::Pass),
                Some("https://github.com/o/r/actions/runs/1")
            )
            .contains("actions/runs/1")
        );
    }

    #[test]
    fn junit_maps_statuses_without_counting_unknown_as_pass() {
        let x = render_junit(&sample(Status::Unknown));
        assert!(x.contains("<skipped message=\"UNKNOWN"), "{x}");
        assert!(x.contains("skipped=\"1\""));
        let x = render_junit(&sample(Status::Fail));
        assert!(x.contains("<failure"));
        assert!(x.contains("&lt;img"));
    }

    #[test]
    fn html_escapes_and_rejects_unsafe_artifact_paths() {
        let mut r = sample(Status::Pass);
        r.checks[0].artifacts = vec![
            "../../etc/passwd.png".into(),
            "runs/ge/shot.png".into(),
            "a\"onerror=x.png".into(),
        ];
        let h = render_html(&r);
        assert!(!h.contains("<img src=x"), "unescaped detail");
        assert!(!h.contains("etc/passwd"));
        assert!(!h.contains("onerror=x.png"));
        assert!(h.contains("src=\"runs/ge/shot.png\""));
    }

    #[test]
    fn html_lists_problems_before_passes() {
        let mut r = sample(Status::Pass);
        r.checks.push(CheckResult::new(
            "PN-RUN-002",
            "Stability",
            Category::Runtime,
            Status::Fail,
            Evidence::Observed,
            "crashed",
        ));
        let h = render_html(&r);
        assert!(h.find("PN-RUN-002").unwrap() < h.find("PN-AC-001").unwrap());
    }

    #[test]
    fn timestamp_has_whole_seconds() {
        let t = sample(Status::Pass).generated_at;
        assert!(t.ends_with('Z') && !t.contains('.'), "{t}");
    }

    #[test]
    fn skipped_checks_are_removed_and_order_is_stable() {
        let mut input_checks = vec![
            CheckResult::new(
                "PN-RUN-001",
                "r",
                Category::Runtime,
                Status::Pass,
                Evidence::Observed,
                "s",
            ),
            CheckResult::new(
                "PN-PE-001",
                "p",
                Category::Static,
                Status::Pass,
                Evidence::Static,
                "s",
            ),
            CheckResult::new(
                "PN-MEDIA-001",
                "m",
                Category::Static,
                Status::Warning,
                Evidence::Heuristic,
                "s",
            ),
        ];
        input_checks.reverse();
        let r = assemble(ReportInput {
            tool_version: "t".into(),
            target: TargetInfo {
                exe: "g.exe".into(),
                args: vec![],
            },
            environment: EnvironmentInfo::default(),
            runs: vec![],
            checks: input_checks,
            fail_on: FailOn::Fail,
            skip: vec!["PN-MEDIA-001".into()],
        });
        let ids: Vec<_> = r.checks.iter().map(|c| c.id.as_str()).collect();
        assert_eq!(ids, vec!["PN-PE-001", "PN-RUN-001"]);
        assert_eq!(r.summary.counts.warning, 0);
    }

    #[test]
    fn redact_replaces_longest_secret_first() {
        let secrets = vec![
            ("/home/runner".to_string(), "~"),
            ("/home/runner/work/game".to_string(), "<build>"),
        ];
        assert_eq!(
            redact("at /home/runner/work/game/x and /home/runner/y", &secrets),
            "at <build>/x and ~/y"
        );
    }

    #[test]
    fn wine_style_paths_are_redacted() {
        let secrets = path_variants("/home/runner/work/game", "<build>");
        let log = r#"needed by L"Z:\\home\\runner\\work\\game\\Game.exe" and Z:\home\runner\work\game\a.dll and /home/runner/work/game/b"#;
        let out = redact(log, &secrets);
        assert!(!out.contains("runner"), "{out}");
        assert_eq!(out.matches("<build>").count(), 3);
    }

    #[test]
    fn safe_relative_path_rules() {
        assert!(is_safe_relative_path(
            "runs/ge-proton11-12/screenshot-1280x800.png"
        ));
        for bad in [
            "",
            "/abs.png",
            "a/../b.png",
            "a//b",
            "a b.png",
            "javascript:x",
        ] {
            assert!(!is_safe_relative_path(bad), "{bad}");
        }
    }
}
