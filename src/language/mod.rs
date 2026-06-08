//! Language adapters.
//!
//! Each supported language is a single self-contained [`LanguageAdapter`]: it
//! declares the file extensions it owns, parses source into the shared
//! [`ModuleFacts`], and resolves its own import specifiers against the shared
//! [`ResolveCtx`]. The registry below is the one place languages are enumerated
//! — discovery (`fs`), parse dispatch (`parse`), and import resolution
//! (`resolve`) all derive from it, so adding a language is a single new adapter
//! plus one registry line.

use std::path::Path;
use std::sync::OnceLock;

use anyhow::Result;

use crate::parse::ModuleFacts;
use crate::resolve::{ResolveCtx, Resolution};

mod javascript;
use javascript::JavaScriptAdapter;

#[cfg(any(feature = "vue", feature = "svelte"))]
mod component;
#[cfg(feature = "svelte")]
use component::SvelteAdapter;
#[cfg(feature = "vue")]
use component::VueAdapter;

#[cfg(feature = "python")]
mod python;
#[cfg(feature = "python")]
use python::PythonAdapter;

#[cfg(feature = "rust")]
mod rust_lang;
#[cfg(feature = "rust")]
use rust_lang::RustAdapter;

#[cfg(feature = "ruby")]
mod ruby;
#[cfg(feature = "ruby")]
use ruby::RubyAdapter;

#[cfg(feature = "java")]
mod java;
#[cfg(feature = "java")]
use java::JavaAdapter;

/// A language's parsing and resolution behavior. Stateless: resolution reads
/// shared indexes from the [`ResolveCtx`] passed in.
pub(crate) trait LanguageAdapter: Send + Sync {
    /// File extensions this adapter owns, in resolution-preference order (e.g.
    /// `ts` before `js`). Used both for repo discovery and extension probing.
    fn extensions(&self) -> &'static [&'static str];

    /// Whether this adapter handles the given file path.
    fn handles(&self, path: &Path) -> bool {
        path.extension()
            .and_then(|ext| ext.to_str())
            .is_some_and(|ext| self.extensions().contains(&ext))
    }

    fn parse(&self, path: &Path, source: &str) -> Result<ModuleFacts>;

    fn resolve(&self, ctx: &ResolveCtx, importer: &Path, specifier: &str) -> Resolution;

    fn is_internal(&self, ctx: &ResolveCtx, importer: &Path, specifier: &str) -> bool;
}

fn registry() -> &'static [Box<dyn LanguageAdapter>] {
    static REGISTRY: OnceLock<Vec<Box<dyn LanguageAdapter>>> = OnceLock::new();
    REGISTRY.get_or_init(|| {
        // JavaScript/TypeScript is always present and is the default fallback,
        // so it must come first (its extensions also win resolution ties).
        // `mut` is unused when no optional language features are enabled.
        #[allow(unused_mut)]
        let mut adapters: Vec<Box<dyn LanguageAdapter>> = vec![Box::new(JavaScriptAdapter)];
        #[cfg(feature = "python")]
        adapters.push(Box::new(PythonAdapter));
        #[cfg(feature = "rust")]
        adapters.push(Box::new(RustAdapter));
        #[cfg(feature = "vue")]
        adapters.push(Box::new(VueAdapter));
        #[cfg(feature = "svelte")]
        adapters.push(Box::new(SvelteAdapter));
        #[cfg(feature = "ruby")]
        adapters.push(Box::new(RubyAdapter));
        #[cfg(feature = "java")]
        adapters.push(Box::new(JavaAdapter));
        adapters
    })
}

/// The adapter that owns `path`, falling back to JavaScript/TypeScript for any
/// extension no other adapter claims (matching historical parser behavior).
pub(crate) fn adapter_for(path: &Path) -> &'static dyn LanguageAdapter {
    let registry = registry();
    registry
        .iter()
        .find(|adapter| adapter.handles(path))
        .unwrap_or(&registry[0])
        .as_ref()
}

/// Every source extension across the compiled-in adapters. Used by repo
/// discovery to decide which files to index. Note this is the union across all
/// languages; per-language resolution only probes its own family's extensions
/// (see each adapter's resolution logic) so a Python import never resolves to a
/// `.ts` file, and vice versa.
fn source_extensions() -> &'static [&'static str] {
    static EXTENSIONS: OnceLock<Vec<&'static str>> = OnceLock::new();
    EXTENSIONS.get_or_init(|| {
        registry()
            .iter()
            .flat_map(|adapter| adapter.extensions().iter().copied())
            .collect()
    })
}

/// Whether `ext` belongs to any compiled-in language (used by repo discovery).
pub(crate) fn is_source_extension(ext: &str) -> bool {
    source_extensions().contains(&ext)
}
