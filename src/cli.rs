use std::path::PathBuf;

use clap::error::ErrorKind;
use clap::{Parser, ValueEnum};

use crate::graph::RiskTier;

#[derive(Debug, Clone, Parser)]
#[command(name = "blast-radius")]
#[command(
    version,
    about = "Estimate the transitive blast radius of frontend code changes"
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,

    #[arg(long, global = true, default_value = ".")]
    pub repo_root: PathBuf,

    #[arg(long, global = true, value_enum, default_value_t = OutputFormat::Tree)]
    pub format: OutputFormat,

    #[arg(long, global = true)]
    pub output: Option<PathBuf>,

    /// Show the full cascade tree and analyzer internals in tree output.
    #[arg(long, short = 'v', global = true, default_value_t = false)]
    pub verbose: bool,

    /// Include grouped unresolved-import diagnostics in warnings.
    #[arg(long, global = true, default_value_t = false)]
    pub explain_unresolved: bool,

    /// Exit non-zero (code 2) when more than this many downstream files are
    /// impacted (the changed files themselves are not counted).
    #[arg(long, global = true)]
    pub fail_threshold: Option<usize>,

    /// Exit non-zero (code 2) when the risk verdict is at or above this tier.
    #[arg(long, global = true, value_enum)]
    pub fail_on_risk: Option<RiskTier>,
}

#[derive(Debug, Clone, Parser)]
pub enum Command {
    /// Analyze downstream impact from a named export.
    Export { file: PathBuf, export_name: String },
    /// Analyze downstream impact from every export of a file.
    File { file: PathBuf },
    /// Blast radius for several files at once (e.g. a pre-commit hook over
    /// staged files). Pass one or more paths.
    Files {
        #[arg(required = true, num_args = 1..)]
        files: Vec<PathBuf>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum OutputFormat {
    Tree,
    Json,
    Mermaid,
    Dot,
}

impl Cli {
    /// Exit code 2 is reserved for tripped risk gates, so usage errors exit
    /// with 64 (EX_USAGE) instead of clap's default 2. `--help`/`--version`
    /// still exit 0.
    pub fn parse_args() -> Self {
        match Self::try_parse() {
            Ok(cli) => cli,
            Err(error) => {
                let code = match error.kind() {
                    ErrorKind::DisplayHelp | ErrorKind::DisplayVersion => 0,
                    _ => 64,
                };
                let _ = error.print();
                std::process::exit(code);
            }
        }
    }
}
