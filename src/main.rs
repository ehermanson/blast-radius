use std::io::IsTerminal;
use std::process::ExitCode;

use anyhow::Result;
use blast_radius::{analyze, cli, fs, report};
use clap::CommandFactory;
use std::fs as std_fs;

fn main() -> Result<ExitCode> {
    let mut app = cli::Cli::parse_args();

    if let cli::Command::Completions { shell } = app.command {
        clap_complete::generate(
            shell,
            &mut cli::Cli::command(),
            "blast-radius",
            &mut std::io::stdout(),
        );
        return Ok(ExitCode::SUCCESS);
    }

    app.expand_stdin_file_list()?;

    let context = fs::RepoContext::discover(&app.repo_root)?;
    let result = analyze::run(&app, &context)?;

    let color = match app.color {
        cli::ColorChoice::Always => true,
        cli::ColorChoice::Never => false,
        // Never write ANSI escapes into a file destination or a pipe unless
        // explicitly forced with --color always.
        cli::ColorChoice::Auto => {
            app.output.is_none()
                && std::io::stdout().is_terminal()
                && std::env::var_os("NO_COLOR").is_none()
        }
    };

    if app.output.is_some() || !app.quiet {
        let output = report::render(&app.format, &result, app.verbose, color)?;
        if let Some(path) = &app.output {
            std_fs::write(path, output)?;
        } else if !app.quiet {
            println!("{output}");
        }
    }

    let over_threshold = app
        .fail_threshold
        .is_some_and(|threshold| result.summary.total_affected_files > threshold);
    let over_risk = app
        .fail_on_risk
        .is_some_and(|tier| result.summary.risk_tier >= tier);
    if over_threshold || over_risk {
        return Ok(ExitCode::from(2));
    }

    Ok(ExitCode::SUCCESS)
}
