SILENT:

PYTHON ?= python

.PHONY: test
test:
	cargo test --all-targets --all-features

.PHONY: lint
lint:
	cargo fmt --all && cargo clippy --all-targets --all-features -- -D warnings

.PHONY: coverage
coverage:
	cargo llvm-cov --all-targets --all-features --summary-only

.PHONY: py-dev
py-dev:
	cd python && maturin develop

.PHONY: py-test
py-test: py-dev
	cd python && $(PYTHON) -m pytest tests/ -v

.PHONY: py-stubs
py-stubs:
	cd python && cargo run --bin stub_gen

.PHONY: py-lint
py-lint:
	cd python && ruff check . && ruff format --check .
