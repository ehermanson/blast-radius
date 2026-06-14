use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use anyhow::Result;

use crate::fs::TsConfigPath;
use crate::parse::{ModuleFacts, parse_javascript_module};
use crate::resolve::{
    Resolution, ResolveCtx, apply_alias_target, match_alias, package_specifier_parts,
    resolve_package_export, resolve_package_import,
};

use super::LanguageAdapter;

pub(super) struct JavaScriptAdapter;

const EXTENSIONS: &[&str] = &["ts", "tsx", "mts", "cts", "js", "jsx", "mjs", "cjs"];

/// Extensions JS/TS resolution may resolve to. This is the web family: JS/TS
/// plus Vue/Svelte components when those features are enabled, since `.ts` files
/// import components and components import `.ts` (and each other). It does not
/// include non-web languages, so a JS import never resolves to a `.py`/`.rb`/etc.
pub(super) fn web_extensions() -> &'static [&'static str] {
    static EXTENSIONS_ONCE: OnceLock<Vec<&'static str>> = OnceLock::new();
    EXTENSIONS_ONCE.get_or_init(|| {
        #[allow(unused_mut)]
        let mut extensions: Vec<&'static str> = EXTENSIONS.to_vec();
        #[cfg(feature = "vue")]
        extensions.push("vue");
        #[cfg(feature = "svelte")]
        extensions.push("svelte");
        extensions
    })
}

impl LanguageAdapter for JavaScriptAdapter {
    fn extensions(&self) -> &'static [&'static str] {
        EXTENSIONS
    }

    fn parse(&self, path: &Path, source: &str) -> Result<ModuleFacts> {
        parse_javascript_module(path, source)
    }

    fn resolve(&self, ctx: &ResolveCtx, importer: &Path, specifier: &str) -> Resolution {
        resolve_javascript_import(ctx, importer, specifier)
    }

    fn is_internal(&self, ctx: &ResolveCtx, importer: &Path, specifier: &str) -> bool {
        is_internal_javascript_specifier(ctx, importer, specifier)
    }
}

// Shared with the Vue/Svelte component adapters, which resolve through JS rules.
pub(super) fn resolve_javascript_import(
    ctx: &ResolveCtx,
    importer: &Path,
    specifier: &str,
) -> Resolution {
    if specifier.starts_with('.') || specifier.starts_with('/') {
        return ctx.resolve_path(
            importer.parent().unwrap_or(&ctx.repo_root),
            specifier,
            web_extensions(),
        );
    }

    if let Some(path) = resolve_tsconfig_alias(ctx, importer, specifier) {
        return Resolution::Resolved(path);
    }

    if let Some(path) = resolve_package_imports(ctx, importer, specifier) {
        return Resolution::Resolved(path);
    }

    if let Some(path) = resolve_workspace_package(ctx, specifier) {
        return Resolution::Resolved(path);
    }

    Resolution::Unresolved
}

pub(super) fn is_internal_javascript_specifier(
    ctx: &ResolveCtx,
    importer: &Path,
    specifier: &str,
) -> bool {
    if specifier.starts_with('.') || specifier.starts_with('/') {
        return true;
    }

    if let Some(tsconfig) = nearest_tsconfig(ctx, importer)
        && (tsconfig
            .compiler_options
            .paths
            .keys()
            .any(|pattern| match_alias(pattern, specifier).is_some())
            || resolve_tsconfig_base_url(ctx, tsconfig, specifier).is_some())
    {
        return true;
    }

    if specifier.starts_with('#') && nearest_package(ctx, importer).is_some() {
        return true;
    }

    // Alias-looking specifiers that matched no configured alias above are still
    // treated as internal, so an unresolved alias (e.g. one defined only in a
    // bundler config we don't read) surfaces as an unresolved-import warning
    // rather than being silently dropped as an external package. `@/…` is not a
    // valid scoped package (those are `@scope/name`) and no npm package starts
    // with `~`, so both are unambiguous alias conventions.
    if specifier.starts_with("@/") || specifier.starts_with('~') {
        return true;
    }

    package_specifier_parts(specifier)
        .map(|(package_name, _)| ctx.package_by_name.contains_key(package_name))
        .unwrap_or(false)
}

fn resolve_tsconfig_alias(ctx: &ResolveCtx, importer: &Path, specifier: &str) -> Option<PathBuf> {
    let tsconfig = nearest_tsconfig(ctx, importer)?;
    // Targets resolve against baseUrl when set, else against the config file
    // that declared the paths (which may be an extended parent).
    let base_dir = tsconfig
        .compiler_options
        .base_dir
        .clone()
        .or_else(|| tsconfig.compiler_options.paths_dir.clone())
        .or_else(|| tsconfig.path.parent().map(Path::to_path_buf))?;

    // TypeScript picks the most specific pattern, not sorted-key order: exact
    // patterns first, then wildcards by longest literal prefix before `*`.
    let mut patterns: Vec<(&String, &Vec<String>)> =
        tsconfig.compiler_options.paths.iter().collect();
    patterns.sort_by_key(|(pattern, _)| {
        let prefix_len = pattern.split('*').next().unwrap_or("").len();
        (pattern.contains('*'), std::cmp::Reverse(prefix_len))
    });

    for (pattern, targets) in patterns {
        let Some(captures) = match_alias(pattern, specifier) else {
            continue;
        };

        for target in targets {
            let candidate = apply_alias_target(target, &captures);
            if let Resolution::Resolved(resolved) =
                ctx.resolve_path(&base_dir, &candidate, web_extensions())
            {
                return Some(resolved);
            }
        }
    }

    resolve_tsconfig_base_url(ctx, tsconfig, specifier)
}

fn resolve_tsconfig_base_url(
    ctx: &ResolveCtx,
    tsconfig: &TsConfigPath,
    specifier: &str,
) -> Option<PathBuf> {
    if specifier.starts_with('.') || specifier.starts_with('/') || specifier.starts_with('#') {
        return None;
    }

    let base_dir = tsconfig.compiler_options.base_dir.as_ref()?;
    if let Resolution::Resolved(resolved) = ctx.resolve_path(base_dir, specifier, web_extensions())
    {
        return Some(resolved);
    }

    None
}

/// The config governing `importer`: the nearest enclosing one whose merged
/// options actually declare paths/baseUrl, so an alias-less project config
/// doesn't shadow an alias-bearing root config. Falls back to nearest overall.
fn nearest_tsconfig<'a>(ctx: &'a ResolveCtx, importer: &Path) -> Option<&'a TsConfigPath> {
    let enclosing = ctx
        .tsconfigs
        .iter()
        .filter(|config| importer.starts_with(config.path.parent().unwrap_or(&ctx.repo_root)));

    enclosing
        .clone()
        .filter(|config| config.compiler_options.has_aliases())
        .max_by_key(|config| config.path.components().count())
        .or_else(|| enclosing.max_by_key(|config| config.path.components().count()))
}

fn resolve_workspace_package(ctx: &ResolveCtx, specifier: &str) -> Option<PathBuf> {
    let (package_name, rest) = package_specifier_parts(specifier)?;
    let package = ctx
        .package_by_name
        .get(package_name)
        .and_then(|index| ctx.packages.get(*index))?;

    if let Some(rest) = rest {
        let export_key = format!("./{rest}");
        for path in resolve_package_export(package, &export_key) {
            if let Some(resolved) = ctx.try_resolve_candidate(&path, web_extensions()) {
                return Some(resolved);
            }
        }

        let direct = package.root.join(rest);
        if let Some(resolved) = ctx.try_resolve_candidate(&direct, web_extensions()) {
            return Some(resolved);
        }

        let src_direct = package.root.join("src").join(rest);
        if let Some(resolved) = ctx.try_resolve_candidate(&src_direct, web_extensions()) {
            return Some(resolved);
        }

        return None;
    }

    for path in resolve_package_export(package, ".") {
        if let Some(resolved) = ctx.try_resolve_candidate(&path, web_extensions()) {
            return Some(resolved);
        }
    }

    for candidate in &package.entry_candidates {
        if let Some(resolved) = ctx.try_resolve_candidate(candidate, web_extensions()) {
            return Some(resolved);
        }
    }

    None
}

fn resolve_package_imports(ctx: &ResolveCtx, importer: &Path, specifier: &str) -> Option<PathBuf> {
    if !specifier.starts_with('#') {
        return None;
    }

    let package = nearest_package(ctx, importer)?;
    for path in resolve_package_import(package, specifier) {
        if let Some(resolved) = ctx.try_resolve_candidate(&path, web_extensions()) {
            return Some(resolved);
        }
    }

    None
}

fn nearest_package<'a>(
    ctx: &'a ResolveCtx,
    importer: &Path,
) -> Option<&'a crate::resolve::PackageInfo> {
    ctx.packages
        .iter()
        .filter(|package| importer.starts_with(&package.root))
        .max_by_key(|package| package.root.components().count())
}
