use std::collections::BTreeMap;
use std::path::PathBuf;

use crate::parse::{ImportFact, ModuleFacts};
use crate::resolve::Resolution;

use super::ResolutionCache;

pub(super) fn count_unresolved_imports(
    modules: &BTreeMap<PathBuf, ModuleFacts>,
    resolution_cache: &mut ResolutionCache<'_>,
) -> usize {
    let mut count = 0;

    for module in modules.values() {
        for import in &module.imports {
            if !should_count_unresolved_import(import) {
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
            }
        }
    }

    count
}

fn should_count_unresolved_import(import: &ImportFact) -> bool {
    if import.type_only {
        return false;
    }

    let source = import.source.as_str();
    if source.contains(".velite") {
        return false;
    }
    if source.contains("/+types/") || source.starts_with("./+types/") {
        return false;
    }
    if source.ends_with("package.json") {
        return false;
    }
    if source.ends_with(".svg") {
        return false;
    }
    if source.contains("styled-system/recipes")
        || source.contains("styled-system/patterns")
        || source.contains("styled-system/css")
    {
        return false;
    }
    if source.contains("/dist/esm/") || source.contains("/dist/cjs/") {
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
    fn skips_unresolved_imports_that_are_known_generated_or_asset_inputs() {
        for source in [
            "./.velite/generated",
            "./+types/root",
            "./routes/+types/page",
            "./package.json",
            "./logo.svg",
            "./styled-system/recipes",
            "./styled-system/patterns",
            "./styled-system/css",
            "./pkg/dist/esm/index",
            "./pkg/dist/cjs/index",
        ] {
            assert!(!should_count_unresolved_import(&import(source, false)));
        }
    }

    #[test]
    fn skips_type_only_unresolved_imports() {
        assert!(!should_count_unresolved_import(&import("./types", true)));
    }

    #[test]
    fn counts_regular_runtime_imports() {
        assert!(should_count_unresolved_import(&import("./missing", false)));
    }
}
