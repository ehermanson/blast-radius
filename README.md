<p align="center">
  <img src="assets/blast-radius-aggressive-transparent.png" alt="blast-radius logo" width="375">
</p>

<p align="center">
  <a href="https://www.npmjs.com/package/blast-radius-cli"><img src="https://img.shields.io/npm/v/blast-radius-cli?logo=npm&color=cb3837" alt="npm version"></a>
  <a href="https://crates.io/crates/blast-radius"><img src="https://img.shields.io/crates/v/blast-radius?logo=rust&color=e43717" alt="crates.io version"></a>
  <a href="https://github.com/ehermanson/blast-radius/actions/workflows/quality.yml"><img src="https://img.shields.io/github/actions/workflow/status/ehermanson/blast-radius/quality.yml?branch=main&logo=github&label=CI" alt="CI status"></a>
  <a href="https://github.com/ehermanson/blast-radius/blob/main/LICENSE"><img src="https://img.shields.io/badge/license-MIT-blue" alt="MIT license"></a>
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
    src/ (3)
      App.tsx  ◎ endpoint
      LegacyButtonCard.jsx
      PromoCard.tsx
  packages/ui (3)
    src/ (3)
      Card.tsx
      Toolbar.tsx
      index.ts
```

Use it to:

- **Gut-check a change** before you start — is this a 2-file tweak or a 200-file ripple?
- **Catch surprises in code review** — surface the files a diff touches that aren't in the diff.
- **Gate risky commits in CI or pre-commit hooks** — fail the build when a change reaches too far.

Run it as a **[GitHub Action](#github-action)** that comments the blast radius on
PRs, a **[local pre-commit hook](#use-it-in-pre-commit-hooks-and-ci)** (lint-staged,
Husky, Lefthook, pre-commit), a **[CI gate](#use-it-in-pre-commit-hooks-and-ci)**
that fails on too-far-reaching changes, or **[ad-hoc on any file](#quick-start)**
from the command line.

It is built first and foremost for JavaScript and TypeScript repos (including
monorepos) — and that includes React: JSX/TSX is parsed natively, and JSX
component usage is tracked at the symbol level, so it can tell a file that
merely imports `Button` from one that actually renders `<Button />`. Vue and
Svelte script imports are supported too, with Python and Rust as beta adapters.
See [Language support](#language-support).

## Quick start

Install via npm — no Rust toolchain required. The package pulls in a prebuilt
binary for your platform (Linux x64/arm64, macOS x64/arm64, Windows x64) with
**all language features included**:

```bash
npm install --save-dev blast-radius-cli

npx blast-radius --help
```

The same prebuilt binaries are attached to each
[GitHub Release](https://github.com/ehermanson/blast-radius/releases). You can
also `cargo install blast-radius` (JS/TS only by default — see
[Contributing](CONTRIBUTING.md#building-with-optional-language-support) for
language features).

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
combined total. Pass `-` to read the list from stdin — the natural fit for
piping from git:

```bash
# What does my working-tree change reach?
git diff --name-only | blast-radius files -

# Gate a PR on everything it touches
git diff --name-only origin/main...HEAD | blast-radius --fail-on-risk risky files -
```

It also receives staged filenames as arguments from hook managers like
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
# Fail (exit code 2) if a change impacts more than 50 downstream files
blast-radius --fail-threshold 50 files "$@"

# Or fail when the risk verdict hits "risky" or above
blast-radius --fail-on-risk risky files "$@"
```

Exit codes: `0` no gate tripped, `1` analysis error, `2` gate tripped, `64`
usage error (so CI can tell a misspelled flag apart from a tripped gate).

See [`docs/local-toolchain.md`](docs/local-toolchain.md) for ready-to-paste
examples with `lint-staged`, Husky, Lefthook, and the `pre-commit` framework.

### GitHub Action

To comment a change's blast radius directly on pull requests (and optionally
gate on risk), use the action:

```yaml
# .github/workflows/blast-radius.yml
name: blast-radius
on: pull_request
permissions:
  contents: read
  pull-requests: write
jobs:
  blast-radius:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
        with: { fetch-depth: 0 }
      - uses: ehermanson/blast-radius@v0.7.3
        with:
          fail-on-risk: high # optional
```

It posts one sticky comment and updates it in place. See
[`docs/github-action.md`](docs/github-action.md) for all inputs.

## Reading the output

The `tree` output leads with a **risk verdict** (`minor`, `moderate`, `risky`,
`high`) and a meter, then the impacted files grouped by package and directory.
Files marked `◎ endpoint` are leaves nothing else depends on (apps, routes,
pages) — a signal the change can reach something user-facing. The last line
reports **confidence** in the result.

Full details — the hotspots chart, rollups for large radii, and how to read the
confidence line — are in [`docs/output.md`](docs/output.md).

## Commands

| Command                | What it does                                        |
| ---------------------- | --------------------------------------------------- |
| `file <path>`          | Everything that depends on this file.               |
| `export <path> <name>` | Everything that depends on a specific named export. |
| `files <path>...`      | Blast radius for each file plus a combined total. `-` reads stdin. |
| `graph`                | Dump the whole-repo import graph.                   |
| `completions <shell>`  | Print a shell completion script.                    |

For all global flags, output formats (`json`/`mermaid`/`dot`), exit codes, and
completion setup, see the [CLI reference](docs/cli.md) — or run
`blast-radius --help`.

## Language support

`blast-radius` is built first and foremost for **JavaScript and TypeScript**
(`js`, `jsx`, `ts`, `tsx`), with React as the primary target: symbol-level JSX
usage tracking, `React.lazy()`/dynamic `import()`, ESM + CommonJS, barrels and
`export *`, `tsconfig`/`jsconfig` path aliases and project references, package
`imports`/`exports`, and cross-package resolution across workspaces.

**Vue and Svelte** track imports in `<script>` blocks. **Python and Rust** are
beta adapters built on real parsers. The prebuilt binaries ship with every
language compiled in; source builds are JS/TS-only by default. See
[`docs/language-support.md`](docs/language-support.md) for the full feature
matrix and each adapter's known blind spots.

## Configuration

An optional `.blast-radius.json` at the repo root lets a repo skip generated or
vendored directories and quiet known-unresolvable import specifiers (CSS-in-JS
codegen, route type stubs, etc.). See
[`docs/configuration.md`](docs/configuration.md).

## Documentation

- [CLI reference](docs/cli.md) — commands, flags, output formats, exit codes
- [Reading the output](docs/output.md) — verdict, hotspots, confidence
- [Local toolchain](docs/local-toolchain.md) — lint-staged, Husky, Lefthook, pre-commit
- [GitHub Action](docs/github-action.md) — inputs and behavior
- [JSON output](docs/json-output.md) — the structured-output contract
- [Configuration](docs/configuration.md) — `.blast-radius.json`
- [Language support](docs/language-support.md) — per-language coverage and limits
- [Contributing](CONTRIBUTING.md) — building from source, examples, dev workflow

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md) for building from source (including
optional language features), running the example fixtures, and the dev workflow.
