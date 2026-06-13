# CI: what each job means

Every push and pull request runs [`.github/workflows/quality.yml`](../.github/workflows/quality.yml).
The jobs fall into two groups: **correctness** (does blast-radius compute the
right answer?) and **hygiene** (does it build and lint cleanly?). If a check goes
red, find the job below to know what broke.

## Correctness

blast-radius's answer has two parts, validated by two independent layers:

1. the **import graph** — does it resolve each import to the right file?
2. the **blast radius** — given a changed file, does it report the right set of
   transitive dependents?

### Layer 1 — the import graph (the `accuracy` job)

Runs the accuracy oracle (`scripts/accuracy/oracle.mjs --strict`), which diffs
blast-radius's import edges against [dependency-cruiser](https://github.com/sverweij/dependency-cruiser),
a mature independent resolver, on the fixtures and the real Chakra UI and
Excalidraw snapshots.

- **A failure means:** blast-radius missed (or mis-resolved) an import edge that
  an independent resolver found — a *resolution* regression (e.g. a tsconfig
  alias, exports-map, or workspace package stopped resolving).
- **It does NOT check the blast radius** (reachability). It can't: dependency-
  cruiser is file-level, while blast-radius is symbol-aware, so it would flag
  blast-radius's *correct* pruning through barrels as false misses. Reachability
  is layer 2.

### Layer 2 — the blast radius (the `cargo test` jobs)

The unit + integration test suite asserts the actual reported blast radius
(`file` / `export` output) against expected results, plus invariants on real
repos. These jobs all run `cargo test`:

| Job | What it runs | A failure means |
| --- | --- | --- |
| `quality` | core JS/TS suite + coverage floor + Chakra stress run | a JS/TS behavior or coverage regression |
| `all-features` | the suite with `python,rust,vue,svelte` compiled in | a cross-language / feature-interaction regression |
| `python` / `rust` / `components` | each optional adapter's tests | that adapter regressed |
| `windows` | the suite on Windows | a path/separator regression |

**What layer 2 catches:** traversal/reachability bugs, symbol precision (a
`Card` change must not reach a file that imports only `Button` through the same
barrel), import cycles, diamonds, dynamic imports, `vi.mock`/`jest.mock` reach,
and the imported-but-unused rule. On the **real** Chakra/Excalidraw repos it
asserts that a hub file reports a substantial, correct-looking radius — non-empty
(not silently zero), the changed file excluded from its own radius, every
dependent downstream, no parse failures, and a hand-verified known dependent
present. We don't assert an exact dependent set on real repos (no ground truth),
but these invariants catch the failure that matters most: reporting nothing, or
reporting something unreachable.

## Hygiene

| Job | A failure means |
| --- | --- |
| `lint` | `cargo fmt --check` or `clippy -D warnings` failed |
| `msrv` | the code used a Rust feature newer than the minimum supported version |

## Summary: "this check is red, so…"

- **`accuracy`** → an import stopped resolving correctly (graph).
- **`quality` / `all-features` / `python` / `rust` / `components` / `windows`** →
  the computed blast radius (or a test/coverage invariant) regressed.
- **`lint` / `msrv`** → formatting, lint, or minimum-Rust-version issue.
