use std::collections::BTreeSet;
use std::path::Path;

use anyhow::Result;
use swc_common::{FileName, SourceMap, sync::Lrc};
use swc_ecma_ast::*;
use swc_ecma_parser::{EsSyntax, Parser, StringInput, Syntax, TsSyntax, lexer::Lexer};
use swc_ecma_visit::{Visit, VisitWith};

use super::*;

mod usage;
use usage::UsageCollector;

pub(crate) fn parse_javascript_module(path: &Path, source: &str) -> Result<ModuleFacts> {
    let module = parse_source(path, source)?;
    module_facts_from_javascript_module(path, &module)
}

pub(super) fn module_facts_from_javascript_module(
    path: &Path,
    module: &Module,
) -> Result<ModuleFacts> {
    let mut facts = ModuleFacts::empty(path);

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
                ModuleDecl::TsImportEquals(import_equals) => {
                    // `import x = require('./y')` binds the whole CommonJS
                    // module value, like `const x = require('./y')`.
                    if let TsModuleRef::TsExternalModuleRef(module_ref) = &import_equals.module_ref
                    {
                        facts.imports.push(ImportFact {
                            source: module_ref.expr.value.to_string_lossy().to_string(),
                            local: import_equals.id.sym.to_string(),
                            imported: ImportTarget::Default,
                            kind: ImportKind::CommonJs,
                            type_only: import_equals.is_type_only,
                        });
                    }
                }
                ModuleDecl::TsExportAssignment(assignment) => {
                    // `export =` exports the file's value; model it as the
                    // default export.
                    let local = match &*assignment.expr {
                        Expr::Ident(ident) => Some(ident.sym.to_string()),
                        _ => None,
                    };
                    facts.exports.push(ExportFact {
                        exported: "default".to_string(),
                        local,
                        kind: ExportKind::Default,
                    });
                }
                _ => {}
            },
            ModuleItem::Stmt(stmt) => collect_commonjs_export_from_stmt(stmt, &mut facts),
        }
    }

    let require_locals = collect_require_imports(module, &mut facts);

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
    facts.jsx_locals = usage_collector.jsx_locals;
    facts.namespace_member_usage = usage_collector.namespace_member_usage;
    facts.jsx_namespace_member_usage = usage_collector.jsx_namespace_member_usage;
    // Value-position require call sites are themselves the usage.
    facts.used_locals.extend(require_locals);

    collect_dynamic_imports(module, &mut facts);

    Ok(facts)
}

/// Collect call-expression dependencies that can appear anywhere in the module:
/// dynamic `import("...")` (lazy routes, code-split components) and test-runner
/// `vi.mock("...")` / `jest.mock("...")` references. Both need a full AST walk
/// rather than the top-level item loop.
fn collect_dynamic_imports(module: &Module, facts: &mut ModuleFacts) {
    let mut collector = DynamicImportCollector::default();
    module.visit_with(&mut collector);

    // Dynamic imports evaluate to the whole module's namespace; record each
    // under a synthetic local that can't collide with a real identifier.
    let mut seen = BTreeSet::new();
    for (index, source) in collector.sources.into_iter().enumerate() {
        if !seen.insert(source.clone()) {
            continue;
        }
        let local = format!("import():{index}");
        facts.imports.push(ImportFact {
            source,
            local: local.clone(),
            imported: ImportTarget::Namespace,
            kind: ImportKind::Dynamic,
            type_only: false,
        });
        // The call site is itself the usage — mark the synthetic local used so
        // the walk counts the edge.
        facts.used_locals.insert(local);
    }

    // Mock references depend on the whole real module (a change to it can break
    // the mock), so model them as side-effect imports — but tagged `Mock` so the
    // edge is labeled `mocks_module` rather than masquerading as a real import.
    let mut seen_mocks = BTreeSet::new();
    for (index, source) in collector.mock_sources.into_iter().enumerate() {
        if !seen_mocks.insert(source.clone()) {
            continue;
        }
        facts
            .imports
            .push(side_effect_import(source, ImportKind::Mock, index));
    }
}

#[derive(Default)]
struct DynamicImportCollector {
    sources: Vec<String>,
    mock_sources: Vec<String>,
}

impl Visit for DynamicImportCollector {
    fn visit_call_expr(&mut self, call: &CallExpr) {
        call.visit_children_with(self);
        if matches!(call.callee, Callee::Import(_))
            && let Some(argument) = call.args.first()
            && let Some(source) = literal_string(&argument.expr)
        {
            self.sources.push(source);
        } else if let Some(source) = mock_call_source(call) {
            self.mock_sources.push(source);
        }
    }
}

/// The string-literal module specifier of a `vi.mock(...)` / `jest.mock(...)`
/// (or `doMock`) call, if this call is one. Other call shapes return `None`.
fn mock_call_source(call: &CallExpr) -> Option<String> {
    let Callee::Expr(callee) = &call.callee else {
        return None;
    };
    let Expr::Member(member) = &**callee else {
        return None;
    };
    let Expr::Ident(object) = &*member.obj else {
        return None;
    };
    let MemberProp::Ident(method) = &member.prop else {
        return None;
    };
    let is_runner = matches!(object.sym.as_ref(), "vi" | "jest");
    let is_mock = matches!(method.sym.as_ref(), "mock" | "doMock");
    if is_runner && is_mock {
        call.args
            .first()
            .and_then(|argument| literal_string(&argument.expr))
    } else {
        None
    }
}

pub(super) fn parse_source(path: &Path, source: &str) -> Result<Module> {
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

    // `import './setup'` binds nothing but still executes the module.
    if import.specifiers.is_empty() {
        let fact = side_effect_import(source, ImportKind::Esm, facts.imports.len());
        facts.imports.push(fact);
        return;
    }

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

fn collect_commonjs_export_from_stmt(stmt: &Stmt, facts: &mut ModuleFacts) {
    if let Stmt::Expr(expr_stmt) = stmt
        && let Expr::Assign(assign) = &*expr_stmt.expr
    {
        collect_commonjs_export(assign, facts);
    }
}

fn side_effect_import(source: String, kind: ImportKind, index: usize) -> ImportFact {
    ImportFact {
        source,
        local: format!("side-effect:{index}"),
        imported: ImportTarget::SideEffect,
        kind,
        type_only: false,
    }
}

/// Collect `require("...")` calls anywhere in the module, not just top-level
/// declarations: lazy in-function requires and bare `require('./x')` statements
/// also create edges. Returns the synthetic locals for value-position calls,
/// which the caller marks used after real usage is collected (the call site is
/// the usage, mirroring dynamic imports).
fn collect_require_imports(module: &Module, facts: &mut ModuleFacts) -> Vec<String> {
    let mut collector = RequireCollector {
        facts,
        synthetic_locals: Vec::new(),
        seen_value_sources: BTreeSet::new(),
    };
    module.visit_with(&mut collector);
    collector.synthetic_locals
}

struct RequireCollector<'a> {
    facts: &'a mut ModuleFacts,
    synthetic_locals: Vec<String>,
    seen_value_sources: BTreeSet<String>,
}

impl Visit for RequireCollector<'_> {
    fn visit_var_declarator(&mut self, declarator: &VarDeclarator) {
        // A binding gets real locals so usage gates the edge.
        if let Some(init) = &declarator.init
            && let Some(source) = require_call_source(init)
        {
            collect_require_binding(&declarator.name, source, self.facts);
            declarator.name.visit_with(self);
            return;
        }
        declarator.visit_children_with(self);
    }

    fn visit_expr_stmt(&mut self, stmt: &ExprStmt) {
        // A bare `require('./x');` statement is a side-effect import.
        if let Some(source) = require_call_source(&stmt.expr) {
            let fact = side_effect_import(source, ImportKind::CommonJs, self.facts.imports.len());
            self.facts.imports.push(fact);
            return;
        }
        stmt.visit_children_with(self);
    }

    fn visit_call_expr(&mut self, call: &CallExpr) {
        call.visit_children_with(self);
        // Any other value-position require depends on the whole module value.
        if let Some(source) = call_require_source(call)
            && self.seen_value_sources.insert(source.clone())
        {
            let local = format!("require():{}", self.synthetic_locals.len());
            self.facts.imports.push(ImportFact {
                source,
                local: local.clone(),
                imported: ImportTarget::Namespace,
                kind: ImportKind::CommonJs,
                type_only: false,
            });
            self.synthetic_locals.push(local);
        }
    }
}

fn require_call_source(expr: &Expr) -> Option<String> {
    let Expr::Call(call) = expr else {
        return None;
    };
    call_require_source(call)
}

fn call_require_source(call: &CallExpr) -> Option<String> {
    let Callee::Expr(callee) = &call.callee else {
        return None;
    };
    let Expr::Ident(ident) = &**callee else {
        return None;
    };
    if ident.sym != *"require" || call.args.len() != 1 {
        return None;
    }
    literal_string(&call.args[0].expr)
}

fn collect_require_binding(name: &Pat, source: String, facts: &mut ModuleFacts) {
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
        // A template literal with no interpolations is still a fixed path.
        Expr::Tpl(tpl) if tpl.exprs.is_empty() && tpl.quasis.len() == 1 => tpl.quasis[0]
            .cooked
            .as_ref()
            .map(|value| value.to_string_lossy().to_string()),
        _ => None,
    }
}

fn member_path_from_assign_target(target: &AssignTarget) -> Option<Vec<&str>> {
    match target {
        AssignTarget::Simple(SimpleAssignTarget::Member(member)) => member_path(member),
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

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;

    fn parse(name: &str, source: &str) -> ModuleFacts {
        parse_javascript_module(Path::new(name), source).unwrap()
    }

    #[test]
    fn collects_side_effect_imports() {
        let facts = parse(
            "setup.js",
            "import './polyfills';\nimport './styles.css';\n",
        );

        for source in ["./polyfills", "./styles.css"] {
            let import = facts
                .imports
                .iter()
                .find(|import| import.source == source)
                .expect("side-effect import should be collected");
            assert_eq!(import.imported, ImportTarget::SideEffect);
            assert_eq!(import.kind, ImportKind::Esm);
        }
    }

    #[test]
    fn collects_ts_import_equals_and_export_assignment() {
        let facts = parse(
            "legacy.ts",
            "import lib = require('./lib');\nexport = lib;\n",
        );

        let import = facts
            .imports
            .iter()
            .find(|import| import.source == "./lib")
            .expect("import-equals should be collected");
        assert_eq!(import.local, "lib");
        assert_eq!(import.imported, ImportTarget::Default);
        assert_eq!(import.kind, ImportKind::CommonJs);
        assert!(!import.type_only);

        let export = facts
            .exports
            .iter()
            .find(|export| export.exported == "default")
            .expect("export assignment should export the file's value");
        assert_eq!(export.local.as_deref(), Some("lib"));
    }

    #[test]
    fn collects_requires_anywhere_in_the_module() {
        let facts = parse(
            "server.cjs",
            r#"
require('./register');
function load() {
  const helper = require('./helper');
  return helper;
}
setTimeout(() => require('./worker').start(), 0);
"#,
        );

        let register = facts
            .imports
            .iter()
            .find(|import| import.source == "./register")
            .expect("bare require should be a side-effect import");
        assert_eq!(register.imported, ImportTarget::SideEffect);
        assert_eq!(register.kind, ImportKind::CommonJs);

        let helper = facts
            .imports
            .iter()
            .find(|import| import.source == "./helper")
            .expect("in-function require should keep its binding");
        assert_eq!(helper.local, "helper");
        assert_eq!(helper.imported, ImportTarget::Default);
        assert_eq!(helper.kind, ImportKind::CommonJs);
        assert!(facts.used_locals.contains("helper"));

        let worker = facts
            .imports
            .iter()
            .find(|import| import.source == "./worker")
            .expect("value-position require should be collected");
        assert_eq!(worker.imported, ImportTarget::Namespace);
        assert_eq!(worker.kind, ImportKind::CommonJs);
        // The call site is the usage, so the edge must count in the walk.
        assert!(facts.used_locals.contains(&worker.local));
    }

    #[test]
    fn collects_vitest_and_jest_mock_references() {
        let facts = parse(
            "widget.test.ts",
            r#"
import { vi } from "vitest";
vi.mock("./real-module");
jest.mock("@scope/pkg");
vi.doMock("./lazy-mock");
vi.unmock("./not-a-dep");
something.mock("./also-not-a-dep");
"#,
        );

        for source in ["./real-module", "@scope/pkg", "./lazy-mock"] {
            let mock = facts
                .imports
                .iter()
                .find(|import| import.source == source)
                .unwrap_or_else(|| panic!("mock of {source} should be collected"));
            assert_eq!(mock.imported, ImportTarget::SideEffect);
            assert_eq!(mock.kind, ImportKind::Mock);
            assert!(!mock.type_only);
        }

        // unmock removes a mock, and a non-runner `.mock(...)` is unrelated.
        assert!(
            !facts
                .imports
                .iter()
                .any(|import| import.source == "./not-a-dep" || import.source == "./also-not-a-dep")
        );
    }

    #[test]
    fn keeps_top_level_destructured_require_locals() {
        let facts = parse(
            "format.cjs",
            "const { format, parse: parseDate } = require('./dates');\nmodule.exports = { format, parseDate };\n",
        );

        assert!(facts.imports.iter().any(|import| {
            import.source == "./dates" && import.imported == ImportTarget::Name("format".into())
        }));
        assert!(facts.imports.iter().any(|import| {
            import.source == "./dates"
                && import.local == "parseDate"
                && import.imported == ImportTarget::Name("parse".into())
        }));
        // No duplicate namespace fact for the same call.
        assert_eq!(
            facts
                .imports
                .iter()
                .filter(|import| import.source == "./dates")
                .count(),
            2
        );
    }
}
