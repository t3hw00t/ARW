set shell := ["bash", "-cu"]

# Build
build:
  cargo build --workspace --release --locked

dev-build:
  cargo build --workspace

# Tauri apps
tauri-launcher-build:
  cargo build -p arw-launcher

tauri-launcher-run:
  cargo run -p arw-launcher

tauri-deps-linux:
  bash scripts/install-tauri-deps.sh

icons-gen:
  .venv/bin/python scripts/gen_icons.py

# Lint & format
fmt:
  cargo fmt --all

lint:
  cargo clippy --workspace --all-targets -- -D warnings

fmt-check:
  cargo fmt --all -- --check

lint-fix:
  cargo clippy --workspace --all-targets --fix -Z unstable-options --allow-dirty --allow-staged || true

# Test
test:
  cargo test --workspace --locked

test-fast:
  if command -v cargo-nextest >/dev/null; then cargo nextest run --workspace; else cargo test --workspace --locked; fi

test-watch:
  cargo watch -x "test --workspace"

# Package
package:
  bash scripts/package.sh --no-build

docker-build:
  docker build -f apps/arw-svc/Dockerfile -t arw-svc:dev .

docker-run:
  docker run --rm -p 8090:8090 -e ARW_PORT=8090 arw-svc:dev

compose-up:
  docker compose up --build -d

compose-down:
  docker compose down -v

helm-template release="arw" ns="default":
  helm template {{release}} deploy/charts/arw-svc --namespace {{ns}}

helm-install release="arw" ns="default":
  helm upgrade --install {{release}} deploy/charts/arw-svc --namespace {{ns}} --create-namespace

helm-uninstall release="arw" ns="default":
  helm uninstall {{release}} --namespace {{ns}}

# Docs
docgen:
  bash scripts/docgen.sh

docs-build: docgen
  mkdocs build --strict

# Docs lint (headings/links/build)
docs-check:
  bash scripts/docs_check.sh

# Service
start port=8090 debug=1:
  ARW_NO_LAUNCHER=1 ARW_NO_TRAY=1 bash scripts/start.sh {{ if debug == "1" { "--debug" } else { "" } }} --port {{port}}

open-debug host="127.0.0.1" port=8090:
  bash scripts/open-url.sh http://{{host}}:{{port}}/debug

hooks-install:
  bash scripts/hooks/install_hooks.sh

# Dev runner (service only)
dev port=8090:
  ARW_DEBUG=1 ARW_PORT={{port}} cargo run -p arw-svc

# Dev runner with auto-reload (requires cargo-watch)
dev-watch port=8090:
  ARW_DEBUG=1 ARW_PORT={{port}} cargo watch -x "run -p arw-svc"

# Docs dev server
docs-serve addr="127.0.0.1:8000":
  mkdocs serve -a {{addr}}

# Run service + docs together (Unix)
dev-all port=8090 addr="127.0.0.1:8000":
  ARW_DEBUG=1 ARW_PORT={{port}} ARW_DOCS_URL=http://{{addr}} bash scripts/dev.sh {{port}} {{addr}}

# Tasks
task-add title desc="":
  bash scripts/tasks.sh add {{title}} {{ if desc != "" { print("--desc \"" + desc + "\"") }}}

task-start id:
  bash scripts/tasks.sh start {{id}}

task-pause id:
  bash scripts/tasks.sh pause {{id}}

task-done id:
  bash scripts/tasks.sh done {{id}}

task-note id text:
  bash scripts/tasks.sh note {{id}} {{text}}
