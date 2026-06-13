# Accuracy oracle

Differential test of blast-radius's import graph against
[dependency-cruiser](https://github.com/sverweij/dependency-cruiser) (a mature,
`enhanced-resolve`-based reference) on a fixture. It answers the question the
hand-built test suite can't: _does blast-radius find the same dependency edges
a serious independent resolver finds?_

```bash
cargo build                                   # the oracle drives ./target/debug/blast-radius
node scripts/accuracy/oracle.mjs <fixture-dir> [--tsconfig <path>] [--json] [--strict]
```

It extracts each tool's forward import-edge set (importer → importee, internal
source files only) and reports the symmetric difference:

- **reference only** — edges dependency-cruiser resolved that blast-radius
  missed. These are the edges that matter: a miss here is a false negative, the
  failure mode this tool most needs to avoid. `--strict` exits non-zero when
  this set is non-empty.
- **blast-radius superior** — edges blast-radius resolved that the reference
  could not (e.g. workspace packages resolved via `package.json` names without a
  `node_modules` symlink, which dependency-cruiser needs).
- **blast-radius extra** — other edges blast-radius found that the reference
  lacks, usually type-only re-exports (`export type { X } from './types'`) that
  dependency-cruiser drops. Reported for review; not a gate failure.

## Scope reconciliation

The two tools are normalized to compare the same thing:

- `node_modules` is excluded on both sides (blast-radius never indexes it).
- Asset imports (CSS, images, `.json`, fonts) are excluded — blast-radius
  scopes them out as non-code by design; dependency-cruiser counts them.

## How the graph is extracted

blast-radius's whole-repo forward graph comes from a single `blast-radius graph`
invocation (one parse of the repo), so the oracle scales to thousands of files
in seconds. Edges are stored depended-upon → consumer; the oracle flips them to
importer → importee.

## Baseline results (2026-06-13)

| Fixture | Files | Reference-only (misses) | Notes |
| --- | --- | --- | --- |
| `tests/fixtures/monorepo` | 7 | 0 | 1 workspace edge only blast-radius resolves |
| `examples/vite-react-ts` | 4 | 0 | exact match |
| `examples/chakra-ui` (library) | 2697 | 0 | every divergence is blast-radius finding more true edges |
| `examples/excalidraw` (app) | 631 | 2 | both are `vi.mock(...)` calls in a test-setup file (test-runner mock magic, not modeled) |

The corpus deliberately spans both shapes: Chakra UI is library-shaped (barrels,
re-exports, package-internal edges); Excalidraw is a real application (route/lazy
code-splitting, dynamic imports, app→feature→shared layering). The only
Excalidraw misses are `vi.mock("…")` module references, which blast-radius does
not currently model.

dependency-cruiser is pinned to an exact version (`dependency-cruiser@16.10.4`)
and fetched on demand via `npx`, so there is no committed Node dependency
footprint and the gate is deterministic.

## CI

The `accuracy` job in `.github/workflows/quality.yml` runs the oracle with
`--strict` on every fixture (committed ones, plus the fetched Chakra UI and
Excalidraw snapshots) on each push and PR. `--strict` fails the build on any
reference-only edge — i.e. a real blast-radius miss — so a regression that drops
a resolved edge cannot land silently. Blast-radius finding *more* edges than the
reference never fails the gate.
