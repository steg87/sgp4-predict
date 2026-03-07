SILENT:

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
	uv run ruff check . && uv run ruff format --check .
