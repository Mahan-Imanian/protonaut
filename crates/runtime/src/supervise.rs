//! Running external programs: trusted helper tools (`curl`, `tar`, `import`)
//! and the untrusted build under test. Arguments are always passed as a
//! vector, never through a shell.

use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::{Duration, Instant};

use nix::sys::signal::{Signal, killpg};
use nix::unistd::Pid;
use tokio::process::{Child, Command};

use crate::RuntimeError;

/// Variables passed to the build under test. Everything else, including
/// `GITHUB_TOKEN`, `ACTIONS_*` and cloud credentials, is dropped.
pub const SAFE_PATH: &str = "/usr/local/bin:/usr/bin:/bin";

/// Runs a trusted helper to completion and fails on a nonzero exit.
pub async fn run_tool(
    program: &str,
    flags: &[&str],
    args: &[&OsStr],
    timeout: Duration,
) -> Result<String, RuntimeError> {
    let mut cmd = Command::new(program);
    cmd.args(flags)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    let child = cmd.spawn().map_err(|e| RuntimeError::Tool {
        tool: program.to_owned(),
        message: e.to_string(),
    })?;
    let out = tokio::time::timeout(timeout, child.wait_with_output())
        .await
        .map_err(|_| RuntimeError::Tool {
            tool: program.to_owned(),
            message: format!("timed out after {}s", timeout.as_secs()),
        })?
        .map_err(|e| RuntimeError::Tool {
            tool: program.to_owned(),
            message: e.to_string(),
        })?;
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr);
        return Err(RuntimeError::Tool {
            tool: program.to_owned(),
            message: format!("{}: {}", out.status, stderr.trim()),
        });
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// Description of a supervised, untrusted process.
#[derive(Debug, Clone)]
pub struct Spec {
    pub program: PathBuf,
    pub args: Vec<OsString>,
    pub env: Vec<(String, OsString)>,
    pub cwd: PathBuf,
    /// stdout and stderr both go here.
    pub log: PathBuf,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Exit {
    /// Still running when the window ended; it was then stopped.
    StillRunning,
    Code(i32),
    Signal(i32),
}

/// A running untrusted process group. Dropping it kills the whole group, so
/// cancellation (Ctrl-C, a timeout in a caller) cannot leave orphans.
pub struct Supervised {
    child: Child,
    pgid: Pid,
    started: Instant,
    reaped: Option<Exit>,
}

impl Supervised {
    pub fn spawn(spec: &Spec) -> Result<Self, RuntimeError> {
        let log = std::fs::File::create(&spec.log).map_err(|e| RuntimeError::io(&spec.log, e))?;
        let log_err = log
            .try_clone()
            .map_err(|e| RuntimeError::io(&spec.log, e))?;
        let mut cmd = Command::new(&spec.program);
        cmd.args(&spec.args)
            .env_clear()
            .envs(spec.env.iter().map(|(k, v)| (k.as_str(), v.as_os_str())))
            .current_dir(&spec.cwd)
            .stdin(Stdio::null())
            .stdout(Stdio::from(log))
            .stderr(Stdio::from(log_err))
            .process_group(0)
            .kill_on_drop(true);
        let child = cmd.spawn().map_err(|e| RuntimeError::Spawn {
            program: spec.program.display().to_string(),
            message: e.to_string(),
        })?;
        let pid = child.id().ok_or_else(|| RuntimeError::Spawn {
            program: spec.program.display().to_string(),
            message: "exited before its pid was read".into(),
        })?;
        let pgid = Pid::from_raw(i32::try_from(pid).map_err(|_| RuntimeError::Spawn {
            program: "pid".into(),
            message: "pid out of range".into(),
        })?);
        Ok(Self {
            child,
            pgid,
            started: Instant::now(),
            reaped: None,
        })
    }

    pub fn elapsed(&self) -> Duration {
        self.started.elapsed()
    }

    /// Non-blocking: has the leader exited?
    pub fn try_exit(&mut self) -> Result<Option<Exit>, RuntimeError> {
        if let Some(e) = self.reaped {
            return Ok(Some(e));
        }
        match self.child.try_wait() {
            Ok(Some(status)) => {
                let e = exit_of(status);
                self.reaped = Some(e);
                Ok(Some(e))
            }
            Ok(None) => Ok(None),
            Err(e) => Err(RuntimeError::Spawn {
                program: "wait".into(),
                message: e.to_string(),
            }),
        }
    }

    /// Stops the group: SIGTERM, a grace period, then SIGKILL. Returns how
    /// the leader ended (`StillRunning` if it had to be stopped).
    pub async fn stop(mut self, grace: Duration) -> Exit {
        let before = self.try_exit().ok().flatten();
        let _ = killpg(self.pgid, Signal::SIGTERM);
        let deadline = Instant::now() + grace;
        while Instant::now() < deadline {
            if matches!(self.child.try_wait(), Ok(Some(_))) && !group_alive(self.pgid) {
                break;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        let _ = killpg(self.pgid, Signal::SIGKILL);
        let _ = self.child.wait().await;
        before.unwrap_or(Exit::StillRunning)
    }
}

impl Drop for Supervised {
    fn drop(&mut self) {
        // Safety net for early returns and cancellation.
        let _ = killpg(self.pgid, Signal::SIGKILL);
    }
}

fn group_alive(pgid: Pid) -> bool {
    killpg(pgid, None).is_ok()
}

fn exit_of(status: std::process::ExitStatus) -> Exit {
    use std::os::unix::process::ExitStatusExt;
    match (status.code(), status.signal()) {
        (Some(c), _) => Exit::Code(c),
        (None, Some(s)) => Exit::Signal(s),
        (None, None) => Exit::Code(-1),
    }
}

/// Reads at most `max` bytes from the end of a log file, lossily decoded.
pub fn read_log_tail(path: &Path, max: u64) -> String {
    use std::io::{Read, Seek, SeekFrom};
    let Ok(mut f) = std::fs::File::open(path) else {
        return String::new();
    };
    let len = f.metadata().map(|m| m.len()).unwrap_or(0);
    if len > max {
        let _ = f.seek(SeekFrom::Start(len - max));
    }
    let mut buf = Vec::new();
    let _ = f.take(max).read_to_end(&mut buf);
    String::from_utf8_lossy(&buf).into_owned()
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn sh(dir: &Path, script: &str) -> Spec {
        Spec {
            program: "/bin/sh".into(),
            args: vec!["-c".into(), script.into()],
            env: vec![("PATH".into(), SAFE_PATH.into())],
            cwd: dir.to_path_buf(),
            log: dir.join("log.txt"),
        }
    }

    #[tokio::test]
    async fn environment_is_cleared_to_the_allowlist() {
        // `cargo test` runs this process with HOME and CARGO_* set; none of it
        // may reach the child, exactly like GITHUB_TOKEN in a CI job.
        assert!(std::env::var_os("HOME").is_some() && std::env::var_os("CARGO_PKG_NAME").is_some());
        let dir = tempfile::tempdir().unwrap();
        let mut p = Supervised::spawn(&sh(dir.path(), "env")).unwrap();
        while p.try_exit().unwrap().is_none() {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        let log = std::fs::read_to_string(dir.path().join("log.txt")).unwrap();
        let names: Vec<&str> = log.lines().filter_map(|l| l.split('=').next()).collect();
        assert!(
            names
                .iter()
                .all(|n| ["PATH", "PWD", "SHLVL", "_"].contains(n)),
            "unexpected variables: {names:?}"
        );
    }

    #[tokio::test]
    async fn stop_kills_the_whole_group_including_grandchildren() {
        let dir = tempfile::tempdir().unwrap();
        let p = Supervised::spawn(&sh(
            dir.path(),
            "sleep 300 & echo $! > child.pid; sleep 300",
        ))
        .unwrap();
        tokio::time::sleep(Duration::from_millis(300)).await;
        let grandchild: i32 = std::fs::read_to_string(dir.path().join("child.pid"))
            .unwrap()
            .trim()
            .parse()
            .unwrap();
        assert_eq!(p.stop(Duration::from_secs(2)).await, Exit::StillRunning);
        tokio::time::sleep(Duration::from_millis(200)).await;
        assert!(
            nix::sys::signal::kill(Pid::from_raw(grandchild), None).is_err(),
            "grandchild survived"
        );
    }

    #[tokio::test]
    async fn exit_codes_and_signals_are_reported() {
        let dir = tempfile::tempdir().unwrap();
        let mut p = Supervised::spawn(&sh(dir.path(), "exit 3")).unwrap();
        while p.try_exit().unwrap().is_none() {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        assert_eq!(p.stop(Duration::from_millis(100)).await, Exit::Code(3));
        let mut p = Supervised::spawn(&sh(dir.path(), "kill -SEGV $$")).unwrap();
        while p.try_exit().unwrap().is_none() {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        assert_eq!(p.stop(Duration::from_millis(100)).await, Exit::Signal(11));
    }

    #[tokio::test]
    async fn dropping_a_supervised_process_kills_it() {
        let dir = tempfile::tempdir().unwrap();
        let p = Supervised::spawn(&sh(dir.path(), "echo $$ > me.pid; sleep 300")).unwrap();
        tokio::time::sleep(Duration::from_millis(300)).await;
        let pid: i32 = std::fs::read_to_string(dir.path().join("me.pid"))
            .unwrap()
            .trim()
            .parse()
            .unwrap();
        drop(p);
        tokio::time::sleep(Duration::from_millis(300)).await;
        let state = std::fs::read_to_string(format!("/proc/{pid}/stat")).unwrap_or_default();
        assert!(
            state.is_empty() || state.contains(") Z "),
            "process still alive: {state}"
        );
    }

    #[tokio::test]
    async fn run_tool_reports_failures_and_timeouts() {
        assert!(
            run_tool("false", &[], &[], Duration::from_secs(5))
                .await
                .is_err()
        );
        assert!(
            run_tool("sleep", &["5"], &[], Duration::from_millis(200))
                .await
                .is_err()
        );
        assert_eq!(
            run_tool("echo", &["hi"], &[], Duration::from_secs(5))
                .await
                .unwrap()
                .trim(),
            "hi"
        );
        assert!(
            run_tool(
                "definitely-not-a-binary-xyz",
                &[],
                &[],
                Duration::from_secs(1)
            )
            .await
            .is_err()
        );
    }

    #[test]
    fn log_tail_is_bounded() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("l");
        std::fs::write(&p, "a".repeat(1000) + "END").unwrap();
        assert_eq!(read_log_tail(&p, 3), "END");
        assert_eq!(read_log_tail(&dir.path().join("missing"), 10), "");
    }
}
