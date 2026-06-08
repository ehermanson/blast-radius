<p align="center">
  <img src="assets/blast-radius-aggressive-transparent.png" alt="blast-radius logo" width="375">
</p>

# blast-radius

**When you change a file, find out what else might break.**

`blast-radius` is a fast CLI that traces every file that depends — directly or
transitively — on the code you're about to touch, and gives you a one-glance
risk verdict. Point it at a file and it answers the question every code review
asks: _"how far does this change reach?"_

```
   MODERATE   ██████████░░░░░░░░░░  6 impacted files · 2 packages
  3 direct, 3 indirect · depth 2 · 1 endpoint

  ── IMPACTED FILES · 6 IN 2 PACKAGES ──────────────────────
  apps/storefront (3)
    apps/storefront/src/App.tsx  ◎ endpoint
    apps/storefront/src/LegacyButtonCard.jsx
    apps/storefront/src/PromoCard.tsx
  packages/ui (3)
    packages/ui/src/Card.tsx
    packages/ui/src/Toolbar.tsx
    packages/ui/src/index.ts
```

Use it to:

- **Gut-check a change** before you start — is this a 2-file tweak or a 200-file ripple?
- **Catch surprises in code review** — surface the files a diff touches that aren't in the diff.
- **Gate risky commits in CI or pre-commit hooks** — fail the build when a change reaches too far.

It works out of the box for JavaScript and TypeScript repos (including monorepos),
and supports Python, Rust, Ruby, Java, Vue, and Svelte as optional add-ons.

## Quick start

Install the binary (requires a Rust toolchain with `cargo`):

```bash
# Straight from GitHub
cargo install --git https://github.com/ehermanson/blast-radius

# Or from a local clone
cargo install --path .
```

Then point it at any file in your repo:

```bash
# What depends on this component?
blast-radius file src/components/Button.tsx

# What depends on a specific export?
blast-radius export src/components/Button.tsx Button

# Check several files at once (e.g. everything in a commit)
blast-radius files src/components/Button.tsx src/components/Card.tsx
```

By default it analyzes the current directory. Use `--repo-root` to point
elsewhere:

```bash
blast-radius --repo-root ../my-app file src/App.tsx
```

## Use it in pre-commit hooks and CI

The most common setup is to run `blast-radius` on changed files so you (and
your reviewers) see the reach of a commit before it lands.

`files` takes a list of paths and reports each file's blast radius plus a
combined total — designed to receive staged filenames from hook managers like
`lint-staged`, Husky, Lefthook, and `pre-commit`. For example, with
`lint-staged`:

```json
{
  "lint-staged": {
    "*.{js,jsx,ts,tsx}": "bash -c 'blast-radius --repo-root . files \"$@\" || true' --"
  }
}
```

To turn the verdict into a gate, exit non-zero when a change reaches too far:

```bash
# Fail (exit code 2) if a change touches more than 50 files
blast-radius --fail-threshold 50 files "$@"

# Or fail when the risk verdict hits "risky" or above
blast-radius --fail-on-risk risky files "$@"
```

See `docs/local-toolchain.md` for ready-to-paste examples with `lint-staged`,
Lefthook, and the `pre-commit` framework.

## Reading the output

The default `tree` output leads with a **risk verdict** — `minor`, `moderate`,
`risky`, or `high` — plus a meter and the counts behind it, then lists the
impacted files grouped by package. Files marked `◎ endpoint` are entry points
(apps, routes, pages) — a signal the change can reach something user-facing.

The last line reports **confidence**: how many files were scanned and whether
any import edges were ambiguous, so you know how much to trust the result.

Pass `--verbose` (`-v`) to see the full root → cascade tree of exactly how the
impact propagates.

## Commands

| Command                | What it does                                        |
| ---------------------- | --------------------------------------------------- |
| `file <path>`          | Everything that depends on this file.               |
| `export <path> <name>` | Everything that depends on a specific named export. |
| `files <path>...`      | Blast radius for each file plus a combined total.   |

Global flags:

| Flag                                  | Purpose                                             |
| ------------------------------------- | --------------------------------------------------- |
| `--repo-root <dir>`                   | Repo to analyze (default: current directory).       |
| `--format <tree\|json\|mermaid\|dot>` | Output format (default: `tree`).                    |
| `--output <file>`                     | Write output to a file instead of stdout.           |
| `--verbose`, `-v`                     | Show the full cascade tree.                         |
| `--fail-threshold <n>`                | Exit code 2 when more than `n` files are affected.  |
| `--fail-on-risk <tier>`               | Exit code 2 when the verdict is at or above `tier`. |

### Output formats

- `tree` — the default human-readable verdict, meter, and impacted-file list.
- `json` — structured output; the per-input-file breakdown lives in the `roots` array.
- `mermaid` — a Mermaid graph definition.
- `dot` — Graphviz DOT.

## Language support

The default binary supports **JavaScript and TypeScript** (`js`, `jsx`, `ts`,
`tsx`), including ESM imports/exports, CommonJS `require`/`module.exports`,
default and named exports, barrels, `export *`, `tsconfig.json` path aliases,
and cross-package resolution across workspace packages.

Other languages are compiled in at **build time** with Cargo features (there is
no runtime `--language` flag — a binary scans whatever was built into it):

```bash
cargo install --path .                              # JS/TS only (default)
cargo install --path . --features python            # + Python
cargo install --path . --features rust              # + Rust
cargo install --path . --features ruby              # + Ruby
cargo install --path . --features java              # + Java
cargo install --path . --features vue,svelte        # + Vue + Svelte
cargo install --path . --features python,rust,vue,svelte,ruby,java   # everything
```

See `docs/language-support.md` for the multi-language architecture.

## Configuration

An optional `.blast-radius.json` at the repo root lets a repository declare
tooling quirks the analyzer shouldn't hardcode. Today it supports ignoring
import specifiers that point at generated/virtual modules (CSS-in-JS codegen,
route type stubs, published `dist` output, etc.) so they don't count against the
unresolved-import confidence signal:

```jsonc
{
  // comments and trailing commas are allowed (parsed as JSONC, like tsconfig)
  "unresolved": {
    "ignore": ["styled-system/css", ".velite", "/+types/"],
  },
}
```

Each entry is matched as a substring of the import specifier. Asset imports
(`.svg`, `.css`, `.json`, images, …) and type-only imports are ignored
automatically. See `examples/chakra-ui/.blast-radius.json`.

## Examples

The `examples/` directory has runnable fixtures for each supported language:

| Fixture                     | Exercises                                                    |
| --------------------------- | ------------------------------------------------------------ |
| `monorepo-demo`             | Aliases, barrels, CommonJS, transitive React usage           |
| `vite-react-ts`             | A real Vite React + TypeScript template                      |
| `chakra-ui` †               | Chakra UI snapshot for large-repo stress testing             |
| `python-demo` / `fastapi` † | Python package, relative, and `__init__.py` reexport imports |
| `rust-demo`                 | `mod`, `pub use`, `crate::` / `self::` imports               |
| `component-demo`            | Mixed Vue/Svelte component imports                           |
| `ruby-demo`                 | `require_relative`, classes, modules, methods                |
| `java-demo`                 | Packages, imports, public classes                            |

† `chakra-ui` and `fastapi` are large real-world snapshots that aren't committed
to the repo. Fetch them on demand (pinned to a known upstream commit) before
running their examples:

```bash
scripts/fetch-examples.sh
```

Run against any of them with `--repo-root`:

```bash
# JS/TS monorepo fixture
cargo run --bin blast-radius -- --repo-root examples/monorepo-demo file apps/storefront/src/App.tsx

# Large React monorepo, with the full cascade tree
cargo run --bin blast-radius -- --repo-root examples/chakra-ui -v file packages/react/src/components/button/button.tsx

# Python (needs the feature compiled in)
cargo run --features python --bin blast-radius -- --repo-root examples/fastapi file fastapi/applications.py

# Rust
cargo run --features rust --bin blast-radius -- --repo-root examples/rust-demo file src/utils/formatting.rs

# Vue/Svelte
cargo run --features vue,svelte --bin blast-radius -- --repo-root examples/component-demo file src/shared.ts
```

## Development

This project expects a Rust toolchain with `cargo` available locally. Common
quality commands:

```bash
make test                 # core JS/TS test suite
make test-all-languages   # every optional adapter
make coverage             # coverage report
make quality              # full quality gate
```

The `Makefile` has the full set, including per-language test/quality/stress
targets (`make test-python`, `make stress-chakra`, etc.). See `docs/quality.md`
for what each command validates and `docs/language-support.md` for the
multi-language architecture.
