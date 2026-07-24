SILENT:

# Pin the Python interpreter so cargo build scripts for the pyo3 crate always
# find the correct venv, regardless of any VIRTUAL_ENV set in the shell.
PYO3_PYTHON := $(CURDIR)/sgp4-predict-py/.venv/bin/python
export PYO3_PYTHON

.PHONY: init
init:
	prek install

.PHONY: test
test:
	cargo test --all-targets --all-features

.PHONY: lint
lint:
	cargo fmt --all && cargo clippy --all-targets --all-features -- -D warnings

.PHONY: validation
validation:  ## downloads de421.bsp (~17 MB) from NASA on first run
	cargo test --test validation validate -- --ignored --nocapture

.PHONY: validation-regen
validation-regen:  ## regenerate reference CSVs from pypredict/skyfield, then validate
	SGP4_PREDICT_REGEN=1 cargo test --test validation validate -- --ignored --nocapture

.PHONY: benchmark
benchmark:
	cargo test --test validation montecarlo_benchmark -- --ignored --nocapture

.PHONY: coverage
coverage:
	cargo llvm-cov --all-targets --all-features --summary-only

.PHONY: audit
audit:
	cargo audit

.PHONY: docs
docs:
	cargo doc --all-features --no-deps --open

