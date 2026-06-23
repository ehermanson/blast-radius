# CLI reference

Run `blast-radius --help` (or `<command> --help`) for the authoritative, always-up-to-date
version. This page mirrors it for browsing.

## Commands

| Command                | What it does                                        |
| ---------------------- | --------------------------------------------------- |
| `file <path>`          | Everything that depends on this file.               |
| `export <path> <name>` | Everything that depends on a specific named export. |
| `files <path>...`      | Blast radius for each file plus a combined total. `-` reads the list from stdin. |
| `graph`                | Dump the whole-repo import graph (every file and resolved import edge). |
| `completions <shell>`  | Print a completion script (bash, zsh, fish, elvish, powershell). |

## Global flags

| Flag                                  | Purpose                                             |
| ------------------------------------- | --------------------------------------------------- |
| `--repo-root <dir>`                   | Repo to analyze (default: current directory).       |
| `--format <tree\|json\|mermaid\|dot>` | Output format (default: `tree`).                    |
| `--output <file>`                     | Write output to a file instead of stdout.           |
| `--verbose`, `-v`                     | Show the full cascade tree.                         |
| `--quiet`, `-q`                       | No stdout output; exit codes and `--output` still apply. |
| `--color <auto\|always\|never>`       | ANSI color handling (default: `auto`; `NO_COLOR` respected). |
| `--explain-unresolved`                | Group unresolved internal imports by likely cause.  |
| `--fail-threshold <n>`                | Exit code 2 when more than `n` downstream files are impacted (the changed files themselves are not counted). |
| `--fail-on-risk <tier>`               | Exit code 2 when the verdict is at or above `tier` (`minor`, `moderate`, `risky`, `high`). |

`--version` prints the version plus the language adapters compiled into the
binary, so you can tell a JS/TS-only source build from the full prebuilt one.

## Exit codes

| Code | Meaning |
| ---- | ------- |
| `0`  | Analysis completed, no gate tripped. |
| `1`  | Analysis error. |
| `2`  | A gate tripped (`--fail-threshold` exceeded, or the verdict reached `--fail-on-risk`). |
| `64` | Usage error (unknown flag, missing argument) — distinct from `2` so CI can tell a misspelled flag apart from a tripped gate. |

## Output formats

- `tree` — the default human-readable verdict, meter, and impacted-file list.
  See [reading the output](output.md).
- `json` — structured output; the per-input-file breakdown lives in the `roots`
  array. Carries a top-level `schema_version` (currently `1`), bumped only on
  breaking shape changes — new fields may appear without a bump. The full
  field-by-field contract is in [json-output.md](json-output.md).
- `mermaid` — a Mermaid graph definition.
- `dot` — Graphviz DOT.

## Shell completions

Write the completion script where your shell looks for them, e.g.:

```bash
# zsh (with ~/.zfunc in fpath)
blast-radius completions zsh > ~/.zfunc/_blast-radius

# bash
blast-radius completions bash > /etc/bash_completion.d/blast-radius
```

Supported shells: bash, zsh, fish, elvish, powershell.
