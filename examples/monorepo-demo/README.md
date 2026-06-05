# monorepo-demo

This example workspace is tailored for `blast-radius` development and testing.

It includes:

- `tsconfig` path aliases
- workspace package resolution
- named and default exports
- barrel re-exports
- CommonJS `require`
- transitive React component usage across packages and apps

Example:

```bash
cargo run -- --repo-root examples/monorepo-demo export packages/ui/src/Button.tsx Button
```
