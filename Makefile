.PHONY: build dev-build build-launcher fmt lint lint-events test package docgen docs-build start clean clean-venv
.PHONY: docs-bootstrap docs-check docs-check-fast docs-cache mise-hash verify verify-fast verify-ci

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

docs-check:
	bash scripts/docs_check.sh

docs-check-fast:
	DOCS_CHECK_FAST=1 bash scripts/docs_check.sh

docs-bootstrap:
	bash scripts/bootstrap_docs.sh

docs-cache:
	bash scripts/build_docs_wheels.sh --archive dist/docs-wheels.tar.gz

mise-hash:
	bash scripts/update_mise_hash.sh

verify:
	bash scripts/dev.sh verify

verify-fast:
	bash scripts/dev.sh verify --fast

verify-ci:
	bash scripts/dev.sh verify --ci

start:
	ARW_NO_LAUNCHER=1 ARW_NO_TRAY=1 bash scripts/start.sh --debug --port 8091

clean:
	bash scripts/clean_workspace.sh

clean-venv:
	bash scripts/clean_workspace.sh --venv
check-style:
	@scripts/check_env_guard.sh
