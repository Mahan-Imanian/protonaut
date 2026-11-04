//! Turns a `RunObservation` into check results. Pure: no I/O, so every rule
//! is unit-tested without Proton.

use crate::model::{Category, CheckResult, Evidence, RunOutcome, Status};
use crate::observe::{RunObservation, ScreenText, error_snippets, representative};

pub fn evaluate(obs: &RunObservation) -> Vec<CheckResult> {
    let mut out = vec![launch(obs), stability(obs), log_errors(obs)];
    out.push(window(obs));
    out.extend(display_checks(obs));
    out.push(dialog_check(obs));
    out.push(screen_text_check(obs));
    out.extend(hardware_not_tested(obs));
    for c in &mut out {
        c.runtime = Some(obs.runtime.clone());
    }
    out
}

fn observed(
    id: &str,
    title: &str,
    category: Category,
    status: Status,
    summary: impl Into<String>,
) -> CheckResult {
    CheckResult::new(id, title, category, status, Evidence::Observed, summary)
}

fn with_log(mut c: CheckResult, obs: &RunObservation) -> CheckResult {
    if let Some(p) = &obs.log_path {
        c.artifacts.push(p.clone());
    }
    c
}

fn launch(obs: &RunObservation) -> CheckResult {
    let title = "Game process starts under Proton";
    let c = match obs.outcome {
        RunOutcome::LaunchFailed => observed(
            "SP-RUN-001",
            title,
            Category::Runtime,
            Status::Fail,
            "The runtime could not start the executable.",
        ),
        _ if !obs.log.missing_dlls.is_empty() => observed(
            "SP-RUN-001",
            title,
            Category::Runtime,
            Status::Fail,
            "The Windows loader could not find required DLLs.",
        )
        .with_details(obs.log.missing_dlls.clone()),
        _ => observed(
            "SP-RUN-001",
            title,
            Category::Runtime,
            Status::Pass,
            "The executable was loaded and started.",
        ),
    };
    with_log(c, obs)
}

fn stability(obs: &RunObservation) -> CheckResult {
    let title = "Game keeps running during the observation window";
    let secs = obs.duration_ms as f64 / 1000.0;
    let c = match obs.outcome {
        RunOutcome::Survived => observed(
            "SP-RUN-002",
            title,
            Category::Runtime,
            Status::Pass,
            format!("Still running after {} s.", obs.observe_secs),
        ),
        RunOutcome::ExitedCleanly => observed(
            "SP-RUN-002",
            title,
            Category::Runtime,
            Status::Warning,
            format!(
                "Exited with code 0 after {secs:.1} s, before the {} s window ended. Fine for a tool that quits on purpose; for a game it usually means a silent failure.",
                obs.observe_secs
            ),
        ),
        RunOutcome::ExitedWithError => observed(
            "SP-RUN-002",
            title,
            Category::Runtime,
            Status::Fail,
            format!(
                "Exited with code {} after {secs:.1} s.",
                obs.exit_code.map_or("?".to_owned(), |c| c.to_string())
            ),
        ),
        RunOutcome::Crashed => observed(
            "SP-RUN-002",
            title,
            Category::Runtime,
            Status::Fail,
            format!("Crashed after {secs:.1} s."),
        )
        .with_details(obs.log.crashes.clone()),
        RunOutcome::LaunchFailed => observed(
            "SP-RUN-002",
            title,
            Category::Runtime,
            Status::NotTested,
            "The game never started.",
        ),
    };
    with_log(c, obs)
}

fn log_errors(obs: &RunObservation) -> CheckResult {
    let title = "No crash signatures in the runtime log";
    let c = if obs.log.crashes.is_empty() {
        observed(
            "SP-RUN-003",
            title,
            Category::Runtime,
            Status::Pass,
            "No unhandled exceptions were logged.",
        )
    } else {
        observed(
            "SP-RUN-003",
            title,
            Category::Runtime,
            Status::Fail,
            format!(
                "{} crash signature(s) in the Wine log.",
                obs.log.crashes.len()
            ),
        )
        .with_details(obs.log.crashes.clone())
    };
    with_log(c, obs)
}

fn window(obs: &RunObservation) -> CheckResult {
    let title = "Game opens a window";
    if obs.outcome == RunOutcome::LaunchFailed {
        return observed(
            "SP-DISP-001",
            title,
            Category::Display,
            Status::NotTested,
            "The game never started.",
        );
    }
    let real: Vec<_> = obs
        .windows
        .iter()
        .filter(|w| w.width > 1 && w.height > 1)
        .collect();
    match real
        .iter()
        .max_by_key(|w| u64::from(w.width) * u64::from(w.height))
    {
        Some(w) => observed(
            "SP-DISP-001",
            title,
            Category::Display,
            Status::Pass,
            format!("Largest window: {}x{}.", w.width, w.height),
        )
        .with_details(
            real.iter()
                .map(|w| format!("{:?} {}x{}", w.title, w.width, w.height))
                .collect(),
        ),
        None if obs.outcome == RunOutcome::Survived => observed(
            "SP-DISP-001",
            title,
            Category::Display,
            Status::Fail,
            "No game window appeared during the observation window.",
        ),
        // It ended early; a window may have existed between two checks.
        None => observed(
            "SP-DISP-001",
            title,
            Category::Display,
            Status::Unknown,
            format!(
                "The game ended after {:.1} s and no window was seen before that.",
                obs.duration_ms as f64 / 1000.0
            ),
        ),
    }
}

fn display_checks(obs: &RunObservation) -> Vec<CheckResult> {
    let ids = [
        ("SP-DISP-002", "Game renders visible content"),
        ("SP-DISP-003", "Game fills the screen"),
    ];
    let not_tested = |why: &str| -> Vec<CheckResult> {
        ids.iter()
            .map(|(id, title)| {
                CheckResult::new(
                    id,
                    title,
                    Category::Display,
                    Status::NotTested,
                    Evidence::None,
                    why,
                )
            })
            .collect()
    };
    if obs.outcome == RunOutcome::LaunchFailed {
        return not_tested("The game never started.");
    }
    match obs.capture_verified {
        Some(true) => {}
        Some(false) => {
            return not_tested(
                "Screen capture does not work with this runtime here: the control fixture's frame was not captured, so a black screenshot would prove nothing.",
            );
        }
        None => return not_tested("The capture control run did not happen."),
    }
    let Some(shot) = representative(&obs.screenshots) else {
        return not_tested("No screenshot was taken (the game exited before the first capture).");
    };
    let st = &shot.stats;
    let mut out = Vec::new();
    let visible = if st.is_black() {
        CheckResult::new(
            ids[0].0,
            ids[0].1,
            Category::Display,
            Status::Warning,
            Evidence::Heuristic,
            format!(
                "Every captured frame was black (last at {:.0} s). Either a long loading screen or a rendering failure.",
                shot.at_secs
            ),
        )
    } else if st.is_uniform() {
        CheckResult::new(
            ids[0].0,
            ids[0].1,
            Category::Display,
            Status::Warning,
            Evidence::Heuristic,
            "The frame is a single flat colour; the game may be stuck before drawing anything.",
        )
    } else {
        CheckResult::new(
            ids[0].0,
            ids[0].1,
            Category::Display,
            Status::Pass,
            Evidence::Heuristic,
            format!(
                "Captured a frame with visible content at {:.0} s.",
                shot.at_secs
            ),
        )
    };
    out.push(visible.with_artifact(shot.path.clone()));

    let (dw, dh) = obs.display;
    let b = st.bars;
    // The largest window that is not tiny; a game in windowed mode leaves the
    // rest of the screen black, which is not letterboxing.
    let game_window = obs
        .windows
        .iter()
        .filter(|w| w.width >= dw / 2 && w.height >= dh / 2)
        .max_by_key(|w| u64::from(w.width) * u64::from(w.height));
    let windowed = game_window.filter(|w| w.width < dw || w.height < dh);
    let fill = if let Some(w) = windowed {
        CheckResult::new(
            ids[1].0,
            ids[1].1,
            Category::Display,
            Status::Warning,
            Evidence::Observed,
            format!(
                "The game opened a {}x{} window on the {dw}x{dh} display instead of filling it. \
                 SteamOS's gamescope scales such a window to the screen, which can blur text; \
                 Valve recommends rendering at the device resolution by default.",
                w.width, w.height
            ),
        )
    } else if st.is_black() || st.is_uniform() {
        CheckResult::new(
            ids[1].0,
            ids[1].1,
            Category::Display,
            Status::Unknown,
            Evidence::Heuristic,
            "No content to measure.",
        )
    } else if b.top + b.bottom > dh / 20 || b.left + b.right > dw / 20 {
        CheckResult::new(
            ids[1].0,
            ids[1].1,
            Category::Display,
            Status::Warning,
            Evidence::Heuristic,
            format!(
                "Black bars at {dw}x{dh}: top {} px, bottom {} px, left {} px, right {} px. The game may not support this aspect ratio, or the scene may simply be dark at the edges.",
                b.top, b.bottom, b.left, b.right
            ),
        )
    } else {
        CheckResult::new(
            ids[1].0,
            ids[1].1,
            Category::Display,
            Status::Pass,
            Evidence::Heuristic,
            format!("Content reaches the edges of the {dw}x{dh} display."),
        )
    };
    out.push(fill.with_artifact(shot.path.clone()));

    out
}

/// Uses only the window list, so it does not depend on screen capture.
fn dialog_check(obs: &RunObservation) -> CheckResult {
    let ids = [("SP-DISP-004", "No startup dialog or extra window")];
    if obs.outcome == RunOutcome::LaunchFailed {
        return CheckResult::new(
            ids[0].0,
            ids[0].1,
            Category::Display,
            Status::NotTested,
            Evidence::None,
            "The game never started.",
        );
    }
    let (dw, dh) = obs.display;
    let small: Vec<String> = obs
        .windows
        .iter()
        .filter(|w| w.width > 1 && w.height > 1 && (w.width < dw / 2 || w.height < dh / 2))
        .map(|w| format!("{:?} {}x{}", w.title, w.width, w.height))
        .collect();
    if small.is_empty() {
        CheckResult::new(
            ids[0].0,
            ids[0].1,
            Category::Display,
            Status::Pass,
            Evidence::Heuristic,
            "No small extra windows appeared.",
        )
    } else {
        CheckResult::new(ids[0].0, ids[0].1, Category::Display, Status::Warning, Evidence::Heuristic, "Small windows appeared, possibly an error, launcher or 'unsupported hardware' dialog. Valve fails games that warn about the OS or GPU.")
            .with_details(small)
    }
}

/// A game can keep running while it shows a fatal problem in its own window
/// (OpenTTD without its graphics set does). OCR is the only signal for that,
/// so the result is a heuristic WARNING, never FAIL.
fn screen_text_check(obs: &RunObservation) -> CheckResult {
    let (id, title) = ("SP-DISP-005", "No error message on screen");
    let not_tested = |why: String| {
        CheckResult::new(
            id,
            title,
            Category::Display,
            Status::NotTested,
            Evidence::None,
            why,
        )
    };
    if obs.outcome == RunOutcome::LaunchFailed {
        return not_tested("The game never started.".into());
    }
    if obs.capture_verified != Some(true) {
        return not_tested(
            "Screen capture is not verified for this runtime, so no text could be read.".into(),
        );
    }
    match &obs.screen_text {
        ScreenText::NotRead(why) => not_tested(why.clone()),
        ScreenText::Read { path, text } => {
            let hits = error_snippets(text);
            let words = text.split_whitespace().count();
            if hits.is_empty() {
                CheckResult::new(
                    id,
                    title,
                    Category::Display,
                    Status::Pass,
                    Evidence::Heuristic,
                    format!("OCR read {words} words on screen; none look like an error message."),
                )
                .with_artifact(path.clone())
            } else {
                CheckResult::new(id, title, Category::Display, Status::Warning, Evidence::Heuristic, "Text on screen reads like an error message. A game can keep running while it shows a fatal problem; check the screenshot.")
                    .with_details(hits)
                    .with_artifact(path.clone())
            }
        }
    }
}

fn hardware_not_tested(_obs: &RunObservation) -> Vec<CheckResult> {
    [
        ("SP-HW-001", "Frame rate at default settings", "Valve's 30 fps target needs real Deck or Steam Machine hardware; this run uses CPU software rendering."),
        ("SP-HW-002", "Controller-only play", "Needs a person with a controller, or scripted input, which this version does not provide."),
        ("SP-HW-003", "Text legible at 1280x800", "Text size is not measured in this version (OCR is used only to find error messages)."),
    ]
    .into_iter()
    .map(|(id, title, why)| CheckResult::new(id, title, Category::Hardware, Status::NotTested, Evidence::None, why))
    .collect()
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::observe::{Bars, FrameStats, LogFindings, Screenshot, WindowSeen};

    fn stats(black: bool, uniform: bool, bars: Bars) -> FrameStats {
        FrameStats {
            width: 1280,
            height: 800,
            mean_rgb: [80.0, 80.0, 80.0],
            stddev: if uniform { 0.5 } else { 40.0 },
            black_fraction: if black { 1.0 } else { 0.1 },
            bars,
        }
    }

    fn obs(outcome: RunOutcome) -> RunObservation {
        RunObservation {
            runtime: "ge-proton11-12".into(),
            runtime_version: "GE-Proton11-12".into(),
            observe_secs: 20,
            outcome,
            exit_code: None,
            duration_ms: 20_000,
            capture_verified: Some(true),
            windows: vec![WindowSeen {
                title: "Game".into(),
                width: 1280,
                height: 800,
            }],
            screenshots: vec![Screenshot {
                path: "runs/x/shot.png".into(),
                at_secs: 18.0,
                stats: stats(false, false, Bars::default()),
            }],
            log: LogFindings::default(),
            log_path: Some("runs/x/runtime.log".into()),
            display: (1280, 800),
            screen_text: ScreenText::Read {
                path: "runs/x/shot.png".into(),
                text: "New Game\nQuit".into(),
            },
        }
    }

    fn get(r: &[CheckResult], id: &str) -> CheckResult {
        r.iter().find(|c| c.id == id).unwrap().clone()
    }

    #[test]
    fn healthy_run_passes_and_hardware_is_not_tested() {
        let r = evaluate(&obs(RunOutcome::Survived));
        for id in [
            "SP-RUN-001",
            "SP-RUN-002",
            "SP-RUN-003",
            "SP-DISP-001",
            "SP-DISP-002",
            "SP-DISP-003",
            "SP-DISP-004",
        ] {
            assert_eq!(get(&r, id).status, Status::Pass, "{id}");
        }
        for id in ["SP-HW-001", "SP-HW-002", "SP-HW-003"] {
            assert_eq!(get(&r, id).status, Status::NotTested, "{id}");
        }
        assert!(
            r.iter()
                .all(|c| c.runtime.as_deref() == Some("ge-proton11-12"))
        );
    }

    #[test]
    fn crash_fails_stability_and_log() {
        let mut o = obs(RunOutcome::Crashed);
        o.log.crashes = vec!["wine: Unhandled page fault".into()];
        let r = evaluate(&o);
        assert_eq!(get(&r, "SP-RUN-002").status, Status::Fail);
        assert_eq!(get(&r, "SP-RUN-003").status, Status::Fail);
    }

    #[test]
    fn nonzero_exit_fails_and_clean_early_exit_warns() {
        let mut o = obs(RunOutcome::ExitedWithError);
        o.exit_code = Some(3);
        assert_eq!(get(&evaluate(&o), "SP-RUN-002").status, Status::Fail);
        assert_eq!(
            get(&evaluate(&obs(RunOutcome::ExitedCleanly)), "SP-RUN-002").status,
            Status::Warning
        );
    }

    #[test]
    fn missing_dll_in_log_fails_launch() {
        let mut o = obs(RunOutcome::ExitedWithError);
        o.log.missing_dlls = vec!["fx_absent.dll".into()];
        assert_eq!(get(&evaluate(&o), "SP-RUN-001").status, Status::Fail);
    }

    #[test]
    fn unverified_capture_never_reports_black_screen() {
        let mut o = obs(RunOutcome::Survived);
        o.capture_verified = Some(false);
        o.screenshots[0].stats = stats(true, true, Bars::default());
        let r = evaluate(&o);
        for id in ["SP-DISP-002", "SP-DISP-003"] {
            assert_eq!(get(&r, id).status, Status::NotTested, "{id}");
        }
        // The dialog check reads the window list, not pixels.
        assert_eq!(get(&r, "SP-DISP-004").status, Status::Pass);
    }

    #[test]
    fn black_frame_is_a_heuristic_warning() {
        let mut o = obs(RunOutcome::Survived);
        o.screenshots[0].stats = stats(true, true, Bars::default());
        let c = get(&evaluate(&o), "SP-DISP-002");
        assert_eq!(
            (c.status, c.evidence),
            (Status::Warning, Evidence::Heuristic)
        );
        assert_eq!(get(&evaluate(&o), "SP-DISP-003").status, Status::Unknown);
    }

    #[test]
    fn windowed_game_is_reported_as_windowed_not_letterboxed() {
        let mut o = obs(RunOutcome::Survived);
        o.windows = vec![WindowSeen {
            title: "OpenTTD".into(),
            width: 960,
            height: 600,
        }];
        o.screenshots[0].stats = stats(
            false,
            false,
            Bars {
                top: 100,
                bottom: 100,
                left: 160,
                right: 160,
            },
        );
        let c = get(&evaluate(&o), "SP-DISP-003");
        assert_eq!(
            (c.status, c.evidence),
            (Status::Warning, Evidence::Observed)
        );
        assert!(c.summary.contains("960x600 window"), "{}", c.summary);
        // A small dialog next to a full-screen game is not the game window.
        let mut o = obs(RunOutcome::Survived);
        o.windows.push(WindowSeen {
            title: "Error".into(),
            width: 400,
            height: 150,
        });
        assert_eq!(get(&evaluate(&o), "SP-DISP-003").status, Status::Pass);
    }

    #[test]
    fn in_window_error_text_warns_even_though_the_game_survived() {
        // Regression: OpenTTD without graphics stays alive and draws its
        // error inside its window; it used to pass every check.
        let mut o = obs(RunOutcome::Survived);
        o.screen_text = ScreenText::Read {
            path: "runs/x/shot.png".into(),
            text:
                "Missing graphics\nOpenTTD requires graphics to function but none could be found."
                    .into(),
        };
        let r = evaluate(&o);
        let c = get(&r, "SP-DISP-005");
        assert_eq!(
            (c.status, c.evidence),
            (Status::Warning, Evidence::Heuristic)
        );
        assert_eq!(get(&r, "SP-RUN-002").status, Status::Pass);
        assert_eq!(
            crate::model::verdict(&r, crate::model::FailOn::Warning),
            crate::model::Verdict::Fail
        );
    }

    #[test]
    fn screen_text_is_not_tested_without_ocr_or_capture() {
        let mut o = obs(RunOutcome::Survived);
        o.screen_text = ScreenText::NotRead("tesseract is not installed".into());
        assert_eq!(get(&evaluate(&o), "SP-DISP-005").status, Status::NotTested);
        let mut o = obs(RunOutcome::Survived);
        o.capture_verified = Some(false);
        assert_eq!(get(&evaluate(&o), "SP-DISP-005").status, Status::NotTested);
        assert_eq!(
            get(&evaluate(&obs(RunOutcome::Survived)), "SP-DISP-005").status,
            Status::Pass
        );
    }

    #[test]
    fn letterbox_warns() {
        let mut o = obs(RunOutcome::Survived);
        o.screenshots[0].stats = stats(
            false,
            false,
            Bars {
                top: 40,
                bottom: 40,
                left: 0,
                right: 0,
            },
        );
        assert_eq!(get(&evaluate(&o), "SP-DISP-003").status, Status::Warning);
    }

    #[test]
    fn missing_window_fails_and_small_window_warns() {
        let mut o = obs(RunOutcome::Survived);
        o.windows.clear();
        assert_eq!(get(&evaluate(&o), "SP-DISP-001").status, Status::Fail);
        // Regression: a crash before the first window check is not proof
        // that no window was opened.
        let mut early = obs(RunOutcome::Crashed);
        early.windows.clear();
        assert_eq!(
            get(&evaluate(&early), "SP-DISP-001").status,
            Status::Unknown
        );
        let mut o = obs(RunOutcome::Survived);
        o.windows.push(WindowSeen {
            title: "Error".into(),
            width: 400,
            height: 150,
        });
        assert_eq!(get(&evaluate(&o), "SP-DISP-004").status, Status::Warning);
    }

    #[test]
    fn launch_failure_marks_dependent_checks_not_tested() {
        let r = evaluate(&obs(RunOutcome::LaunchFailed));
        assert_eq!(get(&r, "SP-RUN-001").status, Status::Fail);
        for id in ["SP-RUN-002", "SP-DISP-001", "SP-DISP-002"] {
            assert_eq!(get(&r, id).status, Status::NotTested, "{id}");
        }
    }

    #[test]
    fn last_non_black_frame_represents_the_run() {
        let mut o = obs(RunOutcome::Survived);
        o.screenshots = vec![
            Screenshot {
                path: "a.png".into(),
                at_secs: 5.0,
                stats: stats(false, false, Bars::default()),
            },
            Screenshot {
                path: "b.png".into(),
                at_secs: 15.0,
                stats: stats(true, true, Bars::default()),
            },
        ];
        assert_eq!(get(&evaluate(&o), "SP-DISP-002").artifacts, vec!["a.png"]);
    }
}
