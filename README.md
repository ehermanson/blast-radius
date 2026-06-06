# blast-radius

`blast-radius` is a Rust CLI for estimating the transitive impact of frontend code changes across a repository.

## Features

- AST-based parsing for `ts`, `tsx`, `js`, and `jsx`
- ESM imports/exports and CommonJS `require` / `module.exports`
- Default exports, named exports, barrels, and `export *`
- `tsconfig.json` path aliases
- Cross-package resolution for in-repo workspace packages
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

Example run:

```bash
cargo run -- --repo-root examples/monorepo-demo export packages/ui/src/Button.tsx Button
```

## Development

This project expects a Rust toolchain with `cargo` available locally.
