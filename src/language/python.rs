use std::path::{Path, PathBuf};

use anyhow::Result;

use crate::parse::{ModuleFacts, parse_python_module};
use crate::resolve::{Resolution, ResolveCtx, clean_path};

use super::LanguageAdapter;

pub(super) struct PythonAdapter;

impl LanguageAdapter for PythonAdapter {
    fn extensions(&self) -> &'static [&'static str] {
        &["py"]
    }

    fn parse(&self, path: &Path, source: &str) -> Result<ModuleFacts> {
        parse_python_module(path, source)
    }

    fn resolve(&self, ctx: &ResolveCtx, importer: &Path, specifier: &str) -> Resolution {
        match resolve_python_import(ctx, importer, specifier) {
            Some(path) => Resolution::Resolved(path),
            None => Resolution::Unresolved,
        }
    }

    fn is_internal(&self, ctx: &ResolveCtx, _importer: &Path, specifier: &str) -> bool {
        if specifier.starts_with('.') {
            return true;
        }
        python_top_level_exists(ctx, specifier)
    }
}

/// Absolute Python imports are resolved against the repo root and a `src/`
/// layout root, covering both flat and `src/my_pkg/...` project shapes.
const PYTHON_SOURCE_ROOTS: &[&str] = &["", "src"];

fn resolve_python_import(ctx: &ResolveCtx, importer: &Path, specifier: &str) -> Option<PathBuf> {
    if specifier.starts_with('.') {
        return resolve_python_relative_import(ctx, importer, specifier);
    }

    let module_path = specifier.replace('.', "/");
    for root in PYTHON_SOURCE_ROOTS {
        let candidate = ctx.repo_root.join(root).join(&module_path);
        if let Some(path) = try_resolve_python_module_candidate(ctx, &candidate) {
            return Some(path);
        }
    }
    None
}

fn resolve_python_relative_import(
    ctx: &ResolveCtx,
    importer: &Path,
    specifier: &str,
) -> Option<PathBuf> {
    let level = specifier.chars().take_while(|char| *char == '.').count();
    let remainder = specifier.trim_start_matches('.');
    let mut base = importer.parent().unwrap_or(&ctx.repo_root).to_path_buf();

    for _ in 1..level {
        base.pop();
    }

    let candidate = if remainder.is_empty() {
        base
    } else {
        base.join(remainder.replace('.', "/"))
    };
    try_resolve_python_module_candidate(ctx, &candidate)
}

fn try_resolve_python_module_candidate(ctx: &ResolveCtx, candidate: &Path) -> Option<PathBuf> {
    if let Some(path) = ctx.try_resolve_candidate(candidate, &["py"]) {
        return Some(path);
    }

    let package_init = clean_path(&candidate.join("__init__.py"));
    if ctx.source_files.contains(&package_init) {
        return Some(package_init);
    }

    None
}

fn python_top_level_exists(ctx: &ResolveCtx, specifier: &str) -> bool {
    let Some(first) = specifier.split('.').next() else {
        return false;
    };
    if first.is_empty() {
        return false;
    }

    PYTHON_SOURCE_ROOTS.iter().any(|root| {
        let base = ctx.repo_root.join(root);
        let module_file = clean_path(&base.join(format!("{first}.py")));
        let package_init = clean_path(&base.join(first).join("__init__.py"));
        ctx.source_files.contains(&module_file) || ctx.source_files.contains(&package_init)
    })
}
