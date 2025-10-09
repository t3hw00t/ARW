.PHONY: build dev-build build-launcher fmt lint lint-events test package docgen docs-build start clean clean-venv

build:
	cargo build --workspace --release --locked --exclude arw-launcher

dev-build:
	cargo build --workspace --exclude arw-launcher

build-launcher:
	cargo build --workspace --release --locked

fmt:
	cargo fmt --all

lint:
	cargo clippy --workspace --all-targets -- -D warnings

lint-events:
	python3 scripts/lint_event_names.py

test:
	cargo test --workspace --locked

package:
	bash scripts/package.sh --no-build

docgen:
	bash scripts/docgen.sh

docs-build: docgen
	mkdocs build --strict

start:
	ARW_NO_LAUNCHER=1 ARW_NO_TRAY=1 bash scripts/start.sh --debug --port 8091

clean:
	bash scripts/clean_workspace.sh

clean-venv:
	bash scripts/clean_workspace.sh --venv
check-style:
	@scripts/check_env_guard.sh
