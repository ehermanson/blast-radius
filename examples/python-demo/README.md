# Python Demo

Small Python package used to exercise feature-gated Python analysis.

```sh
cargo run --features python --bin blast-radius -- --repo-root examples/python-demo file app/utils/formatting.py
cargo run --features python --bin blast-radius -- --repo-root examples/python-demo export app/services/email.py send_email
```
