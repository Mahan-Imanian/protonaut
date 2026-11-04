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

pub const TOOL_NAME: &str = "steamos-preflight";
/// Hidden marker used to find and update an earlier PR comment.
pub const COMMENT_MARKER: &str = "<!-- steamos-preflight-report -->";
const MAX_DETAILS_RENDERED: usize = 10;
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

fn status_icon(s: Status) -> &'static str {
    match s {
        Status::Pass => "✅",
        Status::Fail => "❌",
        Status::Warning => "⚠️",
        Status::Unknown => "❔",
        Status::NotTested => "⏭️",
    }
}

pub fn render_markdown(r: &Report, artifact_url: Option<&str>) -> String {
    let mut s = String::new();
    let c = &r.summary.counts;
    let head = match r.summary.verdict {
        Verdict::Pass => "✅ SteamOS Preflight: no failures",
        Verdict::Fail => "❌ SteamOS Preflight: failures found",
    };
    let _ = writeln!(s, "{COMMENT_MARKER}\n## {head}\n");
    let _ = writeln!(
        s,
        "`{}` · {} pass · {} fail · {} warning · {} unknown · {} not tested\n",
        md_escape(&r.target.exe).replace('`', ""),
        c.pass,
        c.fail,
        c.warning,
        c.unknown,
        c.not_tested
    );
    if !r.runs.is_empty() {
        let _ = writeln!(
            s,
            "| Runtime | Outcome | Exit code | Duration | Screenshot capture |\n|---|---|---|---|---|"
        );
        for run in &r.runs {
            let capture = match run.capture_verified {
                Some(true) => "verified",
                Some(false) => "not working in this runtime",
                None => "not checked",
            };
            let _ = writeln!(
                s,
                "| {} ({}) | {:?} | {} | {:.1} s | {} |",
                md_escape(&run.runtime),
                md_escape(&run.runtime_version),
                run.outcome,
                run.exit_code.map_or("–".to_owned(), |c| c.to_string()),
                run.duration_ms as f64 / 1000.0,
                capture
            );
        }
        s.push('\n');
    }
    let important: Vec<&CheckResult> = r
        .checks
        .iter()
        .filter(|c| matches!(c.status, Status::Fail | Status::Warning))
        .collect();
    if !important.is_empty() {
        let _ = writeln!(s, "### Needs attention\n");
        for check in &important {
            write_check_line(&mut s, check);
        }
        s.push('\n');
    }
    let rest: Vec<&CheckResult> = r
        .checks
        .iter()
        .filter(|c| !matches!(c.status, Status::Fail | Status::Warning))
        .collect();
    if !rest.is_empty() {
        let _ = writeln!(
            s,
            "<details><summary>Other checks ({})</summary>\n",
            rest.len()
        );
        for check in &rest {
            write_check_line(&mut s, check);
        }
        let _ = writeln!(s, "\n</details>\n");
    }
    if let Some(url) = artifact_url.filter(|u| u.starts_with("https://")) {
        let _ = writeln!(
            s,
            "Screenshots, logs and the full report: [workflow artifacts]({})\n",
            url.replace(['(', ')', ' '], "")
        );
    }
    let _ = writeln!(
        s,
        "<sub>Independent community tool, not affiliated with Valve. It cannot predict a Steam Deck Verified rating; software rendering does not test GPU performance or controller feel.</sub>"
    );
    s
}

fn write_check_line(s: &mut String, c: &CheckResult) {
    let rt = c
        .runtime
        .as_deref()
        .map(|r| format!(" ({})", md_escape(r)))
        .unwrap_or_default();
    let ev = format!("{:?}", c.evidence).to_lowercase();
    let _ = writeln!(
        s,
        "- {} **{}** `{}`{} — {} _(evidence: {})_",
        status_icon(c.status),
        md_escape(&c.title),
        md_escape(&c.id).replace('`', ""),
        rt,
        md_escape(&c.summary),
        ev
    );
    for d in c.details.iter().take(MAX_DETAILS_RENDERED) {
        let _ = writeln!(s, "  - {}", md_escape(d));
    }
    if c.details.len() > MAX_DETAILS_RENDERED {
        let _ = writeln!(
            s,
            "  - … {} more in report.json",
            c.details.len() - MAX_DETAILS_RENDERED
        );
    }
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
        "<testsuites name=\"steamos-preflight\" tests=\"{}\" failures=\"{}\" skipped=\"{}\">",
        r.checks.len(),
        c.fail,
        c.unknown + c.not_tested
    );
    let _ = writeln!(
        s,
        "  <testsuite name=\"steamos-preflight\" tests=\"{}\" failures=\"{}\" skipped=\"{}\">",
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
            "    <testcase classname=\"steamos-preflight.{:?}\" name=\"{}\"",
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
    let mut s = String::new();
    let c = &r.summary.counts;
    let verdict = match r.summary.verdict {
        Verdict::Pass => "No failures",
        Verdict::Fail => "Failures found",
    };
    let _ = write!(
        s,
        "<!doctype html><html lang=\"en\"><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width,initial-scale=1\">\
<meta http-equiv=\"Content-Security-Policy\" content=\"default-src 'none'; img-src 'self'; style-src 'unsafe-inline'\">\
<title>SteamOS Preflight report</title><style>{CSS}</style></head><body><main>\
<h1>SteamOS Preflight <span class=\"v {}\">{}</span></h1>\
<p class=\"meta\"><code>{}</code> · {} · {} pass · {} fail · {} warning · {} unknown · {} not tested</p>",
        if r.summary.verdict == Verdict::Pass {
            "ok"
        } else {
            "bad"
        },
        verdict,
        html_escape(&r.target.exe),
        html_escape(&r.generated_at),
        c.pass,
        c.fail,
        c.warning,
        c.unknown,
        c.not_tested
    );
    if !r.runs.is_empty() {
        s.push_str("<h2>Runs</h2><table><tr><th>Runtime</th><th>Outcome</th><th>Exit</th><th>Duration</th><th>Capture</th></tr>");
        for run in &r.runs {
            let _ = write!(
                s,
                "<tr><td>{} <small>{}</small></td><td>{:?}</td><td>{}</td><td>{:.1} s</td><td>{}</td></tr>",
                html_escape(&run.runtime),
                html_escape(&run.runtime_version),
                run.outcome,
                run.exit_code.map_or("–".to_owned(), |c| c.to_string()),
                run.duration_ms as f64 / 1000.0,
                match run.capture_verified {
                    Some(true) => "verified",
                    Some(false) => "not working",
                    None => "not checked",
                }
            );
        }
        s.push_str("</table>");
    }
    s.push_str("<h2>Checks</h2><table><tr><th>Status</th><th>Check</th><th>Runtime</th><th>Result</th><th>Evidence</th></tr>");
    for check in &r.checks {
        let _ = write!(
            s,
            "<tr class=\"{}\"><td>{}</td><td><b>{}</b><br><code>{}</code></td><td>{}</td><td>{}",
            check.status.as_str().to_lowercase(),
            check.status.as_str(),
            html_escape(&check.title),
            html_escape(&check.id),
            html_escape(check.runtime.as_deref().unwrap_or("–")),
            html_escape(&check.summary)
        );
        if !check.details.is_empty() {
            s.push_str("<ul>");
            for d in check.details.iter().take(50) {
                let _ = write!(s, "<li>{}</li>", html_escape(d));
            }
            s.push_str("</ul>");
        }
        for a in check.artifacts.iter().filter(|a| is_safe_relative_path(a)) {
            if a.ends_with(".png") {
                let _ = write!(
                    s,
                    "<a href=\"{0}\"><img src=\"{0}\" alt=\"screenshot\" loading=\"lazy\"></a>",
                    html_escape(a)
                );
            } else {
                let _ = write!(s, "<br><a href=\"{0}\">{0}</a>", html_escape(a));
            }
        }
        let _ = write!(s, "</td><td>{:?}</td></tr>", check.evidence);
    }
    s.push_str("</table><h2>Limitations</h2><ul>");
    for l in &r.limitations {
        let _ = write!(s, "<li>{}</li>", html_escape(l));
    }
    let _ = writeln!(
        s,
        "</ul><p class=\"meta\">{} {} · report schema v{}</p></main></body></html>",
        html_escape(&r.tool.name),
        html_escape(&r.tool.version),
        r.schema_version
    );
    s
}

const CSS: &str = "body{font:15px/1.5 system-ui,sans-serif;margin:0;background:#f7f7f8;color:#1b1b1f}main{max-width:1100px;margin:0 auto;padding:16px}\
table{border-collapse:collapse;width:100%;background:#fff;margin:8px 0 24px}th,td{border:1px solid #ddd;padding:6px 8px;text-align:left;vertical-align:top}\
.meta{color:#555}.v{font-size:.6em;padding:2px 8px;border-radius:6px;vertical-align:middle}.ok{background:#d9f5df}.bad{background:#fbdcdc}\
tr.fail td:first-child{background:#fbdcdc}tr.warning td:first-child{background:#fff1cc}tr.pass td:first-child{background:#d9f5df}\
tr.unknown td:first-child,tr.not_tested td:first-child{background:#eceff3}img{max-width:320px;display:block;margin-top:6px;border:1px solid #ccc}\
@media (prefers-color-scheme:dark){body{background:#151518;color:#e8e8ec}table{background:#1f1f24}th,td{border-color:#3a3a42}.meta{color:#a0a0aa}\
.ok,tr.pass td:first-child{background:#1d3b25}.bad,tr.fail td:first-child{background:#4a2020}tr.warning td:first-child{background:#4a3c14}\
tr.unknown td:first-child,tr.not_tested td:first-child{background:#2a2d33}}";

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::model::{Evidence, RunOutcome};

    fn sample(status: Status) -> Report {
        let check = CheckResult::new(
            "SP-AC-001",
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
    fn markdown_only_links_https_artifacts() {
        assert!(
            !render_markdown(&sample(Status::Pass), Some("javascript:alert(1)"))
                .contains("](javascript")
        );
        assert!(
            !render_markdown(&sample(Status::Pass), Some("javascript:alert(1)"))
                .contains("workflow artifacts")
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
    fn skipped_checks_are_removed_and_order_is_stable() {
        let mut input_checks = vec![
            CheckResult::new(
                "SP-RUN-001",
                "r",
                Category::Runtime,
                Status::Pass,
                Evidence::Observed,
                "s",
            ),
            CheckResult::new(
                "SP-PE-001",
                "p",
                Category::Static,
                Status::Pass,
                Evidence::Static,
                "s",
            ),
            CheckResult::new(
                "SP-MEDIA-001",
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
            skip: vec!["SP-MEDIA-001".into()],
        });
        let ids: Vec<_> = r.checks.iter().map(|c| c.id.as_str()).collect();
        assert_eq!(ids, vec!["SP-PE-001", "SP-RUN-001"]);
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
