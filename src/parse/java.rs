use std::path::Path;

use anyhow::Result;

use super::{ExportFact, ExportKind, ImportFact, ImportKind, ImportTarget, ModuleFacts};

pub(super) fn parse_java_module(path: &Path, source: &str) -> Result<ModuleFacts> {
    let mut facts = ModuleFacts::empty(path);

    for line in source.lines() {
        let line = line.trim();
        if line.starts_with("//") || line.is_empty() {
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
                ImportTarget::Namespace
            } else {
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
