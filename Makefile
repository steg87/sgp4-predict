SILENT:

.PHONY: test
test:
	cargo test && cargo test --features uom

.PHONY: lint
lint:
	cargo clippy --all-targets --all-features -- -D warnings
