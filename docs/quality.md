# Quality Checks

These commands are the current baseline for validating `blast-radius` against
both fixture repos and larger real-world React codebases.

## Commands

```bash
make test
make coverage
make coverage-gate
make stress-chakra
make smoke-mui
make perf
make metrics
make quality
```

## What They Mean

- `make test`
  Runs unit and integration tests, including the vendored Chakra UI regression.
- `make coverage`
  Prints a line and region coverage summary using `cargo-llvm-cov`.
- `make coverage-gate`
  Enforces the current minimum coverage floor: `85%` lines, `83%` regions, and
  `84%` functions.
- `make stress-chakra`
  Runs a large-repo analysis against the vendored Chakra UI monorepo.
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

## Accuracy Metrics To Watch

- `parse_failures`
- `unresolved_imports`
- `ambiguous_edges`
- `total_affected_files` for known regression targets
- runtime on `monorepo-demo`, `vite-react-ts`, and `chakra-ui`
- `target/quality/metrics.json` drift over time

The current suite improves crash-resilience and parser coverage, but it is not
yet a proof of semantic correctness. Additional corpus-based accuracy checks are
still needed.

## Current Baseline

- Coverage: `85.40%` lines, `83.59%` regions, `84.34%` functions
- Runtime: about `1.53s` mean for the Chakra UI stress case on this machine
