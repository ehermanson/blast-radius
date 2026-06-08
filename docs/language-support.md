# Multi-Language Architecture

The analyzer core is language-neutral. Each language is a small adapter, and
optional languages are selected at **build time** with Cargo features — there is
no runtime `--language` flag. A binary scans whatever languages were compiled
into it.

## How it fits together

```text
repo discovery
  -> language adapter selects supported files
  -> adapter parses files into common facts
  -> language resolver maps imports to files
  -> shared analyzer walks the graph
  -> shared report renders output
```

Each language is implemented as a `LanguageAdapter` in the `language` module.
An adapter declares its file extensions, parses source into the shared
`ModuleFacts`, and resolves its own imports against a shared `ResolveCtx`. A
single registry enumerates the compiled-in adapters; discovery (`fs`), parse
dispatch (`parse`), and import resolution (`resolve`) all derive from it. The
analyzer (`analyze.rs`) only ever operates on normalized facts:

- file path
- imports
- exports / public symbols
- reexports
- local symbol usage (when available)
- language metadata for reporting/debugging

The non-JS adapters are intentionally conservative and over-approximate import
usage to avoid false negatives.

## Supported languages

| Language | Feature flag | Notes |
| --- | --- | --- |
| JavaScript / TypeScript | (default) | ESM + CommonJS, default/named exports, barrels, `export *`, `tsconfig` path aliases, cross-package resolution. Also the fallback for any unclaimed extension. |
| Python | `python` | `rustpython-parser`. `import`/`from` imports, relative imports, package + `__init__.py` resolution, top-level `def`/`class`/assignments, simple `__all__`. |
| Rust | `rust` | `syn`. Public items, `mod`, `use crate::`/`self::`/`super::`, grouped use trees, `pub use` reexports. |
| Vue | `vue` | Extracts `<script>` / `<script setup>` blocks (incl. `lang="ts"`) and parses them through the JS/TS parser; component file exposes a synthetic default export. |
| Svelte | `svelte` | Same script-block approach as Vue. |
| Ruby | `ruby` | Lightweight static parser. `require_relative`, in-repo `require`, top-level `class`/`module`/`def`. |
| Java | `java` | Lightweight static parser. `package` context, `import`/`import static`, top-level `class`/`interface`/`enum`/`record`. |

### Known limitations

Each non-JS adapter deliberately skips the harder, lower-value cases — for
example dynamic/runtime imports, macro- or metaprogramming-generated code,
build-system source-set metadata (Maven/Gradle, Cargo workspaces, Rails
autoloading), template/style blocks in components, and precise expression-level
usage. These are over-approximated rather than resolved exactly.
