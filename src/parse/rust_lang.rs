use std::path::Path;

use anyhow::{Context, Result};
use syn as rs_ast;

use super::{
    ExportFact, ExportKind, ImportFact, ImportKind, ImportTarget, ModuleFacts, ReexportFact,
    ReexportTarget,
};

pub(super) fn parse_rust_module(path: &Path, source: &str) -> Result<ModuleFacts> {
    let file = rs_ast::parse_file(source)
        .with_context(|| format!("failed to parse rust module {}", path.display()))?;
    let mut facts = ModuleFacts::empty(path);

    for item in &file.items {
        collect_rust_item(item, &mut facts);
    }

    // First-pass Rust support intentionally over-approximates import usage.
    // It avoids false negatives until we add a Rust expression/body usage pass.
    facts
        .used_locals
        .extend(facts.imports.iter().map(|import| import.local.clone()));

    Ok(facts)
}

fn collect_rust_item(item: &rs_ast::Item, facts: &mut ModuleFacts) {
    match item {
        rs_ast::Item::Fn(item) => {
            add_rust_named_export(facts, &item.vis, item.sig.ident.to_string())
        }
        rs_ast::Item::Struct(item) => {
            add_rust_named_export(facts, &item.vis, item.ident.to_string())
        }
        rs_ast::Item::Enum(item) => add_rust_named_export(facts, &item.vis, item.ident.to_string()),
        rs_ast::Item::Trait(item) => {
            add_rust_named_export(facts, &item.vis, item.ident.to_string())
        }
        rs_ast::Item::Type(item) => add_rust_named_export(facts, &item.vis, item.ident.to_string()),
        rs_ast::Item::Const(item) => {
            add_rust_named_export(facts, &item.vis, item.ident.to_string())
        }
        rs_ast::Item::Static(item) => {
            add_rust_named_export(facts, &item.vis, item.ident.to_string())
        }
        rs_ast::Item::Mod(item) => collect_rust_mod(item, facts),
        rs_ast::Item::Use(item) => collect_rust_use(item, facts),
        _ => {}
    }
}

fn collect_rust_mod(item: &rs_ast::ItemMod, facts: &mut ModuleFacts) {
    let name = item.ident.to_string();
    if is_public(&item.vis) {
        facts.exports.push(ExportFact {
            exported: name.clone(),
            local: Some(name.clone()),
            kind: ExportKind::Local,
        });
    }

    // A bodyless `mod foo;` is a file dependency on `foo.rs` or `foo/mod.rs`.
    if item.content.is_none() {
        facts.imports.push(ImportFact {
            source: format!("mod:{name}"),
            local: name,
            imported: ImportTarget::Namespace,
            kind: ImportKind::Esm,
            type_only: false,
        });
    }
}

fn collect_rust_use(item: &rs_ast::ItemUse, facts: &mut ModuleFacts) {
    let mut entries = Vec::new();
    collect_rust_use_tree(Vec::new(), &item.tree, &mut entries);
    for entry in entries {
        if is_public(&item.vis) {
            facts.reexports.push(ReexportFact {
                source: entry.source,
                imported: entry.imported,
                exported: entry.exported,
                is_ambiguous: entry.is_ambiguous,
            });
        } else {
            facts.imports.push(ImportFact {
                source: entry.source,
                local: entry.local,
                imported: match entry.imported {
                    ReexportTarget::Name(name) => ImportTarget::Name(name),
                    ReexportTarget::Namespace | ReexportTarget::All => ImportTarget::Namespace,
                    ReexportTarget::Default => ImportTarget::Default,
                },
                kind: ImportKind::Esm,
                type_only: false,
            });
        }
    }
}

#[derive(Debug)]
struct RustUseEntry {
    source: String,
    imported: ReexportTarget,
    exported: String,
    local: String,
    is_ambiguous: bool,
}

fn collect_rust_use_tree(
    prefix: Vec<String>,
    tree: &rs_ast::UseTree,
    entries: &mut Vec<RustUseEntry>,
) {
    match tree {
        rs_ast::UseTree::Path(path) => {
            let mut next = prefix;
            next.push(path.ident.to_string());
            collect_rust_use_tree(next, &path.tree, entries);
        }
        rs_ast::UseTree::Name(name) => {
            let imported = name.ident.to_string();
            add_rust_use_entry(prefix, imported.clone(), imported, false, entries);
        }
        rs_ast::UseTree::Rename(rename) => {
            add_rust_use_entry(
                prefix,
                rename.ident.to_string(),
                rename.rename.to_string(),
                false,
                entries,
            );
        }
        rs_ast::UseTree::Glob(_) => {
            entries.push(RustUseEntry {
                source: rust_path_source(&prefix),
                imported: ReexportTarget::All,
                exported: "*".to_string(),
                local: "*".to_string(),
                is_ambiguous: true,
            });
        }
        rs_ast::UseTree::Group(group) => {
            for item in &group.items {
                collect_rust_use_tree(prefix.clone(), item, entries);
            }
        }
    }
}

fn add_rust_use_entry(
    prefix: Vec<String>,
    imported: String,
    local: String,
    is_ambiguous: bool,
    entries: &mut Vec<RustUseEntry>,
) {
    if rust_use_name_is_module(&prefix) {
        let mut source = prefix;
        source.push(imported);
        entries.push(RustUseEntry {
            source: rust_path_source(&source),
            imported: ReexportTarget::Namespace,
            exported: local.clone(),
            local,
            is_ambiguous,
        });
        return;
    }

    entries.push(RustUseEntry {
        source: rust_path_source(&prefix),
        imported: ReexportTarget::Name(imported),
        exported: local.clone(),
        local,
        is_ambiguous,
    });
}

fn rust_use_name_is_module(prefix: &[String]) -> bool {
    prefix.is_empty()
        || prefix
            .last()
            .is_some_and(|part| matches!(part.as_str(), "crate" | "self" | "super"))
}

fn rust_path_source(prefix: &[String]) -> String {
    prefix.join("::")
}

fn add_rust_named_export(facts: &mut ModuleFacts, vis: &rs_ast::Visibility, name: String) {
    if !is_public(vis) {
        return;
    }
    facts.exports.push(ExportFact {
        exported: name.clone(),
        local: Some(name),
        kind: ExportKind::Local,
    });
}

fn is_public(vis: &rs_ast::Visibility) -> bool {
    matches!(vis, rs_ast::Visibility::Public(_))
}
