use std::path::Path;

use anyhow::Result;
use rustpython_parser::{Parse, ast as py_ast};

use super::{
    ExportFact, ExportKind, ImportFact, ImportKind, ImportTarget, ModuleFacts, ReexportFact,
    ReexportTarget,
};

pub(crate) fn parse_python_module(path: &Path, source: &str) -> Result<ModuleFacts> {
    let suite = py_ast::Suite::parse(source, &path.display().to_string()).map_err(|error| {
        anyhow::anyhow!("failed to parse python module {}: {error}", path.display())
    })?;
    let mut facts = ModuleFacts::empty(path);

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

fn python_import_source(level: usize, module: Option<&str>) -> String {
    let mut source = ".".repeat(level);
    if let Some(module) = module {
        source.push_str(module);
    }
    source
}

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
