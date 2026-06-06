# Rust Demo

Small Rust crate used to exercise feature-gated Rust analysis.

```sh
cargo run --features rust --bin blast-radius -- --repo-root examples/rust-demo file src/utils/formatting.rs
cargo run --features rust --bin blast-radius -- --repo-root examples/rust-demo export src/services/email.rs send_email
```
