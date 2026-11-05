use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use protonaut_core::config::{Config, ConfigFile, DEFAULT_CONFIG_FILE, Overrides, STARTER_CONFIG};
use protonaut_core::inspect::{BuildTree, PeInfo};
use protonaut_core::model::{
    EnvironmentInfo, REPORT_SCHEMA_VERSION, Report, RunSummary, TargetInfo, Verdict,
};
use protonaut_core::report::{ReportInput, assemble, render_html, render_junit, render_markdown};
use protonaut_core::runtime_checks;
use protonaut_core::static_checks::{StaticContext, StaticInput, run_static_checks};
use protonaut_runtime::display::{Display, SYSTEM_CONFIG_DIR, SYSTEM_CONFIG_NAME, xorg_config};
use protonaut_runtime::install::{Cache, InstalledRuntime, Manifest, default_cache_dir};
use protonaut_runtime::proton::{GameRequest, ProtonBackend, kill_prefixes};

use crate::{
    BuildArgs, Cli, Command, InspectArgs, ProtonCommand, ReportArgs, ReportFormat, TestArgs,
    diagnose, exit,
};

/// An error with the exit code it maps to.
#[derive(Debug)]
pub struct CliError {
    pub code: u8,
    pub message: String,
}

fn usage(m: impl ToString) -> CliError {
    CliError {
        code: exit::USAGE,
        message: m.to_string(),
    }
}

fn env_err(m: impl ToString) -> CliError {
    CliError {
        code: exit::ENVIRONMENT,
        message: m.to_string(),
    }
}

pub async fn dispatch(cli: Cli) -> u8 {
    let result = match cli.command {
        Command::Init { force } => init(force),
        Command::Inspect(a) => inspect(a).await,
        Command::Test(a) => test(a).await,
        Command::Proton { command } => proton(command).await,
        Command::Report(a) => report(a),
        Command::Diagnose { json } => Ok(diagnose::run(json).await),
        Command::Setup { print } => setup(print),
        Command::Schema { write } => schema(write),
    };
    match result {
        Ok(code) => code,
        Err(e) => {
            eprintln!("error: {}", e.message);
            e.code
        }
    }
}

fn init(force: bool) -> Result<u8, CliError> {
    let path = Path::new(DEFAULT_CONFIG_FILE);
    if path.exists() && !force {
        return Err(usage(format!(
            "{DEFAULT_CONFIG_FILE} already exists; pass --force to overwrite"
        )));
    }
    fs::write(path, STARTER_CONFIG)
        .map_err(|e| env_err(format!("cannot write {DEFAULT_CONFIG_FILE}: {e}")))?;
    println!(
        "Wrote {DEFAULT_CONFIG_FILE}. Set build.dir and build.exe, then run `protonaut test`."
    );
    Ok(exit::PASS)
}

fn cache_for(arg: &Option<PathBuf>) -> Result<Cache, CliError> {
    let dir = arg
        .clone()
        .or_else(default_cache_dir)
        .ok_or_else(|| env_err("no cache directory: set --cache or PROTONAUT_CACHE"))?;
    Ok(Cache::new(dir))
}

fn load_config(b: &BuildArgs, extra: Overrides) -> Result<Config, CliError> {
    let (file, base) = match &b.config {
        Some(p) => (
            ConfigFile::load(p).map_err(usage)?,
            p.parent().map(Path::to_path_buf).unwrap_or_default(),
        ),
        None if Path::new(DEFAULT_CONFIG_FILE).is_file() => (
            ConfigFile::load(Path::new(DEFAULT_CONFIG_FILE)).map_err(usage)?,
            PathBuf::from("."),
        ),
        None => (ConfigFile::default(), PathBuf::from(".")),
    };
    let overrides = Overrides {
        build_dir: b.build.clone(),
        exe: b.exe.clone(),
        ..extra
    };
    Config::resolve(file, overrides, &base).map_err(usage)
}

struct StaticOutcome {
    checks: Vec<protonaut_core::model::CheckResult>,
    exe_rel: String,
}

fn static_pass(
    cfg: &Config,
    runtime: Option<&InstalledRuntime>,
) -> Result<StaticOutcome, CliError> {
    let tree = BuildTree::scan(&cfg.build_dir).map_err(usage)?;
    let exe_rel = tree.find_ci(&cfg.exe).map(str::to_owned).ok_or_else(|| {
        usage(format!(
            "{} not found in {}",
            cfg.exe,
            cfg.build_dir.display()
        ))
    })?;
    let pe = PeInfo::load(&cfg.build_dir.join(&exe_rel)).map_err(usage)?;
    let ctx = StaticContext {
        runtime_dlls: runtime
            .map(InstalledRuntime::builtin_dlls)
            .unwrap_or_default(),
    };
    let checks = run_static_checks(&StaticInput {
        tree: &tree,
        exe: &exe_rel,
        pe: &pe,
        ctx: &ctx,
    });
    Ok(StaticOutcome { checks, exe_rel })
}

fn environment() -> EnvironmentInfo {
    let os = fs::read_to_string("/etc/os-release")
        .ok()
        .and_then(|t| {
            t.lines().find_map(|l| {
                l.strip_prefix("PRETTY_NAME=")
                    .map(|v| v.trim_matches('"').to_owned())
            })
        })
        .unwrap_or_else(|| std::env::consts::OS.to_owned());
    let kernel = nix::sys::utsname::uname()
        .ok()
        .map(|u| u.release().to_string_lossy().into_owned());
    EnvironmentInfo {
        os,
        kernel,
        vulkan_device: None,
        display: None,
    }
}

async fn inspect(a: InspectArgs) -> Result<u8, CliError> {
    let cfg = load_config(&a.build, Overrides::default())?;
    let cache = cache_for(&a.build.cache)?;
    let manifest = Manifest::builtin().map_err(env_err)?;
    let spec = manifest.resolve(&a.runtime).map_err(usage)?;
    let installed = cache.installed(spec);
    let st = static_pass(&cfg, installed.as_ref())?;
    let report = assemble(ReportInput {
        tool_version: env!("CARGO_PKG_VERSION").to_owned(),
        target: TargetInfo {
            exe: st.exe_rel,
            args: cfg.args.clone(),
        },
        environment: environment(),
        runs: vec![],
        checks: st.checks,
        fail_on: cfg.fail_on,
        skip: cfg.skip.clone(),
    });
    write_results(&a.build.results, &report)?;
    print_summary(&report, &a.build.results, a.build.json)?;
    Ok(verdict_code(&report))
}

async fn test(a: TestArgs) -> Result<u8, CliError> {
    let extra = Overrides {
        args: (!a.game_args.is_empty()).then(|| a.game_args.clone()),
        runtimes: (!a.runtimes.is_empty()).then(|| a.runtimes.clone()),
        observe_secs: a.observe_secs,
        resolution: a.resolution,
        fail_on: a.fail_on.map(Into::into),
        ..Overrides::default()
    };
    let cfg = load_config(&a.build, extra)?;
    let cache = cache_for(&a.build.cache)?;
    let manifest = Manifest::builtin().map_err(env_err)?;
    let mut runtimes = Vec::new();
    let mut seen = BTreeSet::new();
    for id in &cfg.runtimes {
        let spec = manifest.resolve(id).map_err(usage)?;
        if !seen.insert(spec.id.clone()) {
            continue;
        }
        let rt = match cache.installed(spec) {
            Some(rt) => rt,
            None if a.install => cache.install(spec, None).await.map_err(env_err)?,
            None => {
                return Err(env_err(format!(
                    "runtime {} is not installed; run `protonaut proton install {}` or pass --install",
                    spec.id, spec.id
                )));
            }
        };
        runtimes.push(rt);
    }
    fs::create_dir_all(&a.build.results)
        .map_err(|e| env_err(format!("cannot create {}: {e}", a.build.results.display())))?;
    let work = tempfile::Builder::new()
        .prefix("protonaut-")
        .tempdir()
        .map_err(|e| env_err(format!("cannot create work directory: {e}")))?;

    let job = run_all(&cfg, &runtimes, &a.build.results, work.path());
    let outcome = tokio::select! {
        r = tokio::time::timeout(Duration::from_secs(a.timeout), job) => match r {
            Ok(r) => r,
            Err(_) => Err(env_err(format!("timed out after {} s (--timeout)", a.timeout))),
        },
        _ = shutdown_signal() => Err(CliError { code: exit::CANCELLED, message: "cancelled".into() }),
    };
    // The run future has been dropped by now, which killed its process
    // groups; Wine's server may still hold a prefix, so stop it explicitly.
    for rt in &runtimes {
        kill_prefixes(rt, work.path()).await;
    }
    let report = outcome?;
    print_summary(&report, &a.build.results, a.build.json)?;
    Ok(verdict_code(&report))
}

async fn shutdown_signal() {
    use tokio::signal::unix::{SignalKind, signal};
    match signal(SignalKind::terminate()) {
        Ok(mut term) => {
            tokio::select! {
                _ = tokio::signal::ctrl_c() => {}
                _ = term.recv() => {}
            }
        }
        Err(_) => {
            let _ = tokio::signal::ctrl_c().await;
        }
    }
}

async fn run_all(
    cfg: &Config,
    runtimes: &[InstalledRuntime],
    results: &Path,
    work: &Path,
) -> Result<Report, CliError> {
    let st = static_pass(cfg, runtimes.first())?;
    let mut checks = st.checks;
    let display = Display::start(cfg.resolution, work)
        .await
        .map_err(env_err)?;
    let mut env = environment();
    env.display = display
        .current_mode()
        .await
        .ok()
        .map(|m| format!("Xorg dummy {m}"));
    let build_dir = fs::canonicalize(&cfg.build_dir).unwrap_or_else(|_| cfg.build_dir.clone());
    let results_abs = fs::canonicalize(results).unwrap_or_else(|_| results.to_path_buf());
    let redactions = redactions(&build_dir, work, &results_abs);
    let mut runs = Vec::new();
    let mut failure = None;
    for rt in runtimes {
        let backend = ProtonBackend {
            runtime: rt,
            display: &display,
        };
        let req = GameRequest {
            build_dir: &build_dir,
            exe: &st.exe_rel,
            args: &cfg.args,
            observe_secs: cfg.observe_secs,
            results: &results_abs,
            work,
            redactions: &redactions,
        };
        match backend.run(&req).await {
            Ok(out) => {
                if env.vulkan_device.is_none() {
                    env.vulkan_device = out.vulkan_device.clone();
                }
                let o = out.observation;
                runs.push(RunSummary {
                    runtime: o.runtime.clone(),
                    runtime_version: o.runtime_version.clone(),
                    outcome: o.outcome,
                    exit_code: o.exit_code,
                    duration_ms: o.duration_ms,
                    capture_verified: o.capture_verified,
                });
                checks.extend(runtime_checks::evaluate(&o));
            }
            Err(e) => {
                failure = Some(env_err(format!("runtime {}: {e}", rt.spec.id)));
                break;
            }
        }
    }
    display.stop().await;
    if let Some(e) = failure {
        return Err(e);
    }
    let report = assemble(ReportInput {
        tool_version: env!("CARGO_PKG_VERSION").to_owned(),
        target: TargetInfo {
            exe: st.exe_rel,
            args: cfg.args.clone(),
        },
        environment: env,
        runs,
        checks,
        fail_on: cfg.fail_on,
        skip: cfg.skip.clone(),
    });
    write_results(results, &report)?;
    Ok(report)
}

/// Host-specific values that must not appear in uploaded artifacts.
fn redactions(build: &Path, work: &Path, results: &Path) -> Vec<(String, &'static str)> {
    let mut v = Vec::new();
    for (p, placeholder) in [(build, "<build>"), (work, "<work>"), (results, "<results>")] {
        v.extend(protonaut_core::report::path_variants(
            &p.display().to_string(),
            placeholder,
        ));
    }
    if let Some(home) = std::env::var_os("HOME") {
        v.extend(protonaut_core::report::path_variants(
            &home.to_string_lossy(),
            "~",
        ));
    }
    if let Ok(Some(user)) = nix::unistd::User::from_uid(nix::unistd::getuid())
        && user.name.len() >= 3
    {
        v.push((user.name, "<user>"));
    }
    if let Ok(host) = nix::unistd::gethostname() {
        let host = host.to_string_lossy().into_owned();
        if host.len() >= 3 {
            v.push((host, "<host>"));
        }
    }
    v
}

fn write_results(dir: &Path, report: &Report) -> Result<(), CliError> {
    fs::create_dir_all(dir)
        .map_err(|e| env_err(format!("cannot create {}: {e}", dir.display())))?;
    let json = serde_json::to_string_pretty(report).map_err(env_err)?;
    let write = |name: &str, body: String| {
        fs::write(dir.join(name), body).map_err(|e| env_err(format!("cannot write {name}: {e}")))
    };
    write("report.json", json + "\n")?;
    write("report.md", render_markdown(report, None))?;
    write("report.html", render_html(report))?;
    write("junit.xml", render_junit(report))?;
    Ok(())
}

fn print_summary(report: &Report, results: &Path, json: bool) -> Result<(), CliError> {
    if json {
        println!("{}", serde_json::to_string_pretty(report).map_err(env_err)?);
        return Ok(());
    }
    for c in &report.checks {
        let rt = c
            .runtime
            .as_deref()
            .map(|r| format!(" [{r}]"))
            .unwrap_or_default();
        println!(
            "{:<10} {:<14} {}{} — {}",
            c.status.as_str(),
            c.id,
            c.title,
            rt,
            c.summary
        );
    }
    let n = &report.summary.counts;
    println!(
        "\n{}: {} pass, {} fail, {} warning, {} unknown, {} not tested. Report: {}",
        protonaut_core::report::headline(report),
        n.pass,
        n.fail,
        n.warning,
        n.unknown,
        n.not_tested,
        results.join("report.html").display()
    );
    Ok(())
}

fn verdict_code(report: &Report) -> u8 {
    match report.summary.verdict {
        Verdict::Pass => exit::PASS,
        Verdict::Fail => exit::FAIL,
    }
}

async fn proton(cmd: ProtonCommand) -> Result<u8, CliError> {
    let manifest = Manifest::builtin().map_err(env_err)?;
    match cmd {
        ProtonCommand::List { cache, json } => {
            let cache = cache_for(&cache)?;
            let rows = cache.list(&manifest);
            if json {
                let v: Vec<_> = rows
                    .iter()
                    .map(|(s, inst)| serde_json::json!({"id": s.id, "name": s.name, "installed": inst, "stable": s.id == manifest.stable, "sha256": s.sha256}))
                    .collect();
                println!("{}", serde_json::to_string_pretty(&v).map_err(env_err)?);
            } else {
                for (s, inst) in rows {
                    let tag = if s.id == manifest.stable {
                        " (stable)"
                    } else {
                        ""
                    };
                    println!(
                        "{:<18} {:<16} {}{}",
                        s.id,
                        s.name,
                        if inst { "installed" } else { "not installed" },
                        tag
                    );
                }
            }
            Ok(exit::PASS)
        }
        ProtonCommand::Install { id, archive, cache } => {
            let cache = cache_for(&cache)?;
            let spec = manifest.resolve(&id).map_err(usage)?;
            let rt = cache
                .install(spec, archive.as_deref())
                .await
                .map_err(env_err)?;
            println!(
                "{} installed and verified at {}",
                rt.spec.id,
                rt.root.display()
            );
            Ok(exit::PASS)
        }
    }
}

fn report(a: ReportArgs) -> Result<u8, CliError> {
    let text = fs::read_to_string(&a.input)
        .map_err(|e| usage(format!("cannot read {}: {e}", a.input.display())))?;
    let value: serde_json::Value = serde_json::from_str(&text)
        .map_err(|e| usage(format!("{} is not JSON: {e}", a.input.display())))?;
    match value
        .get("schema_version")
        .and_then(serde_json::Value::as_u64)
    {
        Some(v) if v == u64::from(REPORT_SCHEMA_VERSION) => {}
        other => {
            return Err(usage(format!(
                "unsupported report schema_version {other:?}; expected {REPORT_SCHEMA_VERSION}"
            )));
        }
    }
    // Re-parsing into the typed model rejects reports that do not match the
    // schema, so a tampered artifact cannot inject arbitrary structure.
    let report: Report =
        serde_json::from_value(value).map_err(|e| usage(format!("invalid report: {e}")))?;
    let body = match a.format {
        ReportFormat::Markdown => render_markdown(&report, a.artifact_url.as_deref()),
        ReportFormat::Html => render_html(&report),
        ReportFormat::Junit => render_junit(&report),
        ReportFormat::Json => serde_json::to_string_pretty(&report).map_err(env_err)? + "\n",
    };
    match a.out {
        Some(p) => fs::write(&p, body)
            .map_err(|e| env_err(format!("cannot write {}: {e}", p.display())))?,
        None => print!("{body}"),
    }
    Ok(exit::PASS)
}

fn setup(print: bool) -> Result<u8, CliError> {
    if print {
        print!("{}", xorg_config());
        return Ok(exit::PASS);
    }
    let path = Path::new(SYSTEM_CONFIG_DIR).join(SYSTEM_CONFIG_NAME);
    fs::write(&path, xorg_config()).map_err(|e| {
        env_err(format!(
            "cannot write {} (run with sudo?): {e}",
            path.display()
        ))
    })?;
    println!("Wrote {}", path.display());
    Ok(exit::PASS)
}

fn schema(write: bool) -> Result<u8, CliError> {
    let schemas = [
        ("report.v1.schema.json", protonaut_core::report_schema()),
        ("config.v1.schema.json", protonaut_core::config_schema()),
    ];
    for (name, value) in schemas {
        let text = serde_json::to_string_pretty(&value).map_err(env_err)? + "\n";
        if write {
            fs::create_dir_all("schemas")
                .map_err(|e| env_err(format!("cannot create schemas/: {e}")))?;
            fs::write(Path::new("schemas").join(name), text)
                .map_err(|e| env_err(format!("cannot write {name}: {e}")))?;
            println!("wrote schemas/{name}");
        } else {
            println!("// {name}\n{text}");
        }
    }
    Ok(exit::PASS)
}
