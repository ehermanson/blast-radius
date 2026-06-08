# Local Toolchain Setup

`blast-radius` expects callers to pass the files they want analyzed. It does not
discover changed files itself or install hooks.

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

## File List Input

Use `files` when a hook manager or CI step already has a list of changed files:

```bash
blast-radius --repo-root . files packages/ui/src/Button.tsx packages/ui/src/Card.tsx
```

In non-blocking local workflows, append `|| true` so the signal does not stop
the developer until the team trusts the output.

Exit codes in blocking mode:

- `0`: analysis completed and no gate was tripped
- `1`: analysis error
- `2`: a gate tripped — `--fail-threshold` exceeded, or the verdict reached
  `--fail-on-risk`

Two gates are available and can be combined; if either trips, the exit code is
`2`:

- `--fail-threshold <count>`: trips when more than `count` files are affected.
  Best when you have a repo-specific budget in mind.
- `--fail-on-risk <minor|moderate|risky|high>`: trips when the headline verdict
  is at or above the given tier. Prefer this in most cases — the tier already
  normalizes for reach and spread, so it travels across repos of different
  sizes better than a raw count.

## Hook Managers

`lint-staged` example:

```json
{
  "lint-staged": {
    "*.{js,jsx,ts,tsx,vue,svelte}": "bash -c 'blast-radius --repo-root . files \"$@\" || true' --"
  }
}
```

Lefthook example:

```yaml
pre-commit:
  commands:
    blast-radius:
      glob: "*.{js,jsx,ts,tsx,vue,svelte}"
      run: blast-radius --repo-root . files {staged_files} || true
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

- Prefer `files` in local hooks and CI because it is deterministic.
- Keep local checks non-blocking until the team trusts the signal.
- Add a gate (`--fail-on-risk <tier>`, or `--fail-threshold <count>`) only after
  the signal is stable.
