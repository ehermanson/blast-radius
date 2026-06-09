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
/// the source-file index, workspace packages, tsconfig aliases, and the
/// language-specific suffix/package indexes used by Ruby and Java.
#[derive(Debug, Clone)]
pub struct ResolveCtx {
    pub(crate) repo_root: PathBuf,
    pub(crate) source_files: HashSet<PathBuf>,
    #[cfg(any(feature = "ruby", feature = "java"))]
    pub(crate) suffix_index: BTreeMap<PathBuf, PathBuf>,
    #[cfg(any(feature = "ruby", feature = "java"))]
    suffix_ambiguities: Vec<SuffixAmbiguity>,
    #[cfg(feature = "java")]
    pub(crate) java_package_index: BTreeMap<PathBuf, Vec<PathBuf>>,
    pub(crate) packages: Vec<PackageInfo>,
    pub(crate) package_by_name: BTreeMap<String, usize>,
    pub(crate) tsconfigs: Vec<TsConfigPath>,
}

#[derive(Debug, Clone)]
pub enum Resolution {
    Resolved(PathBuf),
    Unresolved,
}

#[cfg(any(feature = "ruby", feature = "java"))]
#[derive(Debug, Clone)]
struct SuffixAmbiguity {
    suffix: PathBuf,
    paths: Vec<PathBuf>,
}

#[cfg(any(feature = "ruby", feature = "java"))]
#[derive(Debug, Clone)]
struct SuffixIndex {
    index: BTreeMap<PathBuf, PathBuf>,
    ambiguities: Vec<SuffixAmbiguity>,
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
        #[cfg(any(feature = "ruby", feature = "java"))]
        let suffix_index = build_suffix_index(&context.repo_root, &context.source_files);

        Ok(Self {
            repo_root: context.repo_root.clone(),
            source_files: context.source_files.iter().cloned().collect(),
            #[cfg(any(feature = "ruby", feature = "java"))]
            suffix_index: suffix_index.index,
            #[cfg(any(feature = "ruby", feature = "java"))]
            suffix_ambiguities: suffix_index.ambiguities,
            #[cfg(feature = "java")]
            java_package_index: build_java_package_index(&context.repo_root, &context.source_files),
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

        for extension in extensions {
            let path = candidate.with_extension(extension);
            if self.source_files.contains(&path) {
                return Some(path);
            }
        }

        if let Some(ext) = candidate.extension().and_then(|ext| ext.to_str())
            && !extensions.contains(&ext)
        {
            for extension in extensions {
                let path = candidate.with_extension(format!("{ext}.{extension}"));
                if self.source_files.contains(&path) {
                    return Some(path);
                }
            }
        }

        if candidate.extension().is_none() {
            for extension in extensions {
                let path = candidate.join(format!("index.{extension}"));
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

    pub fn warnings(&self) -> Vec<String> {
        #[cfg(any(feature = "ruby", feature = "java"))]
        {
            let mut warnings = Vec::new();
            for ambiguity in &self.ctx.suffix_ambiguities {
                let paths = ambiguity
                    .paths
                    .iter()
                    .map(|path| path.strip_prefix(&self.ctx.repo_root).unwrap_or(path))
                    .map(|path| path.display().to_string())
                    .collect::<Vec<_>>()
                    .join(", ");
                warnings.push(format!(
                    "ambiguous suffix resolution for {} matched multiple files: {paths}",
                    ambiguity.suffix.display()
                ));
            }
            warnings
        }

        #[cfg(not(any(feature = "ruby", feature = "java")))]
        {
            Vec::new()
        }
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

#[cfg(any(feature = "ruby", feature = "java"))]
fn build_suffix_index(repo_root: &Path, source_files: &[PathBuf]) -> SuffixIndex {
    let mut all_matches: BTreeMap<PathBuf, Vec<PathBuf>> = BTreeMap::new();

    for file in source_files {
        let Some(ext) = file.extension().and_then(|ext| ext.to_str()) else {
            continue;
        };
        if !matches!(ext, "rb" | "java") {
            continue;
        }

        let relative = file.strip_prefix(repo_root).unwrap_or(file);
        for suffix in path_suffixes(relative) {
            all_matches.entry(suffix).or_default().push(file.clone());
        }
    }

    let mut index = BTreeMap::new();
    let mut ambiguities = Vec::new();
    for (suffix, paths) in all_matches {
        if let Some(first) = paths.first() {
            index.insert(suffix.clone(), first.clone());
        }
        if paths.len() > 1 {
            ambiguities.push(SuffixAmbiguity { suffix, paths });
        }
    }

    SuffixIndex { index, ambiguities }
}

#[cfg(feature = "java")]
fn build_java_package_index(
    repo_root: &Path,
    source_files: &[PathBuf],
) -> BTreeMap<PathBuf, Vec<PathBuf>> {
    let mut index: BTreeMap<PathBuf, Vec<PathBuf>> = BTreeMap::new();

    for file in source_files {
        if file.extension().and_then(|ext| ext.to_str()) != Some("java") {
            continue;
        }
        let Some(parent) = file.strip_prefix(repo_root).unwrap_or(file).parent() else {
            continue;
        };

        for suffix in path_suffixes(parent) {
            index.entry(suffix).or_default().push(file.clone());
        }
    }

    index
}

#[cfg(any(feature = "ruby", feature = "java"))]
fn path_suffixes(path: &Path) -> Vec<PathBuf> {
    let components: Vec<_> = path.iter().collect();
    let mut suffixes = Vec::new();

    for start in 0..components.len() {
        let mut suffix = PathBuf::new();
        for component in &components[start..] {
            suffix.push(Path::new(*component));
        }
        suffixes.push(suffix);
    }

    suffixes
}

pub(crate) fn match_alias(pattern: &str, specifier: &str) -> Option<Vec<String>> {
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
