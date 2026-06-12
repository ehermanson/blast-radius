use std::collections::{BTreeMap, BTreeSet};

use swc_ecma_ast::*;
use swc_ecma_visit::{Visit, VisitWith};

pub(super) struct UsageCollector {
    imported_locals: BTreeSet<String>,
    namespace_locals: BTreeSet<String>,
    pub(super) used_locals: BTreeSet<String>,
    pub(super) jsx_locals: BTreeSet<String>,
    pub(super) namespace_member_usage: BTreeMap<String, BTreeSet<String>>,
    pub(super) jsx_namespace_member_usage: BTreeMap<String, BTreeSet<String>>,
}

impl UsageCollector {
    pub(super) fn new(
        imported_locals: BTreeSet<String>,
        namespace_locals: BTreeSet<String>,
    ) -> Self {
        Self {
            imported_locals,
            namespace_locals,
            used_locals: BTreeSet::new(),
            jsx_locals: BTreeSet::new(),
            namespace_member_usage: BTreeMap::new(),
            jsx_namespace_member_usage: BTreeMap::new(),
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
                    self.used_locals.insert(value.clone());
                    self.jsx_locals.insert(value);
                }
            }
            JSXElementName::JSXMemberExpr(expr) => self.mark_jsx_member(expr),
            JSXElementName::JSXNamespacedName(_) => {}
        }
    }

    fn mark_jsx_member(&mut self, expr: &JSXMemberExpr) {
        if let JSXObject::Ident(object) = &expr.obj {
            let namespace = object.sym.to_string();
            if self.namespace_locals.contains(&namespace)
                || self.imported_locals.contains(&namespace)
            {
                // Named imports of namespace objects (`export * as ns` consumed
                // via `import { ns }`) get member tracking too; the object
                // itself still counts as used so the usage gate passes.
                if self.imported_locals.contains(&namespace) {
                    self.used_locals.insert(namespace.clone());
                }
                self.namespace_member_usage
                    .entry(namespace)
                    .or_default()
                    .insert(expr.prop.sym.to_string());
                self.jsx_namespace_member_usage
                    .entry(object.sym.to_string())
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
            if (self.namespace_locals.contains(&namespace)
                || self.imported_locals.contains(&namespace))
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
