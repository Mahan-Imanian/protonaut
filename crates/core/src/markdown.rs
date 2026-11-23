//! GitHub-flavoured Markdown for the PR comment and the job summary. Both
//! use GitHub's own alert blocks and tables, and the same result words and
//! glyphs as the HTML report. Every string from the build goes through
//! `md_escape`.

use std::fmt::Write as _;

use crate::model::{Category, CheckResult, Evidence, Report, RunOutcome, Status, Verdict};
use crate::report::{COMMENT_MARKER, first_sentence, headline, md_escape, next_step};

const COMMENT_ROWS: usize = 10;
const DETAIL_LINES: usize = 3;

fn glyph(s: Status) -> &'static str {
    match s {
        Status::Fail => "✗",
        Status::Warning => "!",
        Status::Unknown => "?",
        Status::NotTested => "○",
        Status::Pass => "✓",
    }
}

fn word(s: Status) -> &'static str {
    match s {
        Status::Fail => "Failed",
        Status::Warning => "Warning",
        Status::Unknown => "Unknown",
        Status::NotTested => "Not tested",
        Status::Pass => "Passed",
    }
}

fn result_cell(s: Status) -> String {
    match s {
        Status::Fail | Status::Warning => format!("{}&nbsp;**{}**", glyph(s), word(s)),
        _ => format!("{}&nbsp;{}", glyph(s), word(s)),
    }
}

fn evidence(e: Evidence) -> &'static str {
    match e {
        Evidence::Observed => "Observed",
        Evidence::Static => "From files",
        Evidence::Heuristic => "Heuristic, can be wrong",
        Evidence::None => "–",
    }
}

fn outcome(o: RunOutcome) -> &'static str {
    match o {
        RunOutcome::Survived => "Still running at the end",
        RunOutcome::ExitedCleanly => "Exited with code 0",
        RunOutcome::ExitedWithError => "Exited with an error",
        RunOutcome::Crashed => "Crashed",
        RunOutcome::LaunchFailed => "Failed to launch",
    }
}

fn capture(v: Option<bool>) -> &'static str {
    match v {
        Some(true) => "Working",
        Some(false) => "Not working",
        None => "Not checked",
    }
}

fn alert(r: &Report) -> &'static str {
    match (r.summary.verdict, r.summary.counts.warning) {
        (Verdict::Fail, _) => "CAUTION",
        (Verdict::Pass, 0) => "NOTE",
        (Verdict::Pass, _) => "WARNING",
    }
}

fn runtimes(r: &Report) -> String {
    if r.runs.is_empty() {
        return "static checks only".to_owned();
    }
    r.runs
        .iter()
        .map(|x| md_escape(&x.runtime_version))
        .collect::<Vec<_>>()
        .join(" and ")
}

fn runtime_name<'a>(r: &'a Report, id: &'a str) -> &'a str {
    r.runs
        .iter()
        .find(|x| x.runtime == id)
        .map_or(id, |x| x.runtime_version.as_str())
}

fn is_hardware(c: &CheckResult) -> bool {
    c.status == Status::NotTested && c.category == Category::Hardware
}

/// Rows to show. Hardware checks repeat per runtime with the same meaning,
/// so each appears once.
fn rows<'a>(r: &'a Report, keep: &dyn Fn(&CheckResult) -> bool) -> Vec<&'a CheckResult> {
    let mut out: Vec<&CheckResult> = Vec::new();
    for c in r.checks.iter().filter(|c| keep(c)) {
        if is_hardware(c) && out.iter().any(|x| x.id == c.id) {
            continue;
        }
        out.push(c);
    }
    out
}

/// The runtime is named per row only when the report covers several.
fn check_cell(r: &Report, c: &CheckResult) -> String {
    let rt = match (&c.runtime, r.runs.len() > 1 && !is_hardware(c)) {
        (Some(x), true) => format!(", {}", md_escape(runtime_name(r, x))),
        _ => String::new(),
    };
    format!(
        "[`{}`]({}) {}{}",
        md_escape(&c.id).replace('`', ""),
        crate::report::check_url(&c.id),
        md_escape(&c.title),
        rt
    )
}

fn sentence(text: &str) -> String {
    md_escape(first_sentence(text).0.trim_end_matches('.'))
}

/// What happened: the first sentence, then up to `lines` detail lines. A
/// detail already shown in an earlier row is named instead of repeated.
fn what_cell(c: &CheckResult, lines: usize, shown: &mut Vec<(String, String)>) -> String {
    let mut out = sentence(&c.summary);
    let lines = crate::report::detail_limit(c, lines);
    let details: Vec<&String> = c.details.iter().take(lines).collect();
    let key = details
        .iter()
        .map(|d| d.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    if !details.is_empty() {
        match shown.iter().find(|(k, _)| *k == key) {
            Some((_, id)) => {
                let _ = write!(
                    out,
                    "<br>Same log {} as `{}`",
                    if details.len() == 1 { "line" } else { "lines" },
                    md_escape(id).replace('`', "")
                );
            }
            None => {
                for d in &details {
                    out.push_str("<br>");
                    out.push_str(&md_escape(&crate::report::short_detail(c, d)));
                }
                shown.push((key, c.id.clone()));
            }
        }
    }
    if c.details.len() > lines {
        let _ = write!(
            out,
            "<br>and {} more in the report",
            c.details.len() - lines
        );
    }
    out
}

/// The next step for a failed or warning row, inline, once per wording.
fn next_suffix(r: &Report, c: &CheckResult, said: &mut Vec<&'static str>) -> String {
    if !matches!(c.status, Status::Fail | Status::Warning) {
        return String::new();
    }
    match next_step(c, &r.target.exe, c.runtime.as_deref()) {
        Some((text, _)) if !said.contains(&text) => {
            said.push(text);
            format!("<br>Next: {}", md_escape(text))
        }
        _ => String::new(),
    }
}

/// Not-tested rows that share a reason are shown as one row.
fn untested_groups<'a>(list: &[&'a CheckResult]) -> Vec<(Vec<&'a CheckResult>, &'a str)> {
    let mut out: Vec<(Vec<&CheckResult>, &str)> = Vec::new();
    for c in list {
        match out.iter_mut().find(|(_, why)| *why == c.summary) {
            Some((cs, _)) => cs.push(c),
            None => out.push((vec![c], c.summary.as_str())),
        }
    }
    out
}

fn counts_line(r: &Report) -> String {
    let n = |f: &dyn Fn(&CheckResult) -> bool| rows(r, f).len();
    let parts = [
        (n(&|c| c.status == Status::Fail), "failed", "failed"),
        (n(&|c| c.status == Status::Warning), "warning", "warnings"),
        (n(&|c| c.status == Status::Unknown), "unknown", "unknown"),
        (
            n(&|c| c.status == Status::NotTested && !is_hardware(c)),
            "not tested",
            "not tested",
        ),
        (n(&is_hardware), "needs hardware", "need hardware"),
        (n(&|c| c.status == Status::Pass), "passed", "passed"),
    ];
    parts
        .iter()
        .filter(|(k, _, _)| *k > 0)
        .map(|(k, one, many)| format!("{k} {}", if *k == 1 { one } else { many }))
        .collect::<Vec<_>>()
        .join(", ")
}

/// A command that reproduces the run, once, for the first problem that has one.
fn next_lines(r: &Report) -> String {
    let cmd = r
        .checks
        .iter()
        .filter(|c| matches!(c.status, Status::Fail | Status::Warning))
        .find_map(|c| next_step(c, &r.target.exe, c.runtime.as_deref()).and_then(|(_, cmd)| cmd));
    match cmd {
        Some(cmd) => format!(
            "Reproduce the run on Ubuntu 24.04 with the same runtime:\n\n```sh\n{}\n```\n\n",
            cmd.replace('`', "")
        ),
        None => String::new(),
    }
}

fn artifact_line(artifact_url: Option<&str>) -> Option<String> {
    artifact_url.filter(|u| u.starts_with("https://")).map(|u| {
        format!(
            "Screenshots, the runtime log and the full `report.html` are in the [artifacts of this run]({}).",
            u.replace(['(', ')', ' '], "")
        )
    })
}

fn footer(r: &Report) -> String {
    format!(
        "<sub>Protonaut {}. ✗ failed, ! warning, ? unknown, ○ not tested, ✓ passed. \
Independent community tool, not affiliated with Valve; it cannot predict a Steam Deck Verified rating.</sub>",
        md_escape(&r.tool.version)
    )
}

/// The PR comment: the verdict, then everything that needs a person,
/// then the rest folded.
pub fn comment(r: &Report, artifact_url: Option<&str>) -> String {
    let mut s = String::new();
    let _ = writeln!(
        s,
        "{COMMENT_MARKER}\n> [!{}]\n> **Protonaut: {}** for `{}` under {}\n",
        alert(r),
        md_escape(&headline(r)),
        md_escape(&r.target.exe).replace('`', ""),
        runtimes(r)
    );
    let mut problems = rows(r, &|c| matches!(c.status, Status::Fail | Status::Warning));
    problems.sort_by_key(|c| c.status);
    let untested = rows(r, &|c| c.status == Status::NotTested && !is_hardware(c));
    if !problems.is_empty() || !untested.is_empty() {
        let mut shown = Vec::new();
        let mut said = Vec::new();
        s.push_str("| Result | Check | Evidence | What happened |\n|:--|:--|:--|:--|\n");
        for c in problems.iter().take(COMMENT_ROWS) {
            let _ = writeln!(
                s,
                "| {} | {} | {} | {}{} |",
                result_cell(c.status),
                check_cell(r, c),
                evidence(c.evidence),
                what_cell(c, 1, &mut shown),
                next_suffix(r, c, &mut said)
            );
        }
        if problems.len() > COMMENT_ROWS {
            let _ = writeln!(
                s,
                "| | {} more problems are listed in the full report | | |",
                problems.len() - COMMENT_ROWS
            );
        }
        for (cs, why) in untested_groups(&untested) {
            let checks = cs
                .iter()
                .map(|c| check_cell(r, c))
                .collect::<Vec<_>>()
                .join("<br>");
            let _ = writeln!(
                s,
                "| {} | {checks} | – | {} |",
                result_cell(Status::NotTested),
                sentence(why)
            );
        }
        s.push('\n');
    }
    s.push_str(&next_lines(r));
    let _ = writeln!(s, "{}\n", counts_line(r));
    if let Some(line) = artifact_line(artifact_url) {
        let _ = writeln!(s, "{line}\n");
    }
    let rest = rows(r, &|c| {
        matches!(c.status, Status::Unknown | Status::Pass) || is_hardware(c)
    });
    if !rest.is_empty() {
        let _ = writeln!(
            s,
            "<details><summary>Unknown, passed and hardware checks ({})</summary>\n\n| Result | Check | Evidence |\n|:--|:--|:--|",
            rest.len()
        );
        for c in rest {
            let _ = writeln!(
                s,
                "| {} | {} | {} |",
                result_cell(c.status),
                check_cell(r, c),
                evidence(c.evidence)
            );
        }
        s.push_str("\n</details>\n\n");
    }
    s.push_str(&footer(r));
    s.push('\n');
    s
}

type Keep = fn(&CheckResult) -> bool;

/// The job summary: more room than a comment, so every group is a table
/// and the runs and environment are shown.
pub fn summary(r: &Report, artifact_url: Option<&str>) -> String {
    let mut s = String::new();
    let _ = writeln!(
        s,
        "## Protonaut: {}\n\n`{}` under {}, generated {}\n",
        md_escape(&headline(r)),
        md_escape(&r.target.exe).replace('`', ""),
        runtimes(r),
        md_escape(&r.generated_at.replace('T', " ").replace('Z', " UTC")),
    );
    if !r.runs.is_empty() {
        s.push_str(
            "| Runtime | Outcome | Exit code | Ran for | Screen capture |\n|:--|:--|--:|--:|:--|\n",
        );
        for run in &r.runs {
            let _ = writeln!(
                s,
                "| {} | {} | {} | {:.1}&nbsp;s | {} |",
                md_escape(&run.runtime_version),
                outcome(run.outcome),
                run.exit_code
                    .map_or_else(|| "none".to_owned(), |c| c.to_string()),
                run.duration_ms as f64 / 1000.0,
                capture(run.capture_verified)
            );
        }
        s.push('\n');
    }
    for run in r.runs.iter().filter(|x| x.capture_verified == Some(false)) {
        let _ = writeln!(
            s,
            "> [!WARNING]\n> Screen capture did not work under {}, so its screenshot checks are not tested.\n",
            md_escape(&run.runtime_version)
        );
    }
    let _ = writeln!(s, "{}\n", counts_line(r));
    s.push_str(&next_lines(r));
    let groups: [(&str, &str, Keep); 5] = [
        ("Failed", "", |c| c.status == Status::Fail),
        (
            "Warnings",
            "A person should look at these. They do not fail the run unless you set `fail-on: warning`.",
            |c| c.status == Status::Warning,
        ),
        (
            "Unknown",
            "The check ran, but the evidence was not enough for a verdict. Unknown is not a failure.",
            |c| c.status == Status::Unknown,
        ),
        (
            "Not tested in this run",
            "No evidence was gathered for these, so none of them is a pass.",
            |c| c.status == Status::NotTested && !is_hardware(c),
        ),
        (
            "Needs hardware",
            "These need a real device or a person with a controller. They are never tested here.",
            is_hardware,
        ),
    ];
    let mut shown = Vec::new();
    let mut said = Vec::new();
    for (title, note, keep) in &groups {
        let list = rows(r, keep);
        if list.is_empty() {
            continue;
        }
        let _ = writeln!(s, "### {title} ({})\n", list.len());
        if !note.is_empty() {
            let _ = writeln!(s, "{note}\n");
        }
        if *title == "Needs hardware" {
            for c in list {
                let _ = writeln!(s, "- {}", check_cell(r, c));
            }
            s.push('\n');
            continue;
        }
        s.push_str("| Check | Evidence | What happened |\n|:--|:--|:--|\n");
        if *title == "Not tested in this run" {
            for (cs, why) in untested_groups(&list) {
                let checks = cs
                    .iter()
                    .map(|c| check_cell(r, c))
                    .collect::<Vec<_>>()
                    .join("<br>");
                let _ = writeln!(s, "| {checks} | – | {} |", sentence(why));
            }
            s.push('\n');
            continue;
        }
        for c in list {
            let _ = writeln!(
                s,
                "| {} | {} | {}{} |",
                check_cell(r, c),
                evidence(c.evidence),
                what_cell(c, DETAIL_LINES, &mut shown),
                next_suffix(r, c, &mut said)
            );
        }
        s.push('\n');
    }
    let passed = rows(r, &|c| c.status == Status::Pass);
    if !passed.is_empty() {
        let _ = writeln!(
            s,
            "<details><summary>Passed ({})</summary>\n\n| Check | Evidence | What happened |\n|:--|:--|:--|",
            passed.len()
        );
        for c in passed {
            let _ = writeln!(
                s,
                "| {} | {} | {} |",
                check_cell(r, c),
                evidence(c.evidence),
                sentence(&c.summary)
            );
        }
        s.push_str("\n</details>\n\n");
    }
    let e = &r.environment;
    let env: Vec<(&str, &str)> = [
        (
            "Operating system",
            Some(e.os.as_str()).filter(|x| !x.is_empty()),
        ),
        ("Kernel", e.kernel.as_deref()),
        ("Vulkan device", e.vulkan_device.as_deref()),
        ("Display", e.display.as_deref()),
    ]
    .into_iter()
    .filter_map(|(k, v)| v.map(|v| (k, v)))
    .collect();
    if !env.is_empty() {
        s.push_str("<details><summary>Environment</summary>\n\n| | |\n|:--|:--|\n");
        for (k, v) in env {
            let _ = writeln!(s, "| {k} | {} |", md_escape(v));
        }
        s.push_str("\n</details>\n\n");
    }
    if let Some(line) = artifact_line(artifact_url) {
        let _ = writeln!(s, "{line}\n");
    }
    if !r.limitations.is_empty() {
        s.push_str("<details><summary>What this report cannot tell you</summary>\n\n");
        for l in &r.limitations {
            let _ = writeln!(s, "- {}", md_escape(l));
        }
        s.push_str("\n</details>\n\n");
    }
    let _ = writeln!(
        s,
        "<sub>Protonaut {}. Independent community tool, not affiliated with Valve.</sub>",
        md_escape(&r.tool.version)
    );
    s
}
