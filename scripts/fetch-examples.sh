#!/usr/bin/env bash
#
# Fetch the large vendored example snapshots that are NOT committed to this repo.
#
# blast-radius uses two real-world repositories as large-scale stress/regression
# fixtures: Chakra UI (React/TS) and FastAPI (Python). They are big, so instead
# of committing them we fetch them on demand, pinned to the exact upstream commit
# recorded in each example's UPSTREAM.md.
#
# Idempotent: skips an example that is already present. Pass --force to refetch.
#
# Usage:
#   scripts/fetch-examples.sh            # fetch any missing snapshots
#   scripts/fetch-examples.sh --force    # delete and refetch all
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
FORCE=0
[ "${1:-}" = "--force" ] && FORCE=1

# name  url  commit  sentinel-file (an upstream file proving the fetch worked)
fetch_one() {
  local name="$1" url="$2" sha="$3" sentinel="$4"
  local dest="$ROOT/examples/$name"

  if [ "$FORCE" = "1" ]; then
    # Remove upstream content but preserve tracked overlay files.
    find "$dest" -mindepth 1 \
      ! -name '.blast-radius.json' ! -name 'UPSTREAM.md' -delete 2>/dev/null || true
  fi

  if [ -f "$dest/$sentinel" ]; then
    echo "✓ $name already present (use --force to refetch)"
    return
  fi

  echo "→ fetching $name @ ${sha:0:12} ..."
  local tmp
  tmp="$(mktemp -d)"
  git -C "$tmp" init -q
  git -C "$tmp" remote add origin "$url"
  # Fetch just the pinned commit (GitHub allows fetching a specific SHA).
  git -C "$tmp" fetch -q --depth 1 origin "$sha"
  git -C "$tmp" checkout -q FETCH_HEAD
  rm -rf "$tmp/.git"

  mkdir -p "$dest"
  # Copy upstream files in; tracked overlay files in $dest are left untouched
  # because upstream does not contain them.
  cp -R "$tmp"/. "$dest"/
  rm -rf "$tmp"
  echo "✓ $name ready at examples/$name"
}

# Commits are mirrored in examples/<name>/UPSTREAM.md — keep them in sync.
fetch_one "chakra-ui" \
  "https://github.com/chakra-ui/chakra-ui" \
  "4384e3f8151e6ed41d638c103eb3fd612a3c44d5" \
  "package.json"

fetch_one "fastapi" \
  "https://github.com/fastapi/fastapi" \
  "5cdf820c8046edaf83c306ebd7435f038fc4a75a" \
  "pyproject.toml"

fetch_one "excalidraw" \
  "https://github.com/excalidraw/excalidraw" \
  "a83ac488536dbf4bc4dcf1f472f72ce3b4bd2073" \
  "package.json"

echo "Done."
