use anyhow::Result;
use blast_radius::{analyze, cli, fs, report};
use std::fs as std_fs;
use std::process::ExitCode;

fn main() -> Result<ExitCode> {
    let app = cli::Cli::parse_args();
    let context = fs::RepoContext::discover(&app.repo_root)?;
    let result = analyze::run(&app, &context)?;
    let output = report::render(&app.format, &result, app.verbose)?;

    if let Some(path) = &app.output {
        std_fs::write(path, output)?;
    } else {
        println!("{output}");
    }

    if let Some(threshold) = app.fail_threshold
        && result.summary.total_affected_files > threshold
    {
        return Ok(ExitCode::from(2));
    }

    Ok(ExitCode::SUCCESS)
}
