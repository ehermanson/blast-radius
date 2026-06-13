# GitHub Action

Comment a pull request's **blast radius** — the files a change reaches — and
optionally fail the check when a change reaches too far. The action computes the
PR's changed files with git and pipes them to `blast-radius`; the binary stays a
pure analyzer (it never touches git or the GitHub API).

## Usage

```yaml
# .github/workflows/blast-radius.yml
name: blast-radius
on: pull_request

permissions:
  contents: read
  pull-requests: write # required to post the comment

jobs:
  blast-radius:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
        with:
          fetch-depth: 0 # needed to diff the PR base against its head
      - uses: ehermanson/blast-radius@main # or a release tag once published
        with:
          # all optional:
          repo-root: .
          fail-on-risk: high # minor | moderate | risky | high (omit to never fail)
```

`fetch-depth: 0` and `pull-requests: write` are the two things people forget —
without them the action can't diff the branch or post the comment.

It posts a single **sticky comment** that it updates in place on each push (it
finds its previous comment by a hidden marker), so a PR never accumulates a pile
of stale comments.

## Inputs

| Input | Default | Purpose |
| --- | --- | --- |
| `repo-root` | `.` | Repository root to analyze. |
| `version` | `latest` | `blast-radius-cli` version to run (pin for reproducibility). |
| `fail-on-risk` | `` (off) | Fail the check when the verdict is at or above this tier. |
| `comment` | `true` | Post/update the sticky PR comment. |
| `github-token` | `${{ github.token }}` | Token used to post the comment. |

## Pinning

Reference a release tag for stability (e.g. `ehermanson/blast-radius@v0.7.0`
once published), or `@main` to track the latest. Pin `version:` too if you want
the analyzer itself frozen.

## How it's tested

The comment rendering (`scripts/pr-comment.mjs`) is decoupled from GitHub: it
turns `blast-radius --format json` output into Markdown, so it is unit-tested
locally with static fixtures (`scripts/pr-comment.test.mjs`, run in CI via
`node --test`). The GitHub plumbing in `action.yml` is intentionally thin.
