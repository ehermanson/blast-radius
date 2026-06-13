use std::io::Read;
use std::path::PathBuf;
use std::sync::OnceLock;

use clap::error::ErrorKind;
use clap::{Parser, ValueEnum};
use clap_complete::Shell;

use crate::graph::RiskTier;

/// `-V` prints the plain version; `--version` adds the language adapters this
/// binary was compiled with, since feature-gated builds differ (prebuilt
/// binaries ship everything; a default `cargo install` is JS/TS only).
fn long_version() -> &'static str {
    static VERSION: OnceLock<String> = OnceLock::new();
    VERSION.get_or_init(|| {
        #[allow(unused_mut)]
        let mut languages = vec!["javascript/typescript"];
        #[cfg(feature = "python")]
        languages.push("python");
        #[cfg(feature = "rust")]
        languages.push("rust");
        #[cfg(feature = "vue")]
        languages.push("vue");
        #[cfg(feature = "svelte")]
        languages.push("svelte");
        format!(
            "{}\nlanguages: {}",
            env!("CARGO_PKG_VERSION"),
            languages.join(", ")
        )
    })
}

#[derive(Debug, Clone, Parser)]
#[command(name = "blast-radius")]
#[command(
    version,
    long_version = long_version(),
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

    /// Suppress stdout output; exit codes (and --output files) still apply.
    #[arg(long, short = 'q', global = true, default_value_t = false)]
    pub quiet: bool,

    /// When to use colors and ANSI styling in tree output.
    #[arg(long, global = true, value_enum, default_value_t = ColorChoice::Auto)]
    pub color: ColorChoice,

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
    /// staged files). Pass one or more paths, or `-` to read a
    /// newline-separated list from stdin (`git diff --name-only | blast-radius files -`).
    Files {
        #[arg(required = true, num_args = 1..)]
        files: Vec<PathBuf>,
    },
    /// Dump the whole-repo import graph (every file and resolved import edge).
    /// Useful for visualization or feeding other tools; `--format json` is the
    /// natural choice, with `mermaid`/`dot` for diagrams.
    Graph,
    /// Print a shell completion script to stdout.
    Completions {
        #[arg(value_enum)]
        shell: Shell,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum OutputFormat {
    Tree,
    Json,
    Mermaid,
    Dot,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum ColorChoice {
    /// Color when writing to a terminal (and `NO_COLOR` is unset).
    Auto,
    /// Always emit ANSI colors, even when piped or written to a file.
    Always,
    /// Never emit ANSI colors.
    Never,
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

    /// Replace `-` entries in `files` with the newline-separated path list
    /// from stdin. No-op for other commands or when `-` is absent.
    pub fn expand_stdin_file_list(&mut self) -> anyhow::Result<()> {
        let Command::Files { files } = &mut self.command else {
            return Ok(());
        };
        if !files.iter().any(|file| file.as_os_str() == "-") {
            return Ok(());
        }

        let mut buffer = String::new();
        std::io::stdin()
            .read_to_string(&mut buffer)
            .map_err(|error| anyhow::anyhow!("failed to read file list from stdin: {error}"))?;
        let stdin_files: Vec<PathBuf> = buffer
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .map(PathBuf::from)
            .collect();

        let mut expanded = Vec::with_capacity(files.len() + stdin_files.len());
        for file in files.drain(..) {
            if file.as_os_str() == "-" {
                expanded.extend(stdin_files.iter().cloned());
            } else {
                expanded.push(file);
            }
        }
        if expanded.is_empty() {
            anyhow::bail!("no files provided: stdin file list was empty");
        }
        *files = expanded;
        Ok(())
    }
}
