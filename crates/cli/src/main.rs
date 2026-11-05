mod diagnose;
mod run;

use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Args, Parser, Subcommand, ValueEnum};
use protonaut_core::config::Resolution;
use protonaut_core::model::FailOn;

/// Exit codes are part of the public interface (docs/reference/cli.md).
pub mod exit {
    pub const PASS: u8 = 0;
    pub const FAIL: u8 = 1;
    pub const USAGE: u8 = 2;
    pub const ENVIRONMENT: u8 = 3;
    pub const CANCELLED: u8 = 130;
}

#[derive(Parser, Debug)]
#[command(name = "protonaut", version, about = "Automated compatibility testing for Windows games targeting SteamOS.", long_about = None)]
#[command(
    after_help = "Independent community project, not affiliated with Valve. Results cannot predict a Steam Deck Verified rating."
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
    /// Increase log detail (-v info, -vv debug, -vvv trace). Logs go to stderr.
    #[arg(short, long, action = clap::ArgAction::Count, global = true)]
    pub verbose: u8,
    /// Log format on stderr.
    #[arg(long, value_enum, default_value_t = LogFormat::Text, global = true, env = "PROTONAUT_LOG_FORMAT")]
    pub log_format: LogFormat,
}

#[derive(Copy, Clone, Debug, ValueEnum)]
pub enum LogFormat {
    Text,
    Json,
}

#[derive(Subcommand, Debug)]
pub enum Command {
    /// Write a starter protonaut.toml in the current directory.
    Init {
        /// Overwrite an existing file.
        #[arg(long)]
        force: bool,
    },
    /// Inspect a build without running it (static checks only).
    Inspect(InspectArgs),
    /// Run the build under Proton and report static and runtime checks.
    Test(TestArgs),
    /// Manage pinned Proton runtimes.
    Proton {
        #[command(subcommand)]
        command: ProtonCommand,
    },
    /// Re-render a report.json as Markdown, HTML or JUnit XML.
    Report(ReportArgs),
    /// Check that this machine can run tests, and explain what is missing.
    Diagnose {
        /// Print machine-readable JSON.
        #[arg(long)]
        json: bool,
    },
    /// Install the Xorg dummy display configuration (needs root).
    Setup {
        /// Print the configuration instead of installing it.
        #[arg(long)]
        print: bool,
    },
    /// Print or write the JSON Schemas for report.json and protonaut.toml.
    Schema {
        /// Write schemas/*.json in the current directory.
        #[arg(long)]
        write: bool,
    },
}

#[derive(Args, Debug, Clone)]
pub struct BuildArgs {
    /// Directory with the packaged Windows build. Overrides build.dir.
    pub build: Option<PathBuf>,
    /// Executable relative to the build directory. Overrides build.exe.
    #[arg(long)]
    pub exe: Option<String>,
    /// Configuration file. Default: ./protonaut.toml when it exists.
    #[arg(long, env = "PROTONAUT_CONFIG")]
    pub config: Option<PathBuf>,
    /// Where to write report.json and artifacts.
    #[arg(long, default_value = "protonaut-results")]
    pub results: PathBuf,
    /// Print the report as JSON on stdout instead of a text summary.
    #[arg(long)]
    pub json: bool,
    /// Directory for downloaded runtimes. Default: $PROTONAUT_CACHE,
    /// $XDG_CACHE_HOME/protonaut or ~/.cache/protonaut.
    #[arg(long)]
    pub cache: Option<PathBuf>,
}

#[derive(Args, Debug)]
pub struct InspectArgs {
    #[command(flatten)]
    pub build: BuildArgs,
    /// Runtime to compare imported DLLs against (when installed).
    #[arg(long, default_value = "stable")]
    pub runtime: String,
}

#[derive(Args, Debug)]
pub struct TestArgs {
    #[command(flatten)]
    pub build: BuildArgs,
    /// Runtime id or `stable`; repeat to test several. Overrides run.runtimes.
    #[arg(long = "runtime")]
    pub runtimes: Vec<String>,
    /// Seconds to watch the game after launch (3-600). Overrides run.observe_secs.
    #[arg(long)]
    pub observe_secs: Option<u64>,
    /// 1280x800, 1280x720 or 1920x1080. Overrides run.resolution.
    #[arg(long, value_parser = parse_resolution)]
    pub resolution: Option<Resolution>,
    /// Fail on FAIL only, or on WARNING too. Overrides run.fail_on.
    #[arg(long, value_enum)]
    pub fail_on: Option<FailOnArg>,
    /// Arguments passed to the game. Overrides build.args.
    #[arg(last = true)]
    pub game_args: Vec<String>,
    /// Download missing runtimes. Without it, a missing runtime is an error.
    #[arg(long)]
    pub install: bool,
    /// Hard limit for the whole command, in seconds.
    #[arg(long, default_value_t = 1800)]
    pub timeout: u64,
}

#[derive(Copy, Clone, Debug, ValueEnum)]
pub enum FailOnArg {
    Fail,
    Warning,
}

impl From<FailOnArg> for FailOn {
    fn from(f: FailOnArg) -> Self {
        match f {
            FailOnArg::Fail => FailOn::Fail,
            FailOnArg::Warning => FailOn::Warning,
        }
    }
}

fn parse_resolution(s: &str) -> Result<Resolution, String> {
    s.parse()
}

#[derive(Subcommand, Debug)]
pub enum ProtonCommand {
    /// List the runtimes this release knows, and which are installed.
    List {
        #[arg(long)]
        cache: Option<PathBuf>,
        #[arg(long)]
        json: bool,
    },
    /// Download, verify and install a runtime.
    Install {
        /// Runtime id or `stable`.
        id: String,
        /// Use an already downloaded tarball (still verified by SHA-256).
        #[arg(long)]
        archive: Option<PathBuf>,
        #[arg(long)]
        cache: Option<PathBuf>,
    },
}

#[derive(Args, Debug)]
pub struct ReportArgs {
    /// Path to report.json.
    pub input: PathBuf,
    #[arg(long, value_enum, default_value_t = ReportFormat::Markdown)]
    pub format: ReportFormat,
    /// Link to the workflow run's artifacts, for the Markdown footer.
    #[arg(long)]
    pub artifact_url: Option<String>,
    /// Write to this file instead of stdout.
    #[arg(long)]
    pub out: Option<PathBuf>,
}

#[derive(Copy, Clone, Debug, ValueEnum)]
pub enum ReportFormat {
    Markdown,
    Html,
    Junit,
    Json,
}

fn init_logging(cli: &Cli) {
    use tracing_subscriber::EnvFilter;
    let level = match cli.verbose {
        0 => "warn",
        1 => "info",
        2 => "debug",
        _ => "trace",
    };
    let filter = EnvFilter::try_from_env("PROTONAUT_LOG").unwrap_or_else(|_| EnvFilter::new(level));
    let builder = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(std::io::stderr)
        .with_target(false);
    match cli.log_format {
        LogFormat::Text => builder.init(),
        LogFormat::Json => builder.json().init(),
    }
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    init_logging(&cli);
    let runtime = match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
    {
        Ok(r) => r,
        Err(e) => {
            eprintln!("error: cannot start async runtime: {e}");
            return ExitCode::from(exit::ENVIRONMENT);
        }
    };
    let code = runtime.block_on(run::dispatch(cli));
    ExitCode::from(code)
}
