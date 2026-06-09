use anyhow::Result;
use blast_radius::{analyze, cli, fs, report};
use std::fs as std_fs;
use std::process::ExitCode;

fn main() -> Result<ExitCode> {
    let app = cli::Cli::parse_args();
    let context = fs::RepoContext::discover(&app.repo_root)?;
    let result = analyze::run(&app, &context)?;
    // Never write ANSI escapes into a file destination.
    let color = app.output.is_none();
    let output = report::render(&app.format, &result, app.verbose, color)?;

    if let Some(path) = &app.output {
        std_fs::write(path, output)?;
    } else {
        println!("{output}");
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
