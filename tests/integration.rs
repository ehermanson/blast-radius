use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use assert_cmd::Command as AssertCommand;
use predicates::prelude::*;
use serde_json::Value;
use tempfile::tempdir;

fn fixture_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/monorepo")
}

fn chakra_example_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("examples/chakra-ui")
}

#[cfg(feature = "python")]
fn python_fixture_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/python")
}

#[cfg(feature = "python")]
fn fastapi_example_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("examples/fastapi")
}

fn copy_dir(from: &Path, to: &Path) {
    fs::create_dir_all(to).unwrap();
    for entry in fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let source_path = entry.path();
        let target_path = to.join(entry.file_name());
        let file_type = entry.file_type().unwrap();
        if file_type.is_dir() {
            copy_dir(&source_path, &target_path);
        } else {
            fs::copy(&source_path, &target_path).unwrap();
        }
    }
}

fn setup_repo() -> tempfile::TempDir {
    let dir = tempdir().unwrap();
    copy_dir(&fixture_root(), dir.path());
    dir
}

#[test]
fn export_mode_reports_transitive_blast_radius() {
    let repo = setup_repo();

    let output = AssertCommand::cargo_bin("blast-radius")
        .unwrap()
        .current_dir(repo.path())
        .args([
            "--repo-root",
            repo.path().to_str().unwrap(),
            "--format",
            "json",
            "export",
            "packages/ui/src/Button.tsx",
            "Button",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let json: Value = serde_json::from_slice(&output).unwrap();
    let labels: Vec<String> = json["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|node| node["label"].as_str().map(ToOwned::to_owned))
        .collect();

    assert!(
        labels
            .iter()
            .any(|label| label.contains("packages/ui/src/Card.tsx"))
    );
    assert!(
        labels
            .iter()
            .any(|label| label.contains("packages/ui/src/Toolbar.tsx"))
    );
    assert!(
        labels
            .iter()
            .any(|label| label.contains("packages/ui/src/index.ts#Button"))
    );
    assert!(
        labels
            .iter()
            .any(|label| label.contains("apps/storefront/src/PromoCard.tsx"))
    );
    assert!(
        labels
            .iter()
            .any(|label| label.contains("apps/storefront/src/App.tsx"))
    );
    assert!(
        labels
            .iter()
            .any(|label| label.contains("apps/storefront/src/LegacyButtonCard.jsx"))
    );

    assert_eq!(json["summary"]["unresolved_imports"].as_u64().unwrap(), 0);
}

#[test]
fn file_mode_reports_tree_output() {
    let repo = setup_repo();

    // Default output leads with the verdict and groups impact by package.
    AssertCommand::cargo_bin("blast-radius")
        .unwrap()
        .current_dir(repo.path())
        .args([
            "--repo-root",
            repo.path().to_str().unwrap(),
            "file",
            "packages/ui/src/Button.tsx",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("IMPACTED FILES"))
        .stdout(predicate::str::contains("confidence:"))
        .stdout(predicate::str::contains("packages/ui"))
        .stdout(predicate::str::contains("apps/storefront/src/App.tsx"));

    // The full cascade tree is available behind --verbose.
    AssertCommand::cargo_bin("blast-radius")
        .unwrap()
        .current_dir(repo.path())
        .args([
            "--repo-root",
            repo.path().to_str().unwrap(),
            "file",
            "packages/ui/src/Button.tsx",
            "--verbose",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("CASCADE · OVERVIEW"))
        .stdout(predicate::str::contains("CASCADE · PATHS"))
        .stdout(predicate::str::contains("packages/ui/src/Card.tsx"))
        .stdout(predicate::str::contains("apps/storefront/src/App.tsx"));
}

#[test]
fn graph_formats_render() {
    let repo = setup_repo();

    AssertCommand::cargo_bin("blast-radius")
        .unwrap()
        .current_dir(repo.path())
        .args([
            "--repo-root",
            repo.path().to_str().unwrap(),
            "--format",
            "mermaid",
            "export",
            "packages/ui/src/Button.tsx",
            "Button",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("graph TD"));

    AssertCommand::cargo_bin("blast-radius")
        .unwrap()
        .current_dir(repo.path())
        .args([
            "--repo-root",
            repo.path().to_str().unwrap(),
            "--format",
            "dot",
            "export",
            "packages/ui/src/Button.tsx",
            "Button",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("digraph blast_radius"));
}

#[test]
fn diff_mode_uses_git_range() {
    let repo = setup_repo();

    Command::new("git")
        .arg("init")
        .current_dir(repo.path())
        .output()
        .unwrap();
    Command::new("git")
        .args(["config", "user.email", "test@example.com"])
        .current_dir(repo.path())
        .output()
        .unwrap();
    Command::new("git")
        .args(["config", "user.name", "Test User"])
        .current_dir(repo.path())
        .output()
        .unwrap();
    Command::new("git")
        .args(["add", "."])
        .current_dir(repo.path())
        .output()
        .unwrap();
    Command::new("git")
        .args(["commit", "-m", "initial"])
        .current_dir(repo.path())
        .output()
        .unwrap();

    fs::write(
        repo.path().join("packages/ui/src/Button.tsx"),
        "export const Button = () => <button>changed</button>;\nexport default Button;\n",
    )
    .unwrap();

    let output = AssertCommand::cargo_bin("blast-radius")
        .unwrap()
        .current_dir(repo.path())
        .args([
            "--repo-root",
            repo.path().to_str().unwrap(),
            "--format",
            "json",
            "diff",
            "HEAD",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let json: Value = serde_json::from_slice(&output).unwrap();
    let changed_files = json["target"]["changed_files"].as_array().unwrap();
    assert_eq!(changed_files.len(), 1);
    let labels: Vec<String> = json["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|node| node["label"].as_str().map(ToOwned::to_owned))
        .collect();
    assert!(
        labels
            .iter()
            .any(|label| label.contains("changed packages/ui/src/Button.tsx"))
    );
}

#[test]
fn diff_mode_breaks_down_each_changed_file() {
    let repo = setup_repo();
    for args in [
        vec!["init"],
        vec!["config", "user.email", "test@example.com"],
        vec!["config", "user.name", "Test User"],
        vec!["add", "."],
        vec!["commit", "-m", "initial"],
    ] {
        Command::new("git")
            .args(&args)
            .current_dir(repo.path())
            .output()
            .unwrap();
    }

    // Change two files, each with its own downstream blast radius.
    fs::write(
        repo.path().join("packages/ui/src/Button.tsx"),
        "export const Button = () => <button>changed</button>;\nexport default Button;\n",
    )
    .unwrap();
    fs::write(
        repo.path().join("packages/ui/src/Card.tsx"),
        "import { Button } from './Button';\nexport const Card = () => <div><Button /> changed</div>;\n",
    )
    .unwrap();

    // The per-file breakdown is exposed in JSON as `roots`.
    let output = AssertCommand::cargo_bin("blast-radius")
        .unwrap()
        .current_dir(repo.path())
        .args([
            "--repo-root",
            repo.path().to_str().unwrap(),
            "--format",
            "json",
            "diff",
            "HEAD",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let json: Value = serde_json::from_slice(&output).unwrap();
    let roots = json["roots"].as_array().unwrap();
    assert_eq!(roots.len(), 2);
    assert!(roots.iter().all(|root| root["affected"].is_number()));

    // And the tree output gets a dedicated per-file section.
    AssertCommand::cargo_bin("blast-radius")
        .unwrap()
        .current_dir(repo.path())
        .args(["--repo-root", repo.path().to_str().unwrap(), "diff", "HEAD"])
        .assert()
        .success()
        .stdout(predicate::str::contains("IMPACT BY CHANGED FILE"))
        .stdout(predicate::str::contains("changed files"))
        .stdout(predicate::str::contains("impacted file"));
}

#[test]
fn file_mode_skips_unparseable_files_and_reports_them() {
    let repo = setup_repo();
    fs::create_dir_all(repo.path().join("src")).unwrap();
    fs::write(
        repo.path().join("src").join("template.js"),
        "export default makeThing({{{placeholder}}});\n",
    )
    .unwrap();
    fs::write(
        repo.path().join("src").join("index.js"),
        "export const ok = () => null;\n",
    )
    .unwrap();

    let output = AssertCommand::cargo_bin("blast-radius")
        .unwrap()
        .current_dir(repo.path())
        .args([
            "--repo-root",
            repo.path().to_str().unwrap(),
            "--format",
            "json",
            "file",
            "src/index.js",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let json: Value = serde_json::from_slice(&output).unwrap();
    assert_eq!(json["summary"]["parse_failures"].as_u64().unwrap(), 1);
    let warnings = json["warnings"].as_array().unwrap();
    assert!(warnings.iter().any(|warning| {
        warning
            .as_str()
            .is_some_and(|warning| warning.contains("could not be parsed"))
    }));
}

#[test]
fn chakra_ui_example_analyzes_real_world_repo() {
    let repo = chakra_example_root();

    let output = AssertCommand::cargo_bin("blast-radius")
        .unwrap()
        .current_dir(&repo)
        .args([
            "--repo-root",
            repo.to_str().unwrap(),
            "--format",
            "json",
            "file",
            "packages/react/src/components/button/button.tsx",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let json: Value = serde_json::from_slice(&output).unwrap();
    assert_eq!(json["summary"]["parse_failures"].as_u64().unwrap(), 0);
    assert!(json["summary"]["total_affected_files"].as_u64().unwrap() > 100);
    assert!(json["summary"]["unresolved_imports"].as_u64().unwrap() <= 1);
    let labels: Vec<String> = json["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|node| node["label"].as_str().map(ToOwned::to_owned))
        .collect();
    assert!(
        labels
            .iter()
            .any(|label| label.contains("packages/react/__stories__/button.stories.tsx"))
    );
    assert!(
        labels
            .iter()
            .any(|label| label.contains("apps/compositions/src/examples/button-basic.tsx"))
    );
}

#[test]
fn vite_example_analyzes_real_world_repo() {
    let repo = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("examples/vite-react-ts");

    let output = AssertCommand::cargo_bin("blast-radius")
        .unwrap()
        .current_dir(&repo)
        .args([
            "--repo-root",
            repo.to_str().unwrap(),
            "--format",
            "json",
            "file",
            "src/App.tsx",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let json: Value = serde_json::from_slice(&output).unwrap();
    assert_eq!(json["summary"]["parse_failures"].as_u64().unwrap(), 0);
    assert_eq!(json["summary"]["total_affected_files"].as_u64().unwrap(), 2);
    let labels: Vec<String> = json["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|node| node["label"].as_str().map(ToOwned::to_owned))
        .collect();
    assert!(labels.iter().any(|label| label == "src/main.tsx"));
}

#[cfg(feature = "python")]
#[test]
fn python_file_mode_reports_transitive_blast_radius() {
    let repo = python_fixture_root();

    let output = AssertCommand::cargo_bin("blast-radius")
        .unwrap()
        .current_dir(&repo)
        .args([
            "--repo-root",
            repo.to_str().unwrap(),
            "--format",
            "json",
            "file",
            "app/utils/formatting.py",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let json: Value = serde_json::from_slice(&output).unwrap();
    assert_eq!(json["summary"]["parse_failures"].as_u64().unwrap(), 0);
    assert_eq!(json["summary"]["unresolved_imports"].as_u64().unwrap(), 0);
    let labels: Vec<String> = json["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|node| node["label"].as_str().map(ToOwned::to_owned))
        .collect();

    assert!(labels.iter().any(|label| label == "app/services/email.py"));
    assert!(labels.iter().any(|label| label == "app/main.py"));
    assert!(labels.iter().any(|label| label == "tests/test_main.py"));
}

#[cfg(feature = "python")]
#[test]
fn python_export_mode_tracks_reexports() {
    let repo = python_fixture_root();

    let output = AssertCommand::cargo_bin("blast-radius")
        .unwrap()
        .current_dir(&repo)
        .args([
            "--repo-root",
            repo.to_str().unwrap(),
            "--format",
            "json",
            "export",
            "app/services/email.py",
            "send_email",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let json: Value = serde_json::from_slice(&output).unwrap();
    assert_eq!(json["summary"]["parse_failures"].as_u64().unwrap(), 0);
    assert_eq!(json["summary"]["unresolved_imports"].as_u64().unwrap(), 0);
    let labels: Vec<String> = json["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|node| node["label"].as_str().map(ToOwned::to_owned))
        .collect();

    assert!(
        labels
            .iter()
            .any(|label| label.contains("app/services/__init__.py#send_email"))
    );
    assert!(labels.iter().any(|label| label == "app/main.py"));
}

#[cfg(feature = "python")]
#[test]
fn fastapi_example_analyzes_real_world_python_repo() {
    let repo = fastapi_example_root();

    let output = AssertCommand::cargo_bin("blast-radius")
        .unwrap()
        .current_dir(&repo)
        .args([
            "--repo-root",
            repo.to_str().unwrap(),
            "--format",
            "json",
            "file",
            "fastapi/applications.py",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let json: Value = serde_json::from_slice(&output).unwrap();
    assert_eq!(json["summary"]["parse_failures"].as_u64().unwrap(), 0);
    assert!(json["summary"]["unresolved_imports"].as_u64().unwrap() <= 1);
    assert!(json["source_file_count"].as_u64().unwrap() > 1_000);
    assert!(json["summary"]["total_affected_files"].as_u64().unwrap() > 600);
}
