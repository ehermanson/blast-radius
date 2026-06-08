use std::path::PathBuf;

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

    #[arg(long, default_value = ".")]
    pub repo_root: PathBuf,

    #[arg(long, value_enum, default_value_t = OutputFormat::Tree)]
    pub format: OutputFormat,

    #[arg(long)]
    pub output: Option<PathBuf>,

    /// Show the full cascade tree and analyzer internals in tree output.
    #[arg(long, short = 'v', global = true, default_value_t = false)]
    pub verbose: bool,

    /// Exit non-zero (code 2) when more than this many files are affected.
    #[arg(long)]
    pub fail_threshold: Option<usize>,

    /// Exit non-zero (code 2) when the risk verdict is at or above this tier.
    #[arg(long, value_enum)]
    pub fail_on_risk: Option<RiskTier>,
}

#[derive(Debug, Clone, Parser)]
pub enum Command {
    /// Analyze downstream impact from a named export.
    Export {
        file: PathBuf,
        export_name: String,
    },
    File {
        file: PathBuf,
    },
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
    pub fn parse_args() -> Self {
        Self::parse()
    }
}
