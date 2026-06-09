# Releasing

Releases are tag-driven. Pushing a `vX.Y.Z` tag runs
[`.github/workflows/release.yml`](.github/workflows/release.yml), which builds
prebuilt binaries for all supported platforms, creates the GitHub Release, and
publishes the npm packages. Publishing to crates.io stays manual (see below).

## One-time setup

- **`NPM_TOKEN` repository secret** — create an npm **automation** token
  (`npm token create --type automation`, or via npmjs.com > Access Tokens) for
  an account with publish rights, then add it as a repo secret named
  `NPM_TOKEN`. If the secret is absent the `npm-publish` job is skipped and the
  release still completes (binaries + GitHub Release only).
- npm package names are already confirmed available: `blast-radius-cli` plus
  the platform packages (`blast-radius-cli-linux-x64`,
  `blast-radius-cli-linux-arm64`, `blast-radius-cli-linux-x64-musl`,
  `blast-radius-cli-darwin-x64`, `blast-radius-cli-darwin-arm64`,
  `blast-radius-cli-win32-x64`).
- crates.io publishing uses your local `cargo login` credentials; no CI secret
  is involved.

## Cutting a release

1. **Bump the version** in `Cargo.toml` (e.g. `0.2.0`) and refresh the
   lockfile so `--locked` builds pass:

   ```sh
   cargo update -p blast-radius
   ```

2. **Update `CHANGELOG.md`** — move the `## [Unreleased]` content into a new
   `## [0.2.0] - YYYY-MM-DD` section. The release job lifts this section
   verbatim into the GitHub Release notes (it falls back to auto-generated
   notes if no matching section exists).

3. **Commit and tag** (tag must be `v` + the exact Cargo.toml version; CI
   fails the release immediately otherwise):

   ```sh
   git commit -am "Release v0.2.0"
   git tag v0.2.0
   git push origin main v0.2.0
   ```

4. **Watch CI** (`gh run watch`). The workflow does, in order:
   - `check` — verifies the tag matches `Cargo.toml`, runs `cargo test` with
     the fat feature set (`python,rust,vue,svelte,ruby,java`) as a smoke gate.
   - `build` — matrix over six targets (Linux glibc x64/arm64, Linux musl x64,
     macOS x64/arm64, Windows x64), `cargo build --release --locked` with fat
     features, strips the binary, and archives it as
     `blast-radius-v{version}-{target}.tar.gz` (`.zip` on Windows) containing
     `blast-radius[.exe]`, `LICENSE`, and `README.md`.
   - `release` — generates `sha256sums.txt` over all archives and creates the
     GitHub Release with the archives and checksum file attached.
   - `npm-publish` — extracts the binaries, runs
     `node npm/build-platform-packages.mjs`, publishes each platform package
     under `npm/dist/`, then publishes the `blast-radius-cli` wrapper **last**
     so its `optionalDependencies` all resolve. Publishes use
     `--provenance --access public`; if provenance generation fails (e.g.
     OIDC unavailable) it retries without provenance rather than failing the
     release.

5. **Publish to crates.io** (manual, after CI is green):

   ```sh
   cargo publish --locked
   ```

   Note: the crates.io build uses **default features only** (JavaScript/
   TypeScript). The prebuilt binaries and npm packages are the **fat** build
   with all language features. `cargo install blast-radius` users who want
   other languages must pass `--features python,rust,...` themselves.

## Rollback / yank

A bad release generally needs a **new patch release** — published npm versions
and crates.io versions are immutable. To stop people installing the bad one in
the meantime:

- **npm** — deprecate (preferred) or unpublish within 72h:

  ```sh
  npm deprecate blast-radius-cli@0.2.0 "Broken, use 0.2.1"
  # repeat for each blast-radius-cli-* platform package
  ```

- **crates.io** — yank (existing lockfiles keep working; new resolutions skip
  it):

  ```sh
  cargo yank --version 0.2.0          # cargo yank --version 0.2.0 --undo to revert
  ```

- **GitHub Release** — delete the release and tag if needed:

  ```sh
  gh release delete v0.2.0 --yes
  git push --delete origin v0.2.0
  ```

Then fix, bump to `0.2.1`, and tag again. Never reuse a tag/version that was
ever published anywhere.
