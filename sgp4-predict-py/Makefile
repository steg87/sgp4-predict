SILENT:

# Creates / refreshes the .venv so the root Makefile can locate the interpreter.
.PHONY: init
init:
	uv sync

.PHONY: dev
dev:
	uv run maturin develop

.PHONY: test
test: dev
	uv run pytest tests/ -v

.PHONY: stubs
stubs:
	cargo run --bin stub_gen

.PHONY: lint
lint:
	uv run ruff check --fix . && uv run ruff format .
