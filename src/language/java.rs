use std::path::{Path, PathBuf};

use anyhow::Result;

use crate::parse::{ModuleFacts, parse_java_module};
use crate::resolve::{ResolveCtx, Resolution};

use super::LanguageAdapter;

pub(super) struct JavaAdapter;

impl LanguageAdapter for JavaAdapter {
    fn extensions(&self) -> &'static [&'static str] {
        &["java"]
    }

    fn parse(&self, path: &Path, source: &str) -> Result<ModuleFacts> {
        parse_java_module(path, source)
    }

    fn resolve(&self, ctx: &ResolveCtx, _importer: &Path, specifier: &str) -> Resolution {
        match resolve_java_import(ctx, specifier) {
            Some(path) => Resolution::Resolved(path),
            None => Resolution::Unresolved,
        }
    }

    fn is_internal(&self, ctx: &ResolveCtx, _importer: &Path, specifier: &str) -> bool {
        resolve_java_import(ctx, specifier).is_some()
    }
}

fn resolve_java_import(ctx: &ResolveCtx, specifier: &str) -> Option<PathBuf> {
    if specifier.ends_with(".*") {
        let package_path = specifier.trim_end_matches(".*").replace('.', "/");
        return ctx
            .java_package_index
            .get(&PathBuf::from(package_path))
            .and_then(|files| files.first().cloned());
    }

    ctx.suffix_index
        .get(&PathBuf::from(format!("{}.java", specifier.replace('.', "/"))))
        .cloned()
}
