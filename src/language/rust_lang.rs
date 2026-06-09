use std::path::{Path, PathBuf};

use anyhow::Result;

use crate::parse::{ModuleFacts, parse_rust_module};
use crate::resolve::{Resolution, ResolveCtx, clean_path};

use super::LanguageAdapter;

pub(super) struct RustAdapter;

impl LanguageAdapter for RustAdapter {
    fn extensions(&self) -> &'static [&'static str] {
        &["rs"]
    }

    fn parse(&self, path: &Path, source: &str) -> Result<ModuleFacts> {
        parse_rust_module(path, source)
    }

    fn resolve(&self, ctx: &ResolveCtx, importer: &Path, specifier: &str) -> Resolution {
        match resolve_rust_import(ctx, importer, specifier) {
            Some(path) => Resolution::Resolved(path),
            None => Resolution::Unresolved,
        }
    }

    fn is_internal(&self, ctx: &ResolveCtx, importer: &Path, specifier: &str) -> bool {
        if specifier.starts_with("mod:")
            || specifier.starts_with("crate::")
            || specifier.starts_with("self::")
            || specifier.starts_with("super::")
        {
            return true;
        }
        rust_top_level_exists(ctx, importer, specifier)
    }
}

fn resolve_rust_import(ctx: &ResolveCtx, importer: &Path, specifier: &str) -> Option<PathBuf> {
    if let Some(module) = specifier.strip_prefix("mod:") {
        let base = rust_child_module_base(importer);
        return try_resolve_rust_module_candidate(ctx, &base.join(module));
    }

    let parts: Vec<&str> = specifier
        .split("::")
        .filter(|part| !part.is_empty())
        .collect();
    let (head, rest) = parts.split_first()?;

    match *head {
        "crate" => resolve_rust_from_crate_roots(ctx, importer, rest),
        "self" => {
            let base = rust_child_module_base(importer).join(rest.join("/"));
            try_resolve_rust_module_candidate(ctx, &base)
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
            try_resolve_rust_module_candidate(ctx, &base)
        }
        _ => {
            if let Some(path) = resolve_rust_from_crate_roots(ctx, importer, &parts) {
                return Some(path);
            }
            let base = rust_child_module_base(importer).join(parts.join("/"));
            if let Some(path) = try_resolve_rust_module_candidate(ctx, &base) {
                return Some(path);
            }
            resolve_rust_workspace_import(ctx, head, rest)
        }
    }
}

/// Resolve `use other_crate::...` against a sibling workspace crate, mapping
/// the path head to a crate root via each crate's `Cargo.toml` package name
/// (hyphens normalized to underscores, as in Rust paths).
fn resolve_rust_workspace_import(ctx: &ResolveCtx, head: &str, rest: &[&str]) -> Option<PathBuf> {
    let root = rust_workspace_crate_root(ctx, head)?;
    if rest.is_empty() {
        // `use other_crate::SomeItem` parses down to the bare crate path, which
        // is the crate's entrypoint file.
        for entry in ["lib.rs", "main.rs"] {
            let candidate = clean_path(&root.join(entry));
            if ctx.source_files.contains(&candidate) {
                return Some(candidate);
            }
        }
        return None;
    }
    try_resolve_rust_module_candidate(ctx, &root.join(rest.join("/")))
}

fn rust_workspace_crate_root(ctx: &ResolveCtx, name: &str) -> Option<PathBuf> {
    for root in rust_crate_roots(ctx) {
        let Some(package_dir) = root.parent() else {
            continue;
        };
        if !package_dir.starts_with(&ctx.repo_root) {
            continue;
        }
        let Ok(manifest) = std::fs::read_to_string(package_dir.join("Cargo.toml")) else {
            continue;
        };
        if cargo_package_name(&manifest).is_some_and(|package| package.replace('-', "_") == name) {
            return Some(root);
        }
    }
    None
}

/// Extract `[package] name` from a Cargo.toml manifest.
fn cargo_package_name(manifest: &str) -> Option<String> {
    let manifest: toml::Value = manifest.parse().ok()?;
    Some(manifest.get("package")?.get("name")?.as_str()?.to_string())
}

/// Resolve a path rooted at a crate root. `crate::` paths are anchored to the
/// importer's own crate (its nearest enclosing `lib.rs`/`main.rs` directory) so
/// that, in a multi-crate workspace, `crate::models` from crate B does not
/// resolve into crate A's identically-named module.
fn resolve_rust_from_crate_roots(
    ctx: &ResolveCtx,
    importer: &Path,
    parts: &[&str],
) -> Option<PathBuf> {
    if parts.is_empty() {
        return None;
    }

    for root in crate_roots_for(ctx, importer) {
        let candidate = root.join(parts.join("/"));
        if let Some(path) = try_resolve_rust_module_candidate(ctx, &candidate) {
            return Some(path);
        }
    }
    None
}

/// The crate roots to resolve against for an import in `importer`: the nearest
/// enclosing crate root if one exists, otherwise every crate root as a fallback.
fn crate_roots_for(ctx: &ResolveCtx, importer: &Path) -> Vec<PathBuf> {
    let roots = rust_crate_roots(ctx);
    match roots
        .iter()
        .filter(|root| importer.starts_with(root))
        .max_by_key(|root| root.components().count())
    {
        Some(enclosing) => vec![enclosing.clone()],
        None => roots,
    }
}

fn rust_crate_roots(ctx: &ResolveCtx) -> Vec<PathBuf> {
    let mut roots = Vec::new();
    for file in &ctx.source_files {
        let Some(name) = file.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        if matches!(name, "lib.rs" | "main.rs")
            && let Some(parent) = file.parent()
        {
            roots.push(parent.to_path_buf());
        }
    }
    roots.sort();
    roots.dedup();
    if roots.is_empty() {
        roots.push(ctx.repo_root.clone());
    }
    roots
}

fn try_resolve_rust_module_candidate(ctx: &ResolveCtx, candidate: &Path) -> Option<PathBuf> {
    if let Some(path) = ctx.try_resolve_candidate(candidate, &["rs"]) {
        return Some(path);
    }

    let mod_file = clean_path(&candidate.join("mod.rs"));
    if ctx.source_files.contains(&mod_file) {
        return Some(mod_file);
    }

    None
}

fn rust_top_level_exists(ctx: &ResolveCtx, importer: &Path, specifier: &str) -> bool {
    let Some(first) = specifier.split("::").next() else {
        return false;
    };
    if first.is_empty() {
        return false;
    }

    crate_roots_for(ctx, importer)
        .into_iter()
        .any(|root| try_resolve_rust_module_candidate(ctx, &root.join(first)).is_some())
        || rust_workspace_crate_root(ctx, first).is_some()
}

fn rust_child_module_base(importer: &Path) -> PathBuf {
    let parent = importer.parent().unwrap_or_else(|| Path::new(""));
    match importer.file_name().and_then(|name| name.to_str()) {
        Some("lib.rs" | "main.rs" | "mod.rs") => parent.to_path_buf(),
        _ => parent.join(importer.file_stem().unwrap_or_default()),
    }
}

fn rust_parent_module_base(importer: &Path) -> PathBuf {
    let child_base = rust_child_module_base(importer);
    child_base
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or(child_base)
}
