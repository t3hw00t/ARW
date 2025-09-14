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

# Interfaces
interfaces-index:
  python3 scripts/interfaces_generate_index.py

interfaces-lint:
  if ! command -v npx >/dev/null 2>&1; then echo 'npx not found (Node). Install Node.js to lint interfaces.' >&2; exit 1; fi
  npx spectral lint -r quality/openapi-spectral.yaml spec/openapi.yaml
  npx spectral lint -r quality/openapi-spectral.yaml spec/asyncapi.yaml

interfaces-diff base="main":
  if ! command -v docker >/dev/null 2>&1; then echo 'docker not found; cannot run oasdiff container' >&2; exit 1; fi
  mkdir -p /tmp/ifc
  if git show origin/{{base}}:spec/openapi.yaml >/tmp/ifc/base.yaml 2>/dev/null; then :; else echo 'missing base OpenAPI in origin/{{base}}' >&2; exit 1; fi
  cp spec/openapi.yaml /tmp/ifc/rev.yaml
  docker run --rm -v /tmp/ifc:/tmp -w /tmp tufin/oasdiff:latest -format markdown -fail-on-breaking -base /tmp/base.yaml -revision /tmp/rev.yaml || true

check-enums:
  python3 scripts/check_models_progress_enums.py

docs-deprecations:
  python3 scripts/generate_deprecations.py

docs-release-notes base="origin/main":
  BASE_REF={{base}} python3 scripts/generate_interface_release_notes.py

# Endpoint scaffolding
endpoint-new method path tag="":
  python3 scripts/new_endpoint_template.py {{method}} {{path}} {{ if tag != "" { print("--tag '" + tag + "'") }}}

endpoint-add method path tag="" summary="" desc="":
  python3 scripts/new_endpoint_template.py {{method}} {{path}} {{ if tag != "" { print("--tag '" + tag + "'") }}} {{ if summary != "" { print("--summary '" + summary + "'") }}} {{ if desc != "" { print("--description '" + desc + "'") }}} --apply
