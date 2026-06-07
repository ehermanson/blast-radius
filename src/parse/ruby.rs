use std::path::Path;

use anyhow::Result;

use super::{ExportFact, ExportKind, ImportFact, ImportKind, ImportTarget, ModuleFacts};

pub(super) fn parse_ruby_module(path: &Path, source: &str) -> Result<ModuleFacts> {
    let mut facts = ModuleFacts::empty(path);

    for line in source.lines() {
        let line = line.trim();
        if line.starts_with('#') || line.is_empty() {
            continue;
        }

        if let Some(source) = ruby_required_path(line, "require_relative") {
            add_ruby_import(&mut facts, ruby_relative_source(&source));
            continue;
        }

        if let Some(source) = ruby_required_path(line, "require") {
            add_ruby_import(&mut facts, source);
            continue;
        }

        if let Some(name) = ruby_declared_constant(line, "class") {
            add_ruby_export(&mut facts, name);
            continue;
        }

        if let Some(name) = ruby_declared_constant(line, "module") {
            add_ruby_export(&mut facts, name);
            continue;
        }

        if let Some(name) = ruby_declared_method(line) {
            add_ruby_export(&mut facts, name);
        }
    }

    facts
        .used_locals
        .extend(facts.imports.iter().map(|import| import.local.clone()));
    Ok(facts)
}

fn ruby_required_path(line: &str, keyword: &str) -> Option<String> {
    let rest = line.strip_prefix(keyword)?.trim_start();
    let quote = rest.chars().next()?;
    if quote != '\'' && quote != '"' {
        return None;
    }
    let value = &rest[quote.len_utf8()..];
    let end = value.find(quote)?;
    Some(value[..end].to_string())
}

fn ruby_relative_source(source: &str) -> String {
    if source.starts_with('.') {
        source.to_string()
    } else {
        format!("./{source}")
    }
}

fn add_ruby_import(facts: &mut ModuleFacts, source: String) {
    let local = source
        .rsplit('/')
        .next()
        .unwrap_or(&source)
        .trim_end_matches(".rb")
        .to_string();
    facts.imports.push(ImportFact {
        source,
        local,
        imported: ImportTarget::Namespace,
        kind: ImportKind::Esm,
        type_only: false,
    });
}

fn ruby_declared_constant(line: &str, keyword: &str) -> Option<String> {
    let rest = line.strip_prefix(keyword)?.trim_start();
    let name = rest
        .split(|ch: char| ch.is_whitespace() || ch == '<' || ch == ';')
        .next()?;
    if name.is_empty() {
        return None;
    }
    Some(name.rsplit("::").next().unwrap_or(name).to_string())
}

fn ruby_declared_method(line: &str) -> Option<String> {
    let rest = line.strip_prefix("def ")?.trim_start();
    let name = rest
        .split(|ch: char| ch.is_whitespace() || ch == '(' || ch == ';')
        .next()?;
    if name.is_empty() {
        return None;
    }
    Some(name.rsplit('.').next().unwrap_or(name).to_string())
}

fn add_ruby_export(facts: &mut ModuleFacts, name: String) {
    facts.exports.push(ExportFact {
        exported: name.clone(),
        local: Some(name),
        kind: ExportKind::Local,
    });
}
