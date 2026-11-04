//! What a runtime adapter observed, and the pure functions that analyse
//! frames and logs. The adapter gathers data; `runtime_checks` judges it.

use crate::model::RunOutcome;

/// Summary statistics of one captured frame (RGB, 8 bits per channel).
#[derive(Debug, Clone, PartialEq)]
pub struct FrameStats {
    pub width: u32,
    pub height: u32,
    pub mean_rgb: [f64; 3],
    /// Mean of per-channel standard deviations.
    pub stddev: f64,
    /// Fraction of pixels whose channels are all below 16.
    pub black_fraction: f64,
    /// Black bars detected on the frame's edges, in pixels.
    pub bars: Bars,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Bars {
    pub top: u32,
    pub bottom: u32,
    pub left: u32,
    pub right: u32,
}

const BLACK: u8 = 16;

/// `rgb` is tightly packed RGB8, row-major.
pub fn frame_stats(width: u32, height: u32, rgb: &[u8]) -> Option<FrameStats> {
    let n = (width as usize).checked_mul(height as usize)?;
    if n == 0 || rgb.len() < n * 3 {
        return None;
    }
    let px = |x: u32, y: u32| {
        let i = (y as usize * width as usize + x as usize) * 3;
        [rgb[i], rgb[i + 1], rgb[i + 2]]
    };
    let mut sum = [0f64; 3];
    let mut sq = [0f64; 3];
    let mut black = 0usize;
    for p in rgb[..n * 3].chunks_exact(3) {
        for c in 0..3 {
            let v = f64::from(p[c]);
            sum[c] += v;
            sq[c] += v * v;
        }
        if p.iter().all(|&v| v < BLACK) {
            black += 1;
        }
    }
    let nf = n as f64;
    let mean = [sum[0] / nf, sum[1] / nf, sum[2] / nf];
    let stddev = (0..3)
        .map(|c| (sq[c] / nf - mean[c] * mean[c]).max(0.0).sqrt())
        .sum::<f64>()
        / 3.0;
    let row_black = |y: u32| (0..width).all(|x| px(x, y).iter().all(|&v| v < BLACK));
    let col_black = |x: u32| (0..height).all(|y| px(x, y).iter().all(|&v| v < BLACK));
    let mut bars = Bars::default();
    if black < n {
        bars.top = (0..height).take_while(|&y| row_black(y)).count() as u32;
        bars.bottom = (0..height).rev().take_while(|&y| row_black(y)).count() as u32;
        bars.left = (0..width).take_while(|&x| col_black(x)).count() as u32;
        bars.right = (0..width).rev().take_while(|&x| col_black(x)).count() as u32;
    }
    Some(FrameStats {
        width,
        height,
        mean_rgb: mean,
        stddev,
        black_fraction: black as f64 / nf,
        bars,
    })
}

impl FrameStats {
    pub fn is_black(&self) -> bool {
        self.black_fraction > 0.995
    }

    /// Nearly a single colour: a cleared but otherwise empty frame.
    pub fn is_uniform(&self) -> bool {
        self.stddev < 2.0
    }

    /// The frame matches the control fixture's clear colour.
    pub fn matches_control(&self) -> bool {
        let expected = CONTROL_RGB.map(f64::from);
        self.is_uniform() && (0..3).all(|c| (self.mean_rgb[c] - expected[c]).abs() <= 6.0)
    }
}

/// The control fixture clears to (0.1, 0.6, 0.2), which sRGB8 stores as
/// (26, 153, 51).
pub const CONTROL_RGB: [u8; 3] = [26, 153, 51];

#[derive(Debug, Clone, PartialEq)]
pub struct WindowSeen {
    pub title: String,
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Screenshot {
    /// Path relative to the results directory.
    pub path: String,
    pub at_secs: f64,
    pub stats: FrameStats,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RunObservation {
    pub runtime: String,
    pub runtime_version: String,
    pub observe_secs: u64,
    pub outcome: RunOutcome,
    pub exit_code: Option<i32>,
    pub duration_ms: u64,
    /// `None` when the control run did not happen.
    pub capture_verified: Option<bool>,
    pub windows: Vec<WindowSeen>,
    pub screenshots: Vec<Screenshot>,
    /// Interesting runtime log lines, already redacted.
    pub log: LogFindings,
    /// Relative path of the saved, redacted runtime log.
    pub log_path: Option<String>,
    pub display: (u32, u32),
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct LogFindings {
    pub crashes: Vec<String>,
    pub missing_dlls: Vec<String>,
    /// The runtime could not open or start the executable at all.
    pub launch_errors: Vec<String>,
}

const MAX_FINDINGS: usize = 20;

/// Scans Wine/Proton stderr for failure signatures. Plain substring matching:
/// a line is reported verbatim (truncated), never interpreted.
pub fn analyse_log(text: &str) -> LogFindings {
    let mut f = LogFindings::default();
    for line in text.lines() {
        let line = line.trim();
        let push = |v: &mut Vec<String>, s: String| {
            if v.len() < MAX_FINDINGS && !v.contains(&s) {
                v.push(s);
            }
        };
        if line.contains("Unhandled exception")
            || (line.starts_with("wine: Unhandled") || line.contains(": Unhandled page fault"))
        {
            push(&mut f.crashes, shorten(line));
        } else if let Some(rest) = line.split("err:module:import_dll Library ").nth(1) {
            let dll = rest.split_whitespace().next().unwrap_or(rest).to_owned();
            push(&mut f.missing_dlls, dll);
        } else if line.starts_with("wine: cannot find")
            || line.starts_with("wine: failed to open")
            || line.contains("could not exec")
        {
            push(&mut f.launch_errors, shorten(line));
        } else if line.contains("err:module:LdrInitializeThunk") || line.contains("could not load")
        {
            push(&mut f.crashes, shorten(line));
        }
    }
    f
}

fn shorten(s: &str) -> String {
    let t: String = s.chars().take(240).collect();
    t
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn solid(w: u32, h: u32, c: [u8; 3]) -> Vec<u8> {
        (0..w * h).flat_map(|_| c).collect()
    }

    #[test]
    fn control_colour_is_recognised() {
        let s = frame_stats(64, 40, &solid(64, 40, CONTROL_RGB)).unwrap();
        assert!(s.matches_control());
        assert!(!s.is_black());
        let off = frame_stats(64, 40, &solid(64, 40, [26, 100, 51])).unwrap();
        assert!(!off.matches_control());
    }

    #[test]
    fn black_frame_is_black_and_has_no_bars() {
        let s = frame_stats(32, 20, &solid(32, 20, [0, 0, 0])).unwrap();
        assert!(s.is_black() && s.is_uniform());
        assert_eq!(s.bars, Bars::default());
    }

    #[test]
    fn letterbox_bars_are_measured() {
        let (w, h) = (40u32, 50u32);
        let mut img = solid(w, h, [0, 0, 0]);
        for y in 5..45 {
            for x in 0..w {
                let i = ((y * w + x) * 3) as usize;
                img[i..i + 3].copy_from_slice(&[200, 50, 50]);
            }
        }
        let s = frame_stats(w, h, &img).unwrap();
        assert_eq!(
            (s.bars.top, s.bars.bottom, s.bars.left, s.bars.right),
            (5, 5, 0, 0)
        );
        assert!(!s.is_black());
    }

    #[test]
    fn short_buffers_are_rejected() {
        assert!(frame_stats(10, 10, &[0; 10]).is_none());
        assert!(frame_stats(0, 10, &[]).is_none());
    }

    #[test]
    fn log_signatures_are_extracted() {
        let log = "\
0024:err:module:import_dll Library fx_absent.dll (which is needed by L\"C:\\\\game\\\\a.exe\") not found
wine: Unhandled division by zero at address 00006FFFFA3EEF5A (thread 0024), starting debugger...
noise line";
        let f = analyse_log(log);
        assert_eq!(f.missing_dlls, vec!["fx_absent.dll"]);
        assert_eq!(f.crashes.len(), 1);
        assert!(f.crashes[0].contains("division by zero"));
    }

    #[test]
    fn launch_errors_are_separated_from_crashes() {
        let f = analyse_log("wine: cannot find L\"C:\\\\x.exe\"\n");
        assert_eq!(f.launch_errors.len(), 1);
        assert!(f.crashes.is_empty());
    }

    #[test]
    fn findings_are_capped_and_deduplicated() {
        let log = "wine: Unhandled page fault\n".repeat(50);
        assert_eq!(analyse_log(&log).crashes.len(), 1);
        let many: String = (0..50)
            .map(|i| format!("err:module:import_dll Library d{i}.dll not found\n"))
            .collect();
        assert_eq!(analyse_log(&many).missing_dlls.len(), MAX_FINDINGS);
    }
}
