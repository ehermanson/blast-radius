use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use ignore::WalkBuilder;
use jsonc_parser::{ParseOptions, parse_to_serde_value};
use serde::Deserialize;

const JAVASCRIPT_SOURCE_EXTENSIONS: &[&str] =
    &["js", "jsx", "mjs", "cjs", "ts", "tsx", "mts", "cts"];

#[derive(Debug, Clone)]
pub struct RepoContext {
    pub repo_root: PathBuf,
    pub source_files: Vec<PathBuf>,
    pub tsconfigs: Vec<TsConfigPath>,
    pub package_jsons: Vec<PathBuf>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct TsConfigPath {
    pub path: PathBuf,
    pub compiler_options: TsCompilerOptions,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct TsCompilerOptions {
    #[serde(default)]
    pub base_url: Option<String>,
    #[serde(default)]
    pub paths: BTreeMap<String, Vec<String>>,
}

#[derive(Debug, Deserialize)]
struct TsConfigFile {
    #[serde(default, rename = "compilerOptions")]
    compiler_options: TsCompilerOptions,
}

impl RepoContext {
    pub fn discover(repo_root: &Path) -> Result<Self> {
        let repo_root = repo_root
            .canonicalize()
            .with_context(|| format!("failed to resolve repo root {}", repo_root.display()))?;

        let mut source_files = Vec::new();
        let mut tsconfigs = Vec::new();
        let mut package_jsons = Vec::new();
        let mut warnings = Vec::new();

        let walker = WalkBuilder::new(&repo_root)
            .hidden(false)
            .git_ignore(true)
            .git_exclude(true)
            .git_global(true)
            .filter_entry(|entry| {
                let name = entry.file_name().to_string_lossy();
                !matches!(
                    name.as_ref(),
                    ".git" | "node_modules" | "dist" | "build" | "coverage" | ".next" | ".turbo"
                )
            })
            .build();

        for entry in walker {
            let entry = entry?;
            if !entry.file_type().is_some_and(|kind| kind.is_file()) {
                continue;
            }

            let path = entry.into_path();
            match path.file_name().and_then(|name| name.to_str()) {
                Some("tsconfig.json") => match load_tsconfig(&path) {
                    Ok(config) => tsconfigs.push(config),
                    Err(error) => warnings.push(format!("{error:#}")),
                },
                Some("package.json") => package_jsons.push(path.clone()),
                _ => {}
            }

            if is_source_file(&path) {
                source_files.push(path);
            }
        }

        source_files.sort();
        tsconfigs.sort_by(|a, b| a.path.cmp(&b.path));
        package_jsons.sort();

        Ok(Self {
            repo_root,
            source_files,
            tsconfigs,
            package_jsons,
            warnings,
        })
    }
}

fn load_tsconfig(path: &Path) -> Result<TsConfigPath> {
    let contents = fs::read_to_string(path)
        .with_context(|| format!("failed to read tsconfig {}", path.display()))?;
    let value: serde_json::Value = parse_to_serde_value(
        &contents,
        &ParseOptions {
            allow_comments: true,
            allow_loose_object_property_names: false,
            allow_trailing_commas: true,
            allow_missing_commas: false,
            allow_single_quoted_strings: false,
            allow_hexadecimal_numbers: false,
            allow_unary_plus_numbers: false,
        },
    )
    .with_context(|| format!("failed to parse tsconfig {}", path.display()))?;
    let parsed: TsConfigFile = serde_json::from_value(value)
        .with_context(|| format!("failed to decode tsconfig {}", path.display()))?;
    Ok(TsConfigPath {
        path: path.to_path_buf(),
        compiler_options: parsed.compiler_options,
    })
}

fn is_source_file(path: &Path) -> bool {
    let Some(ext) = path.extension().and_then(|ext| ext.to_str()) else {
        return false;
    };

    JAVASCRIPT_SOURCE_EXTENSIONS.contains(&ext)
        || is_python_source_extension(ext)
        || is_rust_source_extension(ext)
        || is_vue_source_extension(ext)
        || is_svelte_source_extension(ext)
        || is_ruby_source_extension(ext)
        || is_java_source_extension(ext)
}

#[cfg(feature = "python")]
fn is_python_source_extension(ext: &str) -> bool {
    ext == "py"
}

#[cfg(not(feature = "python"))]
fn is_python_source_extension(_: &str) -> bool {
    false
}

#[cfg(feature = "rust")]
fn is_rust_source_extension(ext: &str) -> bool {
    ext == "rs"
}

#[cfg(not(feature = "rust"))]
fn is_rust_source_extension(_: &str) -> bool {
    false
}

#[cfg(feature = "vue")]
fn is_vue_source_extension(ext: &str) -> bool {
    ext == "vue"
}

#[cfg(not(feature = "vue"))]
fn is_vue_source_extension(_: &str) -> bool {
    false
}

#[cfg(feature = "svelte")]
fn is_svelte_source_extension(ext: &str) -> bool {
    ext == "svelte"
}

#[cfg(not(feature = "svelte"))]
fn is_svelte_source_extension(_: &str) -> bool {
    false
}

#[cfg(feature = "ruby")]
fn is_ruby_source_extension(ext: &str) -> bool {
    ext == "rb"
}

#[cfg(not(feature = "ruby"))]
fn is_ruby_source_extension(_: &str) -> bool {
    false
}

#[cfg(feature = "java")]
fn is_java_source_extension(ext: &str) -> bool {
    ext == "java"
}

#[cfg(not(feature = "java"))]
fn is_java_source_extension(_: &str) -> bool {
    false
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::tempdir;

    use super::RepoContext;

    #[test]
    fn discovers_source_files_and_tsconfig() {
        let dir = tempdir().unwrap();
        fs::create_dir_all(dir.path().join("src")).unwrap();
        fs::write(
            dir.path().join("tsconfig.json"),
            r#"{"compilerOptions":{"baseUrl":".","paths":{"@ui/*":["packages/ui/*"]}}}"#,
        )
        .unwrap();
        fs::write(
            dir.path().join("src").join("Button.tsx"),
            "export const Button = () => null;",
        )
        .unwrap();
        fs::write(
            dir.path().join("src").join("legacy.mjs"),
            "export const legacy = true;",
        )
        .unwrap();
        fs::write(dir.path().join("src").join("server.cts"), "export = {};").unwrap();
        fs::write(
            dir.path().join("src").join("helper.py"),
            "def helper(): pass",
        )
        .unwrap();
        fs::write(dir.path().join("src").join("lib.rs"), "pub fn helper() {}").unwrap();
        fs::write(
            dir.path().join("src").join("Button.vue"),
            "<script setup>import x from './x'</script>",
        )
        .unwrap();
        fs::write(
            dir.path().join("src").join("Card.svelte"),
            "<script>import x from './x'</script>",
        )
        .unwrap();
        fs::write(dir.path().join("src").join("user.rb"), "class User; end").unwrap();
        fs::write(dir.path().join("src").join("User.java"), "class User {}").unwrap();
        fs::write(dir.path().join("package.json"), r#"{"name":"fixture"}"#).unwrap();

        let repo = RepoContext::discover(dir.path()).unwrap();

        let mut expected = 3;
        if cfg!(feature = "python") {
            expected += 1;
        }
        if cfg!(feature = "rust") {
            expected += 1;
        }
        if cfg!(feature = "vue") {
            expected += 1;
        }
        if cfg!(feature = "svelte") {
            expected += 1;
        }
        if cfg!(feature = "ruby") {
            expected += 1;
        }
        if cfg!(feature = "java") {
            expected += 1;
        }
        assert_eq!(repo.source_files.len(), expected);
        assert_eq!(repo.tsconfigs.len(), 1);
        assert_eq!(repo.package_jsons.len(), 1);
    }

    #[test]
    fn reports_invalid_tsconfig_as_warning() {
        let dir = tempdir().unwrap();
        fs::create_dir_all(dir.path().join("src")).unwrap();
        fs::write(dir.path().join("tsconfig.json"), "{ invalid json").unwrap();
        fs::write(
            dir.path().join("src").join("Button.tsx"),
            "export const Button = () => null;",
        )
        .unwrap();

        let repo = RepoContext::discover(dir.path()).unwrap();

        assert_eq!(repo.source_files.len(), 1);
        assert!(repo.tsconfigs.is_empty());
        assert!(
            repo.warnings
                .iter()
                .any(|warning| warning.contains("failed to parse tsconfig")),
            "invalid tsconfig should be reported as a discovery warning"
        );
    }

    #[cfg(feature = "python")]
    #[test]
    fn discovers_python_sources_when_enabled() {
        let dir = tempdir().unwrap();
        fs::create_dir_all(dir.path().join("src")).unwrap();
        fs::write(
            dir.path().join("src").join("helper.py"),
            "def helper(): pass",
        )
        .unwrap();

        let repo = RepoContext::discover(dir.path()).unwrap();

        assert_eq!(repo.source_files.len(), 1);
    }

    #[cfg(feature = "rust")]
    #[test]
    fn discovers_rust_sources_when_enabled() {
        let dir = tempdir().unwrap();
        fs::create_dir_all(dir.path().join("src")).unwrap();
        fs::write(dir.path().join("src").join("lib.rs"), "pub fn helper() {}").unwrap();

        let repo = RepoContext::discover(dir.path()).unwrap();

        assert_eq!(repo.source_files.len(), 1);
    }

    #[cfg(feature = "vue")]
    #[test]
    fn discovers_vue_sources_when_enabled() {
        let dir = tempdir().unwrap();
        fs::create_dir_all(dir.path().join("src")).unwrap();
        fs::write(dir.path().join("src").join("Button.vue"), "<template />").unwrap();

        let repo = RepoContext::discover(dir.path()).unwrap();

        assert_eq!(repo.source_files.len(), 1);
    }

    #[cfg(feature = "svelte")]
    #[test]
    fn discovers_svelte_sources_when_enabled() {
        let dir = tempdir().unwrap();
        fs::create_dir_all(dir.path().join("src")).unwrap();
        fs::write(
            dir.path().join("src").join("Card.svelte"),
            "<script></script>",
        )
        .unwrap();

        let repo = RepoContext::discover(dir.path()).unwrap();

        assert_eq!(repo.source_files.len(), 1);
    }

    #[cfg(feature = "ruby")]
    #[test]
    fn discovers_ruby_sources_when_enabled() {
        let dir = tempdir().unwrap();
        fs::create_dir_all(dir.path().join("lib")).unwrap();
        fs::write(dir.path().join("lib").join("user.rb"), "class User; end").unwrap();

        let repo = RepoContext::discover(dir.path()).unwrap();

        assert_eq!(repo.source_files.len(), 1);
    }

    #[cfg(feature = "java")]
    #[test]
    fn discovers_java_sources_when_enabled() {
        let dir = tempdir().unwrap();
        fs::create_dir_all(dir.path().join("src")).unwrap();
        fs::write(dir.path().join("src").join("User.java"), "class User {}").unwrap();

        let repo = RepoContext::discover(dir.path()).unwrap();

        assert_eq!(repo.source_files.len(), 1);
    }
}
