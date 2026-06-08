use std::path::Path;

use anyhow::Result;

use crate::parse::{ModuleFacts, parse_component_module};
use crate::resolve::{ResolveCtx, Resolution};

use super::LanguageAdapter;
use super::javascript::{is_internal_javascript_specifier, resolve_javascript_import};

// Vue and Svelte single-file components extract their `<script>` block and parse
// it as JS/TS, so they resolve imports through the same JavaScript rules.

#[cfg(feature = "vue")]
pub(super) struct VueAdapter;

#[cfg(feature = "vue")]
impl LanguageAdapter for VueAdapter {
    fn extensions(&self) -> &'static [&'static str] {
        &["vue"]
    }

    fn parse(&self, path: &Path, source: &str) -> Result<ModuleFacts> {
        parse_component_module(path, source, "vue")
    }

    fn resolve(&self, ctx: &ResolveCtx, importer: &Path, specifier: &str) -> Resolution {
        resolve_javascript_import(ctx, importer, specifier)
    }

    fn is_internal(&self, ctx: &ResolveCtx, importer: &Path, specifier: &str) -> bool {
        is_internal_javascript_specifier(ctx, importer, specifier)
    }
}

#[cfg(feature = "svelte")]
pub(super) struct SvelteAdapter;

#[cfg(feature = "svelte")]
impl LanguageAdapter for SvelteAdapter {
    fn extensions(&self) -> &'static [&'static str] {
        &["svelte"]
    }

    fn parse(&self, path: &Path, source: &str) -> Result<ModuleFacts> {
        parse_component_module(path, source, "svelte")
    }

    fn resolve(&self, ctx: &ResolveCtx, importer: &Path, specifier: &str) -> Resolution {
        resolve_javascript_import(ctx, importer, specifier)
    }

    fn is_internal(&self, ctx: &ResolveCtx, importer: &Path, specifier: &str) -> bool {
        is_internal_javascript_specifier(ctx, importer, specifier)
    }
}
