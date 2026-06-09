# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Fixed

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
- CI hardened: lint (fmt + clippy), MSRV, and combined all-features jobs;
  duplicate push/PR runs eliminated.

## [0.1.1] - 2026-06-09

### Changed

- Improved TypeScript module resolution.
- Release hygiene: tightened packaging metadata and repository cleanup.

## [0.1.0] - 2026-06-05

### Added

- Initial release: reverse-dependency graph with transitive blast radius
  reporting, risk verdict and tiered exit gate, per-package impact, and
  multi-file breakdown.
