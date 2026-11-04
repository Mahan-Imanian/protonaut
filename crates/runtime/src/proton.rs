use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use preflight_core::model::RunOutcome;
use preflight_core::observe::{
    LogFindings, RunObservation, ScreenText, Screenshot, WindowSeen, analyse_log, representative,
};
use preflight_core::report::redact;
use tokio::process::Command;

use crate::RuntimeError;
use crate::capture;
use crate::display::Display;
use crate::install::InstalledRuntime;
use crate::supervise::{Exit, SAFE_PATH, Spec, Supervised, read_log_tail};

/// The control fixture: a D3D11 program that clears every frame to a known
/// colour. Built from `fixtures/src/fixture.cpp`, see `fixtures/README.md`.
pub const CONTROL_EXE: &[u8] = include_bytes!("../../../fixtures/bin/fx_render_ok.exe");
const CONTROL_SECS: u64 = 12;
const CONTROL_CAPTURE_AT: f64 = 5.0;
const LOG_LIMIT: u64 = 2 * 1024 * 1024;
const STOP_GRACE: Duration = Duration::from_secs(5);
const POLL: Duration = Duration::from_millis(250);
const WINDOW_POLL: Duration = Duration::from_secs(1);
/// Mesa's lavapipe ICD manifest; the file name differs between distributions
/// (Ubuntu 24.04 ships `lvp_icd.json`).
const LAVAPIPE_ICDS: &[&str] = &[
    "/usr/share/vulkan/icd.d/lvp_icd.json",
    "/usr/share/vulkan/icd.d/lvp_icd.x86_64.json",
];

pub fn lavapipe_icd() -> Option<&'static str> {
    LAVAPIPE_ICDS
        .iter()
        .copied()
        .find(|p| Path::new(p).is_file())
}

pub struct GameRequest<'a> {
    pub build_dir: &'a Path,
    /// Relative to `build_dir`, `/`-separated.
    pub exe: &'a str,
    pub args: &'a [String],
    pub observe_secs: u64,
    /// Results directory; artifacts are written below `runs/<runtime id>/`.
    pub results: &'a Path,
    /// Scratch directory for prefixes and the sandbox home.
    pub work: &'a Path,
    /// Values to replace in logs before saving them.
    pub redactions: &'a [(String, &'a str)],
}

pub struct RunOutput {
    pub observation: RunObservation,
    /// Vulkan device the control fixture reported, if any.
    pub vulkan_device: Option<String>,
}

pub struct ProtonBackend<'a> {
    pub runtime: &'a InstalledRuntime,
    pub display: &'a Display,
}

impl ProtonBackend<'_> {
    fn id(&self) -> &str {
        &self.runtime.spec.id
    }

    fn env(
        &self,
        compat_dir: &Path,
        sandbox: &Path,
    ) -> Result<Vec<(String, OsString)>, RuntimeError> {
        let home = sandbox.join("home");
        let xdg = sandbox.join("xdg");
        let steam = sandbox.join("steam");
        for d in [&home, &xdg, &steam, &compat_dir.to_path_buf()] {
            fs::create_dir_all(d).map_err(|e| RuntimeError::io(d, e))?;
        }
        set_mode_700(&xdg)?;
        let mut env: Vec<(String, OsString)> = vec![
            ("PATH".into(), SAFE_PATH.into()),
            ("HOME".into(), home.into()),
            ("XDG_RUNTIME_DIR".into(), xdg.into()),
            ("DISPLAY".into(), self.display.name().into()),
            ("LANG".into(), "C.UTF-8".into()),
            ("LC_ALL".into(), "C.UTF-8".into()),
            ("STEAM_COMPAT_DATA_PATH".into(), compat_dir.into()),
            ("STEAM_COMPAT_CLIENT_INSTALL_PATH".into(), steam.into()),
            // Keep err: lines (loader failures), drop fixme noise.
            ("WINEDEBUG".into(), "fixme-all".into()),
            // Without a debugger an unhandled exception terminates the
            // process instead of hanging in `winedbg --auto`.
            ("WINEDLLOVERRIDES".into(), "winedbg.exe=d".into()),
        ];
        if let Some(icd) = lavapipe_icd() {
            // Force software Vulkan so results do not depend on host GPUs.
            env.push(("VK_DRIVER_FILES".into(), icd.into()));
            env.push(("VK_ICD_FILENAMES".into(), icd.into()));
        }
        Ok(env)
    }

    fn spec(
        &self,
        exe: &Path,
        args: &[String],
        cwd: &Path,
        env: Vec<(String, OsString)>,
        log: PathBuf,
    ) -> Spec {
        let mut a: Vec<OsString> = vec![
            self.runtime.root.join("proton").into(),
            "run".into(),
            exe.into(),
        ];
        a.extend(args.iter().map(OsString::from));
        Spec {
            program: "/usr/bin/python3".into(),
            args: a,
            env,
            cwd: cwd.to_path_buf(),
            log,
        }
    }

    /// Stops every Wine process that belongs to `compat_dir`'s prefix.
    async fn kill_wineserver(&self, compat_dir: &Path) {
        let wineserver = self.runtime.root.join("files/bin/wineserver");
        let mut cmd = Command::new(&wineserver);
        cmd.arg("-k")
            .env_clear()
            .env("PATH", SAFE_PATH)
            .env("WINEPREFIX", compat_dir.join("pfx"))
            .kill_on_drop(true);
        let _ = tokio::time::timeout(Duration::from_secs(15), cmd.status()).await;
    }

    /// Runs the control fixture. Returns whether its frame was captured
    /// correctly, and the device it reported.
    async fn control(
        &self,
        sandbox: &Path,
        run_dir: &Path,
    ) -> Result<(bool, Option<String>, PathBuf), RuntimeError> {
        let dir = sandbox.join("control");
        fs::create_dir_all(&dir).map_err(|e| RuntimeError::io(&dir, e))?;
        let exe = dir.join("control.exe");
        fs::write(&exe, CONTROL_EXE).map_err(|e| RuntimeError::io(&exe, e))?;
        let compat = sandbox.join("prefix-control");
        let env = self.env(&compat, sandbox)?;
        let log = sandbox.join("control.log");
        let spec = self.spec(
            &exe,
            &[format!("--seconds={CONTROL_SECS}"), "--report".to_owned()],
            &dir,
            env,
            log,
        );
        let mut proc = Supervised::spawn(&spec)?;
        let shot = run_dir.join("control.png");
        let mut verified = false;
        let mut attempts = 0u32;
        let mut next_attempt = Duration::ZERO;
        let deadline = Duration::from_secs(CONTROL_SECS + 90);
        while proc.elapsed() < deadline {
            if proc.try_exit()?.is_some() {
                break;
            }
            // Prefix creation takes several seconds before the fixture
            // starts its own clock. Once its window exists, capture every
            // second until the colour appears: on a cold start DXVK's first
            // frame can arrive seconds after the window does.
            if !verified
                && proc.elapsed().as_secs_f64() >= CONTROL_CAPTURE_AT
                && proc.elapsed() >= next_attempt
            {
                let wins = capture::windows(&self.display.name())
                    .await
                    .unwrap_or_default();
                if wins.iter().any(|w| w.width >= 640) {
                    if let Ok(stats) = capture::screenshot(&self.display.name(), &shot).await {
                        verified = stats.matches_control();
                    }
                    attempts += 1;
                }
                if verified {
                    break;
                }
                next_attempt = proc.elapsed() + Duration::from_secs(1);
            }
            tokio::time::sleep(POLL).await;
        }
        proc.stop(STOP_GRACE).await;
        self.kill_wineserver(&compat).await;
        tracing::debug!(attempts, verified, "capture control attempts");
        let device = fs::read_to_string(dir.join("result.txt"))
            .ok()
            .and_then(|t| {
                t.lines()
                    .find_map(|l| l.strip_prefix("adapter=").map(str::to_owned))
            });
        Ok((verified, device, compat))
    }

    pub async fn run(&self, req: &GameRequest<'_>) -> Result<RunOutput, RuntimeError> {
        let id = self.id().to_owned();
        let run_rel = format!("runs/{id}");
        let run_dir = req.results.join(&run_rel);
        let sandbox = req.work.join(&id);
        for d in [&run_dir, &sandbox] {
            fs::create_dir_all(d).map_err(|e| RuntimeError::io(d, e))?;
        }
        let (capture_verified, device, control_prefix) = self.control(&sandbox, &run_dir).await?;
        tracing::info!(runtime = %id, capture_verified, "control run finished");

        let compat = sandbox.join("prefix-game");
        copy_tree(&control_prefix, &compat).await?;
        let env = self.env(&compat, &sandbox)?;
        let exe_path = req.build_dir.join(req.exe);
        let cwd = exe_path.parent().unwrap_or(req.build_dir).to_path_buf();
        let raw_log = sandbox.join("game.log");
        let spec = self.spec(&exe_path, req.args, &cwd, env, raw_log.clone());

        let observe = Duration::from_secs(req.observe_secs);
        let capture_times = [
            observe.as_secs_f64() * 0.5,
            (observe.as_secs_f64() - 1.5).max(2.0),
        ];
        let mut next_capture = 0usize;
        let mut windows: Vec<WindowSeen> = Vec::new();
        let mut screenshots = Vec::new();
        let started = Instant::now();
        let mut proc = match Supervised::spawn(&spec) {
            Ok(p) => p,
            Err(e) => {
                tracing::warn!(error = %e, "launch failed");
                return Ok(RunOutput {
                    observation: self.observation(
                        req,
                        RunOutcome::LaunchFailed,
                        None,
                        0,
                        capture_verified,
                        vec![],
                        vec![],
                        LogFindings::default(),
                        None,
                        ScreenText::NotRead("The game never started.".into()),
                    ),
                    vulkan_device: device,
                });
            }
        };
        let mut exit = None;
        let mut next_window_poll = Duration::ZERO;
        while proc.elapsed() < observe {
            if let Some(e) = proc.try_exit()? {
                exit = Some(e);
                break;
            }
            // Windows are listed often, so a game that crashes early is
            // still seen to have opened its window.
            if proc.elapsed() >= next_window_poll {
                for w in capture::windows(&self.display.name())
                    .await
                    .unwrap_or_default()
                {
                    if !windows.contains(&w) {
                        windows.push(w);
                    }
                }
                next_window_poll = proc.elapsed() + WINDOW_POLL;
            }
            if next_capture < capture_times.len()
                && proc.elapsed().as_secs_f64() >= capture_times[next_capture]
            {
                for w in capture::windows(&self.display.name())
                    .await
                    .unwrap_or_default()
                {
                    if !windows.contains(&w) {
                        windows.push(w);
                    }
                }
                let name = format!(
                    "screenshot-{}-{}.png",
                    next_capture + 1,
                    self.display.resolution
                );
                let path = run_dir.join(&name);
                match capture::screenshot(&self.display.name(), &path).await {
                    Ok(stats) => screenshots.push(Screenshot {
                        path: format!("{run_rel}/{name}"),
                        at_secs: proc.elapsed().as_secs_f64(),
                        stats,
                    }),
                    Err(e) => tracing::warn!(error = %e, "screenshot failed"),
                }
                next_capture += 1;
            }
            tokio::time::sleep(POLL).await;
        }
        let exit = match exit {
            Some(e) => {
                proc.stop(STOP_GRACE).await;
                e
            }
            None => proc.stop(STOP_GRACE).await,
        };
        let duration_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
        self.kill_wineserver(&compat).await;

        let text = redact(&read_log_tail(&raw_log, LOG_LIMIT), req.redactions);
        let log_rel = format!("{run_rel}/runtime.log");
        let log_out = req.results.join(&log_rel);
        fs::write(&log_out, &text).map_err(|e| RuntimeError::io(&log_out, e))?;
        let findings = analyse_log(&text);
        let (outcome, code) = classify(exit, &findings);
        let screen_text = if !capture_verified {
            ScreenText::NotRead("Screen capture is not verified for this runtime.".into())
        } else {
            match representative(&screenshots) {
                None => ScreenText::NotRead("No screenshot was taken.".into()),
                Some(shot) => {
                    match capture::read_text(&req.results.join(&shot.path), &sandbox).await {
                        Ok(t) => ScreenText::Read {
                            path: shot.path.clone(),
                            text: redact(&t, req.redactions),
                        },
                        Err(e) => ScreenText::NotRead(format!(
                            "OCR did not run ({e}); install tesseract-ocr."
                        )),
                    }
                }
            }
        };
        Ok(RunOutput {
            observation: self.observation(
                req,
                outcome,
                code,
                duration_ms,
                capture_verified,
                windows,
                screenshots,
                findings,
                Some(log_rel),
                screen_text,
            ),
            vulkan_device: device,
        })
    }

    #[allow(clippy::too_many_arguments)]
    fn observation(
        &self,
        req: &GameRequest<'_>,
        outcome: RunOutcome,
        exit_code: Option<i32>,
        duration_ms: u64,
        capture_verified: bool,
        windows: Vec<WindowSeen>,
        screenshots: Vec<Screenshot>,
        log: LogFindings,
        log_path: Option<String>,
        screen_text: ScreenText,
    ) -> RunObservation {
        RunObservation {
            runtime: self.id().to_owned(),
            runtime_version: self.runtime.version(),
            observe_secs: req.observe_secs,
            outcome,
            exit_code,
            duration_ms,
            capture_verified: Some(capture_verified),
            windows,
            screenshots,
            log,
            log_path,
            display: (
                self.display.resolution.width,
                self.display.resolution.height,
            ),
            screen_text,
        }
    }
}

/// Stops Wine in every prefix under `work` for this runtime. Used after a
/// cancelled run, when the normal per-run cleanup did not get to execute.
pub async fn kill_prefixes(runtime: &InstalledRuntime, work: &Path) {
    let sandbox = work.join(&runtime.spec.id);
    for name in ["prefix-control", "prefix-game"] {
        let compat = sandbox.join(name);
        if compat.join("pfx").is_dir() {
            let mut cmd = Command::new(runtime.root.join("files/bin/wineserver"));
            cmd.arg("-k")
                .env_clear()
                .env("PATH", SAFE_PATH)
                .env("WINEPREFIX", compat.join("pfx"))
                .kill_on_drop(true);
            let _ = tokio::time::timeout(Duration::from_secs(15), cmd.status()).await;
        }
    }
}

/// Crash signatures in the log win over the exit code, because Wine reports
/// an unhandled exception as an ordinary small exit code.
pub fn classify(exit: Exit, log: &LogFindings) -> (RunOutcome, Option<i32>) {
    if !log.crashes.is_empty() {
        let code = if let Exit::Code(c) = exit {
            Some(c)
        } else {
            None
        };
        return (RunOutcome::Crashed, code);
    }
    match exit {
        Exit::StillRunning => (RunOutcome::Survived, None),
        Exit::Code(0) => (RunOutcome::ExitedCleanly, Some(0)),
        Exit::Code(c) if !log.launch_errors.is_empty() => (RunOutcome::LaunchFailed, Some(c)),
        Exit::Code(c) => (RunOutcome::ExitedWithError, Some(c)),
        Exit::Signal(_) => (RunOutcome::Crashed, None),
    }
}

async fn copy_tree(from: &Path, to: &Path) -> Result<(), RuntimeError> {
    if to.exists() {
        fs::remove_dir_all(to).map_err(|e| RuntimeError::io(to, e))?;
    }
    crate::supervise::run_tool(
        "cp",
        &["-a", "--"],
        &[from.as_os_str(), to.as_os_str()],
        Duration::from_secs(300),
    )
    .await?;
    Ok(())
}

fn set_mode_700(p: &Path) -> Result<(), RuntimeError> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(p, fs::Permissions::from_mode(0o700)).map_err(|e| RuntimeError::io(p, e))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn log(crash: bool, launch: bool) -> LogFindings {
        LogFindings {
            crashes: if crash {
                vec!["wine: Unhandled page fault".into()]
            } else {
                vec![]
            },
            missing_dlls: vec![],
            launch_errors: if launch {
                vec!["wine: cannot find".into()]
            } else {
                vec![]
            },
        }
    }

    #[test]
    fn classification_prefers_crash_signatures_over_exit_codes() {
        assert_eq!(
            classify(Exit::Code(0), &log(true, false)).0,
            RunOutcome::Crashed
        );
        assert_eq!(
            classify(Exit::StillRunning, &log(false, false)).0,
            RunOutcome::Survived
        );
        assert_eq!(
            classify(Exit::Code(0), &log(false, false)),
            (RunOutcome::ExitedCleanly, Some(0))
        );
        assert_eq!(
            classify(Exit::Code(3), &log(false, false)),
            (RunOutcome::ExitedWithError, Some(3))
        );
        assert_eq!(
            classify(Exit::Code(1), &log(false, true)).0,
            RunOutcome::LaunchFailed
        );
        assert_eq!(
            classify(Exit::Signal(9), &log(false, false)).0,
            RunOutcome::Crashed
        );
    }

    #[test]
    fn embedded_control_fixture_is_a_pe_file() {
        assert_eq!(&CONTROL_EXE[..2], b"MZ");
    }
}
