use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use swc_common::{FileName, SourceMap, sync::Lrc};
use swc_ecma_ast::*;
use swc_ecma_parser::{EsSyntax, Parser, StringInput, Syntax, TsSyntax, lexer::Lexer};
use swc_ecma_visit::{Visit, VisitWith};

#[cfg(feature = "python")]
use rustpython_parser::{Parse, ast as py_ast};

#[cfg(feature = "rust")]
use syn as rs_ast;

#[derive(Debug, Clone)]
pub struct ModuleFacts {
    pub file: PathBuf,
    pub exports: Vec<ExportFact>,
    pub imports: Vec<ImportFact>,
    pub reexports: Vec<ReexportFact>,
    pub used_locals: BTreeSet<String>,
    pub namespace_member_usage: BTreeMap<String, BTreeSet<String>>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct ExportFact {
    pub exported: String,
    pub local: Option<String>,
    pub kind: ExportKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExportKind {
    Local,
    Default,
    Reexport,
    CommonJs,
}

#[derive(Debug, Clone)]
pub struct ImportFact {
    pub source: String,
    pub local: String,
    pub imported: ImportTarget,
    pub kind: ImportKind,
    pub type_only: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImportTarget {
    Name(String),
    Default,
    Namespace,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImportKind {
    Esm,
    CommonJs,
}

#[derive(Debug, Clone)]
pub struct ReexportFact {
    pub source: String,
    pub imported: ReexportTarget,
    pub exported: String,
    pub is_ambiguous: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReexportTarget {
    Name(String),
    Default,
    Namespace,
    All,
}

pub fn parse_module(path: &Path) -> Result<ModuleFacts> {
    let source = fs::read_to_string(path)
        .with_context(|| format!("failed to read source file {}", path.display()))?;

    #[cfg(feature = "python")]
    if path.extension().and_then(|ext| ext.to_str()) == Some("py") {
        return parse_python_module(path, &source);
    }

    #[cfg(feature = "rust")]
    if path.extension().and_then(|ext| ext.to_str()) == Some("rs") {
        return parse_rust_module(path, &source);
    }

    #[cfg(feature = "vue")]
    if path.extension().and_then(|ext| ext.to_str()) == Some("vue") {
        return parse_component_module(path, &source, "vue");
    }

    #[cfg(feature = "svelte")]
    if path.extension().and_then(|ext| ext.to_str()) == Some("svelte") {
        return parse_component_module(path, &source, "svelte");
    }

    #[cfg(feature = "ruby")]
    if path.extension().and_then(|ext| ext.to_str()) == Some("rb") {
        return parse_ruby_module(path, &source);
    }

    #[cfg(feature = "java")]
    if path.extension().and_then(|ext| ext.to_str()) == Some("java") {
        return parse_java_module(path, &source);
    }

    parse_javascript_module(path, &source)
}

fn parse_javascript_module(path: &Path, source: &str) -> Result<ModuleFacts> {
    let module = parse_source(path, source)?;
    module_facts_from_javascript_module(path, &module)
}

fn module_facts_from_javascript_module(path: &Path, module: &Module) -> Result<ModuleFacts> {
    let mut facts = ModuleFacts {
        file: path.to_path_buf(),
        exports: Vec::new(),
        imports: Vec::new(),
        reexports: Vec::new(),
        used_locals: BTreeSet::new(),
        namespace_member_usage: BTreeMap::new(),
        warnings: Vec::new(),
    };

    for item in &module.body {
        match item {
            ModuleItem::ModuleDecl(decl) => match decl {
                ModuleDecl::Import(import) => collect_import_decl(import, &mut facts),
                ModuleDecl::ExportDecl(export_decl) => collect_export_decl(export_decl, &mut facts),
                ModuleDecl::ExportNamed(named) => collect_named_export(named, &mut facts),
                ModuleDecl::ExportDefaultDecl(default_decl) => {
                    let local = match &default_decl.decl {
                        DefaultDecl::Class(class) => {
                            class.ident.as_ref().map(|ident| ident.sym.to_string())
                        }
                        DefaultDecl::Fn(function) => {
                            function.ident.as_ref().map(|ident| ident.sym.to_string())
                        }
                        _ => None,
                    };
                    facts.exports.push(ExportFact {
                        exported: "default".to_string(),
                        local,
                        kind: ExportKind::Default,
                    });
                }
                ModuleDecl::ExportDefaultExpr(default_expr) => {
                    let local = match &*default_expr.expr {
                        Expr::Ident(ident) => Some(ident.sym.to_string()),
                        _ => None,
                    };
                    facts.exports.push(ExportFact {
                        exported: "default".to_string(),
                        local,
                        kind: ExportKind::Default,
                    });
                }
                ModuleDecl::ExportAll(all) => {
                    facts.reexports.push(ReexportFact {
                        source: all.src.value.to_string_lossy().to_string(),
                        imported: ReexportTarget::All,
                        exported: "*".to_string(),
                        is_ambiguous: true,
                    });
                }
                _ => {}
            },
            ModuleItem::Stmt(stmt) => collect_commonjs_from_stmt(stmt, &mut facts),
        }
    }

    let imported_locals: BTreeSet<String> = facts
        .imports
        .iter()
        .map(|fact| fact.local.clone())
        .collect();
    let namespace_locals: BTreeSet<String> = facts
        .imports
        .iter()
        .filter(|fact| fact.imported == ImportTarget::Namespace)
        .map(|fact| fact.local.clone())
        .collect();

    let mut usage_collector = UsageCollector::new(imported_locals, namespace_locals);
    module.visit_with(&mut usage_collector);
    facts.used_locals = usage_collector.used_locals;
    facts.namespace_member_usage = usage_collector.namespace_member_usage;

    Ok(facts)
}

#[cfg(any(feature = "vue", feature = "svelte"))]
fn parse_component_module(path: &Path, source: &str, kind: &str) -> Result<ModuleFacts> {
    let script = extract_component_scripts(source);
    let virtual_path = component_virtual_script_path(path, &script);
    let module = parse_source(&virtual_path, &script.source)?;
    let mut facts = module_facts_from_javascript_module(path, &module)?;

    facts.exports.push(ExportFact {
        exported: "default".to_string(),
        local: None,
        kind: ExportKind::Default,
    });
    facts
        .used_locals
        .extend(facts.imports.iter().map(|import| import.local.clone()));
    facts.warnings.push(format!(
        "parsed {kind} script blocks as JavaScript/TypeScript"
    ));

    Ok(facts)
}

#[cfg(any(feature = "vue", feature = "svelte"))]
#[derive(Debug)]
struct ComponentScript {
    source: String,
    is_typescript: bool,
}

#[cfg(any(feature = "vue", feature = "svelte"))]
fn extract_component_scripts(source: &str) -> ComponentScript {
    let mut remaining = source;
    let mut scripts = Vec::new();
    let mut is_typescript = false;

    while let Some(start) = remaining.find("<script") {
        remaining = &remaining[start + "<script".len()..];
        let Some(tag_end) = remaining.find('>') else {
            break;
        };
        let attrs = &remaining[..tag_end];
        is_typescript |= component_script_is_typescript(attrs);
        remaining = &remaining[tag_end + 1..];
        let Some(script_end) = remaining.find("</script>") else {
            break;
        };
        scripts.push(remaining[..script_end].to_string());
        remaining = &remaining[script_end + "</script>".len()..];
    }

    ComponentScript {
        source: scripts.join("\n"),
        is_typescript,
    }
}

#[cfg(any(feature = "vue", feature = "svelte"))]
fn component_script_is_typescript(attrs: &str) -> bool {
    attrs.contains("lang=\"ts\"")
        || attrs.contains("lang='ts'")
        || attrs.contains("lang=ts")
        || attrs.contains("lang=\"tsx\"")
        || attrs.contains("lang='tsx'")
        || attrs.contains("lang=tsx")
}

#[cfg(any(feature = "vue", feature = "svelte"))]
fn component_virtual_script_path(path: &Path, script: &ComponentScript) -> PathBuf {
    let extension = if script.is_typescript { "ts" } else { "js" };
    path.with_extension(format!(
        "{}.{extension}",
        path.extension()
            .and_then(|ext| ext.to_str())
            .unwrap_or("component")
    ))
}

#[cfg(feature = "python")]
fn parse_python_module(path: &Path, source: &str) -> Result<ModuleFacts> {
    let suite = py_ast::Suite::parse(source, &path.display().to_string()).map_err(|error| {
        anyhow::anyhow!("failed to parse python module {}: {error}", path.display())
    })?;
    let mut facts = ModuleFacts {
        file: path.to_path_buf(),
        exports: Vec::new(),
        imports: Vec::new(),
        reexports: Vec::new(),
        used_locals: BTreeSet::new(),
        namespace_member_usage: BTreeMap::new(),
        warnings: Vec::new(),
    };

    for stmt in &suite {
        collect_python_stmt(stmt, &mut facts);
    }

    // First-pass Python support intentionally over-approximates import usage.
    // Missing an impacted file is worse than marking an imported module as used.
    facts
        .used_locals
        .extend(facts.imports.iter().map(|import| import.local.clone()));

    Ok(facts)
}

#[cfg(feature = "python")]
fn collect_python_stmt(stmt: &py_ast::Stmt, facts: &mut ModuleFacts) {
    match stmt {
        py_ast::Stmt::FunctionDef(function) => add_python_export(facts, function.name.to_string()),
        py_ast::Stmt::AsyncFunctionDef(function) => {
            add_python_export(facts, function.name.to_string());
        }
        py_ast::Stmt::ClassDef(class) => add_python_export(facts, class.name.to_string()),
        py_ast::Stmt::Assign(assign) => {
            for target in &assign.targets {
                collect_python_assignment_target(target, facts);
            }
        }
        py_ast::Stmt::AnnAssign(assign) => collect_python_assignment_target(&assign.target, facts),
        py_ast::Stmt::Import(import) => collect_python_import(import, facts),
        py_ast::Stmt::ImportFrom(import) => collect_python_import_from(import, facts),
        _ => {}
    }
}

#[cfg(feature = "python")]
fn collect_python_import(import: &py_ast::StmtImport, facts: &mut ModuleFacts) {
    for alias in &import.names {
        let source = alias.name.to_string();
        let local = alias
            .asname
            .as_ref()
            .map(ToString::to_string)
            .unwrap_or_else(|| source.split('.').next().unwrap_or(&source).to_string());

        facts.imports.push(ImportFact {
            source,
            local,
            imported: ImportTarget::Namespace,
            kind: ImportKind::Esm,
            type_only: false,
        });
    }
}

#[cfg(feature = "python")]
fn collect_python_import_from(import: &py_ast::StmtImportFrom, facts: &mut ModuleFacts) {
    let level = import
        .level
        .as_ref()
        .map(|level| level.to_usize())
        .unwrap_or(0);
    let module = import.module.as_ref().map(ToString::to_string);
    let base = python_import_source(level, module.as_deref());

    for alias in &import.names {
        let imported_name = alias.name.to_string();
        if imported_name == "*" {
            facts.reexports.push(ReexportFact {
                source: base.clone(),
                imported: ReexportTarget::All,
                exported: "*".to_string(),
                is_ambiguous: true,
            });
            continue;
        }

        let local = alias
            .asname
            .as_ref()
            .map(ToString::to_string)
            .unwrap_or_else(|| imported_name.clone());
        let source = if module.is_none() {
            format!("{base}{imported_name}")
        } else {
            base.clone()
        };
        let imported = if module.is_none() {
            ImportTarget::Namespace
        } else {
            ImportTarget::Name(imported_name.clone())
        };
        facts.imports.push(ImportFact {
            source,
            local: local.clone(),
            imported,
            kind: ImportKind::Esm,
            type_only: false,
        });
        facts.exports.push(ExportFact {
            exported: local,
            local: Some(imported_name),
            kind: ExportKind::Reexport,
        });
    }
}

#[cfg(feature = "python")]
fn python_import_source(level: usize, module: Option<&str>) -> String {
    let mut source = ".".repeat(level);
    if let Some(module) = module {
        source.push_str(module);
    }
    source
}

#[cfg(feature = "python")]
fn collect_python_assignment_target(expr: &py_ast::Expr, facts: &mut ModuleFacts) {
    match expr {
        py_ast::Expr::Name(name) => add_python_export(facts, name.id.to_string()),
        py_ast::Expr::Tuple(tuple) => {
            for element in &tuple.elts {
                collect_python_assignment_target(element, facts);
            }
        }
        py_ast::Expr::List(list) => {
            for element in &list.elts {
                collect_python_assignment_target(element, facts);
            }
        }
        _ => {}
    }
}

#[cfg(feature = "python")]
fn add_python_export(facts: &mut ModuleFacts, name: String) {
    if name.starts_with('_') {
        return;
    }
    facts.exports.push(ExportFact {
        exported: name.clone(),
        local: Some(name),
        kind: ExportKind::Local,
    });
}

#[cfg(feature = "rust")]
fn parse_rust_module(path: &Path, source: &str) -> Result<ModuleFacts> {
    let file = rs_ast::parse_file(source)
        .with_context(|| format!("failed to parse rust module {}", path.display()))?;
    let mut facts = ModuleFacts {
        file: path.to_path_buf(),
        exports: Vec::new(),
        imports: Vec::new(),
        reexports: Vec::new(),
        used_locals: BTreeSet::new(),
        namespace_member_usage: BTreeMap::new(),
        warnings: Vec::new(),
    };

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

#[cfg(feature = "rust")]
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

#[cfg(feature = "rust")]
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

#[cfg(feature = "rust")]
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

#[cfg(feature = "rust")]
#[derive(Debug)]
struct RustUseEntry {
    source: String,
    imported: ReexportTarget,
    exported: String,
    local: String,
    is_ambiguous: bool,
}

#[cfg(feature = "rust")]
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

#[cfg(feature = "rust")]
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

#[cfg(feature = "rust")]
fn rust_use_name_is_module(prefix: &[String]) -> bool {
    prefix.is_empty()
        || prefix
            .last()
            .is_some_and(|part| matches!(part.as_str(), "crate" | "self" | "super"))
}

#[cfg(feature = "rust")]
fn rust_path_source(prefix: &[String]) -> String {
    prefix.join("::")
}

#[cfg(feature = "rust")]
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

#[cfg(feature = "rust")]
fn is_public(vis: &rs_ast::Visibility) -> bool {
    matches!(vis, rs_ast::Visibility::Public(_))
}

#[cfg(feature = "ruby")]
fn parse_ruby_module(path: &Path, source: &str) -> Result<ModuleFacts> {
    let mut facts = ModuleFacts {
        file: path.to_path_buf(),
        exports: Vec::new(),
        imports: Vec::new(),
        reexports: Vec::new(),
        used_locals: BTreeSet::new(),
        namespace_member_usage: BTreeMap::new(),
        warnings: Vec::new(),
    };

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

#[cfg(feature = "ruby")]
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

#[cfg(feature = "ruby")]
fn ruby_relative_source(source: &str) -> String {
    if source.starts_with('.') {
        source.to_string()
    } else {
        format!("./{source}")
    }
}

#[cfg(feature = "ruby")]
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

#[cfg(feature = "ruby")]
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

#[cfg(feature = "ruby")]
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

#[cfg(feature = "ruby")]
fn add_ruby_export(facts: &mut ModuleFacts, name: String) {
    facts.exports.push(ExportFact {
        exported: name.clone(),
        local: Some(name),
        kind: ExportKind::Local,
    });
}

#[cfg(feature = "java")]
fn parse_java_module(path: &Path, source: &str) -> Result<ModuleFacts> {
    let mut facts = ModuleFacts {
        file: path.to_path_buf(),
        exports: Vec::new(),
        imports: Vec::new(),
        reexports: Vec::new(),
        used_locals: BTreeSet::new(),
        namespace_member_usage: BTreeMap::new(),
        warnings: Vec::new(),
    };

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

#[cfg(feature = "java")]
fn java_import(line: &str) -> Option<String> {
    let rest = line.strip_prefix("import ")?.trim_start();
    let rest = rest.strip_prefix("static ").unwrap_or(rest).trim_start();
    Some(rest.trim_end_matches(';').trim().to_string())
}

#[cfg(feature = "java")]
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

fn parse_source(path: &Path, source: &str) -> Result<Module> {
    let cm: Lrc<SourceMap> = Default::default();
    let fm = cm.new_source_file(
        FileName::Real(path.to_path_buf()).into(),
        source.to_string(),
    );
    let syntax = syntax_for_path(path);
    let lexer = Lexer::new(syntax, EsVersion::Es2022, StringInput::from(&*fm), None);
    let mut parser = Parser::new_from(lexer);
    let module = parser.parse_module().map_err(|error| {
        anyhow::anyhow!("failed to parse module {}: {:?}", path.display(), error)
    })?;

    Ok(module)
}

fn syntax_for_path(path: &Path) -> Syntax {
    match path.extension().and_then(|ext| ext.to_str()) {
        Some("ts") | Some("mts") | Some("cts") => Syntax::Typescript(TsSyntax {
            tsx: false,
            decorators: true,
            ..Default::default()
        }),
        Some("tsx") => Syntax::Typescript(TsSyntax {
            tsx: true,
            decorators: true,
            ..Default::default()
        }),
        Some("js") | Some("jsx") | Some("mjs") | Some("cjs") => Syntax::Es(EsSyntax {
            jsx: true,
            decorators: true,
            export_default_from: true,
            ..Default::default()
        }),
        _ => Syntax::Es(EsSyntax {
            jsx: true,
            decorators: true,
            export_default_from: true,
            ..Default::default()
        }),
    }
}

fn collect_import_decl(import: &ImportDecl, facts: &mut ModuleFacts) {
    let source = import.src.value.to_string_lossy().to_string();

    for specifier in &import.specifiers {
        match specifier {
            ImportSpecifier::Named(named) => {
                let imported = named
                    .imported
                    .as_ref()
                    .map(imported_name)
                    .unwrap_or_else(|| named.local.sym.to_string());
                facts.imports.push(ImportFact {
                    source: source.clone(),
                    local: named.local.sym.to_string(),
                    imported: ImportTarget::Name(imported),
                    kind: ImportKind::Esm,
                    type_only: import.type_only || named.is_type_only,
                });
            }
            ImportSpecifier::Default(default) => facts.imports.push(ImportFact {
                source: source.clone(),
                local: default.local.sym.to_string(),
                imported: ImportTarget::Default,
                kind: ImportKind::Esm,
                type_only: import.type_only,
            }),
            ImportSpecifier::Namespace(namespace) => facts.imports.push(ImportFact {
                source: source.clone(),
                local: namespace.local.sym.to_string(),
                imported: ImportTarget::Namespace,
                kind: ImportKind::Esm,
                type_only: import.type_only,
            }),
        }
    }
}

fn collect_export_decl(export_decl: &ExportDecl, facts: &mut ModuleFacts) {
    match &export_decl.decl {
        Decl::Fn(function) => facts.exports.push(ExportFact {
            exported: function.ident.sym.to_string(),
            local: Some(function.ident.sym.to_string()),
            kind: ExportKind::Local,
        }),
        Decl::Class(class) => facts.exports.push(ExportFact {
            exported: class.ident.sym.to_string(),
            local: Some(class.ident.sym.to_string()),
            kind: ExportKind::Local,
        }),
        Decl::Var(variable) => {
            let mut names = Vec::new();
            for declarator in &variable.decls {
                collect_pat_names(&declarator.name, &mut names);
            }

            for name in names {
                facts.exports.push(ExportFact {
                    exported: name.clone(),
                    local: Some(name),
                    kind: ExportKind::Local,
                });
            }
        }
        _ => {}
    }
}

fn collect_named_export(named: &NamedExport, facts: &mut ModuleFacts) {
    if let Some(source) = &named.src {
        let source = source.value.to_string_lossy().to_string();
        for specifier in &named.specifiers {
            match specifier {
                ExportSpecifier::Named(specifier) => {
                    let imported = module_export_name(&specifier.orig);
                    let exported = specifier
                        .exported
                        .as_ref()
                        .map(module_export_name)
                        .unwrap_or_else(|| imported.clone());

                    facts.reexports.push(ReexportFact {
                        source: source.clone(),
                        imported: if imported == "default" {
                            ReexportTarget::Default
                        } else {
                            ReexportTarget::Name(imported)
                        },
                        exported,
                        is_ambiguous: false,
                    });
                }
                ExportSpecifier::Namespace(specifier) => facts.reexports.push(ReexportFact {
                    source: source.clone(),
                    imported: ReexportTarget::Namespace,
                    exported: module_export_name(&specifier.name),
                    is_ambiguous: false,
                }),
                ExportSpecifier::Default(specifier) => facts.reexports.push(ReexportFact {
                    source: source.clone(),
                    imported: ReexportTarget::Default,
                    exported: specifier.exported.sym.to_string(),
                    is_ambiguous: false,
                }),
            }
        }
    } else {
        for specifier in &named.specifiers {
            if let ExportSpecifier::Named(specifier) = specifier {
                let local = module_export_name(&specifier.orig);
                let exported = specifier
                    .exported
                    .as_ref()
                    .map(module_export_name)
                    .unwrap_or_else(|| local.clone());
                facts.exports.push(ExportFact {
                    exported,
                    local: Some(local),
                    kind: ExportKind::Local,
                });
            }
        }
    }
}

fn collect_commonjs_from_stmt(stmt: &Stmt, facts: &mut ModuleFacts) {
    match stmt {
        Stmt::Decl(Decl::Var(variable)) => {
            for declarator in &variable.decls {
                if let Some(init) = &declarator.init {
                    collect_require_import(&declarator.name, init, facts);
                }
            }
        }
        Stmt::Expr(expr_stmt) => {
            if let Expr::Assign(assign) = &*expr_stmt.expr {
                collect_commonjs_export(assign, facts);
            }
        }
        _ => {}
    }
}

fn collect_require_import(name: &Pat, init: &Expr, facts: &mut ModuleFacts) {
    let Expr::Call(call) = init else {
        return;
    };
    let Callee::Expr(callee) = &call.callee else {
        return;
    };
    let Expr::Ident(ident) = &**callee else {
        return;
    };
    if ident.sym != *"require" || call.args.len() != 1 {
        return;
    }

    let Some(source) = literal_string(&call.args[0].expr) else {
        return;
    };

    match name {
        Pat::Ident(binding) => facts.imports.push(ImportFact {
            source,
            local: binding.id.sym.to_string(),
            imported: ImportTarget::Default,
            kind: ImportKind::CommonJs,
            type_only: false,
        }),
        Pat::Object(object) => {
            for prop in &object.props {
                match prop {
                    ObjectPatProp::Assign(assign) => facts.imports.push(ImportFact {
                        source: source.clone(),
                        local: assign.key.sym.to_string(),
                        imported: ImportTarget::Name(assign.key.sym.to_string()),
                        kind: ImportKind::CommonJs,
                        type_only: false,
                    }),
                    ObjectPatProp::KeyValue(key_value) => {
                        let Some(imported_name) = prop_name_to_string(&key_value.key) else {
                            continue;
                        };
                        let mut locals = Vec::new();
                        collect_pat_names(&key_value.value, &mut locals);
                        for local in locals {
                            facts.imports.push(ImportFact {
                                source: source.clone(),
                                local,
                                imported: ImportTarget::Name(imported_name.clone()),
                                kind: ImportKind::CommonJs,
                                type_only: false,
                            });
                        }
                    }
                    ObjectPatProp::Rest(_) => {}
                }
            }
        }
        _ => {}
    }
}

fn collect_commonjs_export(assign: &AssignExpr, facts: &mut ModuleFacts) {
    let Some(path) = member_path_from_assign_target(&assign.left) else {
        return;
    };

    match path.as_slice() {
        ["module", "exports"] => {
            facts.exports.push(ExportFact {
                exported: "default".to_string(),
                local: None,
                kind: ExportKind::CommonJs,
            });

            if let Expr::Object(object) = &*assign.right {
                for prop in &object.props {
                    let PropOrSpread::Prop(prop) = prop else {
                        continue;
                    };
                    if let Some(name) = extract_object_prop_name(prop) {
                        facts.exports.push(ExportFact {
                            exported: name.clone(),
                            local: Some(name),
                            kind: ExportKind::CommonJs,
                        });
                    }
                }
            }
        }
        ["exports", name] | ["module", "exports", name] => facts.exports.push(ExportFact {
            exported: name.to_string(),
            local: Some(name.to_string()),
            kind: ExportKind::CommonJs,
        }),
        _ => {}
    }
}

fn imported_name(name: &ModuleExportName) -> String {
    module_export_name(name)
}

fn module_export_name(name: &ModuleExportName) -> String {
    match name {
        ModuleExportName::Ident(ident) => ident.sym.to_string(),
        ModuleExportName::Str(value) => value.value.to_string_lossy().to_string(),
    }
}

fn collect_pat_names(pat: &Pat, names: &mut Vec<String>) {
    match pat {
        Pat::Ident(ident) => names.push(ident.id.sym.to_string()),
        Pat::Array(array) => {
            for element in array.elems.iter().flatten() {
                collect_pat_names(element, names);
            }
        }
        Pat::Object(object) => {
            for prop in &object.props {
                match prop {
                    ObjectPatProp::Assign(assign) => names.push(assign.key.sym.to_string()),
                    ObjectPatProp::KeyValue(key_value) => {
                        collect_pat_names(&key_value.value, names)
                    }
                    ObjectPatProp::Rest(rest) => collect_pat_names(&rest.arg, names),
                }
            }
        }
        Pat::Rest(rest) => collect_pat_names(&rest.arg, names),
        Pat::Assign(assign) => collect_pat_names(&assign.left, names),
        _ => {}
    }
}

fn literal_string(expr: &Expr) -> Option<String> {
    match expr {
        Expr::Lit(Lit::Str(value)) => Some(value.value.to_string_lossy().to_string()),
        _ => None,
    }
}

fn member_path_from_assign_target(target: &AssignTarget) -> Option<Vec<&str>> {
    match target {
        AssignTarget::Simple(simple) => match simple {
            SimpleAssignTarget::Member(member) => member_path(member),
            _ => None,
        },
        _ => None,
    }
}

fn member_path(member: &MemberExpr) -> Option<Vec<&str>> {
    let mut path = Vec::new();
    let mut current = member;

    loop {
        let prop = match &current.prop {
            MemberProp::Ident(ident) => ident.sym.as_ref(),
            _ => return None,
        };
        path.push(prop);

        match &*current.obj {
            Expr::Ident(ident) => {
                path.push(ident.sym.as_ref());
                path.reverse();
                return Some(path);
            }
            Expr::Member(parent) => current = parent,
            _ => return None,
        }
    }
}

fn extract_object_prop_name(prop: &Prop) -> Option<String> {
    match prop {
        Prop::Shorthand(ident) => Some(ident.sym.to_string()),
        Prop::KeyValue(key_value) => prop_name_to_string(&key_value.key),
        Prop::Method(method) => prop_name_to_string(&method.key),
        Prop::Getter(getter) => prop_name_to_string(&getter.key),
        Prop::Setter(setter) => prop_name_to_string(&setter.key),
        Prop::Assign(assign) => Some(assign.key.sym.to_string()),
    }
}

fn prop_name_to_string(prop: &PropName) -> Option<String> {
    match prop {
        PropName::Ident(ident) => Some(ident.sym.to_string()),
        PropName::Str(value) => Some(value.value.to_string_lossy().to_string()),
        PropName::Num(number) => Some(number.value.to_string()),
        _ => None,
    }
}

struct UsageCollector {
    imported_locals: BTreeSet<String>,
    namespace_locals: BTreeSet<String>,
    used_locals: BTreeSet<String>,
    namespace_member_usage: BTreeMap<String, BTreeSet<String>>,
}

impl UsageCollector {
    fn new(imported_locals: BTreeSet<String>, namespace_locals: BTreeSet<String>) -> Self {
        Self {
            imported_locals,
            namespace_locals,
            used_locals: BTreeSet::new(),
            namespace_member_usage: BTreeMap::new(),
        }
    }

    fn mark_ident(&mut self, ident: &Ident) {
        let name = ident.sym.to_string();
        if self.imported_locals.contains(&name) {
            self.used_locals.insert(name);
        }
    }

    fn mark_jsx_name(&mut self, name: &JSXElementName) {
        match name {
            JSXElementName::Ident(ident) => {
                let value = ident.sym.to_string();
                if self.imported_locals.contains(&value) {
                    self.used_locals.insert(value);
                }
            }
            JSXElementName::JSXMemberExpr(expr) => self.mark_jsx_member(expr),
            JSXElementName::JSXNamespacedName(_) => {}
        }
    }

    fn mark_jsx_member(&mut self, expr: &JSXMemberExpr) {
        if let JSXObject::Ident(object) = &expr.obj {
            let namespace = object.sym.to_string();
            if self.namespace_locals.contains(&namespace) {
                self.namespace_member_usage
                    .entry(namespace)
                    .or_default()
                    .insert(expr.prop.sym.to_string());
            }
        }
    }
}

impl Visit for UsageCollector {
    fn visit_import_decl(&mut self, _: &ImportDecl) {}

    fn visit_named_export(&mut self, _: &NamedExport) {}

    fn visit_ident(&mut self, ident: &Ident) {
        self.mark_ident(ident);
    }

    fn visit_member_expr(&mut self, member: &MemberExpr) {
        if let Expr::Ident(object) = &*member.obj {
            let namespace = object.sym.to_string();
            if self.namespace_locals.contains(&namespace) {
                if let MemberProp::Ident(prop) = &member.prop {
                    self.namespace_member_usage
                        .entry(namespace)
                        .or_default()
                        .insert(prop.sym.to_string());
                }
            }
        }

        member.obj.visit_with(self);
        member.prop.visit_with(self);
    }

    fn visit_jsx_opening_element(&mut self, opening: &JSXOpeningElement) {
        self.mark_jsx_name(&opening.name);
        opening.visit_children_with(self);
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::tempdir;

    use super::parse_module;

    #[test]
    fn parses_js_files_with_jsx() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("renderAvatar.js");
        fs::write(
            &path,
            r#"
import Avatar from '@mui/material/Avatar';

export function renderAvatar(params) {
  if (params.value == null) {
    return '';
  }

  return <Avatar>{params.value.name}</Avatar>;
}
"#,
        )
        .unwrap();

        let facts = parse_module(&path).unwrap();
        assert_eq!(facts.imports.len(), 1);
        assert!(
            facts
                .exports
                .iter()
                .any(|export| export.exported == "renderAvatar")
        );
    }

    #[test]
    fn parses_modern_module_extensions() {
        let dir = tempdir().unwrap();

        let mjs_path = dir.path().join("widget.mjs");
        fs::write(&mjs_path, "export const widget = <div />;").unwrap();
        parse_module(&mjs_path).unwrap();

        let cts_path = dir.path().join("server.cts");
        fs::write(&cts_path, "export const server = 1;").unwrap();
        parse_module(&cts_path).unwrap();
    }

    #[cfg(feature = "python")]
    #[test]
    fn parses_python_imports_and_exports() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("email.py");
        fs::write(
            &path,
            r#"
from ..models import User
from . import formatting

DEFAULT_TEMPLATE = "welcome"

def send_email(user: User) -> str:
    return formatting.format_subject(user.email, DEFAULT_TEMPLATE)
"#,
        )
        .unwrap();

        let facts = parse_module(&path).unwrap();
        assert!(
            facts
                .exports
                .iter()
                .any(|export| export.exported == "send_email")
        );
        assert!(
            facts
                .imports
                .iter()
                .any(|import| import.source == "..models" && import.local == "User")
        );
        assert!(
            facts
                .imports
                .iter()
                .any(|import| import.source == ".formatting" && import.local == "formatting")
        );
    }

    #[cfg(feature = "rust")]
    #[test]
    fn parses_rust_imports_exports_and_reexports() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("lib.rs");
        fs::write(
            &path,
            r#"
pub mod services;

use crate::models::User;
pub use crate::services::email::send_email;

pub struct App;
"#,
        )
        .unwrap();

        let facts = parse_module(&path).unwrap();
        assert!(facts.exports.iter().any(|export| export.exported == "App"));
        assert!(
            facts
                .imports
                .iter()
                .any(|import| import.source == "mod:services")
        );
        assert!(
            facts
                .imports
                .iter()
                .any(|import| import.source == "crate::models" && import.local == "User")
        );
        assert!(
            facts
                .reexports
                .iter()
                .any(|reexport| reexport.source == "crate::services::email"
                    && reexport.exported == "send_email")
        );
    }

    #[cfg(feature = "vue")]
    #[test]
    fn parses_vue_script_imports_and_default_export() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("Button.vue");
        fs::write(
            &path,
            r#"
<script setup lang="ts">
import { formatLabel } from './shared'
const label = formatLabel('save')
</script>
<template><button>{{ label }}</button></template>
"#,
        )
        .unwrap();

        let facts = parse_module(&path).unwrap();
        assert!(
            facts
                .imports
                .iter()
                .any(|import| import.source == "./shared" && import.local == "formatLabel")
        );
        assert!(
            facts
                .exports
                .iter()
                .any(|export| export.exported == "default")
        );
    }

    #[cfg(feature = "svelte")]
    #[test]
    fn parses_svelte_script_imports_and_default_export() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("Card.svelte");
        fs::write(
            &path,
            r#"
<script lang="ts">
  import Button from './Button.vue'
  export let title = 'Settings'
</script>
<Button />
"#,
        )
        .unwrap();

        let facts = parse_module(&path).unwrap();
        assert!(
            facts
                .imports
                .iter()
                .any(|import| import.source == "./Button.vue" && import.local == "Button")
        );
        assert!(
            facts
                .exports
                .iter()
                .any(|export| export.exported == "default")
        );
    }

    #[cfg(feature = "ruby")]
    #[test]
    fn parses_ruby_requires_and_exports() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("email_service.rb");
        fs::write(
            &path,
            r#"
require_relative "../models/user"

class EmailService
  def self.send_email(email)
  end
end
"#,
        )
        .unwrap();

        let facts = parse_module(&path).unwrap();
        assert!(
            facts
                .imports
                .iter()
                .any(|import| import.source == "../models/user")
        );
        assert!(
            facts
                .exports
                .iter()
                .any(|export| export.exported == "EmailService")
        );
        assert!(
            facts
                .exports
                .iter()
                .any(|export| export.exported == "send_email")
        );
    }

    #[cfg(feature = "java")]
    #[test]
    fn parses_java_imports_and_exports() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("EmailService.java");
        fs::write(
            &path,
            r#"
package com.example.service;

import com.example.model.User;

public class EmailService {}
"#,
        )
        .unwrap();

        let facts = parse_module(&path).unwrap();
        assert!(
            facts
                .imports
                .iter()
                .any(|import| import.source == "com.example.model.User" && import.local == "User")
        );
        assert!(
            facts
                .exports
                .iter()
                .any(|export| export.exported == "EmailService")
        );
    }
}
