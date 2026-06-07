use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use anyhow::Result;
use swc_common::{FileName, SourceMap, sync::Lrc};
use swc_ecma_ast::*;
use swc_ecma_parser::{EsSyntax, Parser, StringInput, Syntax, TsSyntax, lexer::Lexer};
use swc_ecma_visit::{Visit, VisitWith};

use super::*;

pub(super) fn parse_javascript_module(path: &Path, source: &str) -> Result<ModuleFacts> {
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
            if self.namespace_locals.contains(&namespace)
                && let MemberProp::Ident(prop) = &member.prop
            {
                self.namespace_member_usage
                    .entry(namespace)
                    .or_default()
                    .insert(prop.sym.to_string());
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
