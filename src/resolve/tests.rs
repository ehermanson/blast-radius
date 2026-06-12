use std::fs;

use tempfile::tempdir;

use crate::fs::RepoContext;

use super::{Resolution, Resolver, match_alias};

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
fn resolves_package_imports_with_custom_source_condition() {
    let dir = tempdir().unwrap();
    fs::create_dir_all(dir.path().join("server/core")).unwrap();
    fs::create_dir_all(dir.path().join("dist/core")).unwrap();
    fs::create_dir_all(dir.path().join("app/src")).unwrap();
    fs::write(
        dir.path().join("package.json"),
        r##"{
            "name":"fixture",
            "imports":{
                "#core/*.js":{
                    "custom-source":"./server/core/*.ts",
                    "default":"./dist/core/*.js"
                }
            }
        }"##,
    )
    .unwrap();
    fs::write(
        dir.path().join("server/core/foo.ts"),
        "export const foo = true;",
    )
    .unwrap();
    fs::write(
        dir.path().join("app/src/App.ts"),
        "import { foo } from '#core/foo.js';",
    )
    .unwrap();

    let context = RepoContext::discover(dir.path()).unwrap();
    let resolver = Resolver::new(&context).unwrap();
    let importer = dir.path().join("app/src/App.ts");

    assert!(matches!(
        resolver.resolve(&importer, "#core/foo.js"),
        Resolution::Resolved(path) if path.ends_with("server/core/foo.ts")
    ));
    assert!(resolver.is_internal_specifier(&importer, "#core/foo.js"));
}

#[test]
fn resolves_vite_tsconfig_paths_and_base_url() {
    let dir = tempdir().unwrap();
    fs::create_dir_all(dir.path().join("app/src/components")).unwrap();
    fs::create_dir_all(dir.path().join("server/core")).unwrap();
    fs::write(
        dir.path().join("tsconfig.json"),
        r#"{"compilerOptions":{"baseUrl":".","paths":{
            "@/*":["app/src/*"],
            "@shared/*":["server/core/*"]
        }}}"#,
    )
    .unwrap();
    fs::write(dir.path().join("package.json"), r#"{"name":"fixture"}"#).unwrap();
    fs::write(
        dir.path().join("app/src/components/Button.tsx"),
        "export const Button = () => null;",
    )
    .unwrap();
    fs::write(
        dir.path().join("server/core/foo.ts"),
        "export const foo = true;",
    )
    .unwrap();
    fs::write(dir.path().join("app/src/env.ts"), "export const env = {};").unwrap();
    fs::write(
        dir.path().join("app/src/App.tsx"),
        "import { Button } from '@/components/Button.js';
         import { foo } from '@shared/foo.js';
         import { env } from 'app/src/env.js';",
    )
    .unwrap();

    let context = RepoContext::discover(dir.path()).unwrap();
    let resolver = Resolver::new(&context).unwrap();
    let importer = dir.path().join("app/src/App.tsx");

    assert!(matches!(
        resolver.resolve(&importer, "@/components/Button.js"),
        Resolution::Resolved(path) if path.ends_with("app/src/components/Button.tsx")
    ));
    assert!(matches!(
        resolver.resolve(&importer, "@shared/foo.js"),
        Resolution::Resolved(path) if path.ends_with("server/core/foo.ts")
    ));
    assert!(matches!(
        resolver.resolve(&importer, "app/src/env.js"),
        Resolution::Resolved(path) if path.ends_with("app/src/env.ts")
    ));
}

#[test]
fn resolves_workspace_exports_with_custom_source_condition() {
    let dir = tempdir().unwrap();
    fs::create_dir_all(dir.path().join("packages/core/server/core")).unwrap();
    fs::create_dir_all(dir.path().join("packages/core/dist/core")).unwrap();
    fs::create_dir_all(dir.path().join("apps/web/src")).unwrap();
    fs::write(
        dir.path().join("packages/core/package.json"),
        r#"{
            "name":"@acme/core",
            "exports":{
                "./*.js":{
                    "custom-source":"./server/core/*.ts",
                    "default":"./dist/core/*.js"
                }
            }
        }"#,
    )
    .unwrap();
    fs::write(
        dir.path().join("packages/core/server/core/foo.ts"),
        "export const foo = true;",
    )
    .unwrap();
    fs::write(
        dir.path().join("apps/web/src/App.ts"),
        "import { foo } from '@acme/core/foo.js';",
    )
    .unwrap();

    let context = RepoContext::discover(dir.path()).unwrap();
    let resolver = Resolver::new(&context).unwrap();
    let importer = dir.path().join("apps/web/src/App.ts");

    assert!(matches!(
        resolver.resolve(&importer, "@acme/core/foo.js"),
        Resolution::Resolved(path) if path.ends_with("packages/core/server/core/foo.ts")
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

#[test]
fn multi_dot_specifier_wins_over_extension_replacement() {
    let dir = tempdir().unwrap();
    fs::create_dir_all(dir.path().join("src")).unwrap();
    fs::write(dir.path().join("package.json"), r#"{"name":"fixture"}"#).unwrap();
    fs::write(
        dir.path().join("src/App.tsx"),
        "import './recipe.types'; import './recipe';",
    )
    .unwrap();
    // Both the multi-dot module and a sibling sharing its stem exist; the
    // appended form must win over extension replacement.
    fs::write(dir.path().join("src/recipe.ts"), "export const recipe = 1;").unwrap();
    fs::write(
        dir.path().join("src/recipe.types.ts"),
        "export type Recipe = {};",
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
        resolver.resolve(&importer, "./recipe"),
        Resolution::Resolved(path) if path.ends_with("src/recipe.ts")
    ));
}

#[test]
fn asset_imports_do_not_resolve_via_extension_replacement() {
    let dir = tempdir().unwrap();
    fs::create_dir_all(dir.path().join("src")).unwrap();
    fs::write(dir.path().join("package.json"), r#"{"name":"fixture"}"#).unwrap();
    fs::write(
        dir.path().join("src/App.tsx"),
        "import './theme.css'; import { theme } from './theme.js';",
    )
    .unwrap();
    fs::write(dir.path().join("src/theme.ts"), "export const theme = {};").unwrap();

    let context = RepoContext::discover(dir.path()).unwrap();
    let resolver = Resolver::new(&context).unwrap();
    let importer = dir.path().join("src/App.tsx");

    // A css asset must stay unresolved instead of hitting theme.ts; only
    // JS-emitted extensions are rewritten to their TS counterparts.
    assert!(matches!(
        resolver.resolve(&importer, "./theme.css"),
        Resolution::Unresolved
    ));
    assert!(matches!(
        resolver.resolve(&importer, "./theme.js"),
        Resolution::Resolved(path) if path.ends_with("src/theme.ts")
    ));
}

#[test]
fn resolves_declaration_only_modules() {
    let dir = tempdir().unwrap();
    fs::create_dir_all(dir.path().join("src/models")).unwrap();
    fs::write(dir.path().join("package.json"), r#"{"name":"fixture"}"#).unwrap();
    fs::write(
        dir.path().join("src/App.tsx"),
        "import './types'; import './models'; import './legacy.js';",
    )
    .unwrap();
    fs::write(dir.path().join("src/types.d.ts"), "export type T = {};").unwrap();
    fs::write(
        dir.path().join("src/models/index.d.ts"),
        "export type M = {};",
    )
    .unwrap();
    fs::write(
        dir.path().join("src/legacy.d.ts"),
        "export const legacy: 1;",
    )
    .unwrap();

    let context = RepoContext::discover(dir.path()).unwrap();
    let resolver = Resolver::new(&context).unwrap();
    let importer = dir.path().join("src/App.tsx");

    assert!(matches!(
        resolver.resolve(&importer, "./types"),
        Resolution::Resolved(path) if path.ends_with("src/types.d.ts")
    ));
    assert!(matches!(
        resolver.resolve(&importer, "./models"),
        Resolution::Resolved(path) if path.ends_with("src/models/index.d.ts")
    ));
    assert!(matches!(
        resolver.resolve(&importer, "./legacy.js"),
        Resolution::Resolved(path) if path.ends_with("src/legacy.d.ts")
    ));
}

#[test]
fn follows_tsconfig_extends_for_paths_anchored_to_declaring_config() {
    let dir = tempdir().unwrap();
    fs::create_dir_all(dir.path().join("packages/ui/src")).unwrap();
    fs::create_dir_all(dir.path().join("apps/web/src")).unwrap();
    // Nx layout: paths live in the root base config (no baseUrl); the project
    // config only extends it. Targets must anchor to the base config's dir.
    fs::write(
        dir.path().join("tsconfig.base.json"),
        r#"{"compilerOptions":{"paths":{"@ui/*":["packages/ui/src/*"]}}}"#,
    )
    .unwrap();
    fs::write(
        dir.path().join("apps/web/tsconfig.json"),
        r#"{"extends":"../../tsconfig.base.json"}"#,
    )
    .unwrap();
    fs::write(dir.path().join("package.json"), r#"{"name":"fixture"}"#).unwrap();
    fs::write(
        dir.path().join("packages/ui/src/Button.tsx"),
        "export const Button = () => null;",
    )
    .unwrap();
    fs::write(
        dir.path().join("apps/web/src/App.tsx"),
        "import { Button } from '@ui/Button';",
    )
    .unwrap();

    let context = RepoContext::discover(dir.path()).unwrap();
    let resolver = Resolver::new(&context).unwrap();
    let importer = dir.path().join("apps/web/src/App.tsx");

    assert!(matches!(
        resolver.resolve(&importer, "@ui/Button"),
        Resolution::Resolved(path) if path.ends_with("packages/ui/src/Button.tsx")
    ));
    assert!(resolver.is_internal_specifier(&importer, "@ui/Button"));
}

#[test]
fn child_paths_replace_extended_parent_paths_wholesale() {
    let dir = tempdir().unwrap();
    fs::create_dir_all(dir.path().join("packages/ui/src")).unwrap();
    fs::create_dir_all(dir.path().join("apps/web/src")).unwrap();
    fs::create_dir_all(dir.path().join("old")).unwrap();
    fs::write(
        dir.path().join("tsconfig.base.json"),
        r#"{"compilerOptions":{"baseUrl":".","paths":{"@old/*":["old/*"]}}}"#,
    )
    .unwrap();
    fs::write(
        dir.path().join("apps/web/tsconfig.json"),
        r#"{"extends":"../../tsconfig.base.json","compilerOptions":{"paths":{"@new/*":["packages/ui/src/*"]}}}"#,
    )
    .unwrap();
    fs::write(dir.path().join("package.json"), r#"{"name":"fixture"}"#).unwrap();
    fs::write(dir.path().join("old/legacy.ts"), "export const legacy = 1;").unwrap();
    fs::write(
        dir.path().join("packages/ui/src/Button.tsx"),
        "export const Button = () => null;",
    )
    .unwrap();
    fs::write(
        dir.path().join("apps/web/src/App.tsx"),
        "import { Button } from '@new/Button'; import { legacy } from '@old/legacy';",
    )
    .unwrap();

    let context = RepoContext::discover(dir.path()).unwrap();
    let resolver = Resolver::new(&context).unwrap();
    let importer = dir.path().join("apps/web/src/App.tsx");

    // Child paths anchor to the inherited baseUrl (base config's dir) and
    // replace the parent's paths wholesale, so @old no longer matches.
    assert!(matches!(
        resolver.resolve(&importer, "@new/Button"),
        Resolution::Resolved(path) if path.ends_with("packages/ui/src/Button.tsx")
    ));
    assert!(matches!(
        resolver.resolve(&importer, "@old/legacy"),
        Resolution::Unresolved
    ));
}

#[test]
fn skips_bare_package_extends_in_array_form() {
    let dir = tempdir().unwrap();
    fs::create_dir_all(dir.path().join("packages/ui/src")).unwrap();
    fs::create_dir_all(dir.path().join("apps/web/src")).unwrap();
    fs::write(
        dir.path().join("tsconfig.json"),
        r#"{"extends":["@tsconfig/node18","./tsconfig.base"]}"#,
    )
    .unwrap();
    // Specifier above omits .json; loading must append it like TypeScript does.
    fs::write(
        dir.path().join("tsconfig.base.json"),
        r#"{"compilerOptions":{"baseUrl":".","paths":{"@ui/*":["packages/ui/src/*"]}}}"#,
    )
    .unwrap();
    fs::write(dir.path().join("package.json"), r#"{"name":"fixture"}"#).unwrap();
    fs::write(
        dir.path().join("packages/ui/src/Button.tsx"),
        "export const Button = () => null;",
    )
    .unwrap();
    fs::write(
        dir.path().join("apps/web/src/App.tsx"),
        "import { Button } from '@ui/Button';",
    )
    .unwrap();

    let context = RepoContext::discover(dir.path()).unwrap();
    let resolver = Resolver::new(&context).unwrap();
    let importer = dir.path().join("apps/web/src/App.tsx");

    assert!(matches!(
        resolver.resolve(&importer, "@ui/Button"),
        Resolution::Resolved(path) if path.ends_with("packages/ui/src/Button.tsx")
    ));
}

#[test]
fn tsconfig_extends_cycle_is_tolerated() {
    let dir = tempdir().unwrap();
    fs::create_dir_all(dir.path().join("src")).unwrap();
    fs::write(
        dir.path().join("tsconfig.json"),
        r#"{"extends":"./tsconfig.base.json"}"#,
    )
    .unwrap();
    fs::write(
        dir.path().join("tsconfig.base.json"),
        r#"{"extends":"./tsconfig.json"}"#,
    )
    .unwrap();
    fs::write(dir.path().join("package.json"), r#"{"name":"fixture"}"#).unwrap();
    fs::write(dir.path().join("src/App.tsx"), "import './util';").unwrap();
    fs::write(dir.path().join("src/util.ts"), "export const util = 1;").unwrap();

    let context = RepoContext::discover(dir.path()).unwrap();
    let resolver = Resolver::new(&context).unwrap();
    let importer = dir.path().join("src/App.tsx");

    assert!(matches!(
        resolver.resolve(&importer, "./util"),
        Resolution::Resolved(path) if path.ends_with("src/util.ts")
    ));
}

#[test]
fn discovers_alias_bearing_sibling_tsconfig() {
    let dir = tempdir().unwrap();
    fs::create_dir_all(dir.path().join("packages/ui/src")).unwrap();
    fs::create_dir_all(dir.path().join("apps/web/src")).unwrap();
    // Vite scaffold: tsconfig.json only carries references; the aliases live in
    // a sibling config that nothing extends.
    fs::write(dir.path().join("tsconfig.json"), r#"{"files":[]}"#).unwrap();
    fs::write(
        dir.path().join("tsconfig.app.json"),
        r#"{"compilerOptions":{"baseUrl":".","paths":{"@ui/*":["packages/ui/src/*"]}}}"#,
    )
    .unwrap();
    fs::write(dir.path().join("package.json"), r#"{"name":"fixture"}"#).unwrap();
    fs::write(
        dir.path().join("packages/ui/src/Button.tsx"),
        "export const Button = () => null;",
    )
    .unwrap();
    fs::write(
        dir.path().join("apps/web/src/App.tsx"),
        "import { Button } from '@ui/Button';",
    )
    .unwrap();

    let context = RepoContext::discover(dir.path()).unwrap();
    let resolver = Resolver::new(&context).unwrap();
    let importer = dir.path().join("apps/web/src/App.tsx");

    assert!(matches!(
        resolver.resolve(&importer, "@ui/Button"),
        Resolution::Resolved(path) if path.ends_with("packages/ui/src/Button.tsx")
    ));
}

#[test]
fn aliasless_nested_tsconfig_does_not_shadow_root_aliases() {
    let dir = tempdir().unwrap();
    fs::create_dir_all(dir.path().join("packages/ui/src")).unwrap();
    fs::create_dir_all(dir.path().join("apps/web/src")).unwrap();
    fs::write(
        dir.path().join("tsconfig.json"),
        r#"{"compilerOptions":{"baseUrl":".","paths":{"@ui/*":["packages/ui/src/*"]}}}"#,
    )
    .unwrap();
    // The nearer config declares no paths/baseUrl and must not shadow the root.
    fs::write(
        dir.path().join("apps/web/tsconfig.json"),
        r#"{"compilerOptions":{"strict":true}}"#,
    )
    .unwrap();
    fs::write(dir.path().join("package.json"), r#"{"name":"fixture"}"#).unwrap();
    fs::write(
        dir.path().join("packages/ui/src/Button.tsx"),
        "export const Button = () => null;",
    )
    .unwrap();
    fs::write(
        dir.path().join("apps/web/src/App.tsx"),
        "import { Button } from '@ui/Button';",
    )
    .unwrap();

    let context = RepoContext::discover(dir.path()).unwrap();
    let resolver = Resolver::new(&context).unwrap();
    let importer = dir.path().join("apps/web/src/App.tsx");

    assert!(matches!(
        resolver.resolve(&importer, "@ui/Button"),
        Resolution::Resolved(path) if path.ends_with("packages/ui/src/Button.tsx")
    ));
}

#[test]
fn overlapping_wildcard_alias_is_no_match() {
    assert_eq!(match_alias("lib/*/lib", "lib/lib"), None);
    assert_eq!(
        match_alias("lib/*/lib", "lib/x/lib"),
        Some(vec!["x".to_string()])
    );
}

#[test]
fn overlapping_wildcard_alias_does_not_panic_during_resolution() {
    let dir = tempdir().unwrap();
    fs::create_dir_all(dir.path().join("src")).unwrap();
    fs::write(
        dir.path().join("tsconfig.json"),
        r#"{"compilerOptions":{"baseUrl":".","paths":{"lib/*/lib":["src/*/lib"]}}}"#,
    )
    .unwrap();
    fs::write(dir.path().join("package.json"), r#"{"name":"fixture"}"#).unwrap();
    fs::write(dir.path().join("src/App.tsx"), "import 'lib/lib';").unwrap();

    let context = RepoContext::discover(dir.path()).unwrap();
    let resolver = Resolver::new(&context).unwrap();
    let importer = dir.path().join("src/App.tsx");

    // Prefix and suffix overlap in the specifier; previously this sliced out of
    // bounds and panicked.
    assert!(matches!(
        resolver.resolve(&importer, "lib/lib"),
        Resolution::Unresolved
    ));
}

#[cfg(unix)]
#[test]
fn unreadable_directory_is_skipped_with_warning() {
    use std::os::unix::fs::PermissionsExt;

    let dir = tempdir().unwrap();
    fs::create_dir_all(dir.path().join("src")).unwrap();
    fs::create_dir_all(dir.path().join("locked")).unwrap();
    fs::write(
        dir.path().join("src/App.tsx"),
        "export const App = () => null;",
    )
    .unwrap();
    let locked = dir.path().join("locked");
    fs::set_permissions(&locked, fs::Permissions::from_mode(0o000)).unwrap();

    let context = RepoContext::discover(dir.path());

    fs::set_permissions(&locked, fs::Permissions::from_mode(0o755)).unwrap();
    let context = context.unwrap();

    assert_eq!(context.source_files.len(), 1);
    assert!(
        context
            .warnings
            .iter()
            .any(|warning| warning.contains("skipping unreadable path")),
        "unreadable directory should surface as a discovery warning"
    );
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
