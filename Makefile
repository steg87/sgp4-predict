SILENT:

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
validation:
	cargo test --test validation pypredict_validation -- --ignored --nocapture

.PHONY: benchmark
benchmark:
	cargo test --test validation montecarlo_benchmark -- --ignored --nocapture

.PHONY: coverage
coverage:
	cargo llvm-cov --all-targets --all-features --summary-only

