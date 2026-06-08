use std::path::{Path, PathBuf};

use anyhow::Result;

use crate::parse::{ModuleFacts, parse_rust_module};
use crate::resolve::{ResolveCtx, Resolution, clean_path};

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
            try_resolve_rust_module_candidate(ctx, &base)
        }
    }
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
