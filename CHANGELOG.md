# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

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
