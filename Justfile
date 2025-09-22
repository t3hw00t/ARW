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

bench-snappy *params:
  cargo run -p snappy-bench -- {{params}}

# Package
package:
  bash scripts/package.sh --no-build

docker-build:
  docker build -f apps/arw-server/Dockerfile -t arw-server:dev .

docker-run:
  docker run --rm -p 8091:8091 -e ARW_PORT=8091 -e ARW_BIND=0.0.0.0 arw-server:dev

# Build and push multi-arch image to GHCR (requires docker login to GHCR)
docker-push ghcr="ghcr.io/t3hw00t/arw-server" tag="dev":
  docker buildx build --platform linux/amd64,linux/arm64 \
    -t {{ghcr}}:{{tag}} -f apps/arw-server/Dockerfile --push .

# Run published image from GHCR
docker-run-ghcr ghcr="ghcr.io/t3hw00t/arw-server" tag="latest":
  docker run --rm -p 8091:8091 -e ARW_PORT=8091 -e ARW_BIND=0.0.0.0 {{ghcr}}:{{tag}}

# Tail latest rolling access log (http.access)
access-tail:
  bash -ceu '
  dir="${ARW_ACCESS_LOG_DIR:-${ARW_LOGS_DIR:-./logs}}";
  prefix="${ARW_ACCESS_LOG_PREFIX:-http-access}";
  file=$(ls -t "$dir"/"$prefix"* 2>/dev/null | head -n1 || true);
  if [[ -z "$file" ]]; then echo "No access log file found in $dir (prefix=$prefix)" >&2; exit 1; fi;
  echo "Tailing $file"; tail -f "$file";
  '

compose-up:
  docker compose up --build -d

compose-down:
  docker compose down -v

helm-template release="arw" ns="default":
  helm template {{release}} deploy/charts/arw-server --namespace {{ns}}

helm-install release="arw" ns="default":
  helm upgrade --install {{release}} deploy/charts/arw-server --namespace {{ns}} --create-namespace

helm-uninstall release="arw" ns="default":
  helm uninstall {{release}} --namespace {{ns}}

# Docs
docgen:
  bash scripts/docgen.sh

check-system-components:
  python3 scripts/check_system_components_integrity.py

docs-build: docgen
  mkdocs build --strict

# Docs lint (headings/links/build)
docs-check:
  bash scripts/docs_check.sh

legacy-check:
  bash scripts/check_legacy_surface.sh

ops-export out='ops/out':
  bash scripts/export_ops_assets.sh --out '{{out}}'

# Service
start port='8091' debug='1':
  ARW_NO_LAUNCHER=1 ARW_NO_TRAY=1 bash -ceu '
  dbg="$1"; port="$2";
  if [ "$dbg" = "1" ]; then
  exec bash scripts/start.sh --debug --port "$port"
  else
  exec bash scripts/start.sh --port "$port"
  fi
  ' _ {{debug}} {{port}}

open-debug host='127.0.0.1' port='8091':
  bash scripts/open-url.sh http://{{host}}:{{port}}/admin/debug

hooks-install:
  bash scripts/hooks/install_hooks.sh

## Unified server dev runners (default)
dev port='8091':
  ARW_DEBUG=1 ARW_PORT={{port}} cargo run -p arw-server

dev-watch port='8091':
  ARW_DEBUG=1 ARW_PORT={{port}} cargo watch -x "run -p arw-server"

# New server (triad slice) — quick dev runners
dev-server port='8091':
  ARW_DEBUG=1 ARW_PORT={{port}} cargo run -p arw-server

dev-server-preset preset='balanced' port='8091':
  ARW_DEBUG=1 ARW_PERF_PRESET={{preset}} ARW_PORT={{port}} cargo run -p arw-server

# Docs dev server
docs-serve addr="127.0.0.1:8000":
  mkdocs serve -a {{addr}}

# Generate Feature Matrix (docs/reference/feature_matrix.md)
features-gen:
  python3 scripts/gen_feature_matrix.py

# Generate Universal Feature Catalog (docs/reference/feature_catalog.md)
feature-catalog-gen:
  python3 scripts/gen_feature_catalog.py

# Validate feature registry integrity
features-validate:
  python3 scripts/check_feature_integrity.py

# Stamp docs with Updated: YYYY-MM-DD from git history
docs-stamp:
  python3 scripts/stamp_docs_updated.py

# Stamp Type: Tutorial/How‑to/Reference/Explanation across docs
docs-type-stamp:
  python3 scripts/stamp_docs_type.py

# Run service + docs together (Unix)
dev-all port='8091' addr='127.0.0.1:8000':
  ARW_DEBUG=1 ARW_PORT={{port}} ARW_DOCS_URL=http://{{addr}} bash scripts/dev.sh {{port}} {{addr}}

# Tasks
task-add title desc="":
  bash -ceu '
  title="$1"; desc="$2";
  if [ -n "$desc" ]; then
  exec bash scripts/tasks.sh add "$title" --desc "$desc"
  else
  exec bash scripts/tasks.sh add "$title"
  fi
  ' _ {{title}} {{desc}}

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

# Generate OpenAPI + schemas + JSON snapshot
openapi-gen:
  cargo build --release --no-default-features -p arw-server
  OPENAPI_OUT=spec/openapi.yaml target/release/arw-server
  python3 scripts/ensure_openapi_descriptions.py
  python3 scripts/generate_openapi_json.py

check-enums:
  python3 scripts/check_models_progress_enums.py

docs-deprecations:
  python3 scripts/generate_deprecations.py

# Design tokens
tokens-sync:
  bash scripts/sync_tokens.sh

tokens-check:
  bash scripts/check_tokens_sync.sh

tokens-build:
  python3 scripts/build_tokens.py

tokens-rebuild:
  python3 scripts/build_tokens.py
  bash scripts/sync_tokens.sh
  bash scripts/check_tokens_sync.sh

tokens-tailwind:
  python3 scripts/gen_tailwind_tokens.py

tokens-sd:
  bash scripts/build_tokens_sd.sh

# Release: bump versions are already committed; tag and push
release-tag v:
  bash -ceu '
  if git status --porcelain | grep . >/dev/null; then
  echo "Working tree not clean. Commit or stash first." >&2; exit 1; fi
  if ! [[ "$v" =~ ^v[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
  echo "Invalid tag. Use vX.Y.Z" >&2; exit 2; fi
  echo "Tagging $v and pushing tags";
  git tag -s "$v" -m "$v" || git tag -a "$v" -m "$v";
  git push origin "$v";
  echo "Done. CI will build/publish artifacts for $v.";
  '

# Meta: verify workspace (fmt, clippy, tests, docs, event kinds)
verify:
  cargo fmt --all -- --check
  cargo clippy --workspace --all-targets -- -D warnings
  cargo test --workspace --locked
  python3 scripts/lint_event_names.py
  bash scripts/docs_check.sh

# Endpoint scaffolding
endpoint-new method path tag="":
  bash -ceu '
  m="$1"; p="$2"; t="$3";
  if [ -n "$t" ]; then
  exec python3 scripts/new_endpoint_template.py "$m" "$p" --tag "$t"
  else
  exec python3 scripts/new_endpoint_template.py "$m" "$p"
  fi
  ' _ {{method}} {{path}} {{tag}}

endpoint-add method path tag="" summary="" desc="":
  bash -ceu '
  m="$1"; p="$2"; t="$3"; s="$4"; d="$5";
  args=()
  if [ -n "$t" ]; then args+=(--tag "$t"); fi
  if [ -n "$s" ]; then args+=(--summary "$s"); fi
  if [ -n "$d" ]; then args+=(--description "$d"); fi
  exec python3 scripts/new_endpoint_template.py "$m" "$p" "${args[@]}" --apply
  ' _ {{method}} {{path}} {{tag}} {{summary}} {{desc}}
egress-get:
  curl -s http://127.0.0.1:8091/state/egress/settings | jq

egress-set json:
  curl -s -X POST http://127.0.0.1:8091/egress/settings \
    -H 'content-type: application/json' \
    -H "X-ARW-Admin: ${ARW_ADMIN_TOKEN:?set ARW_ADMIN_TOKEN}" \
    -d '{{json}}' | jq

egress-proxy-on port='9080':
  just egress-set '{"proxy_enable":true, "proxy_port": {{port}} }'

egress-proxy-off:
  just egress-set '{"proxy_enable":false}'
