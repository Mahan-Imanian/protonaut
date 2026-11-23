//! The result summary printed by `protonaut test` and `protonaut inspect`.
//! Problems come first with their evidence and first log lines, then what
//! was not tested, then one line for passes, the next step, and a
//! pytest-style count line. Words and layout carry the meaning; colour only
//! repeats it.

use std::fmt::Write as _;

use crate::model::{Category, CheckResult, Evidence, Report, Status, Verdict};
use crate::report::{first_sentence, next_step};

const WIDTH: usize = 100;
const INDENT: usize = 24;

fn is_hardware(c: &CheckResult) -> bool {
    c.status == Status::NotTested && c.category == Category::Hardware
}

fn evidence_word(e: Evidence) -> &'static str {
    match e {
        Evidence::Observed => "observed",
        Evidence::Static => "from files",
        Evidence::Heuristic => "heuristic, can be wrong",
        Evidence::None => "no evidence",
    }
}

fn paint(text: &str, code: &str, color: bool) -> String {
    if color && !code.is_empty() {
        format!("\x1b[{code}m{text}\x1b[0m")
    } else {
        text.to_owned()
    }
}

/// The status word padded to one column. NOT_TESTED and UNKNOWN are never
/// dim: they are results a reader has to see. UNKNOWN uses normal cyan, which
/// themes that turn bold into bright do not wash out.
pub fn status_label(status: Status, color: bool) -> String {
    let code = match status {
        Status::Fail => "1;31",
        Status::Warning => "1;33",
        Status::Unknown => "36",
        Status::NotTested => "1",
        Status::Pass => "32",
    };
    paint(&format!("{:<10}", status.as_str()), code, color)
}

/// Breaks text into lines of at most `width` characters. A last line of a
/// single word takes a word from the line above, so no word stands alone.
fn wrap_lines(text: &str, width: usize) -> Vec<String> {
    let mut lines: Vec<Vec<&str>> = vec![Vec::new()];
    let mut len = 0;
    for word in text.split_whitespace() {
        let need = if len == 0 {
            word.len()
        } else {
            len + 1 + word.len()
        };
        if len > 0 && need > width {
            lines.push(Vec::new());
            len = 0;
        }
        len = if len == 0 {
            word.len()
        } else {
            len + 1 + word.len()
        };
        if let Some(last) = lines.last_mut() {
            last.push(word);
        }
    }
    let n = lines.len();
    if n >= 2
        && lines[n - 1].len() == 1
        && lines[n - 2].len() > 2
        && let Some(moved) = lines[n - 2].pop()
    {
        lines[n - 1].insert(0, moved);
    }
    lines
        .into_iter()
        .filter(|l| !l.is_empty())
        .map(|l| l.join(" "))
        .collect()
}

/// Writes `text` indented under the label, wrapped at word boundaries.
fn wrapped(s: &mut String, text: &str) {
    let pad = " ".repeat(INDENT);
    for line in wrap_lines(text, WIDTH - INDENT) {
        let _ = writeln!(s, "{pad}{line}");
    }
}

/// The label, the check id and text that wraps under the same indent.
fn labelled(s: &mut String, label: &str, id: &str, text: &str) {
    let pad = " ".repeat(INDENT);
    for (i, line) in wrap_lines(text, WIDTH - INDENT).iter().enumerate() {
        if i == 0 {
            let _ = writeln!(s, "{label} {id:<13}{line}");
        } else {
            let _ = writeln!(s, "{pad}{line}");
        }
    }
}

fn runtime_name<'a>(r: &'a Report, id: &'a str) -> &'a str {
    r.runs
        .iter()
        .find(|x| x.runtime == id)
        .map_or(id, |x| x.runtime_version.as_str())
}

/// `report_path` is shown as given, so the caller decides how to name it.
pub fn render(r: &Report, report_path: &str, color: bool) -> String {
    let mut s = String::new();
    let several = r.runs.len() > 1;
    let mut shown: Vec<(Vec<&String>, &str)> = Vec::new();
    for status in [Status::Fail, Status::Warning, Status::Unknown] {
        for c in r.checks.iter().filter(|c| c.status == status) {
            let (first, rest) = first_sentence(&c.summary);
            labelled(
                &mut s,
                &status_label(c.status, color),
                &c.id,
                first.trim_end_matches('.'),
            );
            let rt = match (&c.runtime, several) {
                (Some(x), true) => format!(", {}", runtime_name(r, x)),
                _ => String::new(),
            };
            wrapped(
                &mut s,
                &format!("{} ({}{rt})", c.title, evidence_word(c.evidence)),
            );
            if !rest.is_empty() {
                wrapped(&mut s, rest);
            }
            let lines: Vec<&String> = c
                .details
                .iter()
                .take(crate::report::detail_limit(c, 3))
                .collect();
            match shown.iter().find(|(l, _)| *l == lines) {
                Some((l, id)) => wrapped(
                    &mut s,
                    &format!(
                        "same log {} as {id}",
                        if l.len() == 1 { "line" } else { "lines" }
                    ),
                ),
                None => {
                    for d in &lines {
                        wrapped(&mut s, &crate::report::short_detail(c, d));
                    }
                    let more = c.details.len().saturating_sub(lines.len());
                    if more > 0 {
                        wrapped(&mut s, &format!("and {more} more in the report"));
                    }
                    if !lines.is_empty() {
                        shown.push((lines, c.id.as_str()));
                    }
                }
            }
        }
    }
    // Not-tested checks that share a reason are listed together, the reason once.
    let mut groups: Vec<(Vec<&CheckResult>, &str)> = Vec::new();
    for c in r
        .checks
        .iter()
        .filter(|c| c.status == Status::NotTested && !is_hardware(c))
    {
        match groups.iter_mut().find(|(_, why)| *why == c.summary) {
            Some((cs, _)) => cs.push(c),
            None => groups.push((vec![c], c.summary.as_str())),
        }
    }
    for (cs, why) in groups {
        for c in cs {
            let rt = match (&c.runtime, several) {
                (Some(x), true) => format!(" ({})", runtime_name(r, x)),
                _ => String::new(),
            };
            let _ = writeln!(
                s,
                "{} {:<13}{}{rt}",
                status_label(c.status, color),
                c.id,
                c.title
            );
        }
        wrapped(&mut s, why);
    }
    let mut hardware: Vec<&str> = r
        .checks
        .iter()
        .filter(|c| is_hardware(c))
        .map(|c| c.id.as_str())
        .collect();
    hardware.sort_unstable();
    hardware.dedup();
    if !hardware.is_empty() {
        let _ = writeln!(
            s,
            "{} {:<13}{} {} hardware: {}",
            status_label(Status::NotTested, color),
            "",
            hardware.len(),
            if hardware.len() == 1 { "needs" } else { "need" },
            hardware.join(", ")
        );
    }
    let passed = r.checks.iter().filter(|c| c.status == Status::Pass).count();
    if passed > 0 {
        let _ = writeln!(
            s,
            "{} {:<13}{passed} checks, listed in the report",
            status_label(Status::Pass, color),
            ""
        );
    }
    let mut steps: Vec<&str> = Vec::new();
    let mut repro: Option<String> = None;
    for status in [Status::Fail, Status::Warning] {
        for c in r.checks.iter().filter(|c| c.status == status) {
            if let Some((text, cmd)) = next_step(c, &r.target.exe, c.runtime.as_deref()) {
                if !steps.contains(&text) {
                    steps.push(text);
                }
                if repro.is_none() {
                    repro = cmd;
                }
            }
        }
    }
    if !steps.is_empty() {
        let _ = writeln!(s, "\nNext:");
        for text in steps {
            for (i, line) in wrap_lines(text, WIDTH - 4).iter().enumerate() {
                let _ = writeln!(s, "{}{line}", if i == 0 { "  - " } else { "    " });
            }
        }
        if let Some(cmd) = repro {
            let _ = writeln!(
                s,
                "  Reproduce the run on Ubuntu 24.04 with the same runtime:\n    {cmd}"
            );
        }
    }
    let count = |f: &dyn Fn(&CheckResult) -> bool| r.checks.iter().filter(|c| f(c)).count();
    let parts = [
        (
            count(&|c| c.status == Status::Fail),
            "failed",
            "failed",
            "1;31",
        ),
        (
            count(&|c| c.status == Status::Warning),
            "warning",
            "warnings",
            "1;33",
        ),
        (
            count(&|c| c.status == Status::Unknown),
            "unknown",
            "unknown",
            "36",
        ),
        (
            count(&|c| c.status == Status::NotTested && !is_hardware(c)),
            "not tested",
            "not tested",
            "1",
        ),
        (hardware.len(), "needs hardware", "need hardware", ""),
        (passed, "passed", "passed", "32"),
    ];
    let line = parts
        .iter()
        .filter(|(n, _, _, _)| *n > 0)
        .map(|(n, one, many, code)| {
            paint(
                &format!("{n} {}", if *n == 1 { one } else { many }),
                code,
                color,
            )
        })
        .collect::<Vec<_>>()
        .join(", ");
    let _ = writeln!(s, "\n{line}");
    if r.summary.verdict == Verdict::Pass && r.summary.counts.warning > 0 {
        let _ = writeln!(
            s,
            "Warnings do not fail the run; pass --fail-on warning to change that."
        );
    }
    let _ = writeln!(s, "Report: {report_path}");
    let _ = writeln!(s, "Checks explained: {}", crate::report::CHECKS_URL);
    s
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::model::{
        EnvironmentInfo, FailOn, REPORT_SCHEMA_VERSION, StatusCounts, Summary, TargetInfo,
        ToolInfo, verdict,
    };

    fn check(id: &str, status: Status, category: Category) -> CheckResult {
        CheckResult::new(id, "title", category, status, Evidence::Observed, "summary")
    }

    fn sample(checks: Vec<CheckResult>) -> Report {
        let summary = Summary {
            verdict: verdict(&checks, FailOn::Fail),
            counts: StatusCounts::from_checks(&checks),
        };
        Report {
            schema_version: REPORT_SCHEMA_VERSION,
            tool: ToolInfo {
                name: "protonaut".into(),
                version: "0.0.0".into(),
            },
            generated_at: "2026-01-01T00:00:00Z".into(),
            target: TargetInfo {
                exe: "Game.exe".into(),
                args: vec![],
            },
            environment: EnvironmentInfo::default(),
            runs: vec![],
            checks,
            summary,
            limitations: vec![],
        }
    }

    #[test]
    fn labels_are_plain_without_color_and_keep_padding_with_it() {
        assert_eq!(status_label(Status::Fail, false), "FAIL      ");
        assert_eq!(
            status_label(Status::Pass, true),
            "\x1b[32mPASS      \x1b[0m"
        );
    }

    #[test]
    fn not_tested_and_unknown_are_never_dimmed() {
        assert!(!status_label(Status::NotTested, true).contains("\x1b[2m"));
        assert!(!status_label(Status::Unknown, true).contains("\x1b[2m"));
    }

    #[test]
    fn problems_come_first_and_passes_are_counted_not_listed() {
        let r = sample(vec![
            check("PN-PE-001", Status::Pass, Category::Static),
            check("PN-HW-001", Status::NotTested, Category::Hardware),
            check("PN-DISP-002", Status::NotTested, Category::Display),
            check("PN-RUN-002", Status::Fail, Category::Runtime)
                .with_details(vec!["wine: crash".into()]),
        ]);
        let text = render(&r, "out/report.html", false);
        assert!(!text.contains('\x1b'), "{text}");
        assert!(
            text.find("PN-RUN-002").unwrap() < text.find("PN-DISP-002").unwrap(),
            "{text}"
        );
        assert!(text.contains("wine: crash"), "{text}");
        assert!(!text.contains("PN-PE-001"), "{text}");
        assert!(text.contains("1 needs hardware: PN-HW-001"), "{text}");
        assert!(
            text.contains("\n1 failed, 1 not tested, 1 needs hardware, 1 passed\n"),
            "{text}"
        );
        assert!(
            text.contains("Report: out/report.html\nChecks explained: https://"),
            "{text}"
        );
    }

    #[test]
    fn long_text_wraps_at_one_hundred_columns() {
        let long = "word ".repeat(60);
        let mut c = check("PN-DISP-003", Status::Warning, Category::Display);
        c.summary = format!("First. {long}");
        let text = render(&sample(vec![c]), "r.html", false);
        assert!(text.lines().all(|l| l.chars().count() <= 100), "{text}");
    }

    #[test]
    fn warnings_explain_that_they_do_not_fail_the_run() {
        let mut w = check("PN-DISP-003", Status::Warning, Category::Display);
        w.evidence = Evidence::Heuristic;
        let text = render(&sample(vec![w]), "r.html", false);
        assert!(text.contains("(heuristic, can be wrong)"), "{text}");
        assert!(text.contains("pass --fail-on warning"), "{text}");
    }
}
