use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail};

use crate::cli::HookKind;

#[derive(Debug, Clone)]
pub struct InitOptions {
    pub repo_root: PathBuf,
    pub hook: HookKind,
    pub base: String,
    pub blocking: bool,
    pub fail_threshold: Option<usize>,
    pub force: bool,
}

#[derive(Debug, Clone)]
pub struct InitResult {
    pub hook_path: PathBuf,
    pub blocking: bool,
}

pub fn run(options: &InitOptions) -> Result<InitResult> {
    let repo_root = options.repo_root.canonicalize().with_context(|| {
        format!(
            "failed to resolve repo root {}",
            options.repo_root.display()
        )
    })?;
    let hooks_dir = git_hooks_dir(&repo_root)?;
    fs::create_dir_all(&hooks_dir)
        .with_context(|| format!("failed to create hooks directory {}", hooks_dir.display()))?;

    let hook_path = hooks_dir.join(hook_file_name(options.hook));
    if hook_path.exists() && !options.force {
        bail!(
            "{} already exists; rerun with --force to overwrite",
            hook_path.display()
        );
    }

    let script = hook_script(options);
    fs::write(&hook_path, script)
        .with_context(|| format!("failed to write hook {}", hook_path.display()))?;
    make_executable(&hook_path)?;

    Ok(InitResult {
        hook_path,
        blocking: options.blocking,
    })
}

fn hook_file_name(hook: HookKind) -> &'static str {
    match hook {
        HookKind::PreCommit => "pre-commit",
        HookKind::PrePush => "pre-push",
    }
}

fn git_hooks_dir(repo_root: &Path) -> Result<PathBuf> {
    let output = Command::new("git")
        .args(["rev-parse", "--git-path", "hooks"])
        .current_dir(repo_root)
        .output()
        .with_context(|| "failed to run git rev-parse")?;

    if !output.status.success() {
        bail!("{} is not a git repository", repo_root.display());
    }

    let path = String::from_utf8(output.stdout)
        .context("git returned a non-utf8 hooks path")?
        .trim()
        .to_owned();
    let path = PathBuf::from(path);
    if path.is_absolute() {
        Ok(path)
    } else {
        Ok(repo_root.join(path))
    }
}

fn hook_script(options: &InitOptions) -> String {
    let command = match options.hook {
        HookKind::PreCommit => pre_commit_command(options),
        HookKind::PrePush => pre_push_command(options),
    };
    format!(
        r#"#!/usr/bin/env bash
set -u

if ! command -v blast-radius >/dev/null 2>&1; then
  echo "blast-radius: not installed; skipping"
  exit 0
fi

{command}
"#
    )
}

fn pre_commit_command(options: &InitOptions) -> String {
    let threshold = options
        .fail_threshold
        .map(|threshold| format!(" --fail-threshold {threshold}"))
        .unwrap_or_default();
    let command = apply_blocking_mode(
        format!(r#"blast-radius --repo-root .{threshold} files "${{files[@]}}""#),
        options,
    );
    format!(
        r#"mapfile -t files < <(git diff --cached --name-only --diff-filter=ACMR)
if [ "${{#files[@]}}" -eq 0 ]; then
  exit 0
fi

echo "blast-radius: checking staged files"
{command}"#
    )
}

fn pre_push_command(options: &InitOptions) -> String {
    let base = shell_parameter_default(&options.base);
    let threshold = options
        .fail_threshold
        .map(|threshold| format!(" --fail-threshold {threshold}"))
        .unwrap_or_default();
    let command = format!(r#"blast-radius --repo-root .{threshold} diff "$base""#);
    let command = apply_blocking_mode(command, options);
    format!(
        r#"base="${{BLAST_RADIUS_BASE:-{base}}}"

echo "blast-radius: checking diff $base"
{command}"#
    )
}

fn apply_blocking_mode(command: String, options: &InitOptions) -> String {
    if options.blocking {
        command
    } else {
        format!("{command} || true")
    }
}

fn shell_parameter_default(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('$', "\\$")
        .replace('`', "\\`")
}

fn make_executable(path: &Path) -> Result<()> {
    let metadata =
        fs::metadata(path).with_context(|| format!("failed to stat hook {}", path.display()))?;
    let mut permissions = metadata.permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(path, permissions)
        .with_context(|| format!("failed to chmod hook {}", path.display()))
}
