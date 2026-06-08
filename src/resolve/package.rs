use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::Deserialize;
use serde_json::Value;

use super::{apply_alias_target, match_alias};

#[derive(Debug, Clone)]
pub(crate) struct PackageInfo {
    pub(super) name: String,
    pub(super) root: PathBuf,
    pub(super) entry_candidates: Vec<PathBuf>,
    export_mappings: Vec<ExportMapping>,
}

#[derive(Debug, Clone)]
struct ExportMapping {
    key: String,
    target: String,
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
    let export_mappings = collect_export_mappings(parsed.exports.as_ref());

    Ok(Some(PackageInfo {
        name: parsed.name,
        root,
        entry_candidates,
        export_mappings,
    }))
}

fn collect_export_mappings(exports: Option<&Value>) -> Vec<ExportMapping> {
    let Some(Value::Object(map)) = exports else {
        return Vec::new();
    };

    let mut mappings = Vec::new();
    for (key, value) in map {
        if !key.starts_with('.') {
            continue;
        }
        if let Some(target) = export_target(value) {
            mappings.push(ExportMapping {
                key: key.clone(),
                target,
            });
        }
    }
    mappings
}

fn export_target(value: &Value) -> Option<String> {
    match value {
        Value::String(path) => Some(path.clone()),
        Value::Object(map) => {
            for key in ["dev", "source"] {
                if let Some(Value::String(path)) = map.get(key) {
                    return Some(path.clone());
                }
            }

            for key in ["default", "import", "require"] {
                if let Some(target) = map.get(key).and_then(export_target) {
                    return Some(target);
                }
            }

            None
        }
        _ => None,
    }
}

pub(crate) fn resolve_package_export(package: &PackageInfo, export_key: &str) -> Option<PathBuf> {
    for mapping in &package.export_mappings {
        if let Some(captures) = match_alias(&mapping.key, export_key) {
            let target = apply_alias_target(&mapping.target, &captures);
            return Some(package.root.join(target));
        }
    }
    None
}
