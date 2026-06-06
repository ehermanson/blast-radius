SHELL := /bin/zsh

.PHONY: test test-python test-rust test-components test-ruby test-java test-all-languages coverage coverage-gate perf quality quality-python quality-rust quality-components quality-ruby quality-java stress-chakra stress-python-demo stress-fastapi stress-rust-demo stress-components stress-ruby-demo stress-java-demo smoke-mui build metrics

build:
	cargo build

test:
	cargo test

test-python:
	cargo test --features python

test-rust:
	cargo test --features rust

test-components:
	cargo test --features vue,svelte

test-ruby:
	cargo test --features ruby

test-java:
	cargo test --features java

test-all-languages:
	cargo test --features python,rust,vue,svelte,ruby,java

coverage:
	cargo llvm-cov --summary-only

coverage-gate:
	cargo llvm-cov --summary-only --fail-under-lines 85 --fail-under-regions 83 --fail-under-functions 84

stress-chakra: build
	./target/debug/blast-radius --repo-root examples/chakra-ui file packages/react/src/components/button/button.tsx

stress-python-demo:
	cargo run --features python --bin blast-radius -- --repo-root examples/python-demo file app/utils/formatting.py

stress-fastapi:
	cargo run --features python --bin blast-radius -- --repo-root examples/fastapi file fastapi/applications.py

stress-rust-demo:
	cargo run --features rust --bin blast-radius -- --repo-root examples/rust-demo file src/utils/formatting.rs

stress-components:
	cargo run --features vue,svelte --bin blast-radius -- --repo-root examples/component-demo file src/shared.ts

stress-ruby-demo:
	cargo run --features ruby --bin blast-radius -- --repo-root examples/ruby-demo file lib/app/utils/formatter.rb

stress-java-demo:
	cargo run --features java --bin blast-radius -- --repo-root examples/java-demo file src/main/java/com/example/util/Formatter.java

smoke-mui: build
	@if [ ! -d target/tmp/mui-mini/.git ] && [ ! -d target/tmp/mui-mini/docs ]; then \
		echo "Cloning Material UI into target/tmp/mui-mini for smoke testing..."; \
		git clone --depth 1 https://github.com/mui/material-ui.git target/tmp/mui-mini; \
	fi
	./target/debug/blast-radius --repo-root target/tmp/mui-mini file docs/data/experiments/renderers/renderAvatar.js

perf: build
	hyperfine --shell=none --warmup 1 \
		'./target/debug/blast-radius --repo-root tests/fixtures/monorepo file packages/ui/src/Button.tsx' \
		'./target/debug/blast-radius --repo-root examples/vite-react-ts file src/App.tsx' \
		'./target/debug/blast-radius --repo-root examples/chakra-ui file packages/react/src/components/button/button.tsx'

metrics: build
	node scripts/collect_metrics.mjs

quality: test coverage-gate stress-chakra

quality-python: test-python stress-python-demo stress-fastapi

quality-rust: test-rust stress-rust-demo

quality-components: test-components stress-components

quality-ruby: test-ruby stress-ruby-demo

quality-java: test-java stress-java-demo
