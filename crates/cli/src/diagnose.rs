use std::path::Path;

use protonaut_runtime::display::{SYSTEM_CONFIG_DIR, SYSTEM_CONFIG_NAME};
use protonaut_runtime::install::{Cache, Manifest, default_cache_dir};

use crate::exit;

const APT: &str = "sudo apt-get install --no-install-recommends xserver-xorg-core xserver-xorg-video-dummy x11-xserver-utils x11-utils imagemagick mesa-vulkan-drivers python3 curl tesseract-ocr tesseract-ocr-eng";
const APT_I386: &str = "sudo dpkg --add-architecture i386 && sudo apt-get update && sudo apt-get install --no-install-recommends libc6:i386 libx11-6:i386 libxext6:i386 libxrandr2:i386 libxi6:i386 libxcursor1:i386 libxinerama1:i386 libxcomposite1:i386 libxrender1:i386 libxfixes3:i386 libfreetype6:i386 libfontconfig1:i386 libvulkan1:i386 mesa-vulkan-drivers:i386 libgl1:i386 libgl1-mesa-dri:i386";
const MIN_CACHE_BYTES: u64 = 4 * 1024 * 1024 * 1024;
const MIN_TEMP_BYTES: u64 = 2 * 1024 * 1024 * 1024;

struct Item {
    name: &'static str,
    ok: bool,
    required: bool,
    detail: String,
}

fn exists(path: &str) -> bool {
    Path::new(path).exists()
}

fn on_path(tool: &str) -> bool {
    ["/usr/local/bin", "/usr/bin", "/bin"]
        .iter()
        .any(|d| Path::new(d).join(tool).is_file())
}

fn free_bytes(path: &Path) -> Option<u64> {
    let mut p = path.to_path_buf();
    while !p.exists() {
        p = p.parent()?.to_path_buf();
    }
    let s = nix::sys::statvfs::statvfs(&p).ok()?;
    Some(s.blocks_available() * s.fragment_size())
}

pub async fn run(json: bool) -> u8 {
    let mut items = Vec::new();
    let mut tool = |name: &'static str, ok: bool, fix: &str| {
        items.push(Item {
            name,
            ok,
            required: true,
            detail: if ok {
                "found".into()
            } else {
                format!("missing: {fix}")
            },
        });
    };
    tool(
        "python3 (runs Proton's launcher script)",
        exists("/usr/bin/python3"),
        APT,
    );
    tool("Xorg server", exists("/usr/lib/xorg/Xorg"), APT);
    tool(
        "Xorg dummy video driver",
        exists("/usr/lib/xorg/modules/drivers/dummy_drv.so"),
        APT,
    );
    tool("xrandr", on_path("xrandr"), APT);
    tool("xwininfo", on_path("xwininfo"), APT);
    tool("import (ImageMagick)", on_path("import"), APT);
    tool("curl", on_path("curl"), APT);
    tool("tar", on_path("tar"), APT);
    tool(
        "Mesa lavapipe Vulkan driver",
        protonaut_runtime::proton::lavapipe_icd().is_some(),
        APT,
    );

    items.push(Item {
        name: "tesseract (OCR for PN-DISP-005)",
        ok: on_path("tesseract"),
        required: false,
        detail: if on_path("tesseract") {
            "found".into()
        } else {
            "missing: PN-DISP-005 will be NOT_TESTED; install tesseract-ocr tesseract-ocr-eng"
                .into()
        },
    });
    let i386 = [
        "/usr/lib/i386-linux-gnu/libX11.so.6",
        "/usr/lib/i386-linux-gnu/libfreetype.so.6",
        "/usr/lib/i386-linux-gnu/libvulkan.so.1",
        "/usr/lib/i386-linux-gnu/libvulkan_lvp.so",
    ]
    .iter()
    .all(|p| exists(p));
    items.push(Item {
        name: "32-bit libraries (for 32-bit games)",
        ok: i386,
        required: false,
        detail: if i386 {
            "found".into()
        } else {
            format!("missing: 32-bit games will fail to start; {APT_I386}")
        },
    });
    let root = nix::unistd::geteuid().is_root();
    let conf = Path::new(SYSTEM_CONFIG_DIR).join(SYSTEM_CONFIG_NAME);
    items.push(Item {
        name: "Xorg dummy configuration",
        ok: root || conf.is_file(),
        required: true,
        detail: if root {
            "running as root: a temporary config is used".into()
        } else if conf.is_file() {
            format!("{} found", conf.display())
        } else {
            format!("{} missing: run `sudo protonaut setup`", conf.display())
        },
    });
    items.push(Item {
        name: "Not running as root",
        ok: !root,
        required: false,
        detail: if root {
            "the build under test would run as root; use an unprivileged user".into()
        } else {
            "ok".into()
        },
    });

    let cache_dir = default_cache_dir();
    match &cache_dir {
        Some(dir) => {
            let free = free_bytes(dir).unwrap_or(0);
            items.push(Item {
                name: "Disk space for runtimes",
                ok: free >= MIN_CACHE_BYTES,
                required: false,
                detail: format!(
                    "{:.1} GiB free at {} (each runtime needs about 2 GiB)",
                    free as f64 / 1073741824.0,
                    dir.display()
                ),
            });
            if let Ok(m) = Manifest::builtin() {
                let installed: Vec<String> = Cache::new(dir.clone())
                    .list(&m)
                    .into_iter()
                    .filter(|(_, i)| *i)
                    .map(|(s, _)| s.id)
                    .collect();
                items.push(Item {
                    name: "Installed runtimes",
                    ok: !installed.is_empty(),
                    required: false,
                    detail: if installed.is_empty() {
                        "none: run `protonaut proton install stable`".into()
                    } else {
                        installed.join(", ")
                    },
                });
            }
        }
        None => items.push(Item {
            name: "Runtime cache",
            ok: false,
            required: true,
            detail: "no HOME and no PROTONAUT_CACHE".into(),
        }),
    }
    let tmp = std::env::temp_dir();
    let free = free_bytes(&tmp).unwrap_or(0);
    items.push(Item {
        name: "Disk space for Wine prefixes",
        ok: free >= MIN_TEMP_BYTES,
        required: false,
        detail: format!(
            "{:.1} GiB free at {}",
            free as f64 / 1073741824.0,
            tmp.display()
        ),
    });

    let ready = items.iter().all(|i| i.ok || !i.required);
    if json {
        let v: Vec<_> = items.iter().map(|i| serde_json::json!({"check": i.name, "ok": i.ok, "required": i.required, "detail": i.detail})).collect();
        println!("{}", serde_json::json!({"ready": ready, "checks": v}));
    } else {
        for i in &items {
            let mark = match (i.ok, i.required) {
                (true, _) => "ok  ",
                (false, true) => "FAIL",
                (false, false) => "warn",
            };
            println!("{mark}  {:<40} {}", i.name, i.detail);
        }
        println!(
            "\n{}",
            if ready {
                "Ready to run `protonaut test`."
            } else {
                "Not ready: fix the FAIL lines above."
            }
        );
    }
    if ready { exit::PASS } else { exit::ENVIRONMENT }
}
