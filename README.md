# blast-radius

`blast-radius` is a Rust CLI for estimating the transitive impact of frontend code changes across a repository.

## Features

- AST-based parsing for `ts`, `tsx`, `js`, and `jsx`
- ESM imports/exports and CommonJS `require` / `module.exports`
- Default exports, named exports, barrels, and `export *`
- `tsconfig.json` path aliases
- Cross-package resolution for in-repo workspace packages
- Optional Python support behind `--features python`
- Optional Rust support behind `--features rust`
- Terminal tree, JSON, Mermaid, and Graphviz DOT output
- At-a-glance risk verdict (minor / moderate / risky / high) with impacted files listed per package
- Multi-file `diff` runs (e.g. a pre-commit/pre-push hook over staged files) show a combined verdict plus a per-file breakdown
- `export`, `file`, and `diff` modes

## Commands

```bash
blast-radius export packages/ui/src/Button.tsx Button
blast-radius file packages/ui/src/Button.tsx
blast-radius files packages/ui/src/Button.tsx packages/ui/src/Card.tsx
blast-radius diff origin/main...HEAD
```

`files` takes a list of paths and reports each file's blast radius plus a combined
total — handy in a pre-commit hook, where lint-staged passes staged files as args.

## Language Support

Language support is selected at build time with Cargo features, not runtime CLI
flags. The default binary supports JS/TS only.

```bash
# JS/TS only
cargo build

# JS/TS + Python
cargo build --features python

# JS/TS + Rust
cargo build --features rust

# JS/TS + Python + Rust
cargo build --features python,rust
```

There is no `--language` or `--languages` CLI flag yet. A binary scans whatever
file types were compiled into it.

## Output Formats

- `tree` — leads with a risk verdict and meter, then (for multi-file diffs) a per-changed-file breakdown, then the impacted files listed in full and grouped by package, with endpoints flagged. Pass `--verbose` (`-v`) for the full root → cascade tree. The per-file breakdown is also available in `json` as the `roots` array.
- `json`
- `mermaid`
- `dot`

## Examples

- `examples/monorepo-demo`
  A purpose-built workspace fixture that exercises aliases, barrels, CommonJS, and transitive React component usage.
- `examples/vite-react-ts`
  A real React + TypeScript template copied from Vite.
- `examples/chakra-ui`
  A vendored snapshot of the Chakra UI monorepo for large-repo stress testing.
- `examples/python-demo`
  A small Python package that exercises package imports, relative imports, and
  `__init__.py` reexports.
- `examples/fastapi`
  A vendored snapshot of FastAPI for large Python repo stress testing.
- `examples/rust-demo`
  A small Rust crate that exercises `mod`, `pub use`, and `crate::` / `self::`
  imports.

Example run:

```bash
cargo run --bin blast-radius -- --repo-root examples/monorepo-demo export packages/ui/src/Button.tsx Button
```

More example runs:

```bash
# Analyze a single file in the small monorepo fixture
cargo run --bin blast-radius -- --repo-root examples/monorepo-demo file apps/storefront/src/App.tsx

# Analyze a symbol export in the small monorepo fixture
cargo run --bin blast-radius -- --repo-root examples/monorepo-demo export packages/ui/src/Button.tsx Button

# Analyze a real Vite React app file
cargo run --bin blast-radius -- --repo-root examples/vite-react-ts file src/App.tsx

# Stress test against a larger React monorepo
cargo run --bin blast-radius -- --repo-root examples/chakra-ui file packages/react/src/components/button/button.tsx

# Show the full cascade tree for the same Chakra UI file
cargo run --bin blast-radius -- --repo-root examples/chakra-ui --verbose file packages/react/src/components/button/button.tsx

# Analyze a Python package fixture
cargo run --features python --bin blast-radius -- --repo-root examples/python-demo file app/utils/formatting.py

# Stress test against a larger Python repo
cargo run --features python --bin blast-radius -- --repo-root examples/fastapi file fastapi/applications.py

# Analyze a Rust crate fixture
cargo run --features rust --bin blast-radius -- --repo-root examples/rust-demo file src/utils/formatting.rs
```

## Development

This project expects a Rust toolchain with `cargo` available locally.

Useful local quality commands:

```bash
make test
make test-python
make test-rust
make test-all-languages
make coverage
make coverage-gate
make stress-chakra
make stress-python-demo
make stress-fastapi
make stress-rust-demo
make smoke-mui
make perf
make metrics
make quality
make quality-python
make quality-rust
```

See `docs/quality.md` for what each command validates.

See `docs/language-support.md` for the multi-language architecture and next
language-adapter work.
