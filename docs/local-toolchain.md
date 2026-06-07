# Local Toolchain Setup

`blast-radius` is most useful as a local, non-blocking signal. The recommended
setup is to install it once, then let `blast-radius init` create the Git hook.

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

## Recommended Setup

Install the default hook:

```bash
blast-radius init
```

Defaults:

- Installs `.git/hooks/pre-push`
- Runs `blast-radius --repo-root . diff "$base"`
- Uses `origin/main...HEAD` as the default diff range
- Lets you override the range with `BLAST_RADIUS_BASE`
- Runs in non-blocking mode, so it warns but does not fail the push
- Refuses to overwrite an existing hook unless `--force` is passed

Useful variants:

```bash
# Check staged files before commit instead of checking the branch diff before push
blast-radius init --hook pre-commit

# Use a different default comparison range for pre-push
blast-radius init --base main...HEAD

# Replace an existing hook
blast-radius init --force

# Make the hook blocking once the team trusts the signal
blast-radius init --blocking --fail-threshold 25
```

Exit codes in blocking mode:

- `0`: analysis completed and threshold was not exceeded
- `1`: analysis error
- `2`: `--fail-threshold` was exceeded

## Hook Managers

If a repo already uses a hook manager, either keep using `blast-radius init` for
plain Git hooks or call the same CLI commands from the manager config.

`lint-staged` example:

```json
{
  "lint-staged": {
    "*.{js,jsx,ts,tsx,vue,svelte}": "bash -c 'blast-radius --repo-root . files \"$@\" || true' --"
  }
}
```

Husky pre-push example:

```bash
npx husky add .husky/pre-push 'blast-radius --repo-root . diff origin/main...HEAD || true'
```

Lefthook pre-push example:

```yaml
pre-push:
  commands:
    blast-radius:
      run: blast-radius --repo-root . diff origin/main...HEAD || true
```

`pre-commit` framework example:

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

- Use `blast-radius init` for the lowest-friction local setup.
- Use pre-push for broader branch-level awareness.
- Use pre-commit when you want faster staged-file feedback.
- Keep local hooks non-blocking until the team trusts the signal.
- Add `--blocking --fail-threshold <count>` only after the signal is stable.
