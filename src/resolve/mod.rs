use std::collections::{BTreeMap, HashSet};
use std::path::{Component, Path, PathBuf};
use std::sync::OnceLock;

use anyhow::Result;

use crate::fs::{RepoContext, TsConfigPath};

mod javascript;
mod package;
use package::{PackageInfo, load_package_info};
pub(super) use package::{package_specifier_parts, resolve_package_export};

#[cfg(test)]
mod tests;

#[cfg(feature = "python")]
mod python;
#[cfg(feature = "python")]
use python::is_python_file;

#[cfg(feature = "rust")]
mod rust_lang;
#[cfg(feature = "rust")]
use rust_lang::is_rust_file;

#[cfg(feature = "ruby")]
mod ruby;
#[cfg(feature = "ruby")]
use ruby::is_ruby_file;

#[cfg(feature = "java")]
mod java;
#[cfg(feature = "java")]
use java::is_java_file;

const JAVASCRIPT_RESOLUTION_EXTENSIONS: &[&str] =
    &["ts", "tsx", "mts", "cts", "js", "jsx", "mjs", "cjs"];

#[derive(Debug, Clone)]
pub struct Resolver {
    repo_root: PathBuf,
    source_files: HashSet<PathBuf>,
    #[cfg(any(feature = "ruby", feature = "java"))]
    suffix_index: BTreeMap<PathBuf, PathBuf>,
    #[cfg(any(feature = "ruby", feature = "java"))]
    suffix_ambiguities: Vec<SuffixAmbiguity>,
    #[cfg(feature = "java")]
    java_package_index: BTreeMap<PathBuf, Vec<PathBuf>>,
    packages: Vec<PackageInfo>,
    package_by_name: BTreeMap<String, usize>,
    tsconfigs: Vec<TsConfigPath>,
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

impl Resolver {
    pub fn new(context: &RepoContext) -> Result<Self> {
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

    pub fn warnings(&self) -> Vec<String> {
        #[cfg(any(feature = "ruby", feature = "java"))]
        {
            let mut warnings = Vec::new();
            for ambiguity in &self.suffix_ambiguities {
                let paths = ambiguity
                    .paths
                    .iter()
                    .map(|path| path.strip_prefix(&self.repo_root).unwrap_or(path))
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
        let importer = self.normalize_importer(importer);

        #[cfg(feature = "python")]
        if is_python_file(&importer) {
            if let Some(path) = self.resolve_python_import(&importer, specifier) {
                return Resolution::Resolved(path);
            }
            return Resolution::Unresolved;
        }

        #[cfg(feature = "rust")]
        if is_rust_file(&importer) {
            if let Some(path) = self.resolve_rust_import(&importer, specifier) {
                return Resolution::Resolved(path);
            }
            return Resolution::Unresolved;
        }

        #[cfg(feature = "ruby")]
        if is_ruby_file(&importer) {
            if let Some(path) = self.resolve_ruby_import(&importer, specifier) {
                return Resolution::Resolved(path);
            }
            return Resolution::Unresolved;
        }

        #[cfg(feature = "java")]
        if is_java_file(&importer) {
            if let Some(path) = self.resolve_java_import(specifier) {
                return Resolution::Resolved(path);
            }
            return Resolution::Unresolved;
        }

        self.resolve_javascript_import(&importer, specifier)
    }

    fn normalize_importer(&self, importer: &Path) -> PathBuf {
        let cleaned = clean_path(importer);
        if self.source_files.contains(&cleaned) {
            return cleaned;
        }

        importer.canonicalize().unwrap_or(cleaned)
    }

    pub fn is_internal_specifier(&self, importer: &Path, specifier: &str) -> bool {
        #[cfg(feature = "python")]
        if is_python_file(importer) {
            if specifier.starts_with('.') {
                return true;
            }
            return self.python_top_level_exists(specifier);
        }

        #[cfg(feature = "rust")]
        if is_rust_file(importer) {
            if specifier.starts_with("mod:")
                || specifier.starts_with("crate::")
                || specifier.starts_with("self::")
                || specifier.starts_with("super::")
            {
                return true;
            }
            return self.rust_top_level_exists(specifier);
        }

        #[cfg(feature = "ruby")]
        if is_ruby_file(importer) {
            return specifier.starts_with('.')
                || self.resolve_ruby_import(importer, specifier).is_some();
        }

        #[cfg(feature = "java")]
        if is_java_file(importer) {
            return self.resolve_java_import(specifier).is_some();
        }

        self.is_internal_javascript_specifier(importer, specifier)
    }

    pub(super) fn resolve_path(&self, base: &Path, specifier: &str) -> Resolution {
        let path = if specifier.starts_with('/') {
            clean_path(&self.repo_root.join(specifier.trim_start_matches('/')))
        } else {
            clean_path(&base.join(specifier))
        };

        self.try_resolve_candidate(&path)
            .map(Resolution::Resolved)
            .unwrap_or(Resolution::Unresolved)
    }

    pub(super) fn try_resolve_candidate(&self, candidate: &Path) -> Option<PathBuf> {
        let candidate = clean_path(candidate);

        if self.source_files.contains(&candidate) {
            return Some(candidate);
        }

        for extension in resolution_extensions() {
            let path = candidate.with_extension(extension);
            if self.source_files.contains(&path) {
                return Some(path);
            }
        }

        if let Some(ext) = candidate.extension().and_then(|ext| ext.to_str())
            && !resolution_extensions().contains(&ext)
        {
            for extension in resolution_extensions() {
                let path = candidate.with_extension(format!("{ext}.{extension}"));
                if self.source_files.contains(&path) {
                    return Some(path);
                }
            }
        }

        if candidate.extension().is_none() {
            for extension in resolution_extensions() {
                let path = candidate.join(format!("index.{extension}"));
                if self.source_files.contains(&path) {
                    return Some(path);
                }
            }

            #[cfg(feature = "python")]
            {
                let path = candidate.join("__init__.py");
                if self.source_files.contains(&path) {
                    return Some(path);
                }
            }
        }

        None
    }
}

fn resolution_extensions() -> &'static [&'static str] {
    static EXTENSIONS: OnceLock<Vec<&'static str>> = OnceLock::new();
    EXTENSIONS.get_or_init(|| {
        let mut extensions = JAVASCRIPT_RESOLUTION_EXTENSIONS.to_vec();
        if cfg!(feature = "python") {
            extensions.push("py");
        }
        if cfg!(feature = "rust") {
            extensions.push("rs");
        }
        if cfg!(feature = "vue") {
            extensions.push("vue");
        }
        if cfg!(feature = "svelte") {
            extensions.push("svelte");
        }
        if cfg!(feature = "ruby") {
            extensions.push("rb");
        }
        if cfg!(feature = "java") {
            extensions.push("java");
        }
        extensions
    })
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

pub(super) fn match_alias(pattern: &str, specifier: &str) -> Option<Vec<String>> {
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

pub(super) fn apply_alias_target(target: &str, captures: &[String]) -> String {
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
