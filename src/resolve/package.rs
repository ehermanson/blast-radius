use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::Deserialize;
use serde_json::Value;

use super::{apply_alias_target, match_alias};

#[derive(Debug, Clone)]
pub(crate) struct PackageInfo {
    pub(super) name: String,
    pub(crate) root: PathBuf,
    pub(crate) entry_candidates: Vec<PathBuf>,
    export_mappings: Vec<ExportMapping>,
    import_mappings: Vec<ExportMapping>,
}

#[derive(Debug, Clone)]
struct ExportMapping {
    key: String,
    targets: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct PackageJson {
    #[serde(default)]
    name: String,
    #[serde(default)]
    main: Option<String>,
    #[serde(default)]
    module: Option<String>,
    #[serde(default)]
    source: Option<String>,
    #[serde(default)]
    types: Option<String>,
    #[serde(default)]
    exports: Option<Value>,
    #[serde(default)]
    imports: Option<Value>,
}

pub(crate) fn package_specifier_parts(specifier: &str) -> Option<(&str, Option<&str>)> {
    if specifier.is_empty() || specifier.starts_with('.') || specifier.starts_with('/') {
        return None;
    }

    if specifier.starts_with('@') {
        let first_slash = specifier.find('/')?;
        let rest_start = first_slash + 1;
        let second_slash = specifier[rest_start..]
            .find('/')
            .map(|index| rest_start + index);
        return match second_slash {
            Some(index) => Some((&specifier[..index], Some(&specifier[index + 1..]))),
            None => Some((specifier, None)),
        };
    }

    match specifier.split_once('/') {
        Some((name, rest)) => Some((name, Some(rest))),
        None => Some((specifier, None)),
    }
}

pub(super) fn load_package_info(path: &Path) -> Result<Option<PackageInfo>> {
    let contents = fs::read_to_string(path)
        .with_context(|| format!("failed to read package.json {}", path.display()))?;
    let parsed: PackageJson = serde_json::from_str(&contents)
        .with_context(|| format!("failed to parse package.json {}", path.display()))?;

    if parsed.name.is_empty() {
        return Ok(None);
    }

    let root = path.parent().unwrap_or(path).to_path_buf();
    let mut entry_candidates = Vec::new();
    for value in [parsed.source, parsed.module, parsed.types, parsed.main]
        .into_iter()
        .flatten()
    {
        entry_candidates.push(root.join(value));
    }
    entry_candidates.push(root.join("src/index.ts"));
    entry_candidates.push(root.join("src/index.tsx"));
    entry_candidates.push(root.join("src/index.js"));
    entry_candidates.push(root.join("src/index.jsx"));
    entry_candidates.push(root.join("index.ts"));
    entry_candidates.push(root.join("index.tsx"));
    entry_candidates.push(root.join("index.js"));
    entry_candidates.push(root.join("index.jsx"));
    let export_mappings = collect_package_mappings(parsed.exports.as_ref(), ".");
    let import_mappings = collect_package_mappings(parsed.imports.as_ref(), "#");

    Ok(Some(PackageInfo {
        name: parsed.name,
        root,
        entry_candidates,
        export_mappings,
        import_mappings,
    }))
}

fn collect_package_mappings(value: Option<&Value>, required_prefix: &str) -> Vec<ExportMapping> {
    let Some(value) = value else {
        return Vec::new();
    };

    if required_prefix == "." && !matches!(value, Value::Object(_)) {
        let targets = conditional_targets(value);
        return if targets.is_empty() {
            Vec::new()
        } else {
            vec![ExportMapping {
                key: ".".to_string(),
                targets,
            }]
        };
    }

    let Value::Object(map) = value else {
        return Vec::new();
    };

    if required_prefix == "." && !map.keys().any(|key| key.starts_with('.')) {
        let targets = conditional_targets(value);
        return if targets.is_empty() {
            Vec::new()
        } else {
            vec![ExportMapping {
                key: ".".to_string(),
                targets,
            }]
        };
    }

    let mut mappings = Vec::new();
    for (key, value) in map {
        if !key.starts_with(required_prefix) {
            continue;
        }
        let targets = conditional_targets(value);
        if !targets.is_empty() {
            mappings.push(ExportMapping {
                key: key.clone(),
                targets,
            });
        }
    }
    mappings
}

fn conditional_targets(value: &Value) -> Vec<String> {
    match value {
        Value::String(path) => vec![path.clone()],
        Value::Array(values) => values
            .iter()
            .flat_map(conditional_targets)
            .collect::<Vec<_>>(),
        Value::Object(map) => {
            let mut targets = Vec::new();
            for key in ["development", "dev", "source", "types"] {
                if let Some(Value::String(path)) = map.get(key) {
                    targets.push(path.clone());
                } else if let Some(value) = map.get(key) {
                    targets.extend(conditional_targets(value));
                }
            }

            // Unknown custom conditions often point at source in monorepos.
            // Prefer them over runtime fallbacks such as `default`.
            for (key, value) in map {
                if matches!(
                    key.as_str(),
                    "development" | "dev" | "source" | "types" | "default" | "import" | "require"
                ) {
                    continue;
                }
                targets.extend(conditional_targets(value));
            }

            for key in ["import", "require", "default"] {
                if let Some(value) = map.get(key) {
                    targets.extend(conditional_targets(value));
                }
            }

            dedupe(targets)
        }
        _ => Vec::new(),
    }
}

fn dedupe(values: Vec<String>) -> Vec<String> {
    let mut deduped = Vec::new();
    for value in values {
        if !deduped.contains(&value) {
            deduped.push(value);
        }
    }
    deduped
}

pub(crate) fn resolve_package_export(package: &PackageInfo, export_key: &str) -> Vec<PathBuf> {
    resolve_package_mapping(&package.export_mappings, &package.root, export_key)
}

pub(crate) fn resolve_package_import(package: &PackageInfo, import_key: &str) -> Vec<PathBuf> {
    resolve_package_mapping(&package.import_mappings, &package.root, import_key)
}

fn resolve_package_mapping(mappings: &[ExportMapping], root: &Path, key: &str) -> Vec<PathBuf> {
    // Exact (non-wildcard) export keys take precedence over wildcard patterns,
    // matching Node's `exports` resolution.
    for mapping in mappings {
        if !mapping.key.contains('*') && mapping.key == key {
            return mapping
                .targets
                .iter()
                .map(|target| root.join(target))
                .collect();
        }
    }

    // Among wildcard patterns, the most specific wins: the longest literal
    // prefix before `*`.
    let mut wildcards: Vec<&ExportMapping> = mappings
        .iter()
        .filter(|mapping| mapping.key.contains('*'))
        .collect();
    wildcards.sort_by_key(|mapping| {
        std::cmp::Reverse(mapping.key.split('*').next().unwrap_or("").len())
    });

    for mapping in wildcards {
        if let Some(captures) = match_alias(&mapping.key, key) {
            return mapping
                .targets
                .iter()
                .map(|target| root.join(apply_alias_target(target, &captures)))
                .collect();
        }
    }

    Vec::new()
}
