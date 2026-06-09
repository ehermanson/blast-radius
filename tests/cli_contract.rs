//! CLI contract tests: exit codes, gate semantics, `--output`, and usage
//! errors. The contract is: 0 = ok, 1 = analysis error, 2 = gate tripped,
//! 64 = usage error.

use std::fs;
use std::path::Path;

use assert_cmd::Command as AssertCommand;
use predicates::prelude::*;
use tempfile::tempdir;

fn blast_radius(repo: &Path) -> AssertCommand {
    let mut command = AssertCommand::cargo_bin("blast-radius").unwrap();
    command
        .current_dir(repo)
        .args(["--repo-root", repo.to_str().unwrap()]);
    command
}

/// `src/source.ts` with two direct consumers: downstream impact of 2.
fn setup_repo() -> tempfile::TempDir {
    let dir = tempdir().unwrap();
    fs::create_dir_all(dir.path().join("src")).unwrap();
    fs::write(dir.path().join("src/source.ts"), "export const x = 1;\n").unwrap();
    for name in ["a", "b"] {
        fs::write(
            dir.path().join(format!("src/{name}.ts")),
            "import { x } from './source';\nexport const v = x + 1;\n",
        )
        .unwrap();
    }
    dir
}

#[test]
fn fail_threshold_at_exact_downstream_count_passes() {
    let repo = setup_repo();
    blast_radius(repo.path())
        .args(["--fail-threshold", "2", "file", "src/source.ts"])
        .assert()
        .success();
}

#[test]
fn fail_threshold_below_downstream_count_trips_gate() {
    let repo = setup_repo();
    blast_radius(repo.path())
        .args(["--fail-threshold", "1", "file", "src/source.ts"])
        .assert()
        .failure()
        .code(2);
}

#[test]
fn fail_threshold_zero_passes_for_file_with_no_dependents() {
    let repo = setup_repo();
    // Leaf file: nothing imports it, so downstream impact is 0 and the root
    // itself must not count against the gate.
    blast_radius(repo.path())
        .args(["--fail-threshold", "0", "file", "src/a.ts"])
        .assert()
        .success();
}

#[test]
fn output_flag_writes_plain_file_and_keeps_stdout_quiet() {
    let repo = setup_repo();
    let out_path = repo.path().join("report.txt");

    blast_radius(repo.path())
        .args([
            "--output",
            out_path.to_str().unwrap(),
            "file",
            "src/source.ts",
        ])
        .assert()
        .success()
        .stdout(predicate::str::is_empty());

    let contents = fs::read_to_string(&out_path).unwrap();
    assert!(contents.contains("IMPACTED FILES"));
    assert!(
        !contents.contains('\u{1b}'),
        "--output file must not contain ANSI escapes"
    );
}

#[test]
fn output_flag_still_writes_file_when_gate_trips() {
    let repo = setup_repo();
    let out_path = repo.path().join("report.txt");

    blast_radius(repo.path())
        .args([
            "--output",
            out_path.to_str().unwrap(),
            "--fail-threshold",
            "0",
            "file",
            "src/source.ts",
        ])
        .assert()
        .failure()
        .code(2);

    let contents = fs::read_to_string(&out_path).unwrap();
    assert!(contents.contains("IMPACTED FILES"));
    assert!(!contents.contains('\u{1b}'));
}

#[test]
fn nonexistent_repo_root_is_an_analysis_error() {
    let repo = setup_repo();
    AssertCommand::cargo_bin("blast-radius")
        .unwrap()
        .current_dir(repo.path())
        .args(["--repo-root", "does/not/exist", "file", "src/source.ts"])
        .assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains("failed to resolve repo root"));
}

#[test]
fn file_mode_on_missing_path_is_an_analysis_error() {
    let repo = setup_repo();
    blast_radius(repo.path())
        .args(["file", "src/missing.ts"])
        .assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains("failed to resolve input path"));
}

#[test]
fn export_mode_with_unknown_export_is_an_analysis_error() {
    let repo = setup_repo();
    blast_radius(repo.path())
        .args(["export", "src/source.ts", "NoSuchExport"])
        .assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains("export 'NoSuchExport' not found"));
}

#[test]
fn unknown_flag_exits_with_usage_error_not_gate_code() {
    let repo = setup_repo();
    blast_radius(repo.path())
        .args(["--no-such-flag", "file", "src/source.ts"])
        .assert()
        .failure()
        .code(64)
        .stderr(predicate::str::contains("--no-such-flag"));
}

#[test]
fn help_and_version_exit_zero() {
    AssertCommand::cargo_bin("blast-radius")
        .unwrap()
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("--fail-threshold"));

    AssertCommand::cargo_bin("blast-radius")
        .unwrap()
        .arg("--version")
        .assert()
        .success();
}

#[test]
fn global_flags_work_after_the_subcommand() {
    let repo = setup_repo();
    let output = blast_radius(repo.path())
        .args(["file", "src/source.ts", "--format", "json"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let json: serde_json::Value = serde_json::from_slice(&output).unwrap();
    assert_eq!(json["summary"]["total_affected_files"].as_u64().unwrap(), 2);
}
