SILENT:

ifdef VIRTUAL_ENV
PYTHON ?= $(VIRTUAL_ENV)/bin/python
else
PYTHON ?= python
endif

.PHONY: dev
dev:
	maturin develop

# The local .venv should be active to call this
.PHONY: test
test: dev
	$(PYTHON) -m pytest tests/ -v

.PHONY: stubs
stubs:
	cargo run --bin stub_gen

.PHONY: lint
lint:
	ruff check . && ruff format --check .