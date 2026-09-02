SILENT:

.PHONY: init
init:
	uv sync --extra dev

.PHONY: dev
dev: init
	uv run maturin develop

.PHONY: test
test: dev
	uv run pytest tests/ -v

.PHONY: stubs
stubs:
	cargo run --bin stub_gen

# Fixes in place. `lint-check` is what CI runs; keep the two in step.
.PHONY: lint
lint: init
	uv run ruff check --fix . && uv run ruff format .

.PHONY: lint-check
lint-check: init
	uv run ruff check . && uv run ruff format --check .
