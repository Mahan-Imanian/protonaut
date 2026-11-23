//! The self-contained HTML report. One file, no scripts, no network: it is
//! opened from `file://` out of a CI artifact. Filtering uses radio inputs
//! and `:has()`, details use `<details>`, so the report is complete without
//! JavaScript and degrades to a plain list where `:has()` is unsupported.

use std::fmt::Write as _;

use crate::model::{Category, CheckResult, Evidence, Report, RunOutcome, Status, Verdict};
use crate::report::{html_escape, is_safe_relative_path, xml_escape};

const MAX_DETAILS: usize = 50;

/// A display group. Hardware checks are split from the other NOT_TESTED
/// results so a check this run could have tested is not hidden among the
/// ones that are never tested here.
#[derive(Clone, Copy, PartialEq)]
enum Group {
    Fail,
    Warning,
    Unknown,
    NotTested,
    Hardware,
    Pass,
}

impl Group {
    const ALL: [Group; 6] = [
        Group::Fail,
        Group::Warning,
        Group::Unknown,
        Group::NotTested,
        Group::Hardware,
        Group::Pass,
    ];

    fn of(c: &CheckResult) -> Self {
        match c.status {
            Status::Fail => Group::Fail,
            Status::Warning => Group::Warning,
            Status::Unknown => Group::Unknown,
            Status::NotTested if c.category == Category::Hardware => Group::Hardware,
            Status::NotTested => Group::NotTested,
            Status::Pass => Group::Pass,
        }
    }

    fn key(self) -> &'static str {
        match self {
            Group::Fail => "fail",
            Group::Warning => "warning",
            Group::Unknown => "unknown",
            Group::NotTested => "not-tested",
            Group::Hardware => "hardware",
            Group::Pass => "pass",
        }
    }

    fn label(self) -> &'static str {
        match self {
            Group::Fail => "Failed",
            Group::Warning => "Warnings",
            Group::Unknown => "Unknown",
            Group::NotTested => "Not tested",
            Group::Hardware => "Needs hardware",
            Group::Pass => "Passed",
        }
    }

    fn note(self) -> &'static str {
        match self {
            Group::Fail => "",
            Group::Warning => {
                "A person should look at these. They do not fail the run unless you set <code>fail-on: warning</code>."
            }
            Group::Unknown => {
                "The check ran, but the evidence was not enough for a verdict. Unknown is not a failure."
            }
            Group::NotTested => {
                "No evidence was gathered for these in this run, so none of them is a pass."
            }
            Group::Hardware => {
                "These need a real device or a person with a controller. They are never tested here and are not passes."
            }
            Group::Pass => "",
        }
    }

    fn open(self) -> bool {
        matches!(self, Group::Fail | Group::Warning)
    }
}

fn status_class(s: Status) -> &'static str {
    match s {
        Status::Fail => "fail",
        Status::Warning => "warning",
        Status::Unknown => "unknown",
        Status::NotTested => "not-tested",
        Status::Pass => "pass",
    }
}

fn status_word(s: Status) -> &'static str {
    match s {
        Status::Fail => "Failed",
        Status::Warning => "Warning",
        Status::Unknown => "Unknown",
        Status::NotTested => "Not tested",
        Status::Pass => "Passed",
    }
}

/// 16px icons drawn with one stroke weight, so every result reads at the
/// same size. They are decoration next to the status word.
fn icon(s: Status) -> &'static str {
    match s {
        Status::Fail => {
            r#"<svg class="i" viewBox="0 0 16 16" aria-hidden="true"><circle cx="8" cy="8" r="6.5"/><path d="M5.6 5.6l4.8 4.8M10.4 5.6l-4.8 4.8"/></svg>"#
        }
        Status::Warning => {
            r#"<svg class="i" viewBox="0 0 16 16" aria-hidden="true"><path d="M8 1.8l6.6 11.7H1.4z"/><path d="M8 6.2v3.4M8 11.4v.2"/></svg>"#
        }
        Status::Unknown => {
            r#"<svg class="i" viewBox="0 0 16 16" aria-hidden="true"><circle cx="8" cy="8" r="6.5"/><path d="M6.2 6.3a1.9 1.9 0 1 1 2.6 1.8c-.5.2-.8.6-.8 1.1v.4M8 11.4v.2"/></svg>"#
        }
        Status::NotTested => {
            r#"<svg class="i" viewBox="0 0 16 16" aria-hidden="true"><circle cx="8" cy="8" r="6.5" stroke-dasharray="2.6 2.1"/></svg>"#
        }
        Status::Pass => {
            r#"<svg class="i" viewBox="0 0 16 16" aria-hidden="true"><circle cx="8" cy="8" r="6.5"/><path d="M5.2 8.2l1.9 1.9 3.8-4"/></svg>"#
        }
    }
}

fn evidence_class(e: Evidence) -> &'static str {
    match e {
        Evidence::Observed => "observed",
        Evidence::Static => "static",
        Evidence::Heuristic => "heuristic",
        Evidence::None => "none",
    }
}

fn evidence_word(e: Evidence) -> &'static str {
    match e {
        Evidence::Observed => "Observed",
        Evidence::Static => "From files",
        Evidence::Heuristic => "Heuristic",
        Evidence::None => "No evidence",
    }
}

fn evidence_help(e: Evidence) -> &'static str {
    match e {
        Evidence::Observed => "Seen while the build was running.",
        Evidence::Static => "Read from the build files without running them.",
        Evidence::Heuristic => "Image analysis or pattern matching. It can be wrong.",
        Evidence::None => "No evidence was gathered.",
    }
}

fn outcome_word(o: RunOutcome) -> &'static str {
    match o {
        RunOutcome::Survived => "Still running at the end",
        RunOutcome::ExitedCleanly => "Exited with code 0",
        RunOutcome::ExitedWithError => "Exited with an error",
        RunOutcome::Crashed => "Crashed",
        RunOutcome::LaunchFailed => "Failed to launch",
    }
}

fn capture_word(v: Option<bool>) -> &'static str {
    match v {
        Some(true) => "Working",
        Some(false) => "Not working",
        None => "Not checked",
    }
}

fn secs(ms: u64) -> String {
    format!("{:.1}&nbsp;s", ms as f64 / 1000.0)
}

fn plural(n: usize, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}

fn verdict_status(r: &Report) -> Status {
    match (r.summary.verdict, r.summary.counts.warning) {
        (Verdict::Fail, _) => Status::Fail,
        (Verdict::Pass, 0) => Status::Pass,
        (Verdict::Pass, _) => Status::Warning,
    }
}

fn run_names(r: &Report) -> String {
    if r.runs.is_empty() {
        return "static checks only".to_owned();
    }
    r.runs
        .iter()
        .map(|x| x.runtime_version.as_str())
        .collect::<Vec<_>>()
        .join(" and ")
}

/// Rows to show for a group. Hardware checks repeat per runtime with the
/// same meaning, so they are shown once with every runtime listed.
fn rows(r: &Report, g: Group) -> Vec<(&CheckResult, Vec<&str>)> {
    let mut out: Vec<(&CheckResult, Vec<&str>)> = Vec::new();
    for c in r.checks.iter().filter(|c| Group::of(c) == g) {
        let rt = c.runtime.as_deref();
        if g == Group::Hardware
            && let Some(row) = out.iter_mut().find(|(x, _)| x.id == c.id)
        {
            row.1.extend(rt);
            continue;
        }
        out.push((c, rt.into_iter().collect()));
    }
    out
}

struct Seen {
    frames: Vec<String>,
    logs: Vec<(Vec<String>, String)>,
    nexts: Vec<String>,
}

pub fn render(r: &Report) -> String {
    let mut s = String::with_capacity(32 * 1024);
    let title = crate::report::headline(r);
    let _ = write!(
        s,
        "<!doctype html><html lang=\"en\"><head><meta charset=\"utf-8\">\
<meta name=\"viewport\" content=\"width=device-width,initial-scale=1\">\
<meta name=\"color-scheme\" content=\"light dark\">\
<meta http-equiv=\"Content-Security-Policy\" content=\"default-src 'none'; img-src 'self'; style-src 'unsafe-inline'\">\
<title>{} on {}: {}</title><style>{CSS}</style></head><body>\
<header class=\"bar\"><div class=\"wrap\"><span class=\"brand\">Protonaut</span><span class=\"muted\">{}</span></div></header><main class=\"wrap\">",
        html_escape(&r.target.exe),
        html_escape(&run_names(r)),
        html_escape(&title),
        html_escape(&r.tool.version),
    );
    write_summary(&mut s, r, &title);
    s.push_str("<div class=\"tools\">");
    write_filters(&mut s, r);
    s.push_str("<input type=\"checkbox\" id=\"expand-all\"><label for=\"expand-all\" class=\"toggle\">Expand all<span class=\"sr\"> (shows every row's content; each row keeps its own open state)</span></label></div>");
    write_key(&mut s);
    s.push_str("<div class=\"groups\">");
    let mut seen = Seen {
        frames: Vec::new(),
        logs: Vec::new(),
        nexts: Vec::new(),
    };
    for g in Group::ALL {
        write_group(&mut s, r, g, &mut seen);
    }
    s.push_str("</div>");
    write_environment(&mut s, r);
    write_limits(&mut s, r);
    let _ = writeln!(
        s,
        "</main><footer class=\"wrap\">Protonaut {} is an independent community project, not affiliated with Valve. \
Report schema v{}. <a href=\"report.json\">report.json</a> <a href=\"junit.xml\">junit.xml</a></footer></body></html>",
        html_escape(&r.tool.version),
        r.schema_version
    );
    s
}

fn write_summary(s: &mut String, r: &Report, title: &str) {
    let v = verdict_status(r);
    let _ = write!(
        s,
        "<section class=\"summary\" aria-labelledby=\"verdict\"><div class=\"head s-{}\">{}<div class=\"headtext\">\
<h1 id=\"verdict\">{}</h1><p class=\"muted\"><code>{}</code> under {}, generated <time datetime=\"{}\">{}</time></p></div></div>",
        status_class(v),
        icon(v).replace("class=\"i\"", "class=\"i big\""),
        html_escape(title),
        html_escape(&r.target.exe),
        html_escape(&run_names(r)),
        html_escape(&r.generated_at),
        html_escape(&nice_time(&r.generated_at)),
    );
    match r.runs.as_slice() {
        [] => {}
        [run] => {
            let _ = write!(
                s,
                "<dl class=\"facts\"><div><dt>Outcome</dt><dd>{}</dd></div><div><dt>Exit code</dt><dd class=\"num\">{}</dd></div>\
<div><dt>Ran for</dt><dd class=\"num\">{}</dd></div><div><dt>Screen capture</dt><dd>{}</dd></div></dl>",
                outcome_word(run.outcome),
                run.exit_code
                    .map_or_else(|| "none".to_owned(), |c| c.to_string()),
                secs(run.duration_ms),
                capture_word(run.capture_verified),
            );
        }
        runs => {
            s.push_str(
                "<table class=\"runs\"><caption class=\"sr\">Runs</caption><thead><tr>\
<th scope=\"col\">Runtime</th><th scope=\"col\">Outcome</th><th scope=\"col\" class=\"num\">Exit code</th>\
<th scope=\"col\" class=\"num\">Ran for</th><th scope=\"col\">Screen capture</th><th scope=\"col\">Problems</th>\
<th scope=\"col\" class=\"num\">Not tested</th></tr></thead><tbody>",
            );
            for run in runs {
                let mine = |c: &&CheckResult| c.runtime.as_deref() == Some(run.runtime.as_str());
                let problems: Vec<&CheckResult> = r
                    .checks
                    .iter()
                    .filter(mine)
                    .filter(|c| matches!(c.status, Status::Fail | Status::Warning))
                    .collect();
                let untested = r
                    .checks
                    .iter()
                    .filter(mine)
                    .filter(|c| c.status == Status::NotTested && c.category != Category::Hardware)
                    .count();
                let problem_cell = match problems.first() {
                    None => "none".to_owned(),
                    Some(first) => format!(
                        "<a href=\"#{}\" aria-label=\"{}\">{}</a>",
                        html_escape(&format!("{}-{}", first.id, run.runtime)),
                        plural(problems.len(), "problem", "problems"),
                        problems.len()
                    ),
                };
                let _ = write!(
                    s,
                    "<tr><th scope=\"row\" data-label=\"Runtime\">{}</th><td data-label=\"Outcome\">{}</td>\
<td class=\"num\" data-label=\"Exit code\">{}</td><td class=\"num\" data-label=\"Ran for\">{}</td>\
<td data-label=\"Screen capture\">{}</td><td data-label=\"Problems\">{}</td><td class=\"num\" data-label=\"Not tested\">{}</td></tr>",
                    html_escape(&run.runtime_version),
                    outcome_word(run.outcome),
                    run.exit_code
                        .map_or_else(|| "none".to_owned(), |c| c.to_string()),
                    secs(run.duration_ms),
                    capture_word(run.capture_verified),
                    problem_cell,
                    untested,
                );
            }
            s.push_str("</tbody></table>");
        }
    }
    let mut notes = Vec::new();
    for run in r.runs.iter().filter(|x| x.capture_verified == Some(false)) {
        notes.push(format!(
            "Screen capture did not work under {}, so its screenshot checks are not tested. A black or missing frame there proves nothing.",
            html_escape(&run.runtime_version)
        ));
    }
    for c in r
        .checks
        .iter()
        .filter(|c| c.id == "PN-DISP-004" && c.status == Status::Warning)
    {
        for title in c.details.iter().filter_map(|d| quoted(d)) {
            notes.push(format!(
                "Under {} a small window titled <q>{}</q> appeared. See <a href=\"#{}\">PN-DISP-004</a>.",
                html_escape(runtime_name(r, c.runtime.as_deref().unwrap_or(""))),
                html_escape(title),
                html_escape(&match &c.runtime {
                    Some(rt) => format!("{}-{rt}", c.id),
                    None => c.id.clone(),
                }),
            ));
        }
    }
    for c in r
        .checks
        .iter()
        .filter(|c| c.id == "PN-DISP-005" && c.status == Status::Warning)
    {
        notes.push(format!(
            "Under {} OCR read text on screen that looks like an error message. See <a href=\"#{}\">PN-DISP-005</a>.",
            html_escape(runtime_name(r, c.runtime.as_deref().unwrap_or(""))),
            html_escape(&match &c.runtime {
                Some(rt) => format!("{}-{rt}", c.id),
                None => c.id.clone(),
            }),
        ));
    }
    if !notes.is_empty() {
        s.push_str("<div class=\"callout\">");
        for n in notes {
            let _ = write!(s, "<p>{n}</p>");
        }
        s.push_str("</div>");
    }
    s.push_str("</section>");
}

/// The display name of a runtime id, such as GE-Proton10-34 for ge-proton10-34.
fn runtime_name<'a>(r: &'a Report, id: &'a str) -> &'a str {
    r.runs
        .iter()
        .find(|x| x.runtime == id)
        .map_or(id, |x| x.runtime_version.as_str())
}

/// The window title in a detail line such as `"Fatal Application Failure" 458x177`.
fn quoted(d: &str) -> Option<&str> {
    let rest = d.strip_prefix('"')?;
    let end = rest.find('"')?;
    Some(&rest[..end]).filter(|t| !t.is_empty())
}

fn nice_time(rfc3339: &str) -> String {
    rfc3339.replace('T', " ").replace('Z', " UTC")
}

fn write_filters(s: &mut String, r: &Report) {
    s.push_str(
        "<fieldset class=\"filters\"><legend class=\"sr\">Show checks</legend><div class=\"seg\">",
    );
    let all = r.checks.len();
    let _ = write!(
        s,
        "<input type=\"radio\" name=\"show\" id=\"show-all\" checked><label for=\"show-all\">All <span class=\"count\">{all}</span></label>"
    );
    for g in Group::ALL {
        let n = rows(r, g).len();
        let _ = write!(
            s,
            "<input type=\"radio\" name=\"show\" id=\"show-{0}\"{2}><label for=\"show-{0}\">{1} <span class=\"count\">{3}</span></label>",
            g.key(),
            g.label(),
            if n == 0 { " disabled" } else { "" },
            n,
        );
    }
    s.push_str("</div></fieldset>");
}

fn write_group(s: &mut String, r: &Report, g: Group, seen: &mut Seen) {
    let list = rows(r, g);
    if list.is_empty() {
        return;
    }
    let _ = write!(
        s,
        "<section class=\"group g-{0}\" aria-labelledby=\"h-{0}\"><h2 id=\"h-{0}\">{1} <span class=\"count\">{2}</span></h2>",
        g.key(),
        g.label(),
        list.len()
    );
    if !g.note().is_empty() {
        let _ = write!(s, "<p class=\"note\">{}</p>", g.note());
    }
    s.push_str("<div class=\"box list\">");
    for (c, runtimes) in list {
        write_check(s, r, c, &runtimes, g.open(), seen);
    }
    s.push_str("</div></section>");
}

fn write_check(
    s: &mut String,
    r: &Report,
    c: &CheckResult,
    runtimes: &[&str],
    open: bool,
    seen: &mut Seen,
) {
    let anchor = match runtimes {
        [one] => format!("{}-{}", c.id, one),
        _ => c.id.clone(),
    };
    let (heading, rest, check_name) =
        if matches!(c.status, Status::Fail | Status::Warning | Status::Unknown) {
            let (first, rest) = crate::report::first_sentence(&c.summary);
            (first, rest, Some(c.title.as_str()))
        } else {
            (c.title.clone(), c.summary.as_str(), None)
        };
    let heading = heading.trim_end_matches('.').to_owned();
    let rt = runtimes
        .iter()
        .map(|x| format!("<span>{}</span>", html_escape(runtime_name(r, x))))
        .collect::<String>();
    let ev = if c.evidence == Evidence::None {
        String::new()
    } else {
        format!(
            "<span class=\"ev e-{}\">{}</span>",
            evidence_class(c.evidence),
            evidence_word(c.evidence)
        )
    };
    let _ = write!(
        s,
        "<details class=\"check s-{st}\" id=\"{anchor}\"{open}><summary>\
<span class=\"chev\" aria-hidden=\"true\"></span>{icon}\
<span class=\"what\"><span class=\"sr\">{word}: </span><span class=\"title\">{title}</span>\
<span class=\"sub\"><code>{id}</code>{name}{rt}</span></span>{ev}</summary><div class=\"body\">",
        st = status_class(c.status),
        anchor = html_escape(&anchor),
        open = if open { " open" } else { "" },
        icon = icon(c.status),
        word = status_word(c.status),
        title = html_escape(&heading),
        id = html_escape(&c.id),
        name = check_name
            .map(|n| format!("<span>{}</span>", html_escape(n)))
            .unwrap_or_default(),
    );
    if !rest.is_empty() {
        let _ = write!(s, "<p>{}</p>", html_escape(rest));
    }
    if c.evidence == Evidence::Heuristic {
        let _ = write!(s, "<p class=\"evnote\">{}</p>", evidence_help(c.evidence));
    }
    write_details(s, c, seen);
    write_artifacts(s, c, seen);
    if let Some((text, cmd)) = crate::report::next_step(c, &r.target.exe, runtimes.first().copied())
    {
        let key = format!("{text}{}", cmd.as_deref().unwrap_or(""));
        if !seen.nexts.contains(&key) {
            let _ = write!(s, "<div class=\"next\"><p><b>Next:</b> {text}</p>");
            if let Some(cmd) = &cmd {
                let _ = write!(
                    s,
                    "<pre class=\"cmd\" tabindex=\"0\" aria-label=\"Command\">{}</pre>",
                    xml_escape(cmd)
                );
            }
            s.push_str("</div>");
            seen.nexts.push(key);
        }
    }
    let _ = write!(
        s,
        "<p class=\"about\"><a href=\"{2}\">About {1}</a></p><a class=\"perma\" href=\"#{0}\" aria-label=\"Link to {1}\" title=\"Link to this check\">#</a></div></details>",
        html_escape(&anchor),
        html_escape(&c.id),
        html_escape(&crate::report::check_url(&c.id))
    );
}

fn write_details(s: &mut String, c: &CheckResult, seen: &mut Seen) {
    let details: Vec<&String> = c.details.iter().take(MAX_DETAILS).collect();
    if details.is_empty() {
        return;
    }
    if c.category == Category::Runtime {
        let key: Vec<String> = details.iter().map(|d| (*d).clone()).collect();
        if let Some((_, first)) = seen.logs.iter().find(|(lines, _)| *lines == key) {
            let _ = write!(
                s,
                "<p class=\"note\">The same log lines are shown above under {}.</p>",
                html_escape(first)
            );
        } else {
            s.push_str("<pre class=\"frame\" tabindex=\"0\" aria-label=\"Log lines\">");
            for d in &details {
                let _ = writeln!(s, "{}", xml_escape(d));
            }
            s.push_str("</pre>");
            seen.logs.push((key, c.id.clone()));
        }
    } else if c.id == "PN-DISP-005" {
        s.push_str("<p class=\"label\">Text read by OCR (may be garbled):</p><pre class=\"frame\" tabindex=\"0\" aria-label=\"Text read by OCR\">");
        for d in &details {
            let _ = writeln!(s, "{}", xml_escape(d));
        }
        s.push_str("</pre>");
    } else if let [one] = details.as_slice() {
        let _ = write!(s, "<p><code>{}</code></p>", html_escape(one));
    } else {
        s.push_str("<ul class=\"details\">");
        for d in &details {
            let _ = write!(s, "<li><code>{}</code></li>", html_escape(d));
        }
        s.push_str("</ul>");
    }
    if c.details.len() > MAX_DETAILS {
        let _ = write!(
            s,
            "<p class=\"note\">{} more in report.json.</p>",
            c.details.len() - MAX_DETAILS
        );
    }
}

fn write_artifacts(s: &mut String, c: &CheckResult, seen: &mut Seen) {
    let mut files = Vec::new();
    for a in c.artifacts.iter().filter(|a| is_safe_relative_path(a)) {
        if !a.ends_with(".png") {
            files.push(a);
        } else if seen.frames.iter().any(|x| x == a) {
            let _ = write!(
                s,
                "<p class=\"note\">Same frame as shown above: <a href=\"{0}\">{0}</a></p>",
                html_escape(a)
            );
        } else {
            seen.frames.push(a.clone());
            let _ = write!(
                s,
                "<figure><a href=\"{0}\"><img src=\"{0}\" alt=\"Frame captured for {1}: {2}\" loading=\"lazy\"></a>\
<figcaption><a href=\"{0}\">{0}</a></figcaption></figure>",
                html_escape(a),
                html_escape(&c.title),
                html_escape(&c.summary),
            );
        }
    }
    if !files.is_empty() {
        s.push_str("<p class=\"files\">");
        for f in files {
            let _ = write!(s, "<a href=\"{0}\">{0}</a>", html_escape(f));
        }
        s.push_str("</p>");
    }
}

fn write_environment(s: &mut String, r: &Report) {
    let e = &r.environment;
    let mut rows: Vec<(&str, &str)> = Vec::new();
    if !e.os.is_empty() {
        rows.push(("Operating system", &e.os));
    }
    if let Some(k) = &e.kernel {
        rows.push(("Kernel", k));
    }
    if let Some(v) = &e.vulkan_device {
        rows.push(("Vulkan device", v));
    }
    if let Some(d) = &e.display {
        rows.push(("Display", d));
    }
    if rows.is_empty() {
        return;
    }
    s.push_str(
        "<section aria-labelledby=\"h-env\"><h2 id=\"h-env\">Environment</h2><dl class=\"box kv\">",
    );
    for (k, v) in rows {
        let _ = write!(s, "<div><dt>{k}</dt><dd>{}</dd></div>", html_escape(v));
    }
    s.push_str("</dl></section>");
}

fn write_key(s: &mut String) {
    s.push_str("<details class=\"legend\"><summary>How to read this report</summary><div class=\"box key\"><ul>");
    for st in [
        Status::Fail,
        Status::Warning,
        Status::Unknown,
        Status::NotTested,
        Status::Pass,
    ] {
        let meaning = match st {
            Status::Fail => "The problem was seen while running or read from the build files.",
            Status::Warning => "Something a person should look at.",
            Status::Unknown => "The check ran, but the evidence was not enough.",
            Status::NotTested => "No evidence was gathered. Never a pass.",
            Status::Pass => "The expected behaviour was seen.",
        };
        let _ = write!(
            s,
            "<li class=\"s-{}\">{}<span><b>{}</b> {meaning}</span></li>",
            status_class(st),
            icon(st),
            status_word(st)
        );
    }
    s.push_str("</ul><ul>");
    for ev in [Evidence::Observed, Evidence::Static, Evidence::Heuristic] {
        let _ = write!(
            s,
            "<li><span class=\"ev e-{}\">{}</span><span>{}</span></li>",
            evidence_class(ev),
            evidence_word(ev),
            evidence_help(ev)
        );
    }
    s.push_str("</ul></div></details>");
}

fn write_limits(s: &mut String, r: &Report) {
    if r.limitations.is_empty() {
        return;
    }
    s.push_str("<section aria-labelledby=\"h-limits\"><h2 id=\"h-limits\">What this report cannot tell you</h2><ul class=\"limits\">");
    for l in &r.limitations {
        let _ = write!(s, "<li>{}</li>", html_escape(l));
    }
    s.push_str("</ul></section>");
}

const CSS: &str = r#"
:root{color-scheme:light dark;
--bg:#ffffff;--subtle:#f6f8fa;--fg:#1f2328;--muted:#59636e;--border:#d1d9e0;--link:#0969da;--focus:#0969da;
--fail:#cf222e;--warning:#9a6700;--unknown:#4b57cf;--not-tested:#6e7781;--pass:#1a7f37;--callout:#fff8c5;
--sans:-apple-system,BlinkMacSystemFont,"Segoe UI","Noto Sans",Helvetica,Arial,sans-serif;
--mono:ui-monospace,SFMono-Regular,"SF Mono",Menlo,Consolas,"Liberation Mono",monospace}
@media (prefers-color-scheme:dark){:root{
--bg:#0d1117;--subtle:#151b23;--fg:#f0f6fc;--muted:#9198a1;--border:#3d444d;--link:#4493f8;--focus:#4493f8;
--fail:#f85149;--warning:#d29922;--unknown:#8c95f2;--not-tested:#9198a1;--pass:#3fb950;--callout:#272115}}
*,*::before,*::after{box-sizing:border-box}
html{background:var(--bg);color:var(--fg);font:14px/1.5 var(--sans);-webkit-text-size-adjust:100%}
body{margin:0}
a{color:var(--link)}
a:focus-visible,summary:focus-visible,pre:focus-visible,.scroll:focus-visible{outline:2px solid var(--focus);outline-offset:2px;border-radius:6px}
code,pre{font-family:var(--mono)}
code{font-size:12px}
.num{font-variant-numeric:tabular-nums}
h1,h2{margin:0;font-weight:600;line-height:1.25;text-wrap:balance}
h1{font-size:24px}
h2{font-size:16px;display:flex;gap:8px;align-items:center}
p{margin:0}
.sr{position:absolute;width:1px;height:1px;overflow:hidden;clip-path:inset(50%);white-space:nowrap}
.muted,.note{color:var(--muted)}
.wrap{max-width:1012px;margin-inline:auto;padding-inline:16px}
.bar{border-bottom:1px solid var(--border);background:var(--subtle)}
.bar .wrap{display:flex;gap:12px;align-items:baseline;padding-block:12px}
.brand{font-weight:600}
main{padding-block:24px 48px;display:grid;grid-template-columns:minmax(0,1fr);gap:24px}
.i{width:16px;height:16px;flex:none;fill:none;stroke:var(--c);stroke-width:1.6;stroke-linecap:round;stroke-linejoin:round}
.i.big{width:32px;height:32px;stroke-width:1.4}
.s-fail{--c:var(--fail)}.s-warning{--c:var(--warning)}.s-unknown{--c:var(--unknown)}.s-not-tested{--c:var(--not-tested)}.s-pass{--c:var(--pass)}
.summary{display:grid;gap:12px}
.head{display:flex;gap:12px;align-items:flex-start}
.headtext{display:grid;gap:2px;min-width:0}
.facts{margin:0;display:flex;flex-wrap:wrap;gap:8px 32px;padding:12px 16px;border:1px solid var(--border);border-radius:6px}
.facts div{display:grid;gap:2px}
.facts dt{color:var(--muted);font-size:12px}
.facts dd{margin:0;font-weight:600}
.callout{padding:8px 12px;border:1px solid var(--border);border-radius:6px;background:var(--callout)}
.filters{margin:0;padding:0;border:0;min-width:0}
.seg{display:flex;overflow-x:auto;scrollbar-width:thin}
.seg input{position:absolute;opacity:0;width:1px;height:1px}
.seg label{display:flex;gap:6px;align-items:center;flex:none;min-height:32px;padding:5px 12px;border:1px solid var(--border);margin-left:-1px;cursor:pointer;font-weight:500;background:var(--bg)}
.seg label:first-of-type{border-radius:6px 0 0 6px;margin-left:0}
.seg label:last-of-type{border-radius:0 6px 6px 0}
.seg input:checked+label{background:var(--subtle);border-color:var(--muted);position:relative;z-index:1}
.seg input:disabled+label{color:var(--muted);cursor:default;opacity:.6;border-style:dashed}
.seg input:focus-visible+label{outline:2px solid var(--focus);outline-offset:-2px;position:relative;z-index:2}
.count{display:inline-block;min-width:20px;padding:0 6px;border-radius:10px;background:var(--subtle);border:1px solid var(--border);font-size:12px;line-height:18px;font-weight:500;text-align:center;font-variant-numeric:tabular-nums}
.groups{display:grid;gap:24px}
.group{display:grid;gap:8px}
.note{max-width:80ch}
body:has(#show-fail:checked) .group:not(.g-fail),
body:has(#show-warning:checked) .group:not(.g-warning),
body:has(#show-unknown:checked) .group:not(.g-unknown),
body:has(#show-not-tested:checked) .group:not(.g-not-tested),
body:has(#show-hardware:checked) .group:not(.g-hardware),
body:has(#show-pass:checked) .group:not(.g-pass){display:none}
.box{border:1px solid var(--border);border-radius:6px;background:var(--bg)}
.list{padding:4px}
.check{border-radius:4px}
.check+.check{border-top:1px solid var(--border)}
.check summary{display:grid;grid-template-columns:12px 16px minmax(0,1fr) auto;gap:4px 10px;align-items:start;padding:10px 12px 10px 8px;border-radius:4px;cursor:pointer;list-style:none}
.check summary::-webkit-details-marker{display:none}
.check summary .i{margin-top:2px}
.chev{width:12px;height:20px;position:relative}
.chev::before{content:"";position:absolute;left:2px;top:6px;width:6px;height:6px;border-right:1.5px solid var(--muted);border-bottom:1.5px solid var(--muted);transform:rotate(-45deg)}
.check[open]>summary .chev::before{transform:rotate(45deg);top:4px}
@media (hover:hover){.check summary:hover{background:var(--subtle)}.seg input:not(:disabled)+label:hover{background:var(--subtle)}}
.what{display:grid;gap:2px;min-width:0}
.title{font-weight:600}
.sub{display:flex;flex-wrap:wrap;gap:4px 12px;color:var(--muted);font-size:12px}
.ev{justify-self:end;white-space:nowrap;font-size:12px;font-weight:500;line-height:18px;padding:0 7px;border-radius:10px;border:1px solid var(--border)}
.e-heuristic{border-style:dashed}
.body{display:grid;gap:10px;padding:4px 12px 16px 56px;min-width:0}
.evnote{font-size:12px;color:var(--muted)}
.evnote b{color:var(--fg);font-weight:600}
.next{display:grid;gap:6px;padding-top:10px;border-top:1px solid var(--border)}
.cmd{margin:0;padding:8px 12px;border:1px solid var(--border);border-radius:6px;background:var(--subtle);font-size:12px;overflow-x:auto}
.body{position:relative}
.about{font-size:12px}
.perma{position:absolute;top:4px;right:12px;font-size:14px;text-decoration:none;opacity:0;padding:0 4px}
.check:hover .perma,.perma:focus-visible{opacity:1}
@media (hover:none){.perma{opacity:1}}
.tools{display:flex;flex-wrap:wrap;gap:8px 16px;align-items:center}
#expand-all{position:absolute;opacity:0;width:1px;height:1px}
.toggle{display:flex;gap:8px;align-items:center;min-height:32px;cursor:pointer;font-weight:500}
.toggle{position:relative}
.toggle::before{content:"";width:14px;height:14px;border:1px solid var(--muted);border-radius:3px;background:var(--bg)}
.toggle::after{content:"";position:absolute;left:5px;top:50%;width:4px;height:8px;margin-top:-6px;border-right:2px solid var(--bg);border-bottom:2px solid var(--bg);transform:rotate(45deg);opacity:0}
#expand-all:checked+.toggle::after{opacity:1}
#expand-all:checked+.toggle::before{background:var(--fg);border-color:var(--fg)}
#expand-all,.toggle{display:none}
@supports selector(::details-content){#expand-all{display:block}.toggle{display:flex}}
body:has(#expand-all:checked) .check>summary{pointer-events:none}
#expand-all:focus-visible+.toggle{outline:2px solid var(--focus);outline-offset:2px;border-radius:4px}
body:has(#expand-all:checked) .check::details-content{display:block;content-visibility:visible}
body:has(#expand-all:checked) .check:not([open])>summary .chev::before{transform:rotate(45deg);top:4px}
.legend>summary{cursor:pointer;font-weight:600;padding:4px 0;list-style:none;display:flex;gap:8px;align-items:center}
.legend>summary::-webkit-details-marker{display:none}
.legend>summary::before{content:"";width:6px;height:6px;margin:0 3px;border-right:1.5px solid var(--muted);border-bottom:1.5px solid var(--muted);transform:rotate(-45deg)}
.legend[open]>summary::before{transform:rotate(45deg)}
.legend[open]>summary{margin-bottom:8px}
.label{font-size:12px;color:var(--muted)}
.frame{margin:0;padding:8px 12px;border:1px solid var(--border);border-radius:6px;background:var(--subtle);font-size:12px;line-height:20px;overflow-x:auto;white-space:pre}
.details{margin:0;padding-left:18px;display:grid;gap:4px}
figure{margin:0;display:grid;gap:6px}
figure img{display:block;width:100%;height:auto;aspect-ratio:16/10;object-fit:contain;background:#000;border:1px solid var(--border);border-radius:6px}
figcaption,.files{font-size:12px}
.files{display:flex;flex-wrap:wrap;gap:4px 16px;font-family:var(--mono)}
.kv{margin:0;display:grid}
.kv div{display:grid;grid-template-columns:minmax(120px,200px) minmax(0,1fr);gap:16px;padding:8px 16px}
.kv div+div{border-top:1px solid var(--border)}
.kv dt{color:var(--muted)}
.kv dd{margin:0;overflow-wrap:anywhere}
.scroll{overflow-x:auto}
table{border-collapse:separate;border-spacing:0;width:100%;border:1px solid var(--border);border-radius:6px}
th,td{text-align:left;padding:8px 12px;border-top:1px solid var(--border);white-space:nowrap}
thead th{border-top:0;background:var(--subtle);font-size:12px;color:var(--muted);font-weight:600}
thead th:first-child{border-top-left-radius:6px}thead th:last-child{border-top-right-radius:6px}
tbody th{font-weight:600}
th.num,td.num{text-align:right}
.key{display:grid;gap:16px;padding:16px}
.key ul{margin:0;padding:0;list-style:none;display:grid;gap:8px}
.key li{display:flex;gap:10px;align-items:flex-start}
.key .i{margin-top:2px}
.key .ev{flex:none}
.limits{margin:0;padding-left:18px;display:grid;gap:4px;color:var(--muted);max-width:80ch}
footer{padding-block:16px 32px;border-top:1px solid var(--border);color:var(--muted);font-size:12px}
footer a{margin-left:8px}
@media (min-width:768px){.key{grid-template-columns:1fr 1fr}}
@media (max-width:600px){.body{padding-left:12px}.check summary{grid-template-columns:12px 16px minmax(0,1fr)}.ev{grid-column:3;justify-self:start}
.seg{flex-wrap:wrap;gap:6px;overflow:visible}.seg label,.seg label:first-of-type,.seg label:last-of-type{border-radius:6px;margin-left:0}
.runs,.runs tbody,.runs tr,.runs th,.runs td{display:block}.runs thead{display:none}.runs{border:0}.runs tr{border:1px solid var(--border);border-radius:6px;padding:8px 12px;margin-bottom:8px}.runs th,.runs td{border:0;padding:2px 0;white-space:normal;text-align:left}.runs td::before{content:attr(data-label) ": ";color:var(--muted)}}
@media (forced-colors:active){.i{stroke:CanvasText}.seg input:checked+label{outline:2px solid Highlight}}
@media print{
html{color-scheme:light;font-size:10.5pt}
.bar,.filters,.chev{display:none}
.group{display:grid!important}
.check{break-inside:avoid}
details::details-content{display:block;content-visibility:visible}
a{color:inherit}
}
"#;
