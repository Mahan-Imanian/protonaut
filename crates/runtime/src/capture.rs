use std::path::Path;
use std::time::Duration;

use protonaut_core::observe::{FrameStats, WindowSeen, frame_stats};

use crate::RuntimeError;
use crate::supervise::run_tool;

pub async fn screenshot(display: &str, out: &Path) -> Result<FrameStats, RuntimeError> {
    run_tool(
        "import",
        &["-display", display, "-window", "root"],
        &[out.as_os_str()],
        Duration::from_secs(15),
    )
    .await?;
    decode_stats(out)
}

pub fn decode_stats(path: &Path) -> Result<FrameStats, RuntimeError> {
    let file = std::fs::File::open(path).map_err(|e| RuntimeError::io(path, e))?;
    let mut decoder = png::Decoder::new(std::io::BufReader::new(file));
    decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = decoder
        .read_info()
        .map_err(|e| RuntimeError::Image(e.to_string()))?;
    let size = reader
        .output_buffer_size()
        .ok_or_else(|| RuntimeError::Image("image too large".into()))?;
    let mut buf = vec![0; size];
    let info = reader
        .next_frame(&mut buf)
        .map_err(|e| RuntimeError::Image(e.to_string()))?;
    let bytes = &buf[..info.buffer_size()];
    let rgb: Vec<u8> = match info.color_type {
        png::ColorType::Rgb => bytes.to_vec(),
        png::ColorType::Rgba => bytes
            .chunks_exact(4)
            .flat_map(|p| [p[0], p[1], p[2]])
            .collect(),
        png::ColorType::Grayscale => bytes.iter().flat_map(|&g| [g, g, g]).collect(),
        png::ColorType::GrayscaleAlpha => bytes
            .chunks_exact(2)
            .flat_map(|p| [p[0], p[0], p[0]])
            .collect(),
        png::ColorType::Indexed => {
            return Err(RuntimeError::Image("indexed PNG was not expanded".into()));
        }
    };
    frame_stats(info.width, info.height, &rgb)
        .ok_or_else(|| RuntimeError::Image("empty image".into()))
}

/// Reads text from a frame with Tesseract. Upscaling three times in
/// grayscale first is what makes small UI text legible to it.
pub async fn read_text(image: &Path, scratch: &Path) -> Result<String, RuntimeError> {
    let prepared = scratch.join("ocr-input.png");
    run_tool(
        "convert",
        &[],
        &[
            image.as_os_str(),
            "-colorspace".as_ref(),
            "Gray".as_ref(),
            "-resize".as_ref(),
            "300%".as_ref(),
            prepared.as_os_str(),
        ],
        Duration::from_secs(60),
    )
    .await?;
    let text = run_tool(
        "tesseract",
        &[],
        &[
            prepared.as_os_str(),
            "-".as_ref(),
            "--psm".as_ref(),
            "11".as_ref(),
        ],
        Duration::from_secs(120),
    )
    .await;
    let _ = std::fs::remove_file(&prepared);
    text
}

/// Viewable top-level windows. Wine creates many hidden helper windows, so
/// each child of the root window is checked for `IsViewable`.
pub async fn windows(display: &str) -> Result<Vec<WindowSeen>, RuntimeError> {
    let tree = run_tool(
        "xwininfo",
        &["-display", display, "-root", "-children"],
        &[],
        Duration::from_secs(10),
    )
    .await?;
    let mut out = Vec::new();
    for id in parse_child_ids(&tree).into_iter().take(64) {
        let Ok(info) = run_tool(
            "xwininfo",
            &["-display", display, "-id", &id],
            &[],
            Duration::from_secs(10),
        )
        .await
        else {
            continue;
        };
        if let Some(w) = parse_window_info(&info) {
            out.push(w);
        }
    }
    Ok(out)
}

fn parse_child_ids(tree: &str) -> Vec<String> {
    tree.lines()
        .filter_map(|l| l.split_whitespace().next())
        .filter(|tok| {
            tok.starts_with("0x")
                && tok.len() > 2
                && tok[2..].bytes().all(|b| b.is_ascii_hexdigit())
        })
        .map(str::to_owned)
        .collect()
}

fn parse_window_info(info: &str) -> Option<WindowSeen> {
    let mut viewable = false;
    let (mut w, mut h) = (0, 0);
    let mut title = String::new();
    for line in info.lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix("Width:") {
            w = rest.trim().parse().unwrap_or(0);
        } else if let Some(rest) = line.strip_prefix("Height:") {
            h = rest.trim().parse().unwrap_or(0);
        } else if line.starts_with("Map State:") {
            viewable = line.ends_with("IsViewable");
        } else if line.starts_with("xwininfo: Window id:") {
            title = line
                .split_once('"')
                .map(|(_, t)| t.trim_end_matches('"').to_owned())
                .unwrap_or_else(|| "(no name)".into());
        }
    }
    (viewable && w > 1 && h > 1).then_some(WindowSeen {
        title: title.chars().take(80).collect(),
        width: w,
        height: h,
    })
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn child_ids_are_parsed_from_xwininfo_tree() {
        let tree = "\nxwininfo: Window id: 0x3fe (the root window) (has no name)\n\n  Root window id: 0x3fe (the root window) (has no name)\n  Parent window id: 0x0 (none)\n     3 children:\n     0x1a00001 \"d3dtest\": (\"steam_proton\" \"steam_proton\")  1280x800+0+0  +0+0\n     0x1800002 (has no name): ()  1x1+-1+-1  +-1+-1\n";
        assert_eq!(parse_child_ids(tree), vec!["0x1a00001", "0x1800002"]);
    }

    #[test]
    fn only_viewable_real_windows_are_kept() {
        let info = "xwininfo: Window id: 0x1a00001 \"d3dtest\"\n  Width: 1280\n  Height: 800\n  Map State: IsViewable\n";
        assert_eq!(
            parse_window_info(info),
            Some(WindowSeen {
                title: "d3dtest".into(),
                width: 1280,
                height: 800
            })
        );
        assert_eq!(
            parse_window_info(&info.replace("IsViewable", "IsUnMapped")),
            None
        );
        assert_eq!(
            parse_window_info(
                "xwininfo: Window id: 0x2 (has no name)\n Width: 1\n Height: 1\n Map State: IsViewable\n"
            ),
            None
        );
    }

    #[test]
    fn png_is_decoded_to_stats() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("t.png");
        let f = std::fs::File::create(&p).unwrap();
        let mut enc = png::Encoder::new(std::io::BufWriter::new(f), 4, 2);
        enc.set_color(png::ColorType::Rgba);
        enc.set_depth(png::BitDepth::Eight);
        let mut w = enc.write_header().unwrap();
        w.write_image_data(&[26, 153, 51, 255].repeat(8)).unwrap();
        drop(w);
        let s = decode_stats(&p).unwrap();
        assert_eq!((s.width, s.height), (4, 2));
        assert!(s.matches_control());
    }

    /// Needs ImageMagick and Tesseract, which CI installs.
    #[tokio::test]
    async fn ocr_reads_rendered_error_text() {
        let dir = tempfile::tempdir().unwrap();
        let img = dir.path().join("t.png");
        run_tool(
            "convert",
            &[
                "-size",
                "1280x800",
                "xc:black",
                "-font",
                "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
                "-fill",
                "white",
                "-pointsize",
                "28",
                "-annotate",
                "+200+400",
                "Fatal error: required data files are missing",
            ],
            &[img.as_os_str()],
            Duration::from_secs(30),
        )
        .await
        .unwrap();
        let text = read_text(&img, dir.path()).await.unwrap();
        let hits = protonaut_core::observe::error_snippets(&text);
        assert_eq!(hits.len(), 1, "OCR read {text:?}");
        assert!(
            !dir.path().join("ocr-input.png").exists(),
            "scratch image is removed"
        );
    }

    #[test]
    fn corrupt_png_is_an_error_not_a_panic() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("bad.png");
        std::fs::write(&p, b"\x89PNG garbage").unwrap();
        assert!(decode_stats(&p).is_err());
    }
}
