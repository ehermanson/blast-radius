# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.7.2] - 2026-06-13

### Added

- `jsconfig.json` path aliases now resolve (it's `tsconfig.json` for JS
  projects — same `paths`/`baseUrl` format), so JS repos' `@/…` imports connect
  instead of being missed.

### Fixed

- Alias-looking imports (`@/…`, `~…`) that resolve nowhere visible — e.g. an
  alias defined only in a bundler config — are now reported as unresolved (with
  a hint to configure the path) instead of being silently treated as external
  packages. Silent under-counting was the most dangerous failure mode; now it
  surfaces in the confidence read.

## [0.7.1] - 2026-06-13

### Changed

- Source files are now parsed in parallel (rayon), roughly halving cold-cache
  analysis of large repos (the 2,700-file Chakra UI snapshot drops from ~1.1s to
  ~0.55s). Output is unchanged and deterministic regardless of thread count.

## [0.7.0] - 2026-06-13

### Added

- GitHub Action (`action.yml`): post a sticky pull-request comment with a
  change's blast radius — per changed file, the files it reaches, with the risk
  verdict and a linked confidence read — and optionally fail the check via
  `fail-on-risk`. It computes changed files with git and pipes them to the
  binary, so the analyzer stays pure. The comment renderer is decoupled and
  unit-tested (`node --test`); see `docs/github-action.md`.

## [0.6.0] - 2026-06-13

### Changed

- An imported-but-unused symbol now counts as a dependency: a file that imports
  `X` from a module is in that module's blast radius even if it never references
  `X`, because changing or removing `X` would break the (possibly unused)
  import. Symbol precision is unchanged — importing a *different* symbol still
  does not match (a `Card` change never reaches a file that imports only
  `Button` through the same barrel). This makes blast radii slightly more
  inclusive; a future flag may re-enable usage-based pruning.

### Added

- `graph` command: dump the whole-repo import graph (every source file and
  resolved import/re-export edge) in a single pass. `--format json` for tooling,
  `mermaid`/`dot` for diagrams, or the default plain `importer -> importee`
  listing. Useful for visualization and feeding other tools.
- Accuracy oracle (`scripts/accuracy/`): a differential test of blast-radius's
  import graph against dependency-cruiser on real fixtures, wired into CI as a
  gate (the `accuracy-oracle` job) that fails on any edge the reference resolves but
  blast-radius misses. dependency-cruiser is pinned exactly (`16.10.4`) so the
  gate is deterministic. Internal quality tooling, not part of the shipped
  binary. See the script's README.
- Accuracy corpus now spans both repo shapes: the existing Chakra UI snapshot
  (library) plus an Excalidraw snapshot (a real React **application** —
  route/lazy code-splitting, dynamic imports). Baseline: zero missed edges on
  both Chakra UI (2697 files) and Excalidraw.
- `vi.mock("...")` / `jest.mock("...")` (and `doMock`) references now create
  dependency edges from the test to the real module — a change to it can break
  the mock — labeled with a distinct `mocks_module` edge kind so they're not
  confused with real imports. This was the only gap the accuracy oracle found on
  Excalidraw.

## [0.5.0] - 2026-06-13

### Added

- `tsconfig.json` project `references` are followed, so path aliases declared
  in referenced configs with non-standard names (`tsconfig.lib.json`) resolve.
  Directory references, chained references, and cycles are handled.
- The string form of package.json's `browser` field is honored as an entry
  point (after `source` and `module`), so older browser-first packages
  resolve; the object (path-remapping) form is ignored safely.

### Fixed

- `export * as ns from './x'` is now member-precise: changing one export of
  `x` impacts only consumers that touch that member through the namespace
  object (`ns.Button` in code or JSX, including through aliased re-exports
  like `export { ns as kit }`), instead of every consumer of the namespace.
  Wholesale uses of the object still count as depending on every member, and
  nested namespace-of-namespace chains over-approximate rather than miss.
- Member usage is now tracked for named and default imports (not just
  `import * as` namespaces), enabling the precision above.

## [0.4.0] - 2026-06-12

### Added

- `files -` reads the file list from stdin, so changed files pipe straight in:
  `git diff --name-only | blast-radius files -`. Blank lines and surrounding
  whitespace are ignored; an empty list is an analysis error (exit 1).
- `--color <auto|always|never>` — `auto` keeps the existing behavior (color
  only on a terminal, `NO_COLOR` respected); `always` forces ANSI even when
  piped or written via `--output`.
- `--quiet` / `-q` — suppress stdout; exit codes and `--output` files still
  apply, for gate-only CI usage.
- `completions <shell>` subcommand (bash, zsh, fish, elvish, powershell).
- `--version` now lists the language adapters compiled into the binary;
  `-V` stays terse.
- JSON output carries a top-level `schema_version` field (currently `1`),
  bumped only on breaking shape changes; the full field-by-field contract is
  now documented in `docs/json-output.md`.

## [0.3.0] - 2026-06-12

### Removed

- **Ruby and Java language support.** Both adapters were line-based heuristic
  parsers that could not be trusted on real codebases: the Ruby adapter only
  followed explicit `require`/`require_relative`, so Rails/Zeitwerk-autoloaded
  apps produced near-empty graphs that read as "this change is safe"; the Java
  adapter had no Maven/Gradle multi-module awareness and resolved wildcard
  imports by capitalization heuristics. For an impact-analysis tool, a
  confidently wrong answer is worse than none. They may return as real-parser
  adapters once they can meet the accuracy bar; 0.2.1 is the last release that
  includes them.

### Changed

- Repositioned language support: JavaScript/TypeScript (with Vue/Svelte) is
  the primary target; the Python and Rust adapters are documented as beta with
  their known blind spots listed in `docs/language-support.md`.

## [0.2.1] - 2026-06-12

### Fixed

- Verbose cascade tree (`--verbose`): export-mode roots no longer print
  "No downstream dependents found" while the summary reports impacted files;
  chains that pass through named re-exports or CommonJS re-exports no longer
  dead-end; and barrels render as real nodes instead of being skipped — barrel
  consumers were previously attributed to every feeder file, fabricating
  dependency paths that do not exist. Subtrees reachable along several paths
  are printed once and back-referenced with "(paths shown above)".
- Mermaid/DOT output: distinct files whose names differ only in punctuation
  (e.g. `util-x.ts` vs `util.x.ts`) no longer merge into a single graph node;
  sanitized node ids carry a stable fingerprint of the original id.
- Workspace cross-crate Rust resolution survives the `toml` 1.x upgrade:
  manifests are parsed as TOML documents (`toml::Table`), where `toml::Value`
  parsing now silently fails.

## [0.2.0] - 2026-06-09

### Added

- npm distribution: `blast-radius-cli` wrapper package (`npx blast-radius`)
  with esbuild-style per-platform binary packages — no Rust toolchain needed.
- Prebuilt binaries for Linux (x64/arm64 glibc, x64 musl), macOS (x64/arm64),
  and Windows (x64) attached to GitHub Releases with a `sha256sums.txt`, built
  with all language features; tag-driven release workflow.

### Fixed

- Windows: report labels and package/directory grouping now normalize `\` path
  separators to `/`, so files no longer collapse into a single `.` package.

- Module resolution bugs: extension probe order (multi-dot specifiers like
  `./recipe.types` resolve correctly; asset imports such as `./theme.css` no
  longer falsely resolve to same-stem `.ts` files), `tsconfig` `extends`
  chains and alias-bearing sibling configs (`tsconfig.base.json`,
  `tsconfig.app.json`), and `.d.ts` declaration-file handling.
- Star re-export barrels no longer produce a false-safe verdict: file-level
  analysis of a module without statically-enumerable exports now matches all
  of its consumers.
- `total_affected_files` no longer counts the input file(s); it equals
  `directly_affected_files + transitively_affected_files`, and
  `--fail-threshold` gates on downstream impact only.
- `export` mode rejects unknown export names with an error (exit 1) when the
  file's exports are statically enumerable, instead of silently reporting a
  phantom minor result; it warns and proceeds when they are not.
- `files` mode deduplicates repeated input paths.
- One unreadable directory no longer aborts analysis; it is skipped with a
  warning.
- Panic on overlapping wildcard path aliases.
- Side-effect imports (`import "./x"`, bare `require("./x")`) now create
  dependency edges, and `require()` calls are collected anywhere in a module
  (including inside functions), not just at top level. TS
  `import x = require(...)` and `export =` are now modeled.
- Language adapter fixes: Python submodule (`from pkg import submodule`) and
  conditional/nested imports, Rust cross-crate `use` in Cargo workspaces,
  Java wildcard (`import pkg.*`) usage-driven fan-out.

### Changed

- Usage errors (unknown flag, missing argument, bare invocation) exit `64`
  instead of `2`, leaving `2` exclusively for tripped gates; `--help` and
  `--version` still exit `0`.
- `--repo-root`, `--format`, `--output`, `--fail-threshold`, and
  `--fail-on-risk` are global flags and may be passed after the subcommand.
- `--output` files are always written without ANSI escape codes.
- Documented the Ruby adapter's require-only limitation (Rails/Zeitwerk
  autoloading is not modeled).
- CI hardened: lint (fmt + clippy), MSRV, Windows, and combined all-features
  jobs; duplicate push/PR runs eliminated.
- MSRV raised to 1.88 (transitive dependencies require it — `icu`/`idna` need
  1.86 and `ar_archive_writer` uses 1.88 let-chains; the previously declared
  1.85 never actually compiled).
- JSON node ids and edge `from`/`to` paths use `/` separators on all
  platforms.

## [0.1.1] - 2026-06-09

### Changed

- Improved TypeScript module resolution.
- Release hygiene: tightened packaging metadata and repository cleanup.

## [0.1.0] - 2026-06-05

### Added

- Initial release: reverse-dependency graph with transitive blast radius
  reporting, risk verdict and tiered exit gate, per-package impact, and
  multi-file breakdown.
