use std::fs;

use tempfile::tempdir;

use crate::fs::RepoContext;

use super::{Resolution, Resolver};

#[test]
fn resolves_tsconfig_aliases_and_workspace_packages() {
    let dir = tempdir().unwrap();
    fs::create_dir_all(dir.path().join("packages/ui/src")).unwrap();
    fs::create_dir_all(dir.path().join("apps/web/src")).unwrap();
    fs::write(
        dir.path().join("tsconfig.json"),
        r#"{"compilerOptions":{"baseUrl":".","paths":{"@ui/*":["packages/ui/src/*"]}}}"#,
    )
    .unwrap();
    fs::write(
        dir.path().join("packages/ui/package.json"),
        r#"{"name":"@acme/ui","source":"src/index.ts"}"#,
    )
    .unwrap();
    fs::write(
        dir.path().join("packages/ui/src/index.ts"),
        "export * from './Button';",
    )
    .unwrap();
    fs::write(
        dir.path().join("packages/ui/src/Button.tsx"),
        "export const Button = () => null;",
    )
    .unwrap();
    fs::write(
        dir.path().join("apps/web/src/App.tsx"),
        "import { Button } from '@ui/Button'; import { Button as Two } from '@acme/ui';",
    )
    .unwrap();

    let context = RepoContext::discover(dir.path()).unwrap();
    let resolver = Resolver::new(&context).unwrap();
    let importer = dir.path().join("apps/web/src/App.tsx");

    assert!(matches!(
        resolver.resolve(&importer, "@ui/Button"),
        Resolution::Resolved(_)
    ));
    assert!(matches!(
        resolver.resolve(&importer, "@acme/ui"),
        Resolution::Resolved(_)
    ));
}

#[test]
fn resolves_modern_module_extensions() {
    let dir = tempdir().unwrap();
    fs::create_dir_all(dir.path().join("src")).unwrap();
    fs::write(dir.path().join("package.json"), r#"{"name":"fixture"}"#).unwrap();
    fs::write(
        dir.path().join("src/App.tsx"),
        "import { button } from './button'; import { server } from './server';",
    )
    .unwrap();
    fs::write(
        dir.path().join("src/button.mjs"),
        "export const button = 'ok';",
    )
    .unwrap();
    fs::write(
        dir.path().join("src/server.cts"),
        "export const server = 'ok';",
    )
    .unwrap();

    let context = RepoContext::discover(dir.path()).unwrap();
    let resolver = Resolver::new(&context).unwrap();
    let importer = dir.path().join("src/App.tsx");

    assert!(matches!(
        resolver.resolve(&importer, "./button"),
        Resolution::Resolved(path) if path.ends_with("src/button.mjs")
    ));
    assert!(matches!(
        resolver.resolve(&importer, "./server"),
        Resolution::Resolved(path) if path.ends_with("src/server.cts")
    ));
}

#[test]
fn resolves_workspace_package_exports() {
    let dir = tempdir().unwrap();
    fs::create_dir_all(dir.path().join("packages/ui/src/components/button")).unwrap();
    fs::create_dir_all(dir.path().join("apps/web/src")).unwrap();
    fs::write(
        dir.path().join("packages/ui/package.json"),
        r#"{
            "name":"@acme/ui",
            "exports":{
                ".":{"dev":"./src/index.ts"},
                "./preset":{"dev":"./src/preset.ts"},
                "./*":{"dev":"./src/components/*/index.ts"}
            }
        }"#,
    )
    .unwrap();
    fs::write(
        dir.path().join("packages/ui/src/index.ts"),
        "export * from './components/button';",
    )
    .unwrap();
    fs::write(
        dir.path().join("packages/ui/src/preset.ts"),
        "export const preset = true;",
    )
    .unwrap();
    fs::write(
        dir.path()
            .join("packages/ui/src/components/button/index.ts"),
        "export const Button = () => null;",
    )
    .unwrap();
    fs::write(
        dir.path().join("apps/web/src/App.tsx"),
        "import { Button } from '@acme/ui/button'; import { preset } from '@acme/ui/preset';",
    )
    .unwrap();

    let context = RepoContext::discover(dir.path()).unwrap();
    let resolver = Resolver::new(&context).unwrap();
    let importer = dir.path().join("apps/web/src/App.tsx");

    assert!(matches!(
        resolver.resolve(&importer, "@acme/ui/button"),
        Resolution::Resolved(path) if path.ends_with("packages/ui/src/components/button/index.ts")
    ));
    assert!(matches!(
        resolver.resolve(&importer, "@acme/ui/preset"),
        Resolution::Resolved(path) if path.ends_with("packages/ui/src/preset.ts")
    ));
}

#[test]
fn resolves_multi_dot_basenames() {
    let dir = tempdir().unwrap();
    fs::create_dir_all(dir.path().join("src")).unwrap();
    fs::write(dir.path().join("package.json"), r#"{"name":"fixture"}"#).unwrap();
    fs::write(
        dir.path().join("src/App.tsx"),
        "import './recipe.types'; import './docs.config'; import './routeTree.gen';",
    )
    .unwrap();
    fs::write(
        dir.path().join("src/recipe.types.ts"),
        "export type Recipe = {};",
    )
    .unwrap();
    fs::write(
        dir.path().join("src/docs.config.ts"),
        "export const docsConfig = {};",
    )
    .unwrap();
    fs::write(
        dir.path().join("src/routeTree.gen.ts"),
        "export const routeTree = {};",
    )
    .unwrap();

    let context = RepoContext::discover(dir.path()).unwrap();
    let resolver = Resolver::new(&context).unwrap();
    let importer = dir.path().join("src/App.tsx");

    assert!(matches!(
        resolver.resolve(&importer, "./recipe.types"),
        Resolution::Resolved(path) if path.ends_with("src/recipe.types.ts")
    ));
    assert!(matches!(
        resolver.resolve(&importer, "./docs.config"),
        Resolution::Resolved(path) if path.ends_with("src/docs.config.ts")
    ));
    assert!(matches!(
        resolver.resolve(&importer, "./routeTree.gen"),
        Resolution::Resolved(path) if path.ends_with("src/routeTree.gen.ts")
    ));
}

#[cfg(feature = "python")]
#[test]
fn resolves_python_absolute_relative_and_package_imports() {
    let dir = tempdir().unwrap();
    fs::create_dir_all(dir.path().join("app/services")).unwrap();
    fs::create_dir_all(dir.path().join("app/utils")).unwrap();
    fs::write(dir.path().join("app/__init__.py"), "").unwrap();
    fs::write(dir.path().join("app/models.py"), "class User: pass").unwrap();
    fs::write(
        dir.path().join("app/services/email.py"),
        "from ..models import User",
    )
    .unwrap();
    fs::write(
        dir.path().join("app/utils/__init__.py"),
        "def format_subject(): pass",
    )
    .unwrap();

    let context = RepoContext::discover(dir.path()).unwrap();
    let resolver = Resolver::new(&context).unwrap();
    let importer = dir.path().join("app/services/email.py");

    assert!(matches!(
        resolver.resolve(&importer, "..models"),
        Resolution::Resolved(path) if path.ends_with("app/models.py")
    ));
    assert!(matches!(
        resolver.resolve(&importer, "app.utils"),
        Resolution::Resolved(path) if path.ends_with("app/utils/__init__.py")
    ));
    assert!(resolver.is_internal_specifier(&importer, "..models"));
    assert!(resolver.is_internal_specifier(&importer, "app.models"));
    assert!(!resolver.is_internal_specifier(&importer, "dataclasses"));
}

#[test]
fn exact_package_export_wins_over_wildcard() {
    let dir = tempdir().unwrap();
    fs::create_dir_all(dir.path().join("packages/ui/src/components/preset")).unwrap();
    fs::create_dir_all(dir.path().join("apps/web/src")).unwrap();
    fs::write(
        dir.path().join("packages/ui/package.json"),
        r#"{
            "name":"@acme/ui",
            "exports":{
                "./preset":{"dev":"./src/preset.ts"},
                "./*":{"dev":"./src/components/*/index.ts"}
            }
        }"#,
    )
    .unwrap();
    fs::write(
        dir.path().join("packages/ui/src/preset.ts"),
        "export const preset = true;",
    )
    .unwrap();
    // A wildcard target for `preset` also exists; the exact `./preset` export
    // must still win.
    fs::write(
        dir.path()
            .join("packages/ui/src/components/preset/index.ts"),
        "export const wrong = true;",
    )
    .unwrap();
    fs::write(
        dir.path().join("apps/web/src/App.tsx"),
        "import { preset } from '@acme/ui/preset';",
    )
    .unwrap();

    let context = RepoContext::discover(dir.path()).unwrap();
    let resolver = Resolver::new(&context).unwrap();
    let importer = dir.path().join("apps/web/src/App.tsx");

    assert!(matches!(
        resolver.resolve(&importer, "@acme/ui/preset"),
        Resolution::Resolved(path) if path.ends_with("packages/ui/src/preset.ts")
    ));
}

#[test]
fn exact_tsconfig_alias_wins_over_broad_wildcard() {
    let dir = tempdir().unwrap();
    fs::create_dir_all(dir.path().join("src/theme")).unwrap();
    fs::write(
        dir.path().join("tsconfig.json"),
        r#"{"compilerOptions":{"baseUrl":".","paths":{
            "@/*":["src/*"],
            "@/theme":["src/theme/index.ts"]
        }}}"#,
    )
    .unwrap();
    fs::write(dir.path().join("package.json"), r#"{"name":"fixture"}"#).unwrap();
    // Both the broad `@/*` (-> src/theme.ts) and exact `@/theme` targets exist.
    fs::write(dir.path().join("src/theme.ts"), "export const wrong = 1;").unwrap();
    fs::write(
        dir.path().join("src/theme/index.ts"),
        "export const right = 1;",
    )
    .unwrap();
    fs::write(
        dir.path().join("src/App.tsx"),
        "import { right } from '@/theme';",
    )
    .unwrap();

    let context = RepoContext::discover(dir.path()).unwrap();
    let resolver = Resolver::new(&context).unwrap();
    let importer = dir.path().join("src/App.tsx");

    assert!(matches!(
        resolver.resolve(&importer, "@/theme"),
        Resolution::Resolved(path) if path.ends_with("src/theme/index.ts")
    ));
}

#[cfg(feature = "python")]
#[test]
fn javascript_import_does_not_resolve_to_python_file() {
    let dir = tempdir().unwrap();
    fs::create_dir_all(dir.path().join("src")).unwrap();
    fs::write(dir.path().join("package.json"), r#"{"name":"fixture"}"#).unwrap();
    fs::write(dir.path().join("src/app.tsx"), "import './model.py';").unwrap();
    fs::write(dir.path().join("src/model.py"), "x = 1").unwrap();

    let context = RepoContext::discover(dir.path()).unwrap();
    let resolver = Resolver::new(&context).unwrap();
    let importer = dir.path().join("src/app.tsx");

    // An explicit foreign extension must not satisfy a JS import even though the
    // file is indexed (Python feature compiled in).
    assert!(matches!(
        resolver.resolve(&importer, "./model.py"),
        Resolution::Unresolved
    ));
}

#[cfg(feature = "python")]
#[test]
fn python_import_does_not_resolve_to_javascript_file() {
    let dir = tempdir().unwrap();
    fs::create_dir_all(dir.path().join("app")).unwrap();
    fs::write(dir.path().join("app/__init__.py"), "").unwrap();
    fs::write(dir.path().join("app/models.py"), "class User: pass").unwrap();
    // A same-named JS/TS file must not satisfy a Python import: resolution is
    // scoped to the importer's language family.
    fs::write(dir.path().join("app/models.ts"), "export const User = 1;").unwrap();
    fs::write(dir.path().join("app/main.py"), "from app import models").unwrap();

    let context = RepoContext::discover(dir.path()).unwrap();
    let resolver = Resolver::new(&context).unwrap();
    let importer = dir.path().join("app/main.py");

    assert!(matches!(
        resolver.resolve(&importer, "app.models"),
        Resolution::Resolved(path) if path.ends_with("app/models.py")
    ));
}

#[cfg(feature = "python")]
#[test]
fn resolves_python_src_layout_package_imports() {
    let dir = tempdir().unwrap();
    fs::create_dir_all(dir.path().join("src/my_pkg")).unwrap();
    fs::write(dir.path().join("src/my_pkg/__init__.py"), "").unwrap();
    fs::write(dir.path().join("src/my_pkg/models.py"), "class User: pass").unwrap();
    fs::write(
        dir.path().join("src/my_pkg/main.py"),
        "import my_pkg.models",
    )
    .unwrap();

    let context = RepoContext::discover(dir.path()).unwrap();
    let resolver = Resolver::new(&context).unwrap();
    let importer = dir.path().join("src/my_pkg/main.py");

    assert!(matches!(
        resolver.resolve(&importer, "my_pkg.models"),
        Resolution::Resolved(path) if path.ends_with("src/my_pkg/models.py")
    ));
    assert!(resolver.is_internal_specifier(&importer, "my_pkg.models"));
}

#[cfg(feature = "rust")]
#[test]
fn rust_crate_import_resolves_within_importers_crate() {
    let dir = tempdir().unwrap();
    fs::create_dir_all(dir.path().join("crates/a/src")).unwrap();
    fs::create_dir_all(dir.path().join("crates/b/src")).unwrap();
    fs::write(dir.path().join("crates/a/src/lib.rs"), "pub mod models;").unwrap();
    fs::write(dir.path().join("crates/a/src/models.rs"), "pub struct A;").unwrap();
    fs::write(
        dir.path().join("crates/b/src/lib.rs"),
        "pub mod models;\nuse crate::models::B;",
    )
    .unwrap();
    fs::write(dir.path().join("crates/b/src/models.rs"), "pub struct B;").unwrap();

    let context = RepoContext::discover(dir.path()).unwrap();
    let resolver = Resolver::new(&context).unwrap();
    let b_lib = dir.path().join("crates/b/src/lib.rs");

    // `crate::models` from crate B must hit B's module, not crate A's
    // identically-named one (which sorts first among crate roots).
    assert!(matches!(
        resolver.resolve(&b_lib, "crate::models"),
        Resolution::Resolved(path) if path.ends_with("crates/b/src/models.rs")
    ));
}

#[cfg(feature = "rust")]
#[test]
fn resolves_rust_crate_super_self_and_mod_imports() {
    let dir = tempdir().unwrap();
    fs::create_dir_all(dir.path().join("src/services")).unwrap();
    fs::create_dir_all(dir.path().join("src/utils")).unwrap();
    fs::write(dir.path().join("src/lib.rs"), "pub mod services;").unwrap();
    fs::write(dir.path().join("src/models.rs"), "pub struct User;").unwrap();
    fs::write(dir.path().join("src/services/mod.rs"), "pub mod email;").unwrap();
    fs::write(
        dir.path().join("src/services/email.rs"),
        "use super::formatter;",
    )
    .unwrap();
    fs::write(
        dir.path().join("src/services/formatter.rs"),
        "pub fn f() {}",
    )
    .unwrap();
    fs::write(dir.path().join("src/utils/mod.rs"), "pub mod formatting;").unwrap();
    fs::write(dir.path().join("src/utils/formatting.rs"), "pub fn f() {}").unwrap();

    let context = RepoContext::discover(dir.path()).unwrap();
    let resolver = Resolver::new(&context).unwrap();
    let lib = dir.path().join("src/lib.rs");
    let service_mod = dir.path().join("src/services/mod.rs");
    let email = dir.path().join("src/services/email.rs");

    assert!(matches!(
        resolver.resolve(&lib, "mod:services"),
        Resolution::Resolved(path) if path.ends_with("src/services/mod.rs")
    ));
    assert!(matches!(
        resolver.resolve(&lib, "crate::models"),
        Resolution::Resolved(path) if path.ends_with("src/models.rs")
    ));
    assert!(matches!(
        resolver.resolve(&service_mod, "self::email"),
        Resolution::Resolved(path) if path.ends_with("src/services/email.rs")
    ));
    assert!(matches!(
        resolver.resolve(&email, "super::formatter"),
        Resolution::Resolved(path) if path.ends_with("src/services/formatter.rs")
    ));
    assert!(resolver.is_internal_specifier(&email, "crate::models"));
    assert!(!resolver.is_internal_specifier(&email, "serde"));
}

#[cfg(feature = "ruby")]
#[test]
fn resolves_ruby_suffix_imports_from_index() {
    let dir = tempdir().unwrap();
    fs::create_dir_all(dir.path().join("app/services")).unwrap();
    fs::create_dir_all(dir.path().join("app/utils")).unwrap();
    fs::write(dir.path().join("app/services/email.rb"), "class Email; end").unwrap();
    fs::write(
        dir.path().join("app/utils/formatter.rb"),
        "class Formatter; end",
    )
    .unwrap();

    let context = RepoContext::discover(dir.path()).unwrap();
    let resolver = Resolver::new(&context).unwrap();
    let importer = dir.path().join("app/services/email.rb");

    assert!(matches!(
        resolver.resolve(&importer, "utils/formatter"),
        Resolution::Resolved(path) if path.ends_with("app/utils/formatter.rb")
    ));
}

#[cfg(feature = "java")]
#[test]
fn resolves_java_class_and_wildcard_imports_from_index() {
    let dir = tempdir().unwrap();
    fs::create_dir_all(dir.path().join("src/main/java/com/example/service")).unwrap();
    fs::create_dir_all(dir.path().join("src/main/java/com/example/util")).unwrap();
    fs::write(
        dir.path()
            .join("src/main/java/com/example/service/EmailService.java"),
        "package com.example.service; class EmailService {}",
    )
    .unwrap();
    fs::write(
        dir.path()
            .join("src/main/java/com/example/util/Formatter.java"),
        "package com.example.util; class Formatter {}",
    )
    .unwrap();

    let context = RepoContext::discover(dir.path()).unwrap();
    let resolver = Resolver::new(&context).unwrap();
    let importer = dir
        .path()
        .join("src/main/java/com/example/service/EmailService.java");

    assert!(matches!(
        resolver.resolve(&importer, "com.example.util.Formatter"),
        Resolution::Resolved(path) if path.ends_with("src/main/java/com/example/util/Formatter.java")
    ));
    assert!(matches!(
        resolver.resolve(&importer, "com.example.util.*"),
        Resolution::Resolved(path) if path.ends_with("src/main/java/com/example/util/Formatter.java")
    ));
}
