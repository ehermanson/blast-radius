# JSON Output

`--format json` emits a single object whose shape is versioned by the
top-level `schema_version` field (currently `1`).

**Stability contract:** `schema_version` is bumped only on breaking changes —
renamed or removed fields, or changed meanings. New fields may be added
without a bump, so consumers should ignore unknown fields rather than reject
them.

## Top level

| Field               | Type     | Meaning                                                                 |
| ------------------- | -------- | ----------------------------------------------------------------------- |
| `schema_version`    | number   | Schema version of this document. Currently `1`.                         |
| `mode`              | string   | `"export"`, `"file"`, or `"files"` — which subcommand ran.              |
| `target`            | object   | The analyzed input, tagged by `kind` (see below).                       |
| `repo_root`         | string   | Absolute path of the analyzed repository root.                          |
| `source_file_count` | number   | How many source files were discovered and indexed.                      |
| `summary`           | object   | Headline counts and the risk verdict (see below).                       |
| `workspaces`        | array    | Discovered workspace packages: `{ name, root }`, `root` repo-relative (empty string = repo root). |
| `roots`             | array    | Per-input-file impact breakdown; populated only in `files` mode (see below). |
| `nodes`             | array    | Every file/export touched by the analysis (see below).                  |
| `edges`             | array    | Dependency edges between nodes (see below).                             |
| `warnings`          | array    | Human-readable analyzer warnings (parse failures, unresolved-import diagnostics, …). |

## `target`

Tagged union on `kind`:

- `{ "kind": "export", "file": "...", "export_name": "..." }`
- `{ "kind": "file", "file": "..." }`
- `{ "kind": "files", "files": ["...", ...] }`

## `summary`

| Field                        | Type   | Meaning                                                            |
| ---------------------------- | ------ | ------------------------------------------------------------------ |
| `directly_affected_files`    | number | Files that import the changed file(s) directly.                    |
| `transitively_affected_files`| number | Files reached only through other impacted files.                   |
| `total_affected_files`       | number | `direct + transitive`. The changed file(s) themselves are excluded — this is the number `--fail-threshold` gates on. |
| `unresolved_imports`         | number | Internal-looking import specifiers that did not resolve to a repo file (confidence signal). |
| `ambiguous_edges`            | number | Edges whose resolution was ambiguous (confidence signal).           |
| `parse_failures`             | number | Source files that failed to parse and were skipped.                 |
| `skipped_inputs`             | number | `files`-mode inputs skipped (missing on disk or not a recognized source file). |
| `risk_tier`                  | string | `"minor"`, `"moderate"`, `"risky"`, or `"high"`, in ascending severity — the tier `--fail-on-risk` gates on. |

## `roots` (files mode only)

One entry per analyzed input file:

| Field       | Type   | Meaning                                              |
| ----------- | ------ | ---------------------------------------------------- |
| `file`      | string | Repo-relative path of the input file.                |
| `affected`  | number | Downstream files impacted by this input.             |
| `direct`    | number | Direct consumers.                                    |
| `indirect`  | number | Transitive consumers.                                |
| `max_depth` | number | Longest dependency chain from this input.            |
| `packages`  | number | Distinct packages the impact spans.                  |
| `files`     | array  | Impacted files: `{ path, endpoint, depth }` — `depth` is hops from the input (1 = direct consumer), `endpoint` marks leaves nothing else depends on. |

## `nodes`

| Field    | Type           | Meaning                                                          |
| -------- | -------------- | ----------------------------------------------------------------- |
| `id`     | string         | Stable-within-a-run node id (`file:<path>` or `export:<path>#<name>`). Treat as opaque; join `edges` on it. |
| `label`  | string         | Repo-relative path, `/`-separated on every platform. Use this for display and grouping. |
| `file`   | string         | Absolute path of the file.                                        |
| `symbol` | string \| null | Export name, for `export`-kind nodes.                             |
| `kind`   | string         | `"file"` or `"export"`.                                           |
| `depth`  | number         | Hops from the changed file (0 = an analysis root).                |

## `edges`

| Field          | Type    | Meaning                                            |
| -------------- | ------- | --------------------------------------------------- |
| `from`         | string  | Source node `id` (the depended-upon side).          |
| `to`           | string  | Target node `id` (the dependent side).              |
| `kind`         | string  | One of `imports_named`, `imports_default`, `imports_namespace`, `imports_dynamic`, `reexports_named`, `reexports_star`, `uses_jsx_component`, `requires_module`, `commonjs_export`. |
| `is_ambiguous` | boolean | Resolution for this edge was ambiguous; counted in `summary.ambiguous_edges`. |

## Example: gate on the verdict in a script

The exit code (`--fail-on-risk`, `--fail-threshold`) is the supported gating
interface, but the JSON is convenient for reporting:

```bash
git diff --name-only | blast-radius --format json files - |
  jq -r '"\(.summary.risk_tier): \(.summary.total_affected_files) files impacted"'
```
