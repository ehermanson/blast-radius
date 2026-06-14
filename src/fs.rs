use std::collections::{BTreeMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use ignore::WalkBuilder;
use jsonc_parser::{ParseOptions, parse_to_serde_value};
use serde::Deserialize;

#[derive(Debug, Clone)]
pub struct RepoContext {
    pub repo_root: PathBuf,
    pub source_files: Vec<PathBuf>,
    pub tsconfigs: Vec<TsConfigPath>,
    pub package_jsons: Vec<PathBuf>,
    /// Import-specifier substrings the repo asks not to count as unresolved
    /// (generated/virtual modules its tooling produces). From `.blast-radius.json`.
    pub ignore_unresolved: Vec<String>,
    pub warnings: Vec<String>,
}

/// Optional per-repo configuration loaded from `.blast-radius.json` at the repo
/// root. Lets a repo declare tooling-specific quirks the language-neutral core
/// shouldn't hardcode.
#[derive(Debug, Default, Deserialize)]
struct ProjectConfig {
    #[serde(default)]
    unresolved: UnresolvedConfig,
}

#[derive(Debug, Default, Deserialize)]
struct UnresolvedConfig {
    /// Import specifiers whose substring matches are not counted as unresolved.
    #[serde(default)]
    ignore: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct TsConfigPath {
    pub path: PathBuf,
    pub compiler_options: TsCompilerOptions,
}

/// Compiler options after following `extends` chains. Directory-relative
/// fields are resolved against the config file that declared them, so the
/// merged result is anchored to absolute directories.
#[derive(Debug, Clone, Default)]
pub struct TsCompilerOptions {
    /// Absolute directory `baseUrl` points at, when declared.
    pub base_dir: Option<PathBuf>,
    pub paths: BTreeMap<String, Vec<String>>,
    /// Absolute directory relative `paths` targets resolve against when no
    /// `baseUrl` is in effect: the directory of the config declaring `paths`.
    pub paths_dir: Option<PathBuf>,
}

impl TsCompilerOptions {
    pub fn has_aliases(&self) -> bool {
        self.base_dir.is_some() || !self.paths.is_empty()
    }
}

#[derive(Debug, Default, Deserialize)]
struct TsConfigFile {
    #[serde(default)]
    extends: Option<serde_json::Value>,
    #[serde(default, rename = "compilerOptions")]
    compiler_options: RawCompilerOptions,
    /// Project references (`"references": [{ "path": "../lib" }]`). Followed so
    /// referenced configs with non-standard names (tsconfig.lib.json) still
    /// contribute their aliases; directory references resolve to the
    /// `tsconfig.json` inside, which repo discovery usually finds anyway.
    #[serde(default)]
    references: Vec<TsConfigReference>,
}

#[derive(Debug, Deserialize)]
struct TsConfigReference {
    path: String,
}

#[derive(Debug, Default, Deserialize)]
struct RawCompilerOptions {
    #[serde(default, rename = "baseUrl")]
    base_url: Option<String>,
    #[serde(default)]
    paths: Option<BTreeMap<String, Vec<String>>>,
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

        let ignore_unresolved = match load_project_config(&repo_root) {
            Ok(config) => config.unresolved.ignore,
            Err(error) => {
                warnings.push(format!("{error:#}"));
                Vec::new()
            }
        };

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
            let entry = match entry {
                Ok(entry) => entry,
                Err(error) => {
                    warnings.push(format!("skipping unreadable path: {error}"));
                    continue;
                }
            };
            if !entry.file_type().is_some_and(|kind| kind.is_file()) {
                continue;
            }

            let path = entry.into_path();
            match path.file_name().and_then(|name| name.to_str()) {
                // jsconfig.json is tsconfig.json for JS projects (same format,
                // same `paths`/`baseUrl` aliases) — load it the same way so JS
                // repos' path aliases resolve.
                Some("tsconfig.json") | Some("jsconfig.json") => match load_tsconfig(&path) {
                    Ok(config) => {
                        // Nx/Vite scaffolds keep aliases in a sibling config the
                        // tsconfig.json only references; pull those in when the
                        // merged options carry no aliases of their own.
                        if !config.compiler_options.has_aliases() {
                            tsconfigs.extend(load_sibling_tsconfigs(&path, &mut warnings));
                        }
                        // Project references can point at configs with
                        // non-standard names (tsconfig.lib.json) the walk skips.
                        tsconfigs.extend(load_referenced_tsconfigs(&path, &mut warnings));
                        tsconfigs.push(config);
                    }
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
        // Referenced configs can duplicate walk-discovered ones.
        tsconfigs.dedup_by(|a, b| a.path == b.path);
        package_jsons.sort();

        Ok(Self {
            repo_root,
            source_files,
            tsconfigs,
            package_jsons,
            ignore_unresolved,
            warnings,
        })
    }
}

fn load_project_config(repo_root: &Path) -> Result<ProjectConfig> {
    let path = repo_root.join(".blast-radius.json");
    let contents = match fs::read_to_string(&path) {
        Ok(contents) => contents,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(ProjectConfig::default());
        }
        Err(error) => {
            return Err(anyhow::Error::new(error)
                .context(format!("failed to read config {}", path.display())));
        }
    };

    // Parsed as JSONC (comments + trailing commas), matching tsconfig handling.
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
    .with_context(|| format!("failed to parse config {}", path.display()))?;

    serde_json::from_value(value)
        .with_context(|| format!("failed to decode config {}", path.display()))
}

fn load_tsconfig(path: &Path) -> Result<TsConfigPath> {
    let mut visited = HashSet::new();
    Ok(TsConfigPath {
        path: path.to_path_buf(),
        compiler_options: load_tsconfig_options(path, &mut visited)?,
    })
}

fn parse_tsconfig_file(path: &Path) -> Result<TsConfigFile> {
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
    serde_json::from_value(value)
        .with_context(|| format!("failed to decode tsconfig {}", path.display()))
}

/// Load a config's options after following its `extends` chain. Child fields
/// shallow-override parents; `paths` replaces wholesale when redeclared.
fn load_tsconfig_options(path: &Path, visited: &mut HashSet<PathBuf>) -> Result<TsCompilerOptions> {
    let canonical = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    if !visited.insert(canonical) {
        // `extends` cycle: treat the revisited config as empty.
        return Ok(TsCompilerOptions::default());
    }

    let parsed = parse_tsconfig_file(path)?;

    let dir = path.parent().unwrap_or(Path::new("."));
    let mut merged = TsCompilerOptions::default();
    for specifier in extends_specifiers(&parsed.extends) {
        // Bare package specifiers (e.g. @tsconfig/node18) live in node_modules,
        // which isn't indexed; skip them silently.
        let Some(parent_path) = resolve_extends_target(dir, &specifier) else {
            continue;
        };
        let parent = load_tsconfig_options(&parent_path, visited)
            .with_context(|| format!("failed to load extended tsconfig from {}", path.display()))?;
        if parent.base_dir.is_some() {
            merged.base_dir = parent.base_dir;
        }
        if parent.paths_dir.is_some() {
            merged.paths = parent.paths;
            merged.paths_dir = parent.paths_dir;
        }
    }

    if let Some(base_url) = parsed.compiler_options.base_url {
        merged.base_dir = Some(crate::resolve::clean_path(&dir.join(base_url)));
    }
    if let Some(paths) = parsed.compiler_options.paths {
        merged.paths = paths;
        merged.paths_dir = Some(dir.to_path_buf());
    }

    Ok(merged)
}

fn extends_specifiers(value: &Option<serde_json::Value>) -> Vec<String> {
    match value {
        Some(serde_json::Value::String(specifier)) => vec![specifier.clone()],
        Some(serde_json::Value::Array(items)) => items
            .iter()
            .filter_map(|item| item.as_str().map(str::to_string))
            .collect(),
        _ => Vec::new(),
    }
}

/// Map an `extends` specifier to a config file on disk: `./`/`../` specifiers
/// and plain sibling filenames resolve relative to `dir` (with `.json` appended
/// when missing, as TypeScript does); anything else is a package specifier.
fn resolve_extends_target(dir: &Path, specifier: &str) -> Option<PathBuf> {
    let candidate = dir.join(specifier);
    if candidate.is_file() {
        return Some(candidate);
    }
    if !specifier.ends_with(".json") {
        let mut with_json = candidate.into_os_string();
        with_json.push(".json");
        let with_json = PathBuf::from(with_json);
        if with_json.is_file() {
            return Some(with_json);
        }
    }
    None
}

/// Follow `references` entries to their config files and load any that carry
/// aliases. Directory references resolve to the `tsconfig.json` inside (which
/// repo discovery usually finds on its own); file references pick up
/// non-standard names like `tsconfig.lib.json` that discovery skips. Chained
/// references are followed with cycle protection.
fn load_referenced_tsconfigs(tsconfig: &Path, warnings: &mut Vec<String>) -> Vec<TsConfigPath> {
    let mut configs = Vec::new();
    let mut visited = HashSet::new();
    let canonical = tsconfig
        .canonicalize()
        .unwrap_or_else(|_| tsconfig.to_path_buf());
    visited.insert(canonical);
    collect_referenced_tsconfigs(tsconfig, &mut visited, &mut configs, warnings);
    configs
}

fn collect_referenced_tsconfigs(
    tsconfig: &Path,
    visited: &mut HashSet<PathBuf>,
    configs: &mut Vec<TsConfigPath>,
    warnings: &mut Vec<String>,
) {
    let Ok(parsed) = parse_tsconfig_file(tsconfig) else {
        // Unreadable configs are already reported by the primary load.
        return;
    };
    let Some(dir) = tsconfig.parent() else {
        return;
    };

    for reference in &parsed.references {
        let target = crate::resolve::clean_path(&dir.join(&reference.path));
        let target = if target.is_dir() {
            target.join("tsconfig.json")
        } else {
            target
        };
        if !target.is_file() {
            continue;
        }
        let canonical = target.canonicalize().unwrap_or_else(|_| target.clone());
        if !visited.insert(canonical) {
            continue;
        }
        match load_tsconfig(&target) {
            Ok(config) => {
                if config.compiler_options.has_aliases() {
                    configs.push(config);
                }
                collect_referenced_tsconfigs(&target, visited, configs, warnings);
            }
            Err(error) => warnings.push(format!("{error:#}")),
        }
    }
}

/// Probe well-known sibling configs that hold path aliases in Nx and Vite
/// scaffold layouts, where tsconfig.json itself declares none.
fn load_sibling_tsconfigs(tsconfig: &Path, warnings: &mut Vec<String>) -> Vec<TsConfigPath> {
    let Some(dir) = tsconfig.parent() else {
        return Vec::new();
    };

    let mut configs = Vec::new();
    for name in ["tsconfig.base.json", "tsconfig.app.json"] {
        let path = dir.join(name);
        if !path.is_file() {
            continue;
        }
        match load_tsconfig(&path) {
            Ok(config) if config.compiler_options.has_aliases() => configs.push(config),
            Ok(_) => {}
            Err(error) => warnings.push(format!("{error:#}")),
        }
    }
    configs
}

fn is_source_file(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(crate::language::is_source_extension)
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
        assert_eq!(repo.source_files.len(), expected);
        assert_eq!(repo.tsconfigs.len(), 1);
        assert_eq!(repo.package_jsons.len(), 1);
    }

    #[test]
    fn loads_ignore_unresolved_from_project_config() {
        let dir = tempdir().unwrap();
        fs::create_dir_all(dir.path().join("src")).unwrap();
        fs::write(
            dir.path().join("src").join("App.tsx"),
            "export const App = () => null;",
        )
        .unwrap();
        fs::write(
            dir.path().join(".blast-radius.json"),
            r#"{ "unresolved": { "ignore": ["styled-system/css", ".velite"] } }"#,
        )
        .unwrap();

        let repo = RepoContext::discover(dir.path()).unwrap();

        assert_eq!(
            repo.ignore_unresolved,
            vec!["styled-system/css".to_string(), ".velite".to_string()]
        );
    }

    #[test]
    fn follows_project_references_to_nonstandard_config_names() {
        let dir = tempdir().unwrap();
        fs::create_dir_all(dir.path().join("lib/src")).unwrap();
        fs::write(
            dir.path().join("tsconfig.json"),
            r#"{ "files": [], "references": [{ "path": "./lib/tsconfig.lib.json" }] }"#,
        )
        .unwrap();
        fs::write(
            dir.path().join("lib/tsconfig.lib.json"),
            r#"{ "compilerOptions": { "baseUrl": ".", "paths": { "@lib/*": ["src/*"] } } }"#,
        )
        .unwrap();
        fs::write(dir.path().join("lib/src/util.ts"), "export const x = 1;").unwrap();

        let repo = RepoContext::discover(dir.path()).unwrap();

        assert_eq!(repo.tsconfigs.len(), 2, "referenced config must be loaded");
        assert!(
            repo.tsconfigs
                .iter()
                .any(|config| config.path.ends_with("tsconfig.lib.json")
                    && config.compiler_options.has_aliases()),
            "tsconfig.lib.json aliases must be available"
        );
    }

    #[test]
    fn defaults_ignore_unresolved_when_config_absent() {
        let dir = tempdir().unwrap();
        fs::create_dir_all(dir.path().join("src")).unwrap();
        fs::write(
            dir.path().join("src").join("App.tsx"),
            "export const App = () => null;",
        )
        .unwrap();

        let repo = RepoContext::discover(dir.path()).unwrap();

        assert!(repo.ignore_unresolved.is_empty());
    }

    #[test]
    fn reports_invalid_project_config_as_warning() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join(".blast-radius.json"), "{ not valid json").unwrap();

        let repo = RepoContext::discover(dir.path()).unwrap();

        assert!(repo.ignore_unresolved.is_empty());
        assert!(
            repo.warnings
                .iter()
                .any(|warning| warning.contains("failed to parse config")),
            "invalid config should be reported as a discovery warning"
        );
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
}
