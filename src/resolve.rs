use std::collections::{BTreeMap, HashSet};
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::sync::OnceLock;

use anyhow::{Context, Result};
use serde::Deserialize;
use serde_json::Value;

use crate::fs::{RepoContext, TsConfigPath};

const JAVASCRIPT_RESOLUTION_EXTENSIONS: &[&str] =
    &["ts", "tsx", "mts", "cts", "js", "jsx", "mjs", "cjs"];

#[derive(Debug, Clone)]
pub struct Resolver {
    repo_root: PathBuf,
    source_files: HashSet<PathBuf>,
    suffix_index: BTreeMap<PathBuf, PathBuf>,
    java_package_index: BTreeMap<PathBuf, Vec<PathBuf>>,
    packages: Vec<PackageInfo>,
    package_by_name: BTreeMap<String, usize>,
    tsconfigs: Vec<TsConfigPath>,
}

#[derive(Debug, Clone)]
pub struct PackageInfo {
    pub name: String,
    pub root: PathBuf,
    pub entry_candidates: Vec<PathBuf>,
    pub export_mappings: Vec<ExportMapping>,
}

#[derive(Debug, Clone)]
pub struct ExportMapping {
    pub key: String,
    pub target: String,
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
    #[serde(default)]
    exports: Option<Value>,
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

        Ok(Self {
            repo_root: context.repo_root.clone(),
            source_files: context.source_files.iter().cloned().collect(),
            suffix_index: build_suffix_index(&context.repo_root, &context.source_files),
            java_package_index: build_java_package_index(&context.repo_root, &context.source_files),
            packages,
            package_by_name,
            tsconfigs: context.tsconfigs.clone(),
        })
    }

    pub fn resolve(&self, importer: &Path, specifier: &str) -> Resolution {
        let importer = clean_path(importer);

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

        package_specifier_parts(specifier)
            .map(|(package_name, _)| self.package_by_name.contains_key(package_name))
            .unwrap_or(false)
    }

    #[cfg(feature = "python")]
    fn resolve_python_import(&self, importer: &Path, specifier: &str) -> Option<PathBuf> {
        if specifier.starts_with('.') {
            return self.resolve_python_relative_import(importer, specifier);
        }

        let candidate = self.repo_root.join(specifier.replace('.', "/"));
        self.try_resolve_python_module_candidate(&candidate)
    }

    #[cfg(feature = "python")]
    fn resolve_python_relative_import(&self, importer: &Path, specifier: &str) -> Option<PathBuf> {
        let level = specifier.chars().take_while(|char| *char == '.').count();
        let remainder = specifier.trim_start_matches('.');
        let mut base = importer.parent().unwrap_or(&self.repo_root).to_path_buf();

        for _ in 1..level {
            base.pop();
        }

        let candidate = if remainder.is_empty() {
            base
        } else {
            base.join(remainder.replace('.', "/"))
        };
        self.try_resolve_python_module_candidate(&candidate)
    }

    #[cfg(feature = "python")]
    fn try_resolve_python_module_candidate(&self, candidate: &Path) -> Option<PathBuf> {
        if let Some(path) = self.try_resolve_candidate(candidate) {
            return Some(path);
        }

        let package_init = clean_path(&candidate.join("__init__.py"));
        if self.source_files.contains(&package_init) {
            return Some(package_init);
        }

        None
    }

    #[cfg(feature = "python")]
    fn python_top_level_exists(&self, specifier: &str) -> bool {
        let Some(first) = specifier.split('.').next() else {
            return false;
        };
        if first.is_empty() {
            return false;
        }

        let module_file = clean_path(&self.repo_root.join(format!("{first}.py")));
        let package_init = clean_path(&self.repo_root.join(first).join("__init__.py"));
        self.source_files.contains(&module_file) || self.source_files.contains(&package_init)
    }

    #[cfg(feature = "rust")]
    fn resolve_rust_import(&self, importer: &Path, specifier: &str) -> Option<PathBuf> {
        if let Some(module) = specifier.strip_prefix("mod:") {
            let base = rust_child_module_base(importer);
            return self.try_resolve_rust_module_candidate(&base.join(module));
        }

        let parts: Vec<&str> = specifier
            .split("::")
            .filter(|part| !part.is_empty())
            .collect();
        let (head, rest) = parts.split_first()?;

        match *head {
            "crate" => self.resolve_rust_from_crate_roots(rest),
            "self" => {
                let base = rust_child_module_base(importer).join(rest.join("/"));
                self.try_resolve_rust_module_candidate(&base)
            }
            "super" => {
                let mut base = rust_parent_module_base(importer);
                for part in rest {
                    if *part == "super" {
                        base.pop();
                    } else {
                        base.push(part);
                    }
                }
                self.try_resolve_rust_module_candidate(&base)
            }
            _ => {
                if let Some(path) = self.resolve_rust_from_crate_roots(&parts) {
                    return Some(path);
                }
                let base = rust_child_module_base(importer).join(parts.join("/"));
                self.try_resolve_rust_module_candidate(&base)
            }
        }
    }

    #[cfg(feature = "rust")]
    fn resolve_rust_from_crate_roots(&self, parts: &[&str]) -> Option<PathBuf> {
        if parts.is_empty() {
            return None;
        }

        for root in self.rust_crate_roots() {
            let candidate = root.join(parts.join("/"));
            if let Some(path) = self.try_resolve_rust_module_candidate(&candidate) {
                return Some(path);
            }
        }
        None
    }

    #[cfg(feature = "rust")]
    fn rust_crate_roots(&self) -> Vec<PathBuf> {
        let mut roots = Vec::new();
        for file in &self.source_files {
            let Some(name) = file.file_name().and_then(|name| name.to_str()) else {
                continue;
            };
            if matches!(name, "lib.rs" | "main.rs") {
                if let Some(parent) = file.parent() {
                    roots.push(parent.to_path_buf());
                }
            }
        }
        roots.sort();
        roots.dedup();
        if roots.is_empty() {
            roots.push(self.repo_root.clone());
        }
        roots
    }

    #[cfg(feature = "rust")]
    fn try_resolve_rust_module_candidate(&self, candidate: &Path) -> Option<PathBuf> {
        if let Some(path) = self.try_resolve_candidate(candidate) {
            return Some(path);
        }

        let mod_file = clean_path(&candidate.join("mod.rs"));
        if self.source_files.contains(&mod_file) {
            return Some(mod_file);
        }

        None
    }

    #[cfg(feature = "rust")]
    fn rust_top_level_exists(&self, specifier: &str) -> bool {
        let Some(first) = specifier.split("::").next() else {
            return false;
        };
        if first.is_empty() {
            return false;
        }

        self.rust_crate_roots().into_iter().any(|root| {
            self.try_resolve_rust_module_candidate(&root.join(first))
                .is_some()
        })
    }

    #[cfg(feature = "ruby")]
    fn resolve_ruby_import(&self, importer: &Path, specifier: &str) -> Option<PathBuf> {
        if specifier.starts_with('.') {
            let base = importer.parent().unwrap_or(&self.repo_root);
            return self.try_resolve_candidate(&base.join(specifier));
        }

        for candidate in [
            self.repo_root.join(specifier),
            self.repo_root.join("lib").join(specifier),
            self.repo_root.join("app").join(specifier),
        ] {
            if let Some(path) = self.try_resolve_candidate(&candidate) {
                return Some(path);
            }
        }

        self.suffix_index
            .get(&PathBuf::from(format!("{specifier}.rb")))
            .cloned()
    }

    #[cfg(feature = "java")]
    fn resolve_java_import(&self, specifier: &str) -> Option<PathBuf> {
        if specifier.ends_with(".*") {
            let package_path = specifier.trim_end_matches(".*").replace('.', "/");
            return self
                .java_package_index
                .get(&PathBuf::from(package_path))
                .and_then(|files| files.first().cloned());
        }

        self.suffix_index
            .get(&PathBuf::from(format!("{}.java", specifier.replace('.', "/"))))
            .cloned()
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
        let (package_name, rest) = package_specifier_parts(specifier)?;
        let package = self
            .package_by_name
            .get(package_name)
            .and_then(|index| self.packages.get(*index))?;

        if let Some(rest) = rest {
            let export_key = format!("./{rest}");
            if let Some(resolved) = resolve_package_export(package, &export_key)
                .and_then(|path| self.try_resolve_candidate(&path))
            {
                return Some(resolved);
            }

            let direct = package.root.join(rest);
            if let Some(resolved) = self.try_resolve_candidate(&direct) {
                return Some(resolved);
            }

            let src_direct = package.root.join("src").join(rest);
            if let Some(resolved) = self.try_resolve_candidate(&src_direct) {
                return Some(resolved);
            }

            return None;
        }

        if let Some(resolved) =
            resolve_package_export(package, ".").and_then(|path| self.try_resolve_candidate(&path))
        {
            return Some(resolved);
        }

        for candidate in &package.entry_candidates {
            if let Some(resolved) = self.try_resolve_candidate(candidate) {
                return Some(resolved);
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

        for extension in resolution_extensions() {
            let path = candidate.with_extension(extension);
            if self.source_files.contains(&path) {
                return Some(path);
            }
        }

        if let Some(ext) = candidate.extension().and_then(|ext| ext.to_str()) {
            if !resolution_extensions().contains(&ext) {
                for extension in resolution_extensions() {
                    let path = candidate.with_extension(format!("{ext}.{extension}"));
                    if self.source_files.contains(&path) {
                        return Some(path);
                    }
                }
            }
        }

        if candidate.is_dir() || candidate.extension().is_none() {
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

#[cfg(feature = "python")]
fn is_python_file(path: &Path) -> bool {
    path.extension().and_then(|ext| ext.to_str()) == Some("py")
}

#[cfg(feature = "rust")]
fn is_rust_file(path: &Path) -> bool {
    path.extension().and_then(|ext| ext.to_str()) == Some("rs")
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

#[cfg(feature = "ruby")]
fn is_ruby_file(path: &Path) -> bool {
    path.extension().and_then(|ext| ext.to_str()) == Some("rb")
}

#[cfg(feature = "java")]
fn is_java_file(path: &Path) -> bool {
    path.extension().and_then(|ext| ext.to_str()) == Some("java")
}

#[cfg(feature = "rust")]
fn rust_child_module_base(importer: &Path) -> PathBuf {
    let parent = importer.parent().unwrap_or_else(|| Path::new(""));
    match importer.file_name().and_then(|name| name.to_str()) {
        Some("lib.rs" | "main.rs" | "mod.rs") => parent.to_path_buf(),
        _ => parent.join(importer.file_stem().unwrap_or_default()),
    }
}

#[cfg(feature = "rust")]
fn rust_parent_module_base(importer: &Path) -> PathBuf {
    let child_base = rust_child_module_base(importer);
    child_base
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or(child_base)
}

fn build_suffix_index(repo_root: &Path, source_files: &[PathBuf]) -> BTreeMap<PathBuf, PathBuf> {
    let mut index = BTreeMap::new();

    for file in source_files {
        let Some(ext) = file.extension().and_then(|ext| ext.to_str()) else {
            continue;
        };
        if !matches!(ext, "rb" | "java") {
            continue;
        }

        let relative = file.strip_prefix(repo_root).unwrap_or(file);
        for suffix in path_suffixes(relative) {
            index.entry(suffix).or_insert_with(|| file.clone());
        }
    }

    index
}

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

fn package_specifier_parts(specifier: &str) -> Option<(&str, Option<&str>)> {
    if specifier.is_empty() || specifier.starts_with('.') || specifier.starts_with('/') {
        return None;
    }

    if specifier.starts_with('@') {
        let first_slash = specifier.find('/')?;
        let rest_start = first_slash + 1;
        let second_slash = specifier[rest_start..]
            .find('/')
            .map(|index| rest_start + index);
        return match second_slash {
            Some(index) => Some((&specifier[..index], Some(&specifier[index + 1..]))),
            None => Some((specifier, None)),
        };
    }

    match specifier.split_once('/') {
        Some((name, rest)) => Some((name, Some(rest))),
        None => Some((specifier, None)),
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
    let export_mappings = collect_export_mappings(parsed.exports.as_ref());

    Ok(Some(PackageInfo {
        name: parsed.name,
        root,
        entry_candidates,
        export_mappings,
    }))
}

fn collect_export_mappings(exports: Option<&Value>) -> Vec<ExportMapping> {
    let Some(Value::Object(map)) = exports else {
        return Vec::new();
    };

    let mut mappings = Vec::new();
    for (key, value) in map {
        if !key.starts_with('.') {
            continue;
        }
        if let Some(target) = export_target(value) {
            mappings.push(ExportMapping {
                key: key.clone(),
                target,
            });
        }
    }
    mappings
}

fn export_target(value: &Value) -> Option<String> {
    match value {
        Value::String(path) => Some(path.clone()),
        Value::Object(map) => {
            for key in ["dev", "source"] {
                if let Some(Value::String(path)) = map.get(key) {
                    return Some(path.clone());
                }
            }

            for key in ["default", "import", "require"] {
                if let Some(target) = map.get(key).and_then(export_target) {
                    return Some(target);
                }
            }

            None
        }
        _ => None,
    }
}

fn resolve_package_export(package: &PackageInfo, export_key: &str) -> Option<PathBuf> {
    for mapping in &package.export_mappings {
        if let Some(captures) = match_alias(&mapping.key, export_key) {
            let target = apply_alias_target(&mapping.target, &captures);
            return Some(package.root.join(target));
        }
    }
    None
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

    #[test]
    fn resolves_modern_module_extensions() {
        let dir = tempdir().unwrap();
        fs::create_dir_all(dir.path().join("src")).unwrap();
        fs::write(dir.path().join("package.json"), r#"{"name":"fixture"}"#).unwrap();
        fs::write(
            dir.path().join("src/App.tsx"),
            "import { button } from './button'; import { server } from './server';",
        )
        .unwrap();
        fs::write(
            dir.path().join("src/button.mjs"),
            "export const button = 'ok';",
        )
        .unwrap();
        fs::write(
            dir.path().join("src/server.cts"),
            "export const server = 'ok';",
        )
        .unwrap();

        let context = RepoContext::discover(dir.path()).unwrap();
        let resolver = Resolver::new(&context).unwrap();
        let importer = dir.path().join("src/App.tsx");

        assert!(matches!(
            resolver.resolve(&importer, "./button"),
            Resolution::Resolved(path) if path.ends_with("src/button.mjs")
        ));
        assert!(matches!(
            resolver.resolve(&importer, "./server"),
            Resolution::Resolved(path) if path.ends_with("src/server.cts")
        ));
    }

    #[test]
    fn resolves_workspace_package_exports() {
        let dir = tempdir().unwrap();
        fs::create_dir_all(dir.path().join("packages/ui/src/components/button")).unwrap();
        fs::create_dir_all(dir.path().join("apps/web/src")).unwrap();
        fs::write(
            dir.path().join("packages/ui/package.json"),
            r#"{
                "name":"@acme/ui",
                "exports":{
                    ".":{"dev":"./src/index.ts"},
                    "./preset":{"dev":"./src/preset.ts"},
                    "./*":{"dev":"./src/components/*/index.ts"}
                }
            }"#,
        )
        .unwrap();
        fs::write(
            dir.path().join("packages/ui/src/index.ts"),
            "export * from './components/button';",
        )
        .unwrap();
        fs::write(
            dir.path().join("packages/ui/src/preset.ts"),
            "export const preset = true;",
        )
        .unwrap();
        fs::write(
            dir.path()
                .join("packages/ui/src/components/button/index.ts"),
            "export const Button = () => null;",
        )
        .unwrap();
        fs::write(
            dir.path().join("apps/web/src/App.tsx"),
            "import { Button } from '@acme/ui/button'; import { preset } from '@acme/ui/preset';",
        )
        .unwrap();

        let context = RepoContext::discover(dir.path()).unwrap();
        let resolver = Resolver::new(&context).unwrap();
        let importer = dir.path().join("apps/web/src/App.tsx");

        assert!(matches!(
            resolver.resolve(&importer, "@acme/ui/button"),
            Resolution::Resolved(path) if path.ends_with("packages/ui/src/components/button/index.ts")
        ));
        assert!(matches!(
            resolver.resolve(&importer, "@acme/ui/preset"),
            Resolution::Resolved(path) if path.ends_with("packages/ui/src/preset.ts")
        ));
    }

    #[test]
    fn resolves_multi_dot_basenames() {
        let dir = tempdir().unwrap();
        fs::create_dir_all(dir.path().join("src")).unwrap();
        fs::write(dir.path().join("package.json"), r#"{"name":"fixture"}"#).unwrap();
        fs::write(
            dir.path().join("src/App.tsx"),
            "import './recipe.types'; import './docs.config'; import './routeTree.gen';",
        )
        .unwrap();
        fs::write(
            dir.path().join("src/recipe.types.ts"),
            "export type Recipe = {};",
        )
        .unwrap();
        fs::write(
            dir.path().join("src/docs.config.ts"),
            "export const docsConfig = {};",
        )
        .unwrap();
        fs::write(
            dir.path().join("src/routeTree.gen.ts"),
            "export const routeTree = {};",
        )
        .unwrap();

        let context = RepoContext::discover(dir.path()).unwrap();
        let resolver = Resolver::new(&context).unwrap();
        let importer = dir.path().join("src/App.tsx");

        assert!(matches!(
            resolver.resolve(&importer, "./recipe.types"),
            Resolution::Resolved(path) if path.ends_with("src/recipe.types.ts")
        ));
        assert!(matches!(
            resolver.resolve(&importer, "./docs.config"),
            Resolution::Resolved(path) if path.ends_with("src/docs.config.ts")
        ));
        assert!(matches!(
            resolver.resolve(&importer, "./routeTree.gen"),
            Resolution::Resolved(path) if path.ends_with("src/routeTree.gen.ts")
        ));
    }

    #[cfg(feature = "python")]
    #[test]
    fn resolves_python_absolute_relative_and_package_imports() {
        let dir = tempdir().unwrap();
        fs::create_dir_all(dir.path().join("app/services")).unwrap();
        fs::create_dir_all(dir.path().join("app/utils")).unwrap();
        fs::write(dir.path().join("app/__init__.py"), "").unwrap();
        fs::write(dir.path().join("app/models.py"), "class User: pass").unwrap();
        fs::write(
            dir.path().join("app/services/email.py"),
            "from ..models import User",
        )
        .unwrap();
        fs::write(
            dir.path().join("app/utils/__init__.py"),
            "def format_subject(): pass",
        )
        .unwrap();

        let context = RepoContext::discover(dir.path()).unwrap();
        let resolver = Resolver::new(&context).unwrap();
        let importer = dir.path().join("app/services/email.py");

        assert!(matches!(
            resolver.resolve(&importer, "..models"),
            Resolution::Resolved(path) if path.ends_with("app/models.py")
        ));
        assert!(matches!(
            resolver.resolve(&importer, "app.utils"),
            Resolution::Resolved(path) if path.ends_with("app/utils/__init__.py")
        ));
        assert!(resolver.is_internal_specifier(&importer, "..models"));
        assert!(resolver.is_internal_specifier(&importer, "app.models"));
        assert!(!resolver.is_internal_specifier(&importer, "dataclasses"));
    }

    #[cfg(feature = "rust")]
    #[test]
    fn resolves_rust_crate_super_self_and_mod_imports() {
        let dir = tempdir().unwrap();
        fs::create_dir_all(dir.path().join("src/services")).unwrap();
        fs::create_dir_all(dir.path().join("src/utils")).unwrap();
        fs::write(dir.path().join("src/lib.rs"), "pub mod services;").unwrap();
        fs::write(dir.path().join("src/models.rs"), "pub struct User;").unwrap();
        fs::write(dir.path().join("src/services/mod.rs"), "pub mod email;").unwrap();
        fs::write(
            dir.path().join("src/services/email.rs"),
            "use super::formatter;",
        )
        .unwrap();
        fs::write(
            dir.path().join("src/services/formatter.rs"),
            "pub fn f() {}",
        )
        .unwrap();
        fs::write(dir.path().join("src/utils/mod.rs"), "pub mod formatting;").unwrap();
        fs::write(dir.path().join("src/utils/formatting.rs"), "pub fn f() {}").unwrap();

        let context = RepoContext::discover(dir.path()).unwrap();
        let resolver = Resolver::new(&context).unwrap();
        let lib = dir.path().join("src/lib.rs");
        let service_mod = dir.path().join("src/services/mod.rs");
        let email = dir.path().join("src/services/email.rs");

        assert!(matches!(
            resolver.resolve(&lib, "mod:services"),
            Resolution::Resolved(path) if path.ends_with("src/services/mod.rs")
        ));
        assert!(matches!(
            resolver.resolve(&lib, "crate::models"),
            Resolution::Resolved(path) if path.ends_with("src/models.rs")
        ));
        assert!(matches!(
            resolver.resolve(&service_mod, "self::email"),
            Resolution::Resolved(path) if path.ends_with("src/services/email.rs")
        ));
        assert!(matches!(
            resolver.resolve(&email, "super::formatter"),
            Resolution::Resolved(path) if path.ends_with("src/services/formatter.rs")
        ));
        assert!(resolver.is_internal_specifier(&email, "crate::models"));
        assert!(!resolver.is_internal_specifier(&email, "serde"));
    }

    #[cfg(feature = "ruby")]
    #[test]
    fn resolves_ruby_suffix_imports_from_index() {
        let dir = tempdir().unwrap();
        fs::create_dir_all(dir.path().join("app/services")).unwrap();
        fs::create_dir_all(dir.path().join("app/utils")).unwrap();
        fs::write(dir.path().join("app/services/email.rb"), "class Email; end").unwrap();
        fs::write(
            dir.path().join("app/utils/formatter.rb"),
            "class Formatter; end",
        )
        .unwrap();

        let context = RepoContext::discover(dir.path()).unwrap();
        let resolver = Resolver::new(&context).unwrap();
        let importer = dir.path().join("app/services/email.rb");

        assert!(matches!(
            resolver.resolve(&importer, "utils/formatter"),
            Resolution::Resolved(path) if path.ends_with("app/utils/formatter.rb")
        ));
    }

    #[cfg(feature = "java")]
    #[test]
    fn resolves_java_class_and_wildcard_imports_from_index() {
        let dir = tempdir().unwrap();
        fs::create_dir_all(dir.path().join("src/main/java/com/example/service")).unwrap();
        fs::create_dir_all(dir.path().join("src/main/java/com/example/util")).unwrap();
        fs::write(
            dir.path().join("src/main/java/com/example/service/EmailService.java"),
            "package com.example.service; class EmailService {}",
        )
        .unwrap();
        fs::write(
            dir.path().join("src/main/java/com/example/util/Formatter.java"),
            "package com.example.util; class Formatter {}",
        )
        .unwrap();

        let context = RepoContext::discover(dir.path()).unwrap();
        let resolver = Resolver::new(&context).unwrap();
        let importer = dir
            .path()
            .join("src/main/java/com/example/service/EmailService.java");

        assert!(matches!(
            resolver.resolve(&importer, "com.example.util.Formatter"),
            Resolution::Resolved(path) if path.ends_with("src/main/java/com/example/util/Formatter.java")
        ));
        assert!(matches!(
            resolver.resolve(&importer, "com.example.util.*"),
            Resolution::Resolved(path) if path.ends_with("src/main/java/com/example/util/Formatter.java")
        ));
    }
}
