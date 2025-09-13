.PHONY: build dev-build fmt lint test package docgen docs-build start

build:
	cargo build --workspace --release --locked

dev-build:
	cargo build --workspace

fmt:
	cargo fmt --all

lint:
	cargo clippy --workspace --all-targets -- -D warnings

test:
	cargo test --workspace --locked

package:
	bash scripts/package.sh --no-build

docgen:
	bash scripts/docgen.sh

docs-build: docgen
	mkdocs build --strict

start:
	ARW_NO_LAUNCHER=1 ARW_NO_TRAY=1 bash scripts/start.sh --debug --port 8090
