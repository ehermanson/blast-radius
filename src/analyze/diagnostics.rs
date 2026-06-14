use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::parse::{ImportFact, ModuleFacts};
use crate::resolve::Resolution;

use super::{ResolutionCache, relative_label};

#[derive(Debug, Clone, Default)]
pub(super) struct UnresolvedDiagnostics {
    pub(super) count: usize,
    pub(super) warnings: Vec<String>,
}

pub(super) fn unresolved_import_diagnostics(
    modules: &BTreeMap<PathBuf, ModuleFacts>,
    resolution_cache: &mut ResolutionCache<'_>,
    ignore: &[String],
    repo_root: &Path,
    explain: bool,
) -> UnresolvedDiagnostics {
    let mut count = 0;
    let mut groups: BTreeMap<&'static str, BTreeMap<String, UnresolvedImportEntry>> =
        BTreeMap::new();

    for module in modules.values() {
        for import in &module.imports {
            if !should_count_unresolved_import(import, ignore) {
                continue;
            }
            if !resolution_cache.is_internal_specifier(&module.file, &import.source) {
                continue;
            }
            if matches!(
                resolution_cache.resolve(&module.file, &import.source),
                Resolution::Unresolved
            ) {
                count += 1;
                if explain {
                    let reason = unresolved_reason(&import.source);
                    let entry = groups
                        .entry(reason)
                        .or_default()
                        .entry(import.source.clone())
                        .or_insert_with(|| UnresolvedImportEntry {
                            count: 0,
                            example_importer: relative_label(repo_root, &module.file),
                        });
                    entry.count += 1;
                }
            }
        }
    }

    let mut warnings = Vec::new();
    if explain && count > 0 {
        warnings.push(format!(
            "unresolved import details: {count} internal import{} could not be resolved",
            if count == 1 { "" } else { "s" }
        ));
        for (reason, imports) in groups {
            warnings.push(format!("unresolved imports · {reason}:"));
            for (specifier, entry) in imports {
                warnings.push(format!(
                    "  {specifier} ({} occurrence{}, e.g. {})",
                    entry.count,
                    if entry.count == 1 { "" } else { "s" },
                    entry.example_importer
                ));
            }
        }
    }

    UnresolvedDiagnostics { count, warnings }
}

#[derive(Debug, Clone)]
struct UnresolvedImportEntry {
    count: usize,
    example_importer: String,
}

fn unresolved_reason(specifier: &str) -> &'static str {
    if specifier.starts_with('.') || specifier.starts_with('/') {
        return "relative or absolute path";
    }
    if specifier.starts_with('#') {
        return "package.json imports";
    }
    // `@/…` / `~…` are path-alias conventions; if they reach here the alias isn't
    // visible to blast-radius (often defined only in a bundler config, or in a
    // jsconfig/tsconfig we didn't find). Point the user at how to fix it.
    if specifier.starts_with("@/") || specifier.starts_with('~') {
        return "path alias not configured (add it to tsconfig/jsconfig paths, or .blast-radius.json)";
    }
    if specifier.starts_with('@') {
        return "tsconfig paths or workspace package export";
    }
    "workspace package export or tsconfig baseUrl"
}

/// Import extensions that are assets/data rather than code modules, so a missing
/// resolution is expected and shouldn't count against the unresolved metric.
/// These are language-neutral; repo/tooling-specific virtual modules (e.g. CSS-in-JS
/// codegen, route type stubs) are declared per-repo via `ignore_unresolved`.
const NON_CODE_IMPORT_EXTENSIONS: &[&str] = &[
    ".svg", ".png", ".jpg", ".jpeg", ".gif", ".webp", ".avif", ".css", ".scss", ".sass", ".less",
    ".json", ".yaml", ".yml", ".md", ".mdx",
];

fn should_count_unresolved_import(import: &ImportFact, ignore: &[String]) -> bool {
    if import.type_only {
        return false;
    }

    let source = import.source.as_str();
    // Bundler imports carry query/hash suffixes (`./logo.svg?react`, `./a.css#x`);
    // match the extension on the path portion only.
    let path = source.split(['?', '#']).next().unwrap_or(source);
    if NON_CODE_IMPORT_EXTENSIONS
        .iter()
        .any(|extension| path.ends_with(extension))
    {
        return false;
    }

    if ignore.iter().any(|pattern| source.contains(pattern)) {
        return false;
    }

    true
}

#[cfg(test)]
mod tests {
    use crate::parse::{ImportKind, ImportTarget};

    use super::*;

    fn import(source: &str, type_only: bool) -> ImportFact {
        ImportFact {
            source: source.to_string(),
            local: "local".to_string(),
            imported: ImportTarget::Default,
            kind: ImportKind::Esm,
            type_only,
        }
    }

    #[test]
    fn skips_asset_imports_without_any_config() {
        for source in [
            "./package.json",
            "./logo.svg",
            "../styles/theme.css",
            "./tokens.json",
            "./content.mdx",
        ] {
            assert!(!should_count_unresolved_import(&import(source, false), &[]));
        }
    }

    #[test]
    fn skips_asset_imports_with_query_or_hash_suffixes() {
        for source in [
            "./logo.svg?react",
            "./style.css?inline",
            "./data.json?raw",
            "./icon.svg#symbol",
            "./image.png?url",
        ] {
            assert!(!should_count_unresolved_import(&import(source, false), &[]));
        }
    }

    #[test]
    fn skips_repo_configured_ignore_patterns() {
        let ignore = vec![
            ".velite".to_string(),
            "/+types/".to_string(),
            "styled-system/css".to_string(),
            "/dist/esm/".to_string(),
        ];
        for source in [
            "./.velite/generated",
            "./+types/root",
            "./routes/+types/page",
            "./styled-system/css",
            "./pkg/dist/esm/index",
        ] {
            assert!(!should_count_unresolved_import(
                &import(source, false),
                &ignore
            ));
        }
    }

    #[test]
    fn skips_type_only_unresolved_imports() {
        assert!(!should_count_unresolved_import(
            &import("./types", true),
            &[]
        ));
    }

    #[test]
    fn counts_regular_runtime_imports() {
        assert!(should_count_unresolved_import(
            &import("./missing", false),
            &[]
        ));
        // The same specifier with no matching ignore pattern is still counted.
        assert!(should_count_unresolved_import(
            &import("./missing", false),
            &["styled-system/css".to_string()]
        ));
    }
}
