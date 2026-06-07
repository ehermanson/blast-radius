# Local Toolchain Setup

`blast-radius` is most useful as a local, non-blocking signal. The recommended
setup is to install it once, then run it from pre-commit or pre-push hooks in
warning mode.

## Install

From this repository:

```bash
cargo install --path .
```

With optional language support:

```bash
cargo install --path . --features vue,svelte
cargo install --path . --features python,rust,vue,svelte,ruby,java
```

Confirm it is on your `PATH`:

```bash
blast-radius --help
```

## Non-Blocking Pre-Commit Hook

This runs only on staged files and never blocks the commit. It is intended to
surface surprising blast radius early without interrupting flow.

Create `.git/hooks/pre-commit`:

```bash
#!/usr/bin/env bash
set -u

if ! command -v blast-radius >/dev/null 2>&1; then
  echo "blast-radius: not installed; skipping"
  exit 0
fi

mapfile -t files < <(git diff --cached --name-only --diff-filter=ACMR)
if [ "${#files[@]}" -eq 0 ]; then
  exit 0
fi

echo "blast-radius: checking staged files"
blast-radius --repo-root . files "${files[@]}" || true
```

Then make it executable:

```bash
chmod +x .git/hooks/pre-commit
```

## Non-Blocking Pre-Push Hook

This checks the full branch diff before push. It is also non-blocking by
default.

Create `.git/hooks/pre-push`:

```bash
#!/usr/bin/env bash
set -u

if ! command -v blast-radius >/dev/null 2>&1; then
  echo "blast-radius: not installed; skipping"
  exit 0
fi

base="${BLAST_RADIUS_BASE:-origin/main...HEAD}"

echo "blast-radius: checking diff ${base}"
blast-radius --repo-root . diff "${base}" || true
```

Then make it executable:

```bash
chmod +x .git/hooks/pre-push
```

## Optional Blocking Mode

If a team later wants this to block, remove `|| true` and add a threshold:

```bash
blast-radius --repo-root . --fail-threshold 25 diff origin/main...HEAD
```

Exit codes:

- `0`: analysis completed and threshold was not exceeded
- `1`: analysis error
- `2`: `--fail-threshold` was exceeded

## Hook Managers

For repos that use a hook manager, call the same commands from the manager.

Husky example:

```bash
npx husky add .husky/pre-push 'blast-radius --repo-root . diff origin/main...HEAD || true'
```

Lefthook example:

```yaml
pre-push:
  commands:
    blast-radius:
      run: blast-radius --repo-root . diff origin/main...HEAD || true
```

pre-commit framework example:

```yaml
repos:
  - repo: local
    hooks:
      - id: blast-radius
        name: blast-radius
        entry: bash -c 'blast-radius --repo-root . files "$@" || true' --
        language: system
        pass_filenames: true
```

## Practical Defaults

- Use pre-commit for fast staged-file awareness.
- Use pre-push for broader branch-level awareness.
- Keep local hooks non-blocking until the team trusts the signal.
- Use CI thresholds only after the JSON schema and baseline metrics are stable.
