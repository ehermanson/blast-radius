SHELL := /bin/zsh

.PHONY: test test-python coverage coverage-gate perf quality quality-python stress-chakra stress-python-demo stress-fastapi smoke-mui build metrics

build:
	cargo build

test:
	cargo test

test-python:
	cargo test --features python

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
