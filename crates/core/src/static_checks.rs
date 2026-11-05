//! Checks that only read the build, before Proton starts: executable format,
//! DLL imports nothing provides, paths that differ only by case, anti-cheat,
//! launchers, Media Foundation video and the Steamworks APIs.

use std::collections::BTreeSet;

use crate::inspect::{BuildTree, PeArch, PeError, PeInfo};
use crate::model::{Category, CheckResult, Evidence, Status};

/// What the static pass knows about the runtime it will be paired with.
#[derive(Debug, Clone, Default)]
pub struct StaticContext {
    /// Lowercased names of DLLs the runtime provides as builtins, for example
    /// every file in GE-Proton's `files/lib/wine/x86_64-windows`. Empty when
    /// no runtime is installed, which makes the import check UNKNOWN.
    pub runtime_dlls: BTreeSet<String>,
}

pub struct StaticInput<'a> {
    pub tree: &'a BuildTree,
    /// Executable path relative to the build directory, as found in `tree`.
    pub exe: &'a str,
    pub pe: &'a Result<PeInfo, PeError>,
    pub ctx: &'a StaticContext,
}

pub fn run_static_checks(input: &StaticInput<'_>) -> Vec<CheckResult> {
    let mut out = vec![check_pe(input)];
    if let Ok(pe) = input.pe {
        out.push(check_arch(pe));
        out.push(check_imports(input, pe));
        out.push(check_steam_input(pe));
        out.push(check_text_input(pe));
    }
    out.push(check_case_collisions(input.tree));
    out.push(check_anti_cheat(input.tree));
    out.push(check_launcher(input));
    out.push(check_media(input.tree));
    out
}

fn static_result(id: &str, title: &str, status: Status, summary: impl Into<String>) -> CheckResult {
    CheckResult::new(
        id,
        title,
        Category::Static,
        status,
        Evidence::Static,
        summary,
    )
}

fn check_pe(input: &StaticInput<'_>) -> CheckResult {
    match input.pe {
        Ok(_) => static_result(
            "PN-PE-001",
            "Executable is a Windows PE file",
            Status::Pass,
            format!("{} parsed as a PE executable.", input.exe),
        ),
        Err(e) => static_result(
            "PN-PE-001",
            "Executable is a Windows PE file",
            Status::Fail,
            format!("{}: {e}.", input.exe),
        ),
    }
}

fn check_arch(pe: &PeInfo) -> CheckResult {
    let title = "Executable architecture runs under Proton";
    match pe.arch {
        PeArch::X64 => static_result("PN-PE-002", title, Status::Pass, "x86-64 executable."),
        PeArch::X86 => static_result(
            "PN-PE-002",
            title,
            Status::Pass,
            "32-bit x86 executable; Proton runs it through WoW64.",
        ),
        PeArch::Arm64 => static_result(
            "PN-PE-002",
            title,
            Status::Fail,
            "ARM64 executable; SteamOS devices are x86-64 and Proton does not run ARM64 Windows binaries.",
        ),
        PeArch::Other => static_result(
            "PN-PE-002",
            title,
            Status::Fail,
            "Unrecognised machine type.",
        ),
    }
}

/// Every imported DLL must be shipped next to the executable or provided by
/// the runtime. Unresolved imports make Windows' loader refuse to start.
fn check_imports(input: &StaticInput<'_>, pe: &PeInfo) -> CheckResult {
    let title = "Imported DLLs are available";
    let exe_dir = input.exe.rsplit_once('/').map(|(d, _)| d).unwrap_or("");
    let shipped: BTreeSet<String> = input.tree.names_in_dir(exe_dir).into_iter().collect();
    let mut missing = Vec::new();
    let mut unknown = Vec::new();
    for dll in pe.imports.keys() {
        if shipped.contains(dll) {
            continue;
        }
        if input.ctx.runtime_dlls.is_empty() {
            unknown.push(dll.clone());
        } else if !input.ctx.runtime_dlls.contains(dll) && !is_api_set(dll) {
            missing.push(dll.clone());
        }
    }
    if !missing.is_empty() {
        static_result("PN-PE-003", title, Status::Fail, format!("{} imported DLL(s) are neither shipped next to the executable nor provided by the runtime.", missing.len()))
            .with_details(missing)
    } else if !unknown.is_empty() {
        static_result("PN-PE-003", title, Status::Unknown, "No runtime installed to compare against; DLLs not shipped with the build could not be checked.")
            .with_details(unknown)
    } else {
        static_result(
            "PN-PE-003",
            title,
            Status::Pass,
            format!(
                "All {} imported DLLs are shipped or provided by the runtime.",
                pe.imports.len()
            ),
        )
    }
}

/// `api-ms-win-*` and `ext-ms-*` names are API sets the loader redirects.
fn is_api_set(dll: &str) -> bool {
    dll.starts_with("api-ms-win-") || dll.starts_with("ext-ms-")
}

fn steam_imports(pe: &PeInfo) -> Vec<&str> {
    pe.imported_functions("steam_api").collect()
}

/// Only the Steamworks flat API shows up as named imports. Games that call
/// Steam Input through C++ interface pointers import only the accessor, so a
/// missing import is UNKNOWN, never FAIL.
fn check_steam_input(pe: &PeInfo) -> CheckResult {
    let title = "Steam Input API is used";
    let imports = steam_imports(pe);
    if imports.is_empty() {
        return static_result(
            "PN-STEAM-001",
            title,
            Status::Unknown,
            "The executable does not import steam_api directly; Steam Input use could not be determined (an engine plugin or another module may load it).",
        );
    }
    let hits: Vec<String> = imports
        .iter()
        .filter(|n| n.contains("SteamInput"))
        .map(|s| s.to_string())
        .collect();
    if hits.is_empty() {
        static_result(
            "PN-STEAM-001",
            title,
            Status::Unknown,
            "steam_api is imported but no Steam Input function is imported by name; C++ interface calls are invisible to this check.",
        )
    } else {
        static_result("PN-STEAM-001", title, Status::Pass, "The executable imports Steam Input functions; Valve recommends Steam Input for correct controller glyphs.")
            .with_details(hits)
    }
}

fn check_text_input(pe: &PeInfo) -> CheckResult {
    let title = "On-screen keyboard API is used";
    let imports = steam_imports(pe);
    let hits: Vec<String> = imports
        .iter()
        .filter(|n| {
            n.contains("ShowGamepadTextInput") || n.contains("ShowFloatingGamepadTextInput")
        })
        .map(|s| s.to_string())
        .collect();
    if hits.is_empty() {
        static_result(
            "PN-STEAM-002",
            title,
            Status::Unknown,
            "No Steamworks text-input function is imported by name. If the game asks for text, it must open the on-screen keyboard or offer controller-only entry.",
        )
    } else {
        static_result(
            "PN-STEAM-002",
            title,
            Status::Pass,
            "The executable imports a Steamworks text-input function.",
        )
        .with_details(hits)
    }
}

fn check_case_collisions(tree: &BuildTree) -> CheckResult {
    let title = "No files differ only by letter case";
    let groups = tree.case_collisions();
    if groups.is_empty() {
        static_result(
            "PN-FS-001",
            title,
            Status::Pass,
            format!(
                "{} file{} checked.",
                tree.files.len(),
                if tree.files.len() == 1 { "" } else { "s" }
            ),
        )
    } else {
        let details = groups.iter().map(|g| g.join("  <->  ")).collect();
        static_result("PN-FS-001", title, Status::Fail, format!("{} group(s) of paths differ only by case; Windows and Wine resolve names case-insensitively, so only one of each group is reachable.", groups.len()))
            .with_details(details)
    }
}

/// Marker files of anti-cheat systems, matched case-insensitively against
/// path components.
const ANTI_CHEAT: &[(&str, &[&str])] = &[
    (
        "Easy Anti-Cheat",
        &[
            "easyanticheat",
            "easyanticheat_x64.dll",
            "easyanticheat_eos.sys",
            "start_protected_game.exe",
        ],
    ),
    (
        "BattlEye",
        &[
            "battleye",
            "beservice.exe",
            "beservice_x64.exe",
            "beclient_x64.dll",
        ],
    ),
    ("nProtect GameGuard", &["gameguard", "gamemon.des"]),
    ("XIGNCODE3", &["xigncode", "x3.xem"]),
    ("PunkBuster", &["pbsvc.exe", "pnkbstra.exe"]),
    ("Riot Vanguard", &["vgk.sys"]),
    ("mhyprot", &["mhyprot2.sys", "mhyprot3.sys"]),
    ("ACE (Anti-Cheat Expert)", &["ace-base", "sguard64.exe"]),
];

fn check_anti_cheat(tree: &BuildTree) -> CheckResult {
    let title = "Anti-cheat compatibility";
    let mut found = Vec::new();
    for (name, markers) in ANTI_CHEAT {
        let hit = tree.files.iter().find(|f| {
            f.to_lowercase()
                .split('/')
                .any(|part| markers.contains(&part))
        });
        if let Some(path) = hit {
            found.push(format!("{name}: {path}"));
        }
    }
    if found.is_empty() {
        static_result(
            "PN-AC-001",
            title,
            Status::Pass,
            "No known anti-cheat files found.",
        )
    } else {
        static_result("PN-AC-001", title, Status::Warning, "Anti-cheat files found. Easy Anti-Cheat and BattlEye run under Proton only when the developer enables Proton support with the vendor; most others do not run on Linux. This cannot be verified here.")
            .with_details(found)
    }
}

/// Heuristic: launchers must be fully controller-navigable, and Valve
/// recommends not requiring one.
fn check_launcher(input: &StaticInput<'_>) -> CheckResult {
    let title = "No separate launcher";
    let exe_name = input
        .exe
        .rsplit('/')
        .next()
        .unwrap_or(input.exe)
        .to_lowercase();
    let launchers: Vec<String> = input
        .tree
        .files
        .iter()
        .filter(|f| {
            let name = f.rsplit('/').next().unwrap_or(f).to_lowercase();
            name.ends_with(".exe") && name.contains("launcher")
        })
        .cloned()
        .collect();
    let mut r = if exe_name.contains("launcher") {
        static_result(
            "PN-LAUNCH-001",
            title,
            Status::Warning,
            "The configured executable looks like a launcher. Valve requires launchers to be fully controller-navigable and recommends not requiring one.",
        )
    } else if !launchers.is_empty() {
        static_result("PN-LAUNCH-001", title, Status::Warning, "The build contains executables named like launchers. If one runs before the game, it must be fully controller-navigable.")
            .with_details(launchers)
    } else {
        static_result(
            "PN-LAUNCH-001",
            title,
            Status::Pass,
            "No launcher-like executables found.",
        )
    };
    r.evidence = Evidence::Heuristic;
    r
}

/// Video formats commonly decoded through Windows Media Foundation or
/// DirectShow, whose Proton support is partial.
const SYSTEM_DECODED_VIDEO: &[&str] = &["wmv", "asf", "mp4", "m4v", "mov", "avi", "mkv"];
/// Formats normally decoded inside the game by middleware.
const IN_PROCESS_VIDEO: &[&str] = &["bik", "bk2", "usm", "webm", "ogv", "ivf"];

fn check_media(tree: &BuildTree) -> CheckResult {
    let title = "Video formats";
    let ext = |f: &str| {
        f.rsplit_once('.')
            .map(|(_, e)| e.to_lowercase())
            .unwrap_or_default()
    };
    let system: Vec<String> = tree
        .files
        .iter()
        .filter(|f| SYSTEM_DECODED_VIDEO.contains(&ext(f).as_str()))
        .cloned()
        .collect();
    let in_process = tree
        .files
        .iter()
        .filter(|f| IN_PROCESS_VIDEO.contains(&ext(f).as_str()))
        .count();
    let mut r = if !system.is_empty() {
        let mut details = system.clone();
        details.truncate(20);
        static_result("PN-MEDIA-001", title, Status::Warning, format!("{} video file(s) in formats often decoded through Windows Media Foundation, which Proton supports only partially. Confirm cutscenes play to the end in a runtime test.", system.len()))
            .with_details(details)
    } else if in_process > 0 {
        static_result(
            "PN-MEDIA-001",
            title,
            Status::Pass,
            format!("{in_process} video file(s) in formats usually decoded by in-game middleware."),
        )
    } else {
        static_result("PN-MEDIA-001", title, Status::Pass, "No video files found.")
    };
    r.evidence = Evidence::Heuristic;
    r
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;

    fn tree(files: &[&str]) -> BuildTree {
        BuildTree {
            files: files.iter().map(|s| s.to_string()).collect(),
            symlinks: vec![],
        }
    }

    fn pe(arch: PeArch, imports: &[(&str, &[&str])]) -> Result<PeInfo, PeError> {
        let imports: BTreeMap<String, Vec<String>> = imports
            .iter()
            .map(|(l, f)| (l.to_string(), f.iter().map(|s| s.to_string()).collect()))
            .collect();
        Ok(PeInfo { arch, imports })
    }

    fn run(
        tree: &BuildTree,
        pe: &Result<PeInfo, PeError>,
        ctx: &StaticContext,
    ) -> Vec<CheckResult> {
        run_static_checks(&StaticInput {
            tree,
            exe: "Game.exe",
            pe,
            ctx,
        })
    }

    fn get<'a>(results: &'a [CheckResult], id: &str) -> &'a CheckResult {
        results.iter().find(|r| r.id == id).unwrap()
    }

    fn ctx(dlls: &[&str]) -> StaticContext {
        StaticContext {
            runtime_dlls: dlls.iter().map(|s| s.to_string()).collect(),
        }
    }

    #[test]
    fn clean_build_passes_and_static_results_have_no_runtime() {
        let t = tree(&["Game.exe", "engine.dll"]);
        let p = pe(
            PeArch::X64,
            &[
                ("kernel32.dll", &["CreateFileW"]),
                ("engine.dll", &["init"]),
            ],
        );
        let r = run(&t, &p, &ctx(&["kernel32.dll"]));
        for id in [
            "PN-PE-001",
            "PN-PE-002",
            "PN-PE-003",
            "PN-FS-001",
            "PN-AC-001",
            "PN-LAUNCH-001",
            "PN-MEDIA-001",
        ] {
            assert_eq!(get(&r, id).status, Status::Pass, "{id}");
        }
        assert!(
            r.iter()
                .all(|c| c.runtime.is_none() && c.category == Category::Static)
        );
    }

    #[test]
    fn missing_dll_fails_and_names_it() {
        let r = run(
            &tree(&["Game.exe"]),
            &pe(PeArch::X64, &[("fx_absent.dll", &["f"])]),
            &ctx(&["kernel32.dll"]),
        );
        let c = get(&r, "PN-PE-003");
        assert_eq!(c.status, Status::Fail);
        assert_eq!(c.details, vec!["fx_absent.dll"]);
    }

    #[test]
    fn without_runtime_list_imports_are_unknown_not_pass() {
        let r = run(
            &tree(&["Game.exe"]),
            &pe(PeArch::X64, &[("d3d11.dll", &["D3D11CreateDevice"])]),
            &StaticContext::default(),
        );
        assert_eq!(get(&r, "PN-PE-003").status, Status::Unknown);
    }

    #[test]
    fn api_sets_are_not_reported_missing() {
        let r = run(
            &tree(&["Game.exe"]),
            &pe(
                PeArch::X64,
                &[("api-ms-win-crt-runtime-l1-1-0.dll", &["x"])],
            ),
            &ctx(&["kernel32.dll"]),
        );
        assert_eq!(get(&r, "PN-PE-003").status, Status::Pass);
    }

    #[test]
    fn dll_in_another_directory_does_not_count_as_shipped() {
        let t = tree(&["Game.exe", "plugins/engine.dll"]);
        let r = run(
            &t,
            &pe(PeArch::X64, &[("engine.dll", &["x"])]),
            &ctx(&["kernel32.dll"]),
        );
        assert_eq!(get(&r, "PN-PE-003").status, Status::Fail);
    }

    #[test]
    fn arm64_fails_x86_passes() {
        let t = tree(&["Game.exe"]);
        assert_eq!(
            get(&run(&t, &pe(PeArch::Arm64, &[]), &ctx(&[])), "PN-PE-002").status,
            Status::Fail
        );
        assert_eq!(
            get(&run(&t, &pe(PeArch::X86, &[]), &ctx(&[])), "PN-PE-002").status,
            Status::Pass
        );
    }

    #[test]
    fn non_pe_fails_and_skips_pe_rules() {
        let r = run(
            &tree(&["Game.exe"]),
            &Err(PeError::NotPe("Elf".into())),
            &ctx(&[]),
        );
        assert_eq!(get(&r, "PN-PE-001").status, Status::Fail);
        assert!(r.iter().all(|c| c.id != "PN-PE-002" && c.id != "PN-PE-003"));
    }

    #[test]
    fn anti_cheat_is_a_warning_never_a_pass_claim() {
        let r = run(
            &tree(&["Game.exe", "EasyAntiCheat/EasyAntiCheat_EOS_Setup.exe"]),
            &pe(PeArch::X64, &[]),
            &ctx(&[]),
        );
        let c = get(&r, "PN-AC-001");
        assert_eq!(c.status, Status::Warning);
        assert!(c.details[0].starts_with("Easy Anti-Cheat"));
    }

    #[test]
    fn substring_of_a_marker_is_not_a_match() {
        let r = run(
            &tree(&["Game.exe", "docs/battleye-notes.txt"]),
            &pe(PeArch::X64, &[]),
            &ctx(&[]),
        );
        assert_eq!(get(&r, "PN-AC-001").status, Status::Pass);
    }

    #[test]
    fn case_collision_fails() {
        let r = run(
            &tree(&["Game.exe", "Data/A.pak", "data/a.pak"]),
            &pe(PeArch::X64, &[]),
            &ctx(&[]),
        );
        assert_eq!(get(&r, "PN-FS-001").status, Status::Fail);
    }

    #[test]
    fn launcher_and_media_are_heuristic_warnings() {
        let r = run(
            &tree(&["Game.exe", "GameLauncher.exe", "movies/intro.wmv"]),
            &pe(PeArch::X64, &[]),
            &ctx(&[]),
        );
        for id in ["PN-LAUNCH-001", "PN-MEDIA-001"] {
            assert_eq!(get(&r, id).status, Status::Warning, "{id}");
            assert_eq!(get(&r, id).evidence, Evidence::Heuristic, "{id}");
        }
    }

    #[test]
    fn bink_video_passes() {
        let r = run(
            &tree(&["Game.exe", "movies/intro.bk2"]),
            &pe(PeArch::X64, &[]),
            &ctx(&[]),
        );
        assert_eq!(get(&r, "PN-MEDIA-001").status, Status::Pass);
    }

    #[test]
    fn steam_input_detection_is_unknown_unless_seen() {
        let none = run(&tree(&["Game.exe"]), &pe(PeArch::X64, &[]), &ctx(&[]));
        assert_eq!(get(&none, "PN-STEAM-001").status, Status::Unknown);
        let iface = run(
            &tree(&["Game.exe"]),
            &pe(
                PeArch::X64,
                &[(
                    "steam_api64.dll",
                    &["SteamInternal_FindOrCreateUserInterface"],
                )],
            ),
            &ctx(&[]),
        );
        assert_eq!(get(&iface, "PN-STEAM-001").status, Status::Unknown);
        let flat = run(
            &tree(&["Game.exe"]),
            &pe(
                PeArch::X64,
                &[(
                    "steam_api64.dll",
                    &[
                        "SteamAPI_SteamInput_v006",
                        "SteamAPI_ISteamUtils_ShowFloatingGamepadTextInput",
                    ],
                )],
            ),
            &ctx(&[]),
        );
        assert_eq!(get(&flat, "PN-STEAM-001").status, Status::Pass);
        assert_eq!(get(&flat, "PN-STEAM-002").status, Status::Pass);
    }
}
