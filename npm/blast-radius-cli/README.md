# blast-radius-cli

Prebuilt binary distribution of [blast-radius](https://github.com/ehermanson/blast-radius) —
a CLI that analyzes the transitive blast radius of code changes.

No Rust toolchain required: installing this package pulls in a small
platform-specific package (esbuild-style) containing a native binary for your
OS/architecture. Nothing is downloaded at runtime and there is no postinstall
step.

## Install

```sh
npm install --save-dev blast-radius-cli
```

Or run it without installing:

```sh
npx blast-radius-cli --help
```

Either way, the command is `blast-radius`:

```sh
npx blast-radius src/some/file.ts
```

## Notes

- The prebuilt binaries ship with **all language features compiled in**
  (Python, Rust, Vue, Svelte) — unlike a default
  `cargo install blast-radius`, which only includes the default feature set.
- Supported platforms: Linux x64/arm64 (glibc), Linux x64 (musl), macOS
  x64/arm64, Windows x64. On other platforms, build from source with
  `cargo install blast-radius`.

Full documentation: https://github.com/ehermanson/blast-radius

## License

MIT
