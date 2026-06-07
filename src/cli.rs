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

    #[arg(long)]
    pub max_depth: Option<usize>,

    #[arg(long, default_value_t = false)]
    pub include_tests: bool,

    #[arg(long, default_value_t = false)]
    pub include_stories: bool,

    /// Show the full cascade tree and analyzer internals in tree output.
    #[arg(long, short = 'v', global = true, default_value_t = false)]
    pub verbose: bool,

    #[arg(long)]
    pub fail_threshold: Option<usize>,
}

#[derive(Debug, Clone, Parser)]
pub enum Command {
    /// Install a local git hook that runs blast-radius.
    Init {
        /// Git hook to install.
        #[arg(long, value_enum, default_value_t = HookKind::PrePush)]
        hook: HookKind,

        /// Default git range used by the pre-push hook.
        #[arg(long, default_value = "origin/main...HEAD")]
        base: String,

        /// Fail the hook when analysis fails or the threshold is exceeded.
        #[arg(long, default_value_t = false)]
        blocking: bool,

        /// Maximum impacted-file count allowed in blocking mode.
        #[arg(long)]
        fail_threshold: Option<usize>,

        /// Overwrite an existing hook file.
        #[arg(long, default_value_t = false)]
        force: bool,
    },
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
    Diff {
        #[arg(default_value = "origin/main...HEAD")]
        git_range: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum HookKind {
    PreCommit,
    PrePush,
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
