# Multi-Language Support Plan

The goal is to keep the analyzer core language-neutral and add languages as
small adapters. Python is the first non-JS language because its import model is
simpler than Rust, Go, or Java, and there are mature parser options.

Current status: Python support exists behind `--features python`. The first
implementation is intentionally conservative: it resolves normal package and
relative imports, parses top-level public symbols, and over-approximates Python
import usage to avoid false negatives.

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
