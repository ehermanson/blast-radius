# Multi-Language Support Plan

The goal is to keep the analyzer core language-neutral and add languages as
small adapters. Optional languages are currently selected at build time with
Cargo features, not runtime CLI flags.

Current status:

- JS/TS is enabled by default.
- Vue exists behind `--features vue`.
- Svelte exists behind `--features svelte`.
- Python exists behind `--features python`.
- Rust exists behind `--features rust`.
- Ruby exists behind `--features ruby`.
- Java exists behind `--features java`.
- There is no `--language` / `--languages` CLI flag yet; a binary scans the
  languages compiled into it.

The non-JS implementations are intentionally conservative and
over-approximate import usage to avoid false negatives.

## Target Architecture

```text
repo discovery
  -> language adapter selects supported files
  -> adapter parses files into common facts
  -> language resolver maps imports to files
  -> shared analyzer walks the graph
  -> shared report renders output
```

The analyzer should only depend on common facts:

- file path
- imports
- exports or public symbols
- reexports
- local symbol usage when available
- language metadata for reporting/debugging

## Phase 1: Split JS/TS Into An Adapter

- Add a `language` module with a `LanguageAdapter` trait.
- Move current JS/TS parsing behind `JavaScriptAdapter`.
- Move JS/TS-specific file extension detection out of `fs.rs`.
- Keep current `ModuleFacts` shape, but make it language-neutral enough for
  Python.
- Make repo discovery ask adapters which files they support.
- Keep `analyze.rs` operating on normalized facts only.

Definition of done:

- Existing JS/TS behavior is unchanged.
- Chakra, Vite, and monorepo fixture tests still pass.
- No Python code is parsed yet.

## Phase 2: Add Python Parser

Use `rustpython-parser` first. It is Rust-native, avoids shelling out to Python,
and keeps the binary simple.

Parse these Python facts:

- `import package`
- `import package as alias`
- `from package import name`
- `from package import name as alias`
- `from . import name`
- `from .module import name`
- top-level `def`, `class`, and assignment names as public symbols
- `__all__` when it is a simple string list or tuple

Skip initially:

- dynamic `__import__`
- runtime `importlib`
- complex `__all__` expressions
- type-checker-only imports under `if TYPE_CHECKING`

Definition of done:

- Done: Python files are discovered only when Python support is enabled.
- Done: Python import/export facts feed the shared analyzer.
- Done: A small fixture proves relative imports, package imports, and `__init__.py`
  barrels.

## Phase 3: Add Python Resolver

Resolve imports using normal Python package rules:

- package directory with `__init__.py`
- module file like `foo.py`
- relative imports based on package path
- local project packages before external dependencies
- ignore standard library and third-party imports unless they resolve inside
  the repo

Definition of done:

- Done: `from app.service import thing` resolves to `app/service.py` or
  `app/service/__init__.py`.
- Done: `from .models import User` resolves relative to the current package.
- Done: unresolved metric does not count standard library imports.

## Phase 4: Python Example Repos

Add two Python examples:

- `examples/python-demo`
  A small hand-built fixture with packages, relative imports, `__init__.py`
  exports, and a few tests or CLI entrypoints.
- `examples/fastapi`
  A vendored snapshot of `https://github.com/fastapi/fastapi`.

Why FastAPI:

- real-world Python package
- large enough to exercise import resolution
- common app/package structure
- not as huge or framework-specialized as Django

Definition of done:

- Done: `examples/python-demo` is used in focused integration tests.
- Done: `examples/fastapi` has `UPSTREAM.md` with repository, commit, and license.
- `make metrics` includes one Python case once Python support is stable.

## Phase 5: CLI And Build Shape

Keep JS/TS as the default language support. Add Python behind a Cargo feature at
first:

```toml
[features]
default = ["javascript"]
javascript = []
python = ["dep:rustpython-parser"]
```

CLI behavior:

- default run supports compiled-in adapters
- `--languages js,ts,python` can narrow discovery later
- reports should show `source_file_count` by language eventually

Definition of done:

- Done: default builds stay lean.
- Done: `cargo build --features python` enables Python.
- Done: CI has at least one Python feature build/test job.

## Phase 6: Accuracy Metrics

Track Python separately from JS/TS:

- parse failures
- unresolved imports
- ambiguous reexports
- total affected files for known Python targets
- runtime on `python-demo` and `fastapi`

Definition of done:

- `make metrics` reports per-case language.
- Python unresolved imports have a ceiling in tests once the resolver is stable.
- regressions in JS/TS metrics do not hide Python regressions, and vice versa.

## Phase 7: Add Rust Parser And Resolver

Rust support uses `syn` behind the optional `rust` Cargo feature. It currently
tracks:

- `.rs` files only when the feature is enabled
- top-level public functions, structs, enums, traits, type aliases, consts, and
  statics
- `mod foo;` declarations
- `use crate::module::Symbol`
- `use self::module::Symbol`
- `use super::module::Symbol`
- grouped use trees like `use crate::module::{A, B}`
- `pub use` reexports

Skipped initially:

- macro-expanded modules/imports
- Cargo workspace package resolution
- `#[path = "..."]` module attributes
- precise Rust expression/body usage

Definition of done:

- Done: Rust files are discovered only when Rust support is enabled.
- Done: `syn` is behind `--features rust`.
- Done: focused Rust fixture proves `mod`, `crate::`, `self::`, `super::`, and
  `pub use` reexports.
- Done: CI has a Rust feature build/test job.

## Phase 8: Add Vue And Svelte Components

Vue and Svelte support use the existing SWC JavaScript/TypeScript parser. The
adapter extracts `<script>` blocks from `.vue` and `.svelte` files, parses those
blocks, and adds a synthetic default export for the component file.

Tracked:

- `.vue` files when `vue` is enabled
- `.svelte` files when `svelte` is enabled
- `<script>` and `<script setup>` imports
- `lang="ts"` TypeScript script blocks
- default component imports from JS/TS/Vue/Svelte files

Skipped initially:

- template-level dependency extraction
- style blocks
- Svelte/Vue compiler semantics
- generated code from preprocessors

Definition of done:

- Done: Vue/Svelte files are discovered only when their features are enabled.
- Done: component scripts feed the shared JS/TS parser.
- Done: component files expose a default export.
- Done: focused mixed fixture proves Vue -> Svelte -> TS transitive impact.
- Done: CI has a Vue/Svelte feature build/test job.

## Phase 9: Add Ruby Parser And Resolver

Ruby support uses a lightweight static parser behind the optional `ruby` Cargo
feature.

Tracked:

- `.rb` files when `ruby` is enabled
- `require_relative "path"`
- `require "path"` when it resolves inside the repo
- top-level `class`, `module`, and `def` names

Skipped initially:

- Rails autoloading and Zeitwerk conventions
- metaprogramming
- dynamic `require`
- precise method/body usage

Definition of done:

- Done: Ruby files are discovered only when Ruby support is enabled.
- Done: `require_relative` resolves against the importing file.
- Done: focused Ruby fixture proves transitive impact through service/model
  files.
- Done: CI has a Ruby feature build/test job.

## Phase 10: Add Java Parser And Resolver

Java support uses a lightweight static parser behind the optional `java` Cargo
feature.

Tracked:

- `.java` files when `java` is enabled
- `package` declarations for context
- `import package.Type`
- `import static package.Type.member`
- top-level `class`, `interface`, `enum`, and `record` names

Skipped initially:

- Maven/Gradle source-set metadata
- wildcard import precision beyond package-level resolution
- annotation processors/generated sources
- precise method/body usage

Definition of done:

- Done: Java files are discovered only when Java support is enabled.
- Done: package-style imports resolve to matching source files.
- Done: focused Java fixture proves transitive impact through service/model
  files.
- Done: CI has a Java feature build/test job.

## Suggested First Implementation Tasks

1. Introduce `LanguageAdapter` and move JS/TS parser selection behind it.
2. Add `language::javascript` adapter with current behavior.
3. Add `examples/python-demo` with a small package graph.
4. Add `rustpython-parser` behind a `python` Cargo feature.
5. Parse Python imports and top-level public symbols into common facts.
6. Resolve Python relative and package imports inside the repo.
7. Add Python integration tests for `file` mode.
8. Vendor FastAPI under `examples/fastapi`.
9. Add FastAPI smoke metrics.
10. Add CI job for `cargo test --features python`.
11. Add `syn` behind a `rust` Cargo feature.
12. Add `examples/rust-demo`.
13. Parse Rust `use`, `mod`, public symbols, and `pub use` reexports.
14. Resolve Rust `crate`, `self`, `super`, and sibling module paths.
15. Add CI job for `cargo test --features rust`.
16. Add `vue` and `svelte` feature flags.
17. Extract component script blocks and parse them through SWC.
18. Add component default exports and mixed component fixtures.
19. Add CI job for `cargo test --features vue,svelte`.
20. Add `ruby` and `java` feature flags.
21. Parse Ruby `require` / `require_relative` and public-ish symbols.
22. Parse Java imports and top-level type declarations.
23. Add Ruby and Java fixtures, examples, stress commands, and CI jobs.
