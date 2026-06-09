use std::collections::BTreeSet;
use std::path::Path;

use anyhow::Result;

use super::{ExportFact, ExportKind, ImportFact, ImportKind, ImportTarget, ModuleFacts};

pub(crate) fn parse_java_module(path: &Path, source: &str) -> Result<ModuleFacts> {
    let mut facts = ModuleFacts::empty(path);
    let mut wildcard_packages = Vec::new();
    let mut imported_names = BTreeSet::new();
    let mut used_types = BTreeSet::new();

    for line in source.lines() {
        let line = line.trim();
        if line.starts_with("//") || line.starts_with("package ") || line.is_empty() {
            continue;
        }

        if let Some(import) = java_import(line) {
            let local = import
                .rsplit('.')
                .next()
                .unwrap_or(&import)
                .trim_end_matches(".*")
                .to_string();
            let imported = if import.ends_with(".*") {
                if let Some(package) = import.strip_suffix(".*") {
                    wildcard_packages.push(package.to_string());
                }
                ImportTarget::Namespace
            } else {
                imported_names.insert(local.clone());
                ImportTarget::Name(local.clone())
            };
            facts.imports.push(ImportFact {
                source: import,
                local,
                imported,
                kind: ImportKind::Esm,
                type_only: false,
            });
            continue;
        }

        if let Some(name) = java_declared_type(line) {
            facts.exports.push(ExportFact {
                exported: name.clone(),
                local: Some(name),
                kind: ExportKind::Local,
            });
        }

        collect_java_type_tokens(line, &mut used_types);
    }

    // A wildcard import can bind any type of its package, so fan out: emit one
    // named fact per type-looking identifier used in the body. Candidates that
    // do not exist in the package simply stay unresolved (and external, so they
    // are not counted as unresolved internal imports).
    let declared: BTreeSet<&str> = facts
        .exports
        .iter()
        .filter_map(|export| export.local.as_deref())
        .collect();
    for package in &wildcard_packages {
        for name in &used_types {
            if declared.contains(name.as_str()) || imported_names.contains(name) {
                continue;
            }
            facts.imports.push(ImportFact {
                source: format!("{package}.{name}"),
                local: name.clone(),
                imported: ImportTarget::Name(name.clone()),
                kind: ImportKind::Esm,
                type_only: false,
            });
        }
    }

    facts
        .used_locals
        .extend(facts.imports.iter().map(|import| import.local.clone()));
    Ok(facts)
}

fn java_import(line: &str) -> Option<String> {
    let rest = line.strip_prefix("import ")?.trim_start();
    let rest = rest.strip_prefix("static ").unwrap_or(rest).trim_start();
    Some(rest.trim_end_matches(';').trim().to_string())
}

fn java_declared_type(line: &str) -> Option<String> {
    let tokens: Vec<&str> = line
        .split(|ch: char| ch.is_whitespace() || ch == '{' || ch == '<')
        .filter(|token| !token.is_empty())
        .collect();
    for pair in tokens.windows(2) {
        if matches!(pair[0], "class" | "interface" | "enum" | "record") {
            return Some(pair[1].to_string());
        }
    }
    None
}

/// Collect identifiers that look like type references (leading uppercase, Java
/// convention) as candidates for wildcard-import fan-out.
fn collect_java_type_tokens(line: &str, used: &mut BTreeSet<String>) {
    for token in line.split(|ch: char| !ch.is_ascii_alphanumeric() && ch != '_') {
        if token
            .chars()
            .next()
            .is_some_and(|ch| ch.is_ascii_uppercase())
        {
            used.insert(token.to_string());
        }
    }
}
