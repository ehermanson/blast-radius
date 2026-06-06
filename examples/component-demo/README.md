# Component Demo

Small mixed Vue/Svelte fixture used to exercise feature-gated component
analysis.

```sh
cargo run --features vue,svelte --bin blast-radius -- --repo-root examples/component-demo file src/shared.ts
cargo run --features vue,svelte --bin blast-radius -- --repo-root examples/component-demo file src/Button.vue
```
