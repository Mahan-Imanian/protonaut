//! CLI integration tests. They run the real binary but never Proton, so they
//! work on any Linux machine (the runtime tests are in scripts/e2e.py).
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn bin() -> Command {
    let mut c = Command::new(env!("CARGO_BIN_EXE_steamos-preflight"));
    c.env_remove("STEAMOS_PREFLIGHT_CONFIG");
    c
}

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/bin")
        .join(name)
}

/// A build directory containing one fixture as Game.exe, plus extra files.
fn build_with(exe: &str, extra: &[&str]) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    std::fs::copy(fixture(exe), dir.path().join("Game.exe")).unwrap();
    for rel in extra {
        let p = dir.path().join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, b"x").unwrap();
    }
    dir
}

fn inspect(build: &Path, cache: &Path) -> (Output, serde_json::Value) {
    inspect_into(build, cache, tempfile::tempdir().unwrap().path())
}

fn inspect_into(build: &Path, cache: &Path, results: &Path) -> (Output, serde_json::Value) {
    let out = bin()
        .args([
            "inspect",
            build.to_str().unwrap(),
            "--exe",
            "game.EXE",
            "--json",
            "--cache",
            cache.to_str().unwrap(),
        ])
        .arg("--results")
        .arg(results)
        .output()
        .unwrap();
    let report = serde_json::from_slice(&out.stdout).unwrap_or(serde_json::Value::Null);
    (out, report)
}

fn status(report: &serde_json::Value, id: &str) -> String {
    report["checks"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["id"] == id)
        .map(|c| c["status"].as_str().unwrap().to_owned())
        .unwrap_or_default()
}

#[test]
fn inspect_clean_fixture_passes_with_exit_0() {
    let build = build_with("fx_scene.exe", &[]);
    let cache = tempfile::tempdir().unwrap();
    let (out, r) = inspect(build.path(), cache.path());
    assert_eq!(
        out.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(r["schema_version"], 1);
    assert_eq!(status(&r, "SP-PE-001"), "PASS");
    assert_eq!(status(&r, "SP-PE-002"), "PASS");
    // No runtime installed in this empty cache: imports cannot be judged.
    assert_eq!(status(&r, "SP-PE-003"), "UNKNOWN");
    assert_eq!(
        r["target"]["exe"], "Game.exe",
        "case-insensitive lookup returns the real name"
    );
}

#[test]
fn inspect_flags_anti_cheat_and_case_collisions() {
    let build = build_with(
        "fx_scene.exe",
        &[
            "EasyAntiCheat/EasyAntiCheat_x64.dll",
            "Data/Level.pak",
            "data/level.pak",
        ],
    );
    let cache = tempfile::tempdir().unwrap();
    let (out, r) = inspect(build.path(), cache.path());
    assert_eq!(out.status.code(), Some(1), "case collision is a FAIL");
    assert_eq!(status(&r, "SP-AC-001"), "WARNING");
    assert_eq!(status(&r, "SP-FS-001"), "FAIL");
}

#[test]
fn inspect_rejects_non_pe_executable() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("Game.exe"), b"#!/bin/sh\necho hi\n").unwrap();
    let cache = tempfile::tempdir().unwrap();
    let (out, r) = inspect(dir.path(), cache.path());
    assert_eq!(out.status.code(), Some(1));
    assert_eq!(status(&r, "SP-PE-001"), "FAIL");
}

#[test]
fn missing_exe_and_bad_config_are_usage_errors() {
    let build = build_with("fx_scene.exe", &[]);
    let out = bin()
        .args([
            "inspect",
            build.path().to_str().unwrap(),
            "--exe",
            "Nope.exe",
        ])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&out.stderr).contains("not found"));

    let out = bin()
        .args([
            "inspect",
            build.path().to_str().unwrap(),
            "--exe",
            "../escape.exe",
        ])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&out.stderr).contains(".."));

    let cfg = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(cfg.path(), "[run]\nobserve_sec = 5\n").unwrap();
    let out = bin()
        .args(["inspect", "--config", cfg.path().to_str().unwrap()])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2), "typo in a key is rejected");
}

#[test]
fn test_without_installed_runtime_is_an_environment_error() {
    let build = build_with("fx_scene.exe", &[]);
    let cache = tempfile::tempdir().unwrap();
    let results = tempfile::tempdir().unwrap();
    let out = bin()
        .args([
            "test",
            build.path().to_str().unwrap(),
            "--exe",
            "Game.exe",
            "--cache",
            cache.path().to_str().unwrap(),
        ])
        .arg("--results")
        .arg(results.path())
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(3));
    assert!(String::from_utf8_lossy(&out.stderr).contains("proton install"));
}

#[test]
fn unknown_runtime_is_a_usage_error() {
    let build = build_with("fx_scene.exe", &[]);
    let out = bin()
        .args([
            "test",
            build.path().to_str().unwrap(),
            "--exe",
            "Game.exe",
            "--runtime",
            "proton-9000",
        ])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
}

#[test]
fn report_rerenders_and_rejects_tampered_or_future_reports() {
    let build = build_with("fx_scene.exe", &["EasyAntiCheat/EasyAntiCheat_x64.dll"]);
    let cache = tempfile::tempdir().unwrap();
    let results = tempfile::tempdir().unwrap();
    bin()
        .args([
            "inspect",
            build.path().to_str().unwrap(),
            "--exe",
            "Game.exe",
            "--cache",
            cache.path().to_str().unwrap(),
        ])
        .arg("--results")
        .arg(results.path())
        .output()
        .unwrap();
    let json = results.path().join("report.json");
    for format in ["markdown", "html", "junit", "json"] {
        let out = bin()
            .args(["report", json.to_str().unwrap(), "--format", format])
            .output()
            .unwrap();
        assert_eq!(out.status.code(), Some(0), "{format}");
        assert!(!out.stdout.is_empty());
    }
    let md = bin()
        .args(["report", json.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(String::from_utf8_lossy(&md.stdout).contains("Anti-cheat"));

    let mut v: serde_json::Value = serde_json::from_slice(&std::fs::read(&json).unwrap()).unwrap();
    v["schema_version"] = 2.into();
    std::fs::write(&json, v.to_string()).unwrap();
    assert_eq!(
        bin()
            .args(["report", json.to_str().unwrap()])
            .output()
            .unwrap()
            .status
            .code(),
        Some(2)
    );

    v["schema_version"] = 1.into();
    v["checks"][0]["status"] = "TOTALLY_FINE".into();
    std::fs::write(&json, v.to_string()).unwrap();
    assert_eq!(
        bin()
            .args(["report", json.to_str().unwrap()])
            .output()
            .unwrap()
            .status
            .code(),
        Some(2)
    );
}

#[test]
fn init_writes_a_valid_starter_config_once() {
    let dir = tempfile::tempdir().unwrap();
    let run = || bin().arg("init").current_dir(dir.path()).output().unwrap();
    assert_eq!(run().status.code(), Some(0));
    assert_eq!(run().status.code(), Some(2), "refuses to overwrite");
    let text = std::fs::read_to_string(dir.path().join("preflight.toml")).unwrap();
    assert!(text.contains("schema_version = 1"));
}

#[test]
fn proton_list_and_schema_and_version_work() {
    let cache = tempfile::tempdir().unwrap();
    let out = bin()
        .args([
            "proton",
            "list",
            "--json",
            "--cache",
            cache.path().to_str().unwrap(),
        ])
        .output()
        .unwrap();
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert!(
        v.as_array()
            .unwrap()
            .iter()
            .any(|r| r["stable"] == true && r["installed"] == false)
    );
    let out = bin().arg("schema").output().unwrap();
    assert!(String::from_utf8_lossy(&out.stdout).contains("report.v1.schema.json"));
    let out = bin().arg("--version").output().unwrap();
    assert!(String::from_utf8_lossy(&out.stdout).starts_with("steamos-preflight "));
}

#[test]
fn setup_print_emits_dummy_config() {
    let out = bin().args(["setup", "--print"]).output().unwrap();
    assert!(String::from_utf8_lossy(&out.stdout).contains("Driver \"dummy\""));
}
