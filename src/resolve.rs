use std::collections::HashSet;
use std::fs;
use std::path::{Component, Path, PathBuf};

use anyhow::{Context, Result};
use serde::Deserialize;

use crate::fs::{RepoContext, TsConfigPath};

const RESOLUTION_EXTENSIONS: &[&str] = &["ts", "tsx", "js", "jsx"];

#[derive(Debug, Clone)]
pub struct Resolver {
    repo_root: PathBuf,
    source_files: HashSet<PathBuf>,
    packages: Vec<PackageInfo>,
    tsconfigs: Vec<TsConfigPath>,
}

#[derive(Debug, Clone)]
pub struct PackageInfo {
    pub name: String,
    pub root: PathBuf,
    pub entry_candidates: Vec<PathBuf>,
}

#[derive(Debug, Clone)]
pub enum Resolution {
    Resolved(PathBuf),
    Unresolved,
}

#[derive(Debug, Deserialize)]
struct PackageJson {
    #[serde(default)]
    name: String,
    #[serde(default)]
    main: Option<String>,
    #[serde(default)]
    module: Option<String>,
    #[serde(default)]
    source: Option<String>,
    #[serde(default)]
    types: Option<String>,
}

impl Resolver {
    pub fn new(context: &RepoContext) -> Result<Self> {
        let mut packages = Vec::new();
        for package_json in &context.package_jsons {
            if let Some(package) = load_package_info(package_json)? {
                packages.push(package);
            }
        }

        Ok(Self {
            repo_root: context.repo_root.clone(),
            source_files: context.source_files.iter().cloned().collect(),
            packages,
            tsconfigs: context.tsconfigs.clone(),
        })
    }

    pub fn resolve(&self, importer: &Path, specifier: &str) -> Resolution {
        let importer = importer
            .canonicalize()
            .unwrap_or_else(|_| clean_path(importer));

        if specifier.starts_with('.') || specifier.starts_with('/') {
            return self.resolve_path(importer.parent().unwrap_or(&self.repo_root), specifier);
        }

        if let Some(path) = self.resolve_tsconfig_alias(&importer, specifier) {
            return Resolution::Resolved(path);
        }

        if let Some(path) = self.resolve_workspace_package(specifier) {
            return Resolution::Resolved(path);
        }

        Resolution::Unresolved
    }

    pub fn is_internal_specifier(&self, importer: &Path, specifier: &str) -> bool {
        if specifier.starts_with('.') || specifier.starts_with('/') {
            return true;
        }

        if let Some(tsconfig) = self.nearest_tsconfig(importer) {
            if tsconfig
                .compiler_options
                .paths
                .keys()
                .any(|pattern| match_alias(pattern, specifier).is_some())
            {
                return true;
            }
        }

        self.packages.iter().any(|package| {
            specifier == package.name || specifier.starts_with(&format!("{}/", package.name))
        })
    }

    fn resolve_tsconfig_alias(&self, importer: &Path, specifier: &str) -> Option<PathBuf> {
        let tsconfig = self.nearest_tsconfig(importer)?;
        let tsconfig_dir = tsconfig.path.parent()?;
        let base_dir = tsconfig
            .compiler_options
            .base_url
            .as_ref()
            .map(|base| clean_path(&tsconfig_dir.join(base)))
            .unwrap_or_else(|| tsconfig_dir.to_path_buf());

        for (pattern, targets) in &tsconfig.compiler_options.paths {
            let Some(captures) = match_alias(pattern, specifier) else {
                continue;
            };

            for target in targets {
                let candidate = apply_alias_target(target, &captures);
                if let Resolution::Resolved(resolved) = self.resolve_path(&base_dir, &candidate) {
                    return Some(resolved);
                }
            }
        }

        None
    }

    fn nearest_tsconfig(&self, importer: &Path) -> Option<&TsConfigPath> {
        self.tsconfigs
            .iter()
            .filter(|config| importer.starts_with(config.path.parent().unwrap_or(&self.repo_root)))
            .max_by_key(|config| config.path.components().count())
    }

    fn resolve_workspace_package(&self, specifier: &str) -> Option<PathBuf> {
        for package in &self.packages {
            if specifier == package.name {
                for candidate in &package.entry_candidates {
                    if let Some(resolved) = self.try_resolve_candidate(candidate) {
                        return Some(resolved);
                    }
                }
            }

            if let Some(rest) = specifier.strip_prefix(&format!("{}/", package.name)) {
                let direct = package.root.join(rest);
                if let Some(resolved) = self.try_resolve_candidate(&direct) {
                    return Some(resolved);
                }

                let src_direct = package.root.join("src").join(rest);
                if let Some(resolved) = self.try_resolve_candidate(&src_direct) {
                    return Some(resolved);
                }
            }
        }

        None
    }

    fn resolve_path(&self, base: &Path, specifier: &str) -> Resolution {
        let path = if specifier.starts_with('/') {
            clean_path(&self.repo_root.join(specifier.trim_start_matches('/')))
        } else {
            clean_path(&base.join(specifier))
        };

        self.try_resolve_candidate(&path)
            .map(Resolution::Resolved)
            .unwrap_or(Resolution::Unresolved)
    }

    fn try_resolve_candidate(&self, candidate: &Path) -> Option<PathBuf> {
        let candidate = clean_path(candidate);

        if self.source_files.contains(&candidate) {
            return Some(candidate);
        }

        if candidate.extension().is_some() && self.source_files.contains(&candidate) {
            return Some(candidate);
        }

        for extension in RESOLUTION_EXTENSIONS {
            let path = candidate.with_extension(extension);
            if self.source_files.contains(&path) {
                return Some(path);
            }
        }

        if candidate.is_dir() || !candidate.extension().is_some() {
            for extension in RESOLUTION_EXTENSIONS {
                let path = candidate.join(format!("index.{extension}"));
                if self.source_files.contains(&path) {
                    return Some(path);
                }
            }
        }

        None
    }
}

fn load_package_info(path: &Path) -> Result<Option<PackageInfo>> {
    let contents = fs::read_to_string(path)
        .with_context(|| format!("failed to read package.json {}", path.display()))?;
    let parsed: PackageJson = serde_json::from_str(&contents)
        .with_context(|| format!("failed to parse package.json {}", path.display()))?;

    if parsed.name.is_empty() {
        return Ok(None);
    }

    let root = path.parent().unwrap_or(path).to_path_buf();
    let mut entry_candidates = Vec::new();
    for field in [parsed.source, parsed.module, parsed.types, parsed.main] {
        if let Some(value) = field {
            entry_candidates.push(root.join(value));
        }
    }
    entry_candidates.push(root.join("src/index.ts"));
    entry_candidates.push(root.join("src/index.tsx"));
    entry_candidates.push(root.join("src/index.js"));
    entry_candidates.push(root.join("src/index.jsx"));
    entry_candidates.push(root.join("index.ts"));
    entry_candidates.push(root.join("index.tsx"));
    entry_candidates.push(root.join("index.js"));
    entry_candidates.push(root.join("index.jsx"));

    Ok(Some(PackageInfo {
        name: parsed.name,
        root,
        entry_candidates,
    }))
}

fn match_alias(pattern: &str, specifier: &str) -> Option<Vec<String>> {
    if let Some((prefix, suffix)) = pattern.split_once('*') {
        if specifier.starts_with(prefix) && specifier.ends_with(suffix) {
            let middle = &specifier[prefix.len()..specifier.len() - suffix.len()];
            return Some(vec![middle.to_string()]);
        }
        return None;
    }

    if pattern == specifier {
        Some(Vec::new())
    } else {
        None
    }
}

fn apply_alias_target(target: &str, captures: &[String]) -> String {
    let mut resolved = target.to_string();
    for capture in captures {
        if let Some(index) = resolved.find('*') {
            resolved.replace_range(index..=index, capture);
        }
    }
    resolved
}

fn clean_path(path: &Path) -> PathBuf {
    let mut result = PathBuf::new();

    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                result.pop();
            }
            other => result.push(other.as_os_str()),
        }
    }

    result
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::tempdir;

    use crate::fs::RepoContext;

    use super::{Resolution, Resolver};

    #[test]
    fn resolves_tsconfig_aliases_and_workspace_packages() {
        let dir = tempdir().unwrap();
        fs::create_dir_all(dir.path().join("packages/ui/src")).unwrap();
        fs::create_dir_all(dir.path().join("apps/web/src")).unwrap();
        fs::write(
            dir.path().join("tsconfig.json"),
            r#"{"compilerOptions":{"baseUrl":".","paths":{"@ui/*":["packages/ui/src/*"]}}}"#,
        )
        .unwrap();
        fs::write(
            dir.path().join("packages/ui/package.json"),
            r#"{"name":"@acme/ui","source":"src/index.ts"}"#,
        )
        .unwrap();
        fs::write(
            dir.path().join("packages/ui/src/index.ts"),
            "export * from './Button';",
        )
        .unwrap();
        fs::write(
            dir.path().join("packages/ui/src/Button.tsx"),
            "export const Button = () => null;",
        )
        .unwrap();
        fs::write(
            dir.path().join("apps/web/src/App.tsx"),
            "import { Button } from '@ui/Button'; import { Button as Two } from '@acme/ui';",
        )
        .unwrap();

        let context = RepoContext::discover(dir.path()).unwrap();
        let resolver = Resolver::new(&context).unwrap();
        let importer = dir.path().join("apps/web/src/App.tsx");

        assert!(matches!(
            resolver.resolve(&importer, "@ui/Button"),
            Resolution::Resolved(_)
        ));
        assert!(matches!(
            resolver.resolve(&importer, "@acme/ui"),
            Resolution::Resolved(_)
        ));
    }
}
