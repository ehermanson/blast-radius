use anyhow::Result;
use blast_radius::{analyze, cli, fs, init, report};
use std::fs as std_fs;
use std::process::ExitCode;

fn main() -> Result<ExitCode> {
    let app = cli::Cli::parse_args();
    if let cli::Command::Init {
        hook,
        base,
        blocking,
        fail_threshold,
        force,
    } = &app.command
    {
        let result = init::run(&init::InitOptions {
            repo_root: app.repo_root.clone(),
            hook: *hook,
            base: base.clone(),
            blocking: *blocking,
            fail_threshold: *fail_threshold,
            force: *force,
        })?;
        let mode = if result.blocking {
            "blocking"
        } else {
            "non-blocking"
        };
        println!(
            "installed {mode} blast-radius hook: {}",
            result.hook_path.display()
        );
        return Ok(ExitCode::SUCCESS);
    }

    let context = fs::RepoContext::discover(&app.repo_root)?;
    let result = analyze::run(&app, &context)?;
    let output = report::render(&app.format, &result, app.verbose)?;

    if let Some(path) = &app.output {
        std_fs::write(path, output)?;
    } else {
        println!("{output}");
    }

    if let Some(threshold) = app.fail_threshold {
        if result.summary.total_affected_files > threshold {
            return Ok(ExitCode::from(2));
        }
    }

    Ok(ExitCode::SUCCESS)
}
