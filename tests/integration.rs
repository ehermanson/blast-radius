use std::fs;
use std::path::{Path, PathBuf};

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

#[cfg(feature = "rust")]
fn rust_fixture_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/rust")
}

#[cfg(all(feature = "vue", feature = "svelte"))]
fn component_fixture_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/components")
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

fn run_json(repo: &Path, args: &[&str]) -> Value {
    let mut command = AssertCommand::cargo_bin("blast-radius").unwrap();
    command
        .current_dir(repo)
        .args(["--repo-root", repo.to_str().unwrap(), "--format", "json"])
        .args(args);

    let output = command.assert().success().get_output().stdout.clone();
    serde_json::from_slice(&output).unwrap()
}

fn node_labels(json: &Value) -> Vec<String> {
    json["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|node| node["label"].as_str().map(ToOwned::to_owned))
        .collect()
}

fn label_count(labels: &[String], expected: &str) -> usize {
    labels
        .iter()
        .filter(|label| label.as_str() == expected)
        .count()
}

/// Hops from the changed file for a given file node in the blast radius.
fn depth_of(json: &Value, label: &str) -> Option<u64> {
    json["nodes"].as_array().unwrap().iter().find_map(|node| {
        (node["label"] == label && node["kind"] == "file").then(|| node["depth"].as_u64())?
    })
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
fn export_mode_keeps_default_and_named_imports_separate() {
    let repo = tempdir().unwrap();
    fs::create_dir_all(repo.path().join("src")).unwrap();
    fs::write(
        repo.path().join("src/source.ts"),
        "export default function DefaultThing() {}\nexport function namedThing() {}\n",
    )
    .unwrap();
    fs::write(
        repo.path().join("src/default-consumer.ts"),
        "import DefaultThing from './source';\nexport const useDefault = () => DefaultThing();\n",
    )
    .unwrap();
    fs::write(
        repo.path().join("src/named-consumer.ts"),
        "import { namedThing } from './source';\nexport const useNamed = () => namedThing();\n",
    )
    .unwrap();

    let named = run_json(repo.path(), &["export", "src/source.ts", "namedThing"]);
    let labels = node_labels(&named);
    assert!(labels.iter().any(|label| label == "src/named-consumer.ts"));
    assert!(
        !labels
            .iter()
            .any(|label| label == "src/default-consumer.ts")
    );
    assert!(
        named["edges"].as_array().unwrap().iter().any(|edge| {
            edge["to"]
                .as_str()
                .is_some_and(|to| to.contains("src/named-consumer.ts"))
                && edge["kind"] == "imports_named"
        }),
        "ordinary named import usage should not be reported as JSX component usage"
    );
    assert!(
        !named["edges"].as_array().unwrap().iter().any(|edge| {
            edge["to"]
                .as_str()
                .is_some_and(|to| to.contains("src/named-consumer.ts"))
                && edge["kind"] == "uses_jsx_component"
        }),
        "ordinary named import usage was incorrectly reported as JSX component usage"
    );

    let default = run_json(repo.path(), &["export", "src/source.ts", "default"]);
    let labels = node_labels(&default);
    assert!(
        labels
            .iter()
            .any(|label| label == "src/default-consumer.ts")
    );
    assert!(!labels.iter().any(|label| label == "src/named-consumer.ts"));
}

/// The core symbol-aware promise, end to end on the reported blast radius:
/// changing `Card.tsx` reaches its real consumers but NOT files that import
/// through the same `index.ts` barrel while using only `Button`. A file-level
/// reachability tool would wrongly report every barrel consumer; blast-radius
/// must prune by which symbol actually flows. Depths are asserted so the
/// transitive chain (Card -> index -> PromoCard -> App) is exactly right.
#[test]
fn file_mode_prunes_barrel_consumers_that_use_other_symbols() {
    let repo = fixture_root();
    let json = run_json(&repo, &["file", "packages/ui/src/Card.tsx"]);
    let labels = node_labels(&json);

    // index.ts re-exports Card (`export * from "./Card"`); PromoCard uses Card;
    // App uses PromoCard. Exact depths prove the traversal, not just presence.
    assert_eq!(depth_of(&json, "packages/ui/src/index.ts"), Some(1));
    assert_eq!(
        depth_of(&json, "apps/storefront/src/PromoCard.tsx"),
        Some(2)
    );
    assert_eq!(depth_of(&json, "apps/storefront/src/App.tsx"), Some(3));

    // Button-only consumers reached through the SAME barrel must be pruned.
    for excluded in [
        "packages/ui/src/Toolbar.tsx",
        "packages/ui/src/Button.tsx",
        "apps/storefront/src/LegacyButtonCard.jsx",
    ] {
        assert!(
            !labels.iter().any(|label| label == excluded),
            "changing Card must NOT reach {excluded} (it uses only Button); got {labels:?}"
        );
    }

    // Exactly three downstream files — no over-reporting.
    assert_eq!(json["summary"]["total_affected_files"].as_u64().unwrap(), 3);
}

/// Import cycles must not hang the traversal or mis-depth nodes. With a <-> b
/// cyclic and c -> a, changing b reaches a (depth 1) and c (depth 2), and the
/// walk terminates.
#[test]
fn file_mode_terminates_and_is_correct_on_import_cycles() {
    let repo = tempdir().unwrap();
    fs::create_dir_all(repo.path().join("src")).unwrap();
    fs::write(repo.path().join("package.json"), r#"{"name":"cyc"}"#).unwrap();
    fs::write(
        repo.path().join("src/a.ts"),
        "import { b } from './b';\nexport const a = () => b();\n",
    )
    .unwrap();
    fs::write(
        repo.path().join("src/b.ts"),
        "import { a } from './a';\nexport const b = () => a();\n",
    )
    .unwrap();
    fs::write(
        repo.path().join("src/c.ts"),
        "import { a } from './a';\nexport const c = () => a();\n",
    )
    .unwrap();

    let json = run_json(repo.path(), &["file", "src/b.ts"]);
    assert_eq!(depth_of(&json, "src/a.ts"), Some(1));
    assert_eq!(depth_of(&json, "src/c.ts"), Some(2));
    assert_eq!(json["summary"]["total_affected_files"].as_u64().unwrap(), 2);
}

/// A file reachable by several paths is reported once, at its shortest depth.
/// d is imported by e and f (depth 1), both imported by g — g must appear once
/// at depth 2.
#[test]
fn file_mode_reports_diamond_dependents_once_at_shortest_depth() {
    let repo = tempdir().unwrap();
    fs::create_dir_all(repo.path().join("src")).unwrap();
    fs::write(repo.path().join("package.json"), r#"{"name":"diamond"}"#).unwrap();
    fs::write(repo.path().join("src/d.ts"), "export const d = 1;\n").unwrap();
    fs::write(
        repo.path().join("src/e.ts"),
        "import { d } from './d';\nexport const e = d;\n",
    )
    .unwrap();
    fs::write(
        repo.path().join("src/f.ts"),
        "import { d } from './d';\nexport const f = d;\n",
    )
    .unwrap();
    fs::write(
        repo.path().join("src/g.ts"),
        "import { e } from './e';\nimport { f } from './f';\nexport const g = e + f;\n",
    )
    .unwrap();

    let json = run_json(repo.path(), &["file", "src/d.ts"]);
    assert_eq!(depth_of(&json, "src/e.ts"), Some(1));
    assert_eq!(depth_of(&json, "src/f.ts"), Some(1));
    assert_eq!(depth_of(&json, "src/g.ts"), Some(2));
    assert_eq!(label_count(&node_labels(&json), "src/g.ts"), 1);
    assert_eq!(json["summary"]["total_affected_files"].as_u64().unwrap(), 3);
}

/// `vi.mock("./real")` makes the test depend on the real module — changing the
/// real module must reach the mocking test in the blast radius (and nothing
/// unrelated).
#[test]
fn file_mode_reaches_test_files_that_mock_the_module() {
    let repo = tempdir().unwrap();
    fs::create_dir_all(repo.path().join("src")).unwrap();
    fs::write(repo.path().join("package.json"), r#"{"name":"mock-reach"}"#).unwrap();
    fs::write(
        repo.path().join("src/real.ts"),
        "export const realThing = 1;\n",
    )
    .unwrap();
    fs::write(
        repo.path().join("src/real.test.ts"),
        "import { vi } from \"vitest\";\nvi.mock(\"./real\");\n",
    )
    .unwrap();
    fs::write(
        repo.path().join("src/other.ts"),
        "export const unrelated = 2;\n",
    )
    .unwrap();

    let json = run_json(repo.path(), &["file", "src/real.ts"]);
    let labels = node_labels(&json);
    assert!(
        labels.iter().any(|label| label == "src/real.test.ts"),
        "changing the real module should reach the test that mocks it; got {labels:?}"
    );
    assert!(
        !labels.iter().any(|label| label == "src/other.ts"),
        "unrelated files must not be in the blast radius; got {labels:?}"
    );
    assert_eq!(json["summary"]["total_affected_files"].as_u64().unwrap(), 1);
}

#[test]
fn export_mode_tracks_namespace_member_usage() {
    let repo = tempdir().unwrap();
    fs::create_dir_all(repo.path().join("src")).unwrap();
    fs::write(
        repo.path().join("src/source.ts"),
        "export function alpha() {}\nexport function beta() {}\n",
    )
    .unwrap();
    fs::write(
        repo.path().join("src/consumer.ts"),
        "import * as source from './source';\nexport const useAlpha = () => source.alpha();\n",
    )
    .unwrap();

    let alpha = run_json(repo.path(), &["export", "src/source.ts", "alpha"]);
    assert!(
        node_labels(&alpha)
            .iter()
            .any(|label| label == "src/consumer.ts")
    );

    let beta = run_json(repo.path(), &["export", "src/source.ts", "beta"]);
    assert!(
        !node_labels(&beta)
            .iter()
            .any(|label| label == "src/consumer.ts")
    );
}

#[test]
fn export_mode_follows_star_reexport_chains() {
    let repo = tempdir().unwrap();
    fs::create_dir_all(repo.path().join("src")).unwrap();
    fs::write(
        repo.path().join("src/source.ts"),
        "export const target = 1;\n",
    )
    .unwrap();
    fs::write(
        repo.path().join("src/barrel-one.ts"),
        "export * from './source';\n",
    )
    .unwrap();
    fs::write(
        repo.path().join("src/barrel-two.ts"),
        "export * from './barrel-one';\n",
    )
    .unwrap();
    fs::write(
        repo.path().join("src/app.ts"),
        "import { target } from './barrel-two';\nexport const value = target;\n",
    )
    .unwrap();

    let json = run_json(repo.path(), &["export", "src/source.ts", "target"]);
    let labels = node_labels(&json);
    assert!(labels.iter().any(|label| label == "src/barrel-one.ts"));
    assert!(labels.iter().any(|label| label == "src/barrel-two.ts"));
    assert!(labels.iter().any(|label| label == "src/app.ts"));
    assert_eq!(
        json["edges"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|edge| edge["kind"].as_str() == Some("reexports_star"))
            .count(),
        2
    );
}

#[test]
fn file_mode_reports_consumers_of_star_only_barrels() {
    let repo = tempdir().unwrap();
    fs::create_dir_all(repo.path().join("src")).unwrap();
    fs::write(
        repo.path().join("src/widget.ts"),
        "export const widget = () => null;\n",
    )
    .unwrap();
    // The barrel has no statically-enumerable exports of its own.
    fs::write(
        repo.path().join("src/barrel.ts"),
        "export * from './widget';\n",
    )
    .unwrap();
    fs::write(
        repo.path().join("src/consumer.ts"),
        "import { widget } from './barrel';\nexport const render = () => widget();\n",
    )
    .unwrap();

    // Changing the barrel itself must reach the consumer, not report "safe".
    let json = run_json(repo.path(), &["file", "src/barrel.ts"]);
    let labels = node_labels(&json);
    assert!(labels.iter().any(|label| label == "src/consumer.ts"));
    assert_eq!(json["summary"]["total_affected_files"].as_u64().unwrap(), 1);
    assert_eq!(
        json["summary"]["directly_affected_files"].as_u64().unwrap(),
        1
    );
}

#[test]
fn export_mode_rejects_unknown_export_names() {
    let repo = tempdir().unwrap();
    fs::create_dir_all(repo.path().join("src")).unwrap();
    fs::write(
        repo.path().join("src/source.ts"),
        "export const alpha = 1;\nexport const beta = 2;\n",
    )
    .unwrap();

    AssertCommand::cargo_bin("blast-radius")
        .unwrap()
        .current_dir(repo.path())
        .args([
            "--repo-root",
            repo.path().to_str().unwrap(),
            "export",
            "src/source.ts",
            "NoSuchExport",
        ])
        .assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains("export 'NoSuchExport' not found"))
        .stderr(predicate::str::contains("available exports: alpha, beta"));
}

#[test]
fn export_mode_warns_instead_of_failing_when_exports_are_not_enumerable() {
    let repo = tempdir().unwrap();
    fs::create_dir_all(repo.path().join("src")).unwrap();
    fs::write(
        repo.path().join("src/widget.ts"),
        "export const widget = () => null;\n",
    )
    .unwrap();
    // Star re-exports mean the barrel's export set isn't statically known, so
    // an unrecognized name proceeds with a warning rather than a hard error.
    fs::write(
        repo.path().join("src/barrel.ts"),
        "export * from './widget';\n",
    )
    .unwrap();

    let json = run_json(repo.path(), &["export", "src/barrel.ts", "NoSuchExport"]);
    let warnings = json["warnings"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|warning| warning.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(warnings.contains("not a statically-known export"));
    assert!(warnings.contains("src/barrel.ts"));
}

#[test]
fn files_mode_deduplicates_overlapping_multi_root_impact() {
    let repo = tempdir().unwrap();
    fs::create_dir_all(repo.path().join("src")).unwrap();
    fs::write(repo.path().join("src/source-a.ts"), "export const a = 1;\n").unwrap();
    fs::write(repo.path().join("src/source-b.ts"), "export const b = 2;\n").unwrap();
    fs::write(
        repo.path().join("src/app.ts"),
        "import { a } from './source-a';\nimport { b } from './source-b';\nexport const value = a + b;\n",
    )
    .unwrap();

    let json = run_json(
        repo.path(),
        &["files", "src/source-a.ts", "src/source-b.ts"],
    );
    let labels = node_labels(&json);
    assert_eq!(label_count(&labels, "src/app.ts"), 1);
    assert_eq!(json["roots"].as_array().unwrap().len(), 2);
    // Downstream impact only: the changed files themselves are not counted.
    assert_eq!(json["summary"]["total_affected_files"].as_u64().unwrap(), 1);
}

#[test]
fn total_affected_files_excludes_the_changed_file() {
    let repo = tempdir().unwrap();
    fs::create_dir_all(repo.path().join("src")).unwrap();
    fs::write(repo.path().join("src/source.ts"), "export const x = 1;\n").unwrap();
    fs::write(
        repo.path().join("src/app.ts"),
        "import { x } from './source';\nexport const v = x + 1;\n",
    )
    .unwrap();

    // One direct consumer: total equals direct + transitive, not roots + impact.
    let json = run_json(repo.path(), &["file", "src/source.ts"]);
    assert_eq!(json["summary"]["total_affected_files"].as_u64().unwrap(), 1);
    assert_eq!(
        json["summary"]["directly_affected_files"].as_u64().unwrap(),
        1
    );
    assert_eq!(
        json["summary"]["transitively_affected_files"]
            .as_u64()
            .unwrap(),
        0
    );

    // A file nothing depends on has a zero blast radius, so --fail-threshold 0
    // must pass.
    AssertCommand::cargo_bin("blast-radius")
        .unwrap()
        .current_dir(repo.path())
        .args([
            "--repo-root",
            repo.path().to_str().unwrap(),
            "--fail-threshold",
            "0",
            "file",
            "src/app.ts",
        ])
        .assert()
        .success();
}

#[test]
fn files_mode_deduplicates_repeated_inputs() {
    let repo = tempdir().unwrap();
    fs::create_dir_all(repo.path().join("src")).unwrap();
    fs::write(repo.path().join("src/source.ts"), "export const x = 1;\n").unwrap();
    fs::write(
        repo.path().join("src/app.ts"),
        "import { x } from './source';\nexport const v = x + 1;\n",
    )
    .unwrap();

    let json = run_json(
        repo.path(),
        &["files", "src/source.ts", "src/source.ts", "src/app.ts"],
    );
    let target_files = json["target"]["files"].as_array().unwrap();
    assert_eq!(target_files.len(), 2);
    assert_eq!(json["roots"].as_array().unwrap().len(), 2);
    assert_eq!(json["summary"]["skipped_inputs"].as_u64().unwrap(), 0);
}

#[test]
fn files_mode_skips_unknown_inputs_and_analyzes_the_rest() {
    let repo = tempdir().unwrap();
    fs::create_dir_all(repo.path().join("src")).unwrap();
    fs::write(repo.path().join("src/source-a.ts"), "export const a = 1;\n").unwrap();
    fs::write(repo.path().join("src/source-b.ts"), "export const b = 2;\n").unwrap();
    fs::write(
        repo.path().join("src/app.ts"),
        "import { a } from './source-a';\nimport { b } from './source-b';\nexport const value = a + b;\n",
    )
    .unwrap();
    // Exists on disk but is not a parsed source module.
    fs::write(repo.path().join("src/styles.css"), ".x { color: red; }\n").unwrap();

    // A hook batch can mix valid sources with a deleted/renamed path and a
    // non-source file; the valid inputs must still be analyzed.
    let json = run_json(
        repo.path(),
        &[
            "files",
            "src/source-a.ts",
            "src/does-not-exist.ts",
            "src/styles.css",
            "src/source-b.ts",
        ],
    );

    let labels = node_labels(&json);
    assert_eq!(label_count(&labels, "src/app.ts"), 1);
    assert_eq!(json["roots"].as_array().unwrap().len(), 2);
    assert_eq!(json["summary"]["total_affected_files"].as_u64().unwrap(), 1);
    assert_eq!(json["summary"]["skipped_inputs"].as_u64().unwrap(), 2);

    let warnings = json["warnings"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|warning| warning.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(warnings.contains("src/does-not-exist.ts"));
    assert!(warnings.contains("not found on disk"));
    assert!(warnings.contains("styles.css"));
    assert!(warnings.contains("not a recognized source file"));
}

#[test]
fn files_mode_with_all_unknown_inputs_reports_empty_with_warnings() {
    let repo = tempdir().unwrap();
    fs::create_dir_all(repo.path().join("src")).unwrap();
    fs::write(repo.path().join("src/real.ts"), "export const x = 1;\n").unwrap();
    fs::write(repo.path().join("src/styles.css"), ".x { color: red; }\n").unwrap();

    // Every passed path is unanalyzable: the run succeeds with an empty radius
    // rather than erroring, so a hook never fails the commit over input shape.
    let json = run_json(repo.path(), &["files", "src/gone.ts", "src/styles.css"]);

    assert_eq!(json["summary"]["total_affected_files"].as_u64().unwrap(), 0);
    assert_eq!(json["summary"]["skipped_inputs"].as_u64().unwrap(), 2);
    assert!(json["roots"].as_array().unwrap().is_empty());

    let warnings = json["warnings"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|warning| warning.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(warnings.contains("no recognized source files among 2 input paths"));
}

#[test]
fn fail_on_risk_gates_exit_code_at_or_above_tier() {
    let repo = tempdir().unwrap();
    fs::create_dir_all(repo.path().join("src")).unwrap();
    fs::write(repo.path().join("src/source.ts"), "export const x = 1;\n").unwrap();
    for name in ["a", "b", "c", "d"] {
        fs::write(
            repo.path().join(format!("src/{name}.ts")),
            "import { x } from './source';\nexport const v = x + 1;\n",
        )
        .unwrap();
    }

    // 4 downstream files in a single package => Moderate verdict, surfaced in JSON.
    let json = run_json(repo.path(), &["file", "src/source.ts"]);
    assert_eq!(json["summary"]["risk_tier"].as_str().unwrap(), "moderate");

    // A threshold at the verdict trips the gate (exit 2).
    AssertCommand::cargo_bin("blast-radius")
        .unwrap()
        .current_dir(repo.path())
        .args([
            "--repo-root",
            repo.path().to_str().unwrap(),
            "--fail-on-risk",
            "moderate",
            "file",
            "src/source.ts",
        ])
        .assert()
        .failure()
        .code(2);

    // A threshold stricter than the verdict passes.
    AssertCommand::cargo_bin("blast-radius")
        .unwrap()
        .current_dir(repo.path())
        .args([
            "--repo-root",
            repo.path().to_str().unwrap(),
            "--fail-on-risk",
            "risky",
            "file",
            "src/source.ts",
        ])
        .assert()
        .success();
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
        // Files are listed as bare names under their directory's header.
        .stdout(predicate::str::contains("apps/storefront"))
        .stdout(predicate::str::contains("App.tsx"));

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
fn files_mode_breaks_down_each_changed_file() {
    let repo = setup_repo();

    // Analyze two explicit files, each with its own downstream blast radius.
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
            "files",
            "packages/ui/src/Button.tsx",
            "packages/ui/src/Card.tsx",
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
        .args([
            "--repo-root",
            repo.path().to_str().unwrap(),
            "files",
            "packages/ui/src/Button.tsx",
            "packages/ui/src/Card.tsx",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("IMPACT BY INPUT FILE"))
        .stdout(predicate::str::contains("input files"))
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
fn explain_unresolved_groups_internal_imports_by_reason() {
    let repo = tempdir().unwrap();
    fs::create_dir_all(repo.path().join("src")).unwrap();
    fs::write(
        repo.path().join("tsconfig.json"),
        r#"{"compilerOptions":{"baseUrl":".","paths":{"@/*":["src/*"]}}}"#,
    )
    .unwrap();
    fs::write(repo.path().join("package.json"), r#"{"name":"fixture"}"#).unwrap();
    fs::write(
        repo.path().join("src/App.ts"),
        "import { missing } from './missing';
         import { alsoMissing } from '@/also-missing';
         import { notConfigured } from '#not-configured';
         export const app = [missing, alsoMissing, notConfigured];",
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
            "--explain-unresolved",
            "file",
            "src/App.ts",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let json: Value = serde_json::from_slice(&output).unwrap();
    assert_eq!(json["summary"]["unresolved_imports"].as_u64().unwrap(), 3);
    let warnings = json["warnings"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|warning| warning.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(warnings.contains("unresolved imports · relative or absolute path"));
    assert!(warnings.contains("./missing"));
    assert!(warnings.contains("unresolved imports · tsconfig paths or workspace package export"));
    assert!(warnings.contains("@/also-missing"));
    assert!(warnings.contains("unresolved imports · package.json imports"));
    assert!(warnings.contains("#not-configured"));
}

#[test]
fn dynamic_imports_resolve_through_aliases_and_report_dynamic_edges() {
    let repo = tempdir().unwrap();
    fs::create_dir_all(repo.path().join("src/pages")).unwrap();
    fs::write(
        repo.path().join("tsconfig.json"),
        r#"{"compilerOptions":{"baseUrl":".","paths":{"@/*":["src/*"]}}}"#,
    )
    .unwrap();
    fs::write(repo.path().join("package.json"), r#"{"name":"fixture"}"#).unwrap();
    fs::write(
        repo.path().join("src/pages/Dashboard.ts"),
        "export const Dashboard = () => null;",
    )
    .unwrap();
    fs::write(
        repo.path().join("src/routes.ts"),
        "export const loadDashboard = () => import('@/pages/Dashboard.js');",
    )
    .unwrap();

    let json = run_json(
        repo.path(),
        &["export", "src/pages/Dashboard.ts", "Dashboard"],
    );

    assert_eq!(json["summary"]["unresolved_imports"].as_u64().unwrap(), 0);
    assert!(
        node_labels(&json)
            .iter()
            .any(|label| label == "src/routes.ts")
    );
    assert!(json["edges"].as_array().unwrap().iter().any(|edge| {
        edge["kind"] == "imports_dynamic"
            && edge["to"]
                .as_str()
                .is_some_and(|to| to.contains("src/routes.ts"))
    }));
}

#[test]
fn chakra_ui_example_analyzes_real_world_repo() {
    let repo = chakra_example_root();
    if !repo.join("package.json").exists() {
        eprintln!(
            "skipping chakra_ui_example_analyzes_real_world_repo: examples/chakra-ui \
             not fetched (run scripts/fetch-examples.sh)"
        );
        return;
    }

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
    assert_eq!(json["summary"]["total_affected_files"].as_u64().unwrap(), 1);
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
fn python_submodule_import_reaches_the_submodule_file() {
    let repo = python_fixture_root();

    // `from app.utils import helpers` must bind to app/utils/helpers.py, not
    // just app/utils/__init__.py.
    let json = run_json(&repo, &["file", "app/utils/helpers.py"]);
    assert_eq!(json["summary"]["parse_failures"].as_u64().unwrap(), 0);
    assert_eq!(json["summary"]["unresolved_imports"].as_u64().unwrap(), 0);
    let labels = node_labels(&json);
    assert!(labels.iter().any(|label| label == "app/main.py"));
}

#[cfg(feature = "python")]
#[test]
fn python_symbol_import_from_package_init_still_resolves() {
    let repo = python_fixture_root();

    // `from app.services import send_email` is a plain symbol import and must
    // keep binding to app/services/__init__.py.
    let json = run_json(&repo, &["file", "app/services/__init__.py"]);
    assert_eq!(json["summary"]["unresolved_imports"].as_u64().unwrap(), 0);
    let labels = node_labels(&json);
    assert!(labels.iter().any(|label| label == "app/main.py"));
}

#[cfg(feature = "python")]
#[test]
fn python_conditional_imports_create_edges() {
    let repo = python_fixture_root();

    // app/compat.py imports app.models only under `if TYPE_CHECKING:`.
    let json = run_json(&repo, &["file", "app/models.py"]);
    assert_eq!(json["summary"]["unresolved_imports"].as_u64().unwrap(), 0);
    assert!(
        node_labels(&json)
            .iter()
            .any(|label| label == "app/compat.py")
    );

    // ...and app.utils.formatting only inside `try/except ImportError`.
    let json = run_json(&repo, &["file", "app/utils/formatting.py"]);
    assert!(
        node_labels(&json)
            .iter()
            .any(|label| label == "app/compat.py")
    );
}

#[cfg(feature = "python")]
#[test]
fn fastapi_example_analyzes_real_world_python_repo() {
    let repo = fastapi_example_root();
    if !repo.join("pyproject.toml").exists() {
        eprintln!(
            "skipping fastapi_example_analyzes_real_world_python_repo: examples/fastapi \
             not fetched (run scripts/fetch-examples.sh)"
        );
        return;
    }

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

#[cfg(feature = "rust")]
#[test]
fn rust_file_mode_reports_transitive_blast_radius() {
    let repo = rust_fixture_root();

    let output = AssertCommand::cargo_bin("blast-radius")
        .unwrap()
        .current_dir(&repo)
        .args([
            "--repo-root",
            repo.to_str().unwrap(),
            "--format",
            "json",
            "file",
            "src/utils/formatting.rs",
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

    assert!(labels.iter().any(|label| label == "src/services/email.rs"));
    assert!(labels.iter().any(|label| label == "src/services/mod.rs"));
    assert!(labels.iter().any(|label| label == "src/lib.rs"));
    assert!(labels.iter().any(|label| label == "src/main.rs"));
}

#[cfg(feature = "rust")]
#[test]
fn rust_export_mode_tracks_reexports() {
    let repo = rust_fixture_root();

    let output = AssertCommand::cargo_bin("blast-radius")
        .unwrap()
        .current_dir(&repo)
        .args([
            "--repo-root",
            repo.to_str().unwrap(),
            "--format",
            "json",
            "export",
            "src/services/email.rs",
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
            .any(|label| label.contains("src/services/mod.rs#send_email"))
    );
    assert!(
        labels
            .iter()
            .any(|label| label.contains("src/lib.rs#send_email"))
    );
}

#[cfg(feature = "rust")]
#[test]
fn rust_workspace_cross_crate_import_resolves() {
    let repo = rust_fixture_root();

    // crates/api imports `demo_core::models::Account`; the edge must cross
    // crate boundaries via the Cargo.toml package-name mapping.
    let json = run_json(&repo, &["file", "crates/core/src/models.rs"]);
    assert_eq!(json["summary"]["parse_failures"].as_u64().unwrap(), 0);
    assert_eq!(json["summary"]["unresolved_imports"].as_u64().unwrap(), 0);
    let labels = node_labels(&json);
    assert!(labels.iter().any(|label| label == "crates/core/src/lib.rs"));
    assert!(labels.iter().any(|label| label == "crates/api/src/main.rs"));
}

#[cfg(all(feature = "vue", feature = "svelte"))]
#[test]
fn component_file_mode_reports_vue_svelte_transitive_blast_radius() {
    let repo = component_fixture_root();

    let output = AssertCommand::cargo_bin("blast-radius")
        .unwrap()
        .current_dir(&repo)
        .args([
            "--repo-root",
            repo.to_str().unwrap(),
            "--format",
            "json",
            "file",
            "src/shared.ts",
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

    assert!(labels.iter().any(|label| label == "src/Button.vue"));
    assert!(labels.iter().any(|label| label == "src/Card.svelte"));
    assert!(labels.iter().any(|label| label == "src/App.ts"));
}

#[cfg(all(feature = "vue", feature = "svelte"))]
#[test]
fn component_file_mode_tracks_default_component_imports() {
    let repo = component_fixture_root();

    let output = AssertCommand::cargo_bin("blast-radius")
        .unwrap()
        .current_dir(&repo)
        .args([
            "--repo-root",
            repo.to_str().unwrap(),
            "--format",
            "json",
            "file",
            "src/Button.vue",
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

    assert!(labels.iter().any(|label| label == "src/Card.svelte"));
    assert!(labels.iter().any(|label| label == "src/App.ts"));
}

/// Regression: export-mode roots used to render "No downstream dependents
/// found" in the verbose cascade while the summary reported impacted files —
/// the root's out-edges hang off its `export:` node, which the renderer
/// never looked at.
#[test]
fn verbose_cascade_renders_export_mode_chains() {
    let repo = tempdir().unwrap();
    fs::create_dir_all(repo.path().join("src")).unwrap();
    fs::write(repo.path().join("package.json"), "{\"name\": \"repro\"}").unwrap();
    fs::write(
        repo.path().join("src/util.ts"),
        "export function helper(): string { return \"x\"; }\n",
    )
    .unwrap();
    fs::write(
        repo.path().join("src/consumer.ts"),
        "import { helper } from \"./util\";\nexport const value = helper();\n",
    )
    .unwrap();
    fs::write(
        repo.path().join("src/deep.ts"),
        "import { value } from \"./consumer\";\nexport const final = value;\n",
    )
    .unwrap();

    let output = AssertCommand::cargo_bin("blast-radius")
        .unwrap()
        .current_dir(repo.path())
        .args([
            "--repo-root",
            repo.path().to_str().unwrap(),
            "--verbose",
            "export",
            "src/util.ts",
            "helper",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let stdout = String::from_utf8(output).unwrap();

    assert!(
        !stdout.contains("No downstream dependents found"),
        "{stdout}"
    );
    let paths_section = stdout.split("CASCADE · PATHS").nth(1).unwrap_or("");
    assert!(paths_section.contains("consumer.ts"), "{stdout}");
    assert!(paths_section.contains("deep.ts"), "{stdout}");
}

/// Regression: the cascade used to treat barrels as transparent and fan every
/// barrel consumer out under every feeder file — claiming, e.g., that a file
/// importing only `Toolbar` from the barrel depends directly on `Card.tsx`.
/// Barrels now render as real nodes so every printed hop is a true edge.
#[test]
fn verbose_cascade_shows_barrels_without_false_direct_attribution() {
    let repo = setup_repo();

    let output = AssertCommand::cargo_bin("blast-radius")
        .unwrap()
        .current_dir(repo.path())
        .args([
            "--repo-root",
            repo.path().to_str().unwrap(),
            "--verbose",
            "export",
            "packages/ui/src/Button.tsx",
            "Button",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let stdout = String::from_utf8(output).unwrap();

    // The barrel is a real direct dependent (named re-export of Button) and
    // must appear in the overview, not be silently skipped.
    let overview = stdout.split("CASCADE · PATHS").next().unwrap_or("");
    assert!(
        overview
            .lines()
            .any(|line| line.contains("direct") && line.contains("index.ts")),
        "barrel missing from direct overview:\n{stdout}"
    );

    // In the paths tree, barrel consumers must hang under the barrel node,
    // never directly under a feeder file they do not import.
    let paths_section = stdout.split("CASCADE · PATHS").nth(1).unwrap_or("");
    let indent_of = |line: &str| line.find(['├', '└']).unwrap_or(usize::MAX);
    let lines: Vec<&str> = paths_section.lines().collect();
    for (index, line) in lines.iter().enumerate() {
        if !line.contains("LegacyButtonCard.jsx") {
            continue;
        }
        let has_barrel_ancestor = lines[..index]
            .iter()
            .rev()
            .take_while(|previous| indent_of(previous) != usize::MAX)
            .any(|previous| previous.contains("index.ts") && indent_of(previous) < indent_of(line));
        assert!(
            has_barrel_ancestor,
            "LegacyButtonCard attributed to a non-barrel parent:\n{stdout}"
        );
    }

    // Repeated subtrees collapse to a back-reference instead of re-printing.
    assert!(paths_section.contains("(paths shown above)"), "{stdout}");
}

/// `export * as ns from './x'` must stay member-precise: a change to one
/// export of the underlying module impacts only consumers that touch that
/// member through the namespace object (named-import, JSX, aliased re-export),
/// while wholesale users of the object are always impacted.
#[test]
fn namespace_reexport_tracks_member_usage_precisely() {
    let repo = tempdir().unwrap();
    fs::create_dir_all(repo.path().join("src")).unwrap();
    fs::write(repo.path().join("package.json"), r#"{"name":"ns-fixture"}"#).unwrap();
    fs::write(
        repo.path().join("src/widgets.ts"),
        "export const Button = 1;\nexport const Card = 2;\n",
    )
    .unwrap();
    fs::write(
        repo.path().join("src/barrel.ts"),
        "export * as UI from './widgets';\n",
    )
    .unwrap();
    fs::write(
        repo.path().join("src/button-user.ts"),
        "import { UI } from './barrel';\nconsole.log(UI.Button);\n",
    )
    .unwrap();
    fs::write(
        repo.path().join("src/card-user.tsx"),
        "import { UI } from './barrel';\nexport const App = () => <UI.Card />;\n",
    )
    .unwrap();
    fs::write(
        repo.path().join("src/alias-barrel.ts"),
        "export { UI as Widgets } from './barrel';\n",
    )
    .unwrap();
    fs::write(
        repo.path().join("src/alias-user.ts"),
        "import { Widgets } from './alias-barrel';\nconsole.log(Widgets.Button);\n",
    )
    .unwrap();
    fs::write(
        repo.path().join("src/wholesale-user.ts"),
        "import { UI } from './barrel';\nexport function dump(x: unknown) {}\ndump(UI);\n",
    )
    .unwrap();

    let json = run_json(repo.path(), &["export", "src/widgets.ts", "Button"]);
    let labels = node_labels(&json);
    for expected in [
        "src/barrel.ts",
        "src/button-user.ts",
        "src/alias-barrel.ts",
        "src/alias-user.ts",
        "src/wholesale-user.ts",
    ] {
        assert!(
            labels.iter().any(|label| label == expected),
            "Button change must reach {expected}; got {labels:?}"
        );
    }
    assert!(
        !labels.iter().any(|label| label == "src/card-user.tsx"),
        "Button change must not reach the UI.Card-only consumer; got {labels:?}"
    );

    let json = run_json(repo.path(), &["export", "src/widgets.ts", "Card"]);
    let labels = node_labels(&json);
    assert!(
        labels.iter().any(|label| label == "src/card-user.tsx"),
        "Card change must reach the JSX UI.Card consumer; got {labels:?}"
    );
    for excluded in ["src/button-user.ts", "src/alias-user.ts"] {
        assert!(
            !labels.iter().any(|label| label == excluded),
            "Card change must not reach {excluded}; got {labels:?}"
        );
    }
    assert!(
        labels.iter().any(|label| label == "src/wholesale-user.ts"),
        "wholesale namespace users depend on every member; got {labels:?}"
    );

    // Querying the namespace object itself impacts every member consumer.
    let json = run_json(repo.path(), &["export", "src/barrel.ts", "UI"]);
    let labels = node_labels(&json);
    for expected in ["src/button-user.ts", "src/card-user.tsx"] {
        assert!(
            labels.iter().any(|label| label == expected),
            "whole-object query must reach {expected}; got {labels:?}"
        );
    }
}
