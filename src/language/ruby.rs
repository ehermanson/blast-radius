use std::path::{Path, PathBuf};

use anyhow::Result;

use crate::parse::{ModuleFacts, parse_ruby_module};
use crate::resolve::{ResolveCtx, Resolution};

use super::LanguageAdapter;

pub(super) struct RubyAdapter;

impl LanguageAdapter for RubyAdapter {
    fn extensions(&self) -> &'static [&'static str] {
        &["rb"]
    }

    fn parse(&self, path: &Path, source: &str) -> Result<ModuleFacts> {
        parse_ruby_module(path, source)
    }

    fn resolve(&self, ctx: &ResolveCtx, importer: &Path, specifier: &str) -> Resolution {
        match resolve_ruby_import(ctx, importer, specifier) {
            Some(path) => Resolution::Resolved(path),
            None => Resolution::Unresolved,
        }
    }

    fn is_internal(&self, ctx: &ResolveCtx, importer: &Path, specifier: &str) -> bool {
        specifier.starts_with('.') || resolve_ruby_import(ctx, importer, specifier).is_some()
    }
}

fn resolve_ruby_import(ctx: &ResolveCtx, importer: &Path, specifier: &str) -> Option<PathBuf> {
    if specifier.starts_with('.') {
        let base = importer.parent().unwrap_or(&ctx.repo_root);
        return ctx.try_resolve_candidate(&base.join(specifier));
    }

    for candidate in [
        ctx.repo_root.join(specifier),
        ctx.repo_root.join("lib").join(specifier),
        ctx.repo_root.join("app").join(specifier),
    ] {
        if let Some(path) = ctx.try_resolve_candidate(&candidate) {
            return Some(path);
        }
    }

    ctx.suffix_index
        .get(&PathBuf::from(format!("{specifier}.rb")))
        .cloned()
}
