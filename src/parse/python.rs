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
        py_ast::Stmt::FunctionDef(function) => {
            add_python_export(facts, function.name.to_string());
            collect_python_nested_imports(&function.body, facts);
        }
        py_ast::Stmt::AsyncFunctionDef(function) => {
            add_python_export(facts, function.name.to_string());
            collect_python_nested_imports(&function.body, facts);
        }
        py_ast::Stmt::ClassDef(class) => {
            add_python_export(facts, class.name.to_string());
            collect_python_nested_imports(&class.body, facts);
        }
        py_ast::Stmt::Assign(assign) => {
            for target in &assign.targets {
                collect_python_assignment_target(target, facts);
            }
        }
        py_ast::Stmt::AnnAssign(assign) => collect_python_assignment_target(&assign.target, facts),
        py_ast::Stmt::Import(import) => collect_python_import(import, facts),
        py_ast::Stmt::ImportFrom(import) => collect_python_import_from(import, facts),
        // Compound statements at module level (`if TYPE_CHECKING:`, `try/except
        // ImportError:`, ...) commonly guard imports and conditional definitions,
        // so their suites are treated as module scope.
        other => {
            for suite in python_nested_suites(other) {
                for stmt in suite {
                    collect_python_stmt(stmt, facts);
                }
            }
        }
    }
}

/// Collect imports (only) from a function or class body, recursing through
/// nested compound statements and definitions. Definitions and assignments in
/// these scopes are locals, not module exports.
fn collect_python_nested_imports(suite: &[py_ast::Stmt], facts: &mut ModuleFacts) {
    for stmt in suite {
        match stmt {
            py_ast::Stmt::Import(import) => collect_python_import(import, facts),
            py_ast::Stmt::ImportFrom(import) => collect_python_import_from(import, facts),
            py_ast::Stmt::FunctionDef(function) => {
                collect_python_nested_imports(&function.body, facts);
            }
            py_ast::Stmt::AsyncFunctionDef(function) => {
                collect_python_nested_imports(&function.body, facts);
            }
            py_ast::Stmt::ClassDef(class) => collect_python_nested_imports(&class.body, facts),
            other => {
                for inner in python_nested_suites(other) {
                    collect_python_nested_imports(inner, facts);
                }
            }
        }
    }
}

/// The statement suites nested directly inside a compound statement.
fn python_nested_suites(stmt: &py_ast::Stmt) -> Vec<&[py_ast::Stmt]> {
    match stmt {
        py_ast::Stmt::If(stmt) => vec![&stmt.body, &stmt.orelse],
        py_ast::Stmt::While(stmt) => vec![&stmt.body, &stmt.orelse],
        py_ast::Stmt::For(stmt) => vec![&stmt.body, &stmt.orelse],
        py_ast::Stmt::AsyncFor(stmt) => vec![&stmt.body, &stmt.orelse],
        py_ast::Stmt::With(stmt) => vec![&stmt.body],
        py_ast::Stmt::AsyncWith(stmt) => vec![&stmt.body],
        py_ast::Stmt::Try(stmt) => {
            python_try_suites(&stmt.body, &stmt.handlers, &stmt.orelse, &stmt.finalbody)
        }
        py_ast::Stmt::TryStar(stmt) => {
            python_try_suites(&stmt.body, &stmt.handlers, &stmt.orelse, &stmt.finalbody)
        }
        py_ast::Stmt::Match(stmt) => stmt.cases.iter().map(|case| case.body.as_slice()).collect(),
        _ => Vec::new(),
    }
}

fn python_try_suites<'a>(
    body: &'a [py_ast::Stmt],
    handlers: &'a [py_ast::ExceptHandler],
    orelse: &'a [py_ast::Stmt],
    finalbody: &'a [py_ast::Stmt],
) -> Vec<&'a [py_ast::Stmt]> {
    let mut suites = vec![body, orelse, finalbody];
    suites.extend(handlers.iter().map(|handler| {
        let py_ast::ExceptHandler::ExceptHandler(handler) = handler;
        handler.body.as_slice()
    }));
    suites
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
        // `from pkg import name` may bind a submodule (`pkg/name.py`) rather
        // than a symbol of `pkg/__init__.py`. Emit a probe fact for the dotted
        // path so the edge lands on the submodule file when it exists; when
        // `name` is a plain symbol the probe stays unresolved. `type_only`
        // keeps an unresolved probe out of the unresolved-import diagnostics.
        if module.is_some() {
            facts.imports.push(ImportFact {
                source: format!("{base}.{imported_name}"),
                local: local.clone(),
                imported: ImportTarget::Namespace,
                kind: ImportKind::Esm,
                type_only: true,
            });
        }
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
