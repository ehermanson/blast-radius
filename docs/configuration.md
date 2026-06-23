# Configuration

An optional `.blast-radius.json` at the repo root lets a repository declare
tooling quirks the analyzer shouldn't hardcode. It can ignore import specifiers
that point at generated/virtual modules (CSS-in-JS codegen, route type stubs,
published `dist` output, etc.) so they don't count against the unresolved-import
confidence signal, and it can skip generated or vendored directories during repo
discovery:

```jsonc
{
  // comments and trailing commas are allowed (parsed as JSONC, like tsconfig)
  "discovery": {
    // repo-relative files or directory prefixes to skip while walking
    "exclude": ["generated/", "vendor/snapshot/"],
  },
  "unresolved": {
    "ignore": ["styled-system/css", ".velite", "/+types/"],
  },
}
```

- `discovery.exclude` entries are repo-relative prefixes.
- `unresolved.ignore` entries are matched as substrings of the import specifier.

Asset imports (`.svg`, `.css`, `.json`, images, …) and type-only imports are
ignored automatically.

See [`examples/chakra-ui/.blast-radius.json`](../examples/chakra-ui/.blast-radius.json)
for a real-world example.
