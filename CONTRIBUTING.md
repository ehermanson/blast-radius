# Contributing

This project expects a Rust toolchain with `cargo` available locally.

## Development

Common quality commands:

```bash
make test                 # core JS/TS test suite
make test-all-languages   # every optional adapter
make coverage             # coverage report
make quality              # full quality gate
```

The `Makefile` has the full set, including per-language test/quality/stress
targets (`make test-python`, `make stress-chakra`, etc.). See
[`docs/quality.md`](docs/quality.md) for what each command validates,
[`docs/ci.md`](docs/ci.md) for what each CI job protects, and
[`docs/language-support.md`](docs/language-support.md) for the multi-language
architecture.

## Building with optional language support

Other languages are compiled in at **build time** with Cargo features (there is
no runtime `--language` flag — a binary scans whatever was built into it). The
prebuilt binaries ship with everything compiled in; this only matters when
building from source:

```bash
cargo install --path .                              # JS/TS only (default)
cargo install --path . --features vue,svelte        # + Vue + Svelte
cargo install --path . --features python            # + Python (beta)
cargo install --path . --features rust              # + Rust (beta)
cargo install --path . --features python,rust,vue,svelte   # everything
```

## Examples

The `examples/` directory has runnable fixtures for each supported language:

| Fixture                     | Exercises                                                    |
| --------------------------- | ------------------------------------------------------------ |
| `monorepo-demo`             | Aliases, barrels, CommonJS, transitive React usage           |
| `vite-react-ts`             | A real Vite React + TypeScript template                      |
| `chakra-ui` †               | Chakra UI snapshot — large library-shaped React monorepo     |
| `excalidraw` †              | Excalidraw snapshot — large real-world React **application**  |
| `python-demo` / `fastapi` † | Python package, relative, and `__init__.py` reexport imports |
| `rust-demo`                 | `mod`, `pub use`, `crate::` / `self::` imports               |
| `component-demo`            | Mixed Vue/Svelte component imports                           |

† `chakra-ui`, `excalidraw`, and `fastapi` are large real-world snapshots that
aren't committed to the repo. Fetch them on demand (pinned to a known upstream
commit) before running their examples:

```bash
scripts/fetch-examples.sh
```

Run against any of them with `--repo-root`:

```bash
# JS/TS monorepo fixture
cargo run --bin blast-radius -- --repo-root examples/monorepo-demo file apps/storefront/src/App.tsx

# Large React monorepo, with the full cascade tree
cargo run --bin blast-radius -- --repo-root examples/chakra-ui -v file packages/react/src/components/button/button.tsx

# Large real-world React app (Excalidraw)
cargo run --bin blast-radius -- --repo-root examples/excalidraw file packages/element/src/index.ts

# Python (needs the feature compiled in)
cargo run --features python --bin blast-radius -- --repo-root examples/fastapi file fastapi/applications.py

# Rust
cargo run --features rust --bin blast-radius -- --repo-root examples/rust-demo file src/utils/formatting.rs

# Vue/Svelte
cargo run --features vue,svelte --bin blast-radius -- --repo-root examples/component-demo file src/shared.ts
```
