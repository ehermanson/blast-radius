use std::collections::{BTreeMap, HashSet};
use std::path::{Component, Path, PathBuf};

use anyhow::Result;

use crate::fs::{RepoContext, TsConfigPath};

mod package;
use package::load_package_info;
pub(crate) use package::{
    PackageInfo, package_specifier_parts, resolve_package_export, resolve_package_import,
};

#[cfg(test)]
mod tests;

/// Shared resolution state and primitives, borrowed by language adapters. Holds
/// the source-file index, workspace packages, and tsconfig aliases.
#[derive(Debug, Clone)]
pub struct ResolveCtx {
    pub(crate) repo_root: PathBuf,
    pub(crate) source_files: HashSet<PathBuf>,
    pub(crate) packages: Vec<PackageInfo>,
    pub(crate) package_by_name: BTreeMap<String, usize>,
    pub(crate) tsconfigs: Vec<TsConfigPath>,
}

#[derive(Debug, Clone)]
pub enum Resolution {
    Resolved(PathBuf),
    Unresolved,
}

impl ResolveCtx {
    fn new(context: &RepoContext) -> Result<Self> {
        let mut packages = Vec::new();
        for package_json in &context.package_jsons {
            if let Some(package) = load_package_info(package_json)? {
                packages.push(package);
            }
        }
        let mut package_by_name = BTreeMap::new();
        for (index, package) in packages.iter().enumerate() {
            package_by_name.entry(package.name.clone()).or_insert(index);
        }
        Ok(Self {
            repo_root: context.repo_root.clone(),
            source_files: context.source_files.iter().cloned().collect(),
            packages,
            package_by_name,
            tsconfigs: context.tsconfigs.clone(),
        })
    }

    fn normalize_importer(&self, importer: &Path) -> PathBuf {
        let cleaned = clean_path(importer);
        if self.source_files.contains(&cleaned) {
            return cleaned;
        }

        importer.canonicalize().unwrap_or(cleaned)
    }

    /// Resolve a relative or absolute path specifier against `base`, probing the
    /// given `extensions`. The caller passes only its own language family's
    /// extensions so resolution never crosses language boundaries.
    pub(crate) fn resolve_path(
        &self,
        base: &Path,
        specifier: &str,
        extensions: &[&str],
    ) -> Resolution {
        let path = if specifier.starts_with('/') {
            clean_path(&self.repo_root.join(specifier.trim_start_matches('/')))
        } else {
            clean_path(&base.join(specifier))
        };

        self.try_resolve_candidate(&path, extensions)
            .map(Resolution::Resolved)
            .unwrap_or(Resolution::Unresolved)
    }

    /// Map a path candidate to a concrete source file, trying exact match, then
    /// the given `extensions`, then `index.*` directory entrypoints. Probing is
    /// scoped to the caller's extensions, so e.g. a JS import cannot resolve to
    /// a `.py` file. Language-specific directory entrypoints (Python's
    /// `__init__.py`) are handled by the owning adapter.
    pub(crate) fn try_resolve_candidate(
        &self,
        candidate: &Path,
        extensions: &[&str],
    ) -> Option<PathBuf> {
        let candidate = clean_path(candidate);

        // An exact hit still has to be in the caller's language family — a `.py`
        // file must not satisfy a JS `import "./model.py"` even though it exists.
        if self.source_files.contains(&candidate) {
            let cross_language = candidate
                .extension()
                .and_then(|ext| ext.to_str())
                .is_some_and(|ext| !extensions.contains(&ext));
            if !cross_language {
                return Some(candidate);
            }
        }

        let candidate_ext = candidate.extension().and_then(|ext| ext.to_str());

        if candidate_ext.is_none() {
            for extension in extensions {
                let path = candidate.with_extension(extension);
                if self.source_files.contains(&path) {
                    return Some(path);
                }
            }
        }

        if let Some(ext) = candidate_ext {
            // A multi-dot basename ('./recipe.types' -> recipe.types.ts) wins
            // over extension rewriting, so probe the appended form first.
            if !extensions.contains(&ext) {
                for extension in extensions {
                    let path = candidate.with_extension(format!("{ext}.{extension}"));
                    if self.source_files.contains(&path) {
                        return Some(path);
                    }
                }
            }

            // Extension replacement only applies to JS-emitted specifiers that
            // TS rewrites to their source counterparts; './theme.css' stays an
            // unresolved asset rather than hitting theme.ts.
            if extensions.contains(&"ts") {
                for replacement in ts_counterparts(ext) {
                    let path = candidate.with_extension(replacement);
                    if self.source_files.contains(&path) {
                        return Some(path);
                    }
                }
            }
        }

        // Declaration-only modules: './types' backed by types.d.ts.
        if extensions.contains(&"ts") {
            let mut appended = candidate.clone().into_os_string();
            appended.push(".d.ts");
            let path = PathBuf::from(appended);
            if self.source_files.contains(&path) {
                return Some(path);
            }
        }

        if candidate_ext.is_none() {
            for extension in extensions {
                let path = candidate.join(format!("index.{extension}"));
                if self.source_files.contains(&path) {
                    return Some(path);
                }
            }
            if extensions.contains(&"ts") {
                let path = candidate.join("index.d.ts");
                if self.source_files.contains(&path) {
                    return Some(path);
                }
            }
        }

        None
    }
}

/// Resolves import specifiers to repo source files by dispatching to the
/// language adapter that owns the importing file.
#[derive(Debug, Clone)]
pub struct Resolver {
    ctx: ResolveCtx,
}

impl Resolver {
    pub fn new(context: &RepoContext) -> Result<Self> {
        Ok(Self {
            ctx: ResolveCtx::new(context)?,
        })
    }

    pub fn resolve(&self, importer: &Path, specifier: &str) -> Resolution {
        let importer = self.ctx.normalize_importer(importer);
        crate::language::adapter_for(&importer).resolve(&self.ctx, &importer, specifier)
    }

    pub fn is_internal_specifier(&self, importer: &Path, specifier: &str) -> bool {
        let importer = self.ctx.normalize_importer(importer);
        crate::language::adapter_for(&importer).is_internal(&self.ctx, &importer, specifier)
    }
}

/// TS source extensions a JS-emitted specifier extension may map back to,
/// per TypeScript module resolution.
fn ts_counterparts(ext: &str) -> &'static [&'static str] {
    match ext {
        "js" => &["ts", "tsx", "d.ts"],
        "jsx" => &["tsx"],
        "mjs" => &["mts", "d.mts"],
        "cjs" => &["cts", "d.cts"],
        _ => &[],
    }
}

pub(crate) fn match_alias(pattern: &str, specifier: &str) -> Option<Vec<String>> {
    if let Some((prefix, suffix)) = pattern.split_once('*') {
        // Prefix and suffix must not overlap in the specifier ("lib/*/lib"
        // cannot match "lib/lib").
        if specifier.len() >= prefix.len() + suffix.len()
            && specifier.starts_with(prefix)
            && specifier.ends_with(suffix)
        {
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

pub(crate) fn apply_alias_target(target: &str, captures: &[String]) -> String {
    let mut resolved = target.to_string();
    for capture in captures {
        if let Some(index) = resolved.find('*') {
            resolved.replace_range(index..=index, capture);
        }
    }
    resolved
}

pub(crate) fn clean_path(path: &Path) -> PathBuf {
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
