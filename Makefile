SILENT:

.PHONY: test
test:
	cargo test --all-targets --all-features

.PHONY: lint
lint:
	cargo clippy --all-targets --all-features -- -D warnings

.PHONY: coverage
coverage:
	cargo llvm-cov --all-targets --all-features --summary-only
