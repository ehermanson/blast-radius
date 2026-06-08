# Quality Checks

These commands are the current baseline for validating `blast-radius` against
both fixture repos and larger real-world React codebases.

## Commands

```bash
make test
make test-python
make test-rust
make test-components
make test-ruby
make test-java
make test-all-languages
make coverage
make coverage-gate
make stress-chakra
make stress-python-demo
make stress-fastapi
make stress-rust-demo
make stress-components
make stress-ruby-demo
make stress-java-demo
make smoke-mui
make perf
make metrics
make quality
make quality-python
make quality-rust
make quality-components
make quality-ruby
make quality-java
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
- `make test-ruby`
  Runs tests with Ruby support compiled in via `--features ruby`.
- `make test-java`
  Runs tests with Java support compiled in via `--features java`.
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
- `make stress-ruby-demo`
  Runs the small Ruby fixture through the Ruby feature build.
- `make stress-java-demo`
  Runs the small Java fixture through the Java feature build.
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
  Runs the main local gate: tests, enforced coverage floor, and the Chakra
  stress run.
- `make quality-python`
  Runs Python feature tests plus Python stress cases.
- `make quality-rust`
  Runs Rust feature tests plus the Rust fixture stress case.
- `make quality-components`
  Runs Vue/Svelte feature tests plus the component fixture stress case.
- `make quality-ruby`
  Runs Ruby feature tests plus the Ruby fixture stress case.
- `make quality-java`
  Runs Java feature tests plus the Java fixture stress case.

## Accuracy Metrics To Watch

- `parse_failures`
- `unresolved_imports`
- `ambiguous_edges`
- `skipped_inputs` (paths passed to `files` mode that were not analyzable)
- `total_affected_files` for known regression targets
- runtime on `monorepo-demo`, `vite-react-ts`, and `chakra-ui`
- runtime on optional language fixtures when their features are enabled
- `target/quality/metrics.json` drift over time

The current suite improves crash-resilience and parser coverage, but it is not
yet a proof of semantic correctness. Additional corpus-based accuracy checks are
still needed.

## Current Baseline

- Coverage: `85.40%` lines, `83.59%` regions, `84.34%` functions
- Runtime: about `1.53s` mean for the Chakra UI stress case on this machine
