# Quality Checks

These commands are the current baseline for validating `blast-radius` against
both fixture repos and larger real-world React codebases.

For what runs in CI and what each job's failure means, see [`ci.md`](ci.md).

## Commands

```bash
make test
make test-python
make test-rust
make test-components
make test-all-languages
make coverage
make coverage-gate
make stress-chakra
make stress-python-demo
make stress-fastapi
make stress-rust-demo
make stress-components
make smoke-mui
make perf
make metrics
make quality
make quality-python
make quality-rust
make quality-components
```

## What They Mean

- `make test`
  Runs unit and integration tests, including the vendored Chakra UI regression.
- `make test-python`
  Runs tests with Python support compiled in via `--features python`.
- `make test-rust`
  Runs tests with Rust support compiled in via `--features rust`.
- `make test-components`
  Runs tests with Vue and Svelte support compiled in via `--features vue,svelte`.
- `make test-all-languages`
  Runs tests with every optional language adapter compiled in.
- `make coverage`
  Prints a line and region coverage summary using `cargo-llvm-cov`.
- `make coverage-gate`
  Enforces the current minimum coverage floor: `85%` lines, `83%` regions, and
  `84%` functions.
- `make stress-chakra`
  Runs a large-repo analysis against the vendored Chakra UI monorepo.
- `make stress-python-demo`
  Runs the small Python fixture through the Python feature build.
- `make stress-fastapi`
  Runs a large Python analysis against the vendored FastAPI snapshot.
- `make stress-rust-demo`
  Runs the small Rust fixture through the Rust feature build.
- `make stress-components`
  Runs the mixed Vue/Svelte fixture through the component feature build.
- `make smoke-mui`
  Clones Material UI into `target/tmp/mui-mini` if needed and runs a real-world
  smoke test. This currently succeeds with one skipped template parse failure
  reported as a warning instead of aborting the whole analysis.
- `make perf`
  Benchmarks runtime on the monorepo fixture, Vite example, and Chakra UI using
  `hyperfine`.
- `make metrics`
  Writes a machine-readable snapshot to `target/quality/metrics.json` for the
  monorepo fixture, Vite example, and Chakra UI example.
- `make quality`
  Runs the main local gate: formatting check, clippy lint (`-D warnings`),
  tests, enforced coverage floor, and the Chakra stress run.
- `make quality-python`
  Runs Python feature tests plus Python stress cases.
- `make quality-rust`
  Runs Rust feature tests plus the Rust fixture stress case.
- `make quality-components`
  Runs Vue/Svelte feature tests plus the component fixture stress case.

## Accuracy Metrics To Watch

- `parse_failures`
- `unresolved_imports`
- `ambiguous_edges`
- `skipped_inputs` (paths passed to `files` mode that were not analyzable)
- `total_affected_files` for known regression targets (counts downstream
  impacted files only — the changed file(s) themselves are excluded, so
  `total == direct + transitive`; baselines recorded before this change are
  higher by one per analysis root)
- runtime on `monorepo-demo`, `vite-react-ts`, and `chakra-ui`
- runtime on optional language fixtures when their features are enabled
- `target/quality/metrics.json` drift over time

## Accuracy oracle

Beyond the hand-built suite, the `accuracy` CI job differential-tests
blast-radius's import graph against dependency-cruiser (a mature independent
resolver) on real fixtures — the Chakra UI snapshot (a library) and the
Excalidraw snapshot (a real application) — and fails on any edge the reference
resolves that blast-radius misses. This is the corpus-based correctness check
the hand-built fixtures can't provide. See `scripts/accuracy/README.md`. Run it
locally with `node scripts/accuracy/oracle.mjs <fixture> --strict`.

## Current Baseline

- Coverage: `85.40%` lines, `83.59%` regions, `84.34%` functions
- Runtime: about `1.53s` mean for the Chakra UI stress case on this machine
