use std::path::PathBuf;

use clap::{Parser, ValueEnum};

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

    #[arg(long)]
    pub fail_threshold: Option<usize>,
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
