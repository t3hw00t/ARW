set shell := ["bash", "-cu"]

# Guardrails
verify:
  bash scripts/dev.sh verify

verify-fast:
  bash scripts/dev.sh verify --fast

verify-ci:
  bash scripts/dev.sh verify --ci

env:
  bash scripts/env/status.sh

# Cleanup
clean *args:
  bash scripts/clean_workspace.sh {{args}}

# Build
build:
  if command -v cargo >/dev/null; then cargo build --workspace --release --locked --exclude arw-launcher; else "$HOME/.cargo/bin/cargo" build --workspace --release --locked --exclude arw-launcher; fi

dev-build:
  if command -v cargo >/dev/null; then cargo build --workspace --exclude arw-launcher; else "$HOME/.cargo/bin/cargo" build --workspace --exclude arw-launcher; fi

# Build including launcher (requires platform deps)
build-launcher:
  if command -v cargo >/dev/null; then cargo build --workspace --release --locked --exclude arw-launcher; else "$HOME/.cargo/bin/cargo" build --workspace --release --locked --exclude arw-launcher; fi
  if uname -s | grep -q '^Linux'; then if command -v cargo >/dev/null; then cargo build -p arw-launcher --release --locked --features launcher-linux-ui; else "$HOME/.cargo/bin/cargo" build -p arw-launcher --release --locked --features launcher-linux-ui; fi; else if command -v cargo >/dev/null; then cargo build -p arw-launcher --release --locked; else "$HOME/.cargo/bin/cargo" build -p arw-launcher --release --locked; fi; fi

# Tauri apps
tauri-launcher-build:
  cargo build -p arw-launcher

tauri-launcher-run *args:
  cargo run -p arw-launcher -- {{args}}

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

lint-events:
  python3 scripts/lint_event_names.py

# Test
test:
  cargo test --workspace --locked -- --test-threads=1

# Run only the server crate tests serially to avoid test lock contention.
# Useful when arw-server tests appear to "hang" due to global state guards.
test-server:
  RUST_TEST_THREADS=1 cargo test -p arw-server -- --nocapture

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
  ARW_ADMIN_TOKEN="${ARW_ADMIN_TOKEN:-$(openssl rand -hex 32)}"; \
  docker run --rm -p 8091:8091 \
    -e ARW_BIND=0.0.0.0 \
    -e ARW_PORT=8091 \
    -e ARW_ADMIN_TOKEN="$ARW_ADMIN_TOKEN" \
    arw-server:dev

# Build and push multi-arch image to GHCR (requires docker login to GHCR)
docker-push ghcr="ghcr.io/t3hw00t/arw-server" tag="dev":
  docker buildx build --platform linux/amd64,linux/arm64 \
    -t {{ghcr}}:{{tag}} -f apps/arw-server/Dockerfile --push .

# Run published image from GHCR
docker-run-ghcr ghcr="ghcr.io/t3hw00t/arw-server" tag="latest":
  ARW_ADMIN_TOKEN="${ARW_ADMIN_TOKEN:-$(openssl rand -hex 32)}"; \
  docker run --rm -p 8091:8091 \
    -e ARW_BIND=0.0.0.0 \
    -e ARW_PORT=8091 \
    -e ARW_ADMIN_TOKEN="$ARW_ADMIN_TOKEN" \
    {{ghcr}}:{{tag}}

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
  python3 scripts/docs_check.py || python scripts/docs_check.py

docs-check-fast:
  DOCS_CHECK_FAST=1 python3 scripts/docs_check.py || DOCS_CHECK_FAST=1 python scripts/docs_check.py

bootstrap-docs *args:
  bash scripts/bootstrap_docs.sh {{args}}

docs-cache *args:
  bash scripts/build_docs_wheels.sh {{args}}

mise-hash *args:
  bash scripts/update_mise_hash.sh {{args}}

legacy-check:
  bash scripts/check_legacy_surface.sh

ops-export out='ops/out':
  bash scripts/export_ops_assets.sh --out '{{out}}'

triad-smoke:
  bash scripts/triad_smoke.sh

# Orchestrator CLI helpers
orchestrator-start goal persona base="http://127.0.0.1:8091" preset="" follow="true":
	set -euo pipefail; goal='{{goal}}'; persona='{{persona}}'; base='{{base}}'; preset='{{preset}}'; follow='{{follow}}'; \
	if [ -z "$goal" ]; then echo "error: goal required (invoke as: just orchestrator-start \"goal\" persona)" >&2; exit 1; fi; \
	if [ -z "$persona" ]; then echo "error: persona required (invoke as: just orchestrator-start \"goal\" persona)" >&2; exit 1; fi; \
	args=(orchestrator start "$goal" --base "$base"); \
	if [ -n "$preset" ]; then args+=(--preset "$preset"); fi; \
	if [ "$follow" = "true" ]; then args+=(--follow); fi; \
	ARW_PERSONA_ID="$persona" cargo run -p arw-cli -- "${args[@]}"

orchestrator-jobs base="http://127.0.0.1:8091" limit="25" json="false":
	set -euo pipefail; base='{{base}}'; limit='{{limit}}'; json='{{json}}'; \
	args=(orchestrator jobs --base "$base" --limit "$limit"); \
	if [ "$json" = "true" ]; then args+=(--json --pretty); fi; \
	cargo run -p arw-cli -- "${args[@]}"

orchestrator-catalog base="http://127.0.0.1:8091" status="" category="" json="false":
	set -euo pipefail; base='{{base}}'; status='{{status}}'; category='{{category}}'; json='{{json}}'; \
	args=(orchestrator catalog --base "$base"); \
	if [ -n "$status" ]; then args+=(--status "$status"); fi; \
	if [ -n "$category" ]; then args+=(--category "$category"); fi; \
	if [ "$json" = "true" ]; then args+=(--json --pretty); fi; \
	cargo run -p arw-cli -- "${args[@]}"

persona-seed base="http://127.0.0.1:8091" id="persona.alpha" name="" archetype="" telemetry="false" scope="" state_dir="" json="false" pretty="false":
	set -euo pipefail; base='{{base}}'; id='{{id}}'; name='{{name}}'; archetype='{{archetype}}'; \
	telemetry='{{telemetry}}'; scope='{{scope}}'; state_dir='{{state_dir}}'; json='{{json}}'; pretty='{{pretty}}'; \
	args=(admin persona seed --base "$base" --id "$id"); \
	if [ -n "$name" ]; then args+=(--name "$name"); fi; \
	if [ -n "$archetype" ]; then args+=(--archetype "$archetype"); fi; \
	if [ "$telemetry" = "true" ]; then args+=(--enable-telemetry); if [ -n "$scope" ]; then args+=(--telemetry-scope "$scope"); fi; fi; \
	if [ -n "$state_dir" ]; then args+=(--state-dir "$state_dir"); fi; \
	if [ "$json" = "true" ]; then args+=(--json); if [ "$pretty" = "true" ]; then args+=(--pretty); fi; fi; \
	cargo run -p arw-cli -- "${args[@]}"

runtime-smoke:
  RUNTIME_SMOKE_GPU_POLICY="${RUNTIME_SMOKE_GPU_POLICY:-auto}" bash scripts/runtime_smoke_suite.sh

runtime-llama-build:
  bash scripts/runtime_llama_build.sh

runtime-smoke-cpu:
  MODE=cpu bash scripts/runtime_llama_smoke.sh

runtime-smoke-gpu:
  RUNTIME_SMOKE_GPU_POLICY=require bash scripts/runtime_smoke_suite.sh

runtime-smoke-gpu-sim:
  RUNTIME_SMOKE_GPU_POLICY=simulate bash scripts/runtime_smoke_suite.sh

runtime-smoke-gpu-real:
  # Real GPU helper: skips stub stage, keeps artifacts, and enforces the CUDA-capable llama-server.
  RUNTIME_SMOKE_KEEP_TMP=1 \
  RUNTIME_SMOKE_GPU_POLICY=require \
  RUNTIME_SMOKE_ALLOW_GPU=1 \
  RUNTIME_SMOKE_SKIP_STUB="${RUNTIME_SMOKE_SKIP_STUB:-1}" \
  RUNTIME_SMOKE_ALLOW_HIGH_MEM="${RUNTIME_SMOKE_ALLOW_HIGH_MEM:-1}" \
  RUNTIME_SMOKE_LLAMA_SERVER_BIN="${RUNTIME_SMOKE_LLAMA_SERVER_BIN:-$(pwd)/cache/llama.cpp/build-windows/bin/llama-server.exe}" \
  ARW_SERVER_BIN="${ARW_SERVER_BIN:-$(pwd)/target/release/arw-server.exe}" \
  bash scripts/runtime_smoke_suite.sh

runtime-smoke-dry-run:
  RUNTIME_SMOKE_DRY_RUN=1 RUNTIME_SMOKE_GPU_POLICY="${RUNTIME_SMOKE_GPU_POLICY:-auto}" bash scripts/runtime_smoke_suite.sh

smoke-safe:
  bash scripts/smoke_safe.sh

runtime-smoke-safe:
  bash -c '. scripts/smoke_safe.sh; bash scripts/runtime_smoke_suite.sh'

runtime-smoke-vision:
  # Uses ARW_SERVER_BIN when provided; auto-builds arw-server otherwise.
  bash scripts/runtime_vision_smoke.sh

runtime-weights *args:
  python3 scripts/runtime_weights.py {{args}}

runtime-check:
  bash scripts/runtime_check.sh
runtime-check-weights-only:
  bash scripts/runtime_check.sh --weights-only

runtime-bundles-publish catalog bundle_root:
  set -euo pipefail
  base_url="${ARW_RUNTIME_BUNDLE_BASE_URL:-}"
  sign_key_b64="${ARW_RUNTIME_BUNDLE_SIGN_KEY_B64:-}"
  sign_key_file="${ARW_RUNTIME_BUNDLE_SIGN_KEY_FILE:-}"
  sign_key_id="${ARW_RUNTIME_BUNDLE_SIGN_KEY_ID:-}"
  sign_issuer="${ARW_RUNTIME_BUNDLE_SIGN_ISSUER:-}"
  sign_cli="${ARW_RUNTIME_BUNDLE_SIGN_CLI:-arw-cli}"
  declare -a args=("python3" "scripts/runtime_bundle_publish.py" "--bundle-root" "{{bundle_root}}" "--catalog" "{{catalog}}")
  if [[ -n "${base_url}" ]]; then
  args+=("--base-url" "${base_url}")
  fi
  if [[ -n "${sign_key_b64}" ]]; then
  args+=("--sign" "--sign-key-b64" "${sign_key_b64}")
  elif [[ -n "${sign_key_file}" ]]; then
  args+=("--sign" "--sign-key-file" "${sign_key_file}")
  fi
  if [[ -n "${sign_key_id}" ]]; then
  args+=("--sign-key-id" "${sign_key_id}")
  fi
  if [[ -n "${sign_issuer}" ]]; then
  args+=("--sign-issuer" "${sign_issuer}")
  fi
  if [[ "${sign_cli}" != "arw-cli" ]]; then
  args+=("--sign-cli" "${sign_cli}")
  fi
  "${args[@]}"

runtime-mirrors *args:
  python3 scripts/runtime_config.py {{args}}

verify-signatures base='http://127.0.0.1:8091' token='':
  BASE_URL='{{base}}' ARW_ADMIN_TOKEN='{{token}}' bash scripts/verify_bundle_signatures.sh

research-watcher-list status='pending' limit='' base='http://127.0.0.1:8091' token='':
	bash -ceu '
	base="$1"; status="$2"; limit="$3"; token="$4";
	args=(research-watcher list --base "$base");
	if [ -n "$status" ]; then args+=(--status "$status"); fi
	if [ -n "$limit" ]; then args+=(--limit "$limit"); fi
	if command -v arw-cli >/dev/null 2>&1; then
	cli=(arw-cli)
	else
	cli=(cargo run --quiet -p arw-cli --)
	fi
	if [ -n "$token" ]; then export ARW_ADMIN_TOKEN="$token"; fi
	exec "${cli[@]}" "${args[@]}"
	' _ {{base}} {{status}} {{limit}} {{token}}

research-watcher-approve base='http://127.0.0.1:8091' token='' ids='' from_status='pending' filter_source='' filter_contains='' limit='' note='' dry_run='false' json='false' pretty='false':
	bash -ceu '
	base="$1"; token="$2"; ids_str="$3"; from_status="$4"; filter_source="$5"; filter_contains="$6"; limit="$7"; note="$8"; dry_run="$9"; json="${10}"; pretty="${11}";
	args=(research-watcher approve --base "$base");
	if [ -n "$from_status" ]; then args+=(--from-status "$from_status"); fi
	if [ -n "$filter_source" ]; then args+=(--filter-source "$filter_source"); fi
	if [ -n "$filter_contains" ]; then args+=(--filter-contains "$filter_contains"); fi
	if [ -n "$limit" ]; then args+=(--limit "$limit"); fi
	if [ -n "$note" ]; then args+=(--note "$note"); fi
	if [ "$dry_run" = "true" ]; then args+=(--dry-run); fi
	if [ "$json" = "true" ]; then
	args+=(--json)
	if [ "$pretty" = "true" ]; then args+=(--pretty); fi
	fi
	if [ -n "$ids_str" ]; then
	for id in $ids_str; do
	args+=("$id")
	done
	fi
	if command -v arw-cli >/dev/null 2>&1; then
	cli=(arw-cli)
	else
	cli=(cargo run --quiet -p arw-cli --)
	fi
	if [ -n "$token" ]; then export ARW_ADMIN_TOKEN="$token"; fi
	exec "${cli[@]}" "${args[@]}"
	' _ {{base}} {{token}} {{ids}} {{from_status}} {{filter_source}} {{filter_contains}} {{limit}} {{note}} {{dry_run}} {{json}} {{pretty}}

research-watcher-archive base='http://127.0.0.1:8091' token='' ids='' from_status='' filter_source='' filter_contains='' limit='' note='' dry_run='false' json='false' pretty='false':
	bash -ceu '
	base="$1"; token="$2"; ids_str="$3"; from_status="$4"; filter_source="$5"; filter_contains="$6"; limit="$7"; note="$8"; dry_run="$9"; json="${10}"; pretty="${11}";
	args=(research-watcher archive --base "$base");
	if [ -n "$from_status" ]; then args+=(--from-status "$from_status"); fi
	if [ -n "$filter_source" ]; then args+=(--filter-source "$filter_source"); fi
	if [ -n "$filter_contains" ]; then args+=(--filter-contains "$filter_contains"); fi
	if [ -n "$limit" ]; then args+=(--limit "$limit"); fi
	if [ -n "$note" ]; then args+=(--note "$note"); fi
	if [ "$dry_run" = "true" ]; then args+=(--dry-run); fi
	if [ "$json" = "true" ]; then
	args+=(--json)
	if [ "$pretty" = "true" ]; then args+=(--pretty); fi
	fi
	if [ -n "$ids_str" ]; then
	for id in $ids_str; do
	args+=("$id")
	done
	fi
	if command -v arw-cli >/dev/null 2>&1; then
	cli=(arw-cli)
	else
	cli=(cargo run --quiet -p arw-cli --)
	fi
	if [ -n "$token" ]; then export ARW_ADMIN_TOKEN="$token"; fi
	exec "${cli[@]}" "${args[@]}"
	' _ {{base}} {{token}} {{ids}} {{from_status}} {{filter_source}} {{filter_contains}} {{limit}} {{note}} {{dry_run}} {{json}} {{pretty}}

context-ci:
  python3 scripts/context_ci.py || python scripts/context_ci.py

trials-preflight:
  bash scripts/trials_preflight.sh

trials-guardrails preset='trial' dry_run='false' base='http://127.0.0.1:8091' token='':
	bash -ceu 'preset="$1"; dry="$2"; base="$3"; token="$4"; args=( --preset "$preset" --base "$base" ); if [ "$dry" = "true" ]; then args+=( --dry-run ); fi; if [ -n "$token" ]; then args+=( --token "$token" ); fi; exec bash scripts/trials_guardrails.sh "${args[@]}"' _ {{preset}} {{dry_run}} {{base}} {{token}}

context-watch *extra:
	bash scripts/context_watch.sh {{extra}}

autonomy-rollback *params:
  bash scripts/autonomy_rollback.sh {{params}}

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
  {{ if os() == "windows" { "pwsh -NoLogo -NoProfile -File scripts/hooks/install_hooks.ps1" } else { "bash scripts/hooks/install_hooks.sh" } }}

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

# TLS
tls-dev *hosts:
  bash scripts/dev_tls_profile.sh {{hosts}}

proxy-caddy-generate host='localhost' backend='' email='' tls_module='' config_name='':
  bash -ceu '
  host="$1"; backend="$2"; email="$3"; tls_module="$4"; cfg="$5";
  args=(caddy generate --host "$host");
  if [[ -n "$backend" ]]; then args+=(--backend "$backend"); fi
  if [[ -n "$email" ]]; then args+=(--email "$email"); fi
  if [[ -n "$tls_module" ]]; then args+=(--tls-module "$tls_module"); fi
  if [[ -n "$cfg" ]]; then args+=(--config-name "$cfg"); fi
  exec bash scripts/reverse_proxy.sh "${args[@]}"
  ' _ {{host}} {{backend}} {{email}} {{tls_module}} {{config_name}}

proxy-caddy-start host='localhost' config_name='':
  bash -ceu '
  host="$1"; cfg="$2"; args=(caddy start);
  args+=(--host "$host");
  if [[ -n "$cfg" ]]; then args+=(--config-name "$cfg"); fi
  exec bash scripts/reverse_proxy.sh "${args[@]}"
  ' _ {{host}} {{config_name}}

proxy-caddy-stop host='localhost' config_name='':
  bash -ceu '
  host="$1"; cfg="$2"; args=(caddy stop);
  args+=(--host "$host");
  if [[ -n "$cfg" ]]; then args+=(--config-name "$cfg"); fi
  exec bash scripts/reverse_proxy.sh "${args[@]}"
  ' _ {{host}} {{config_name}}

proxy-nginx-generate host cert key backend='' config_name='':
  bash -ceu '
  host="$1"; cert="$2"; key="$3"; backend="$4"; cfg="$5";
  [[ -n "$host" ]] || { echo "host is required" >&2; exit 1; };
  [[ -n "$cert" ]] || { echo "cert is required" >&2; exit 1; };
  [[ -n "$key" ]] || { echo "key is required" >&2; exit 1; };
  args=(nginx generate --host "$host" --cert "$cert" --key "$key");
  if [[ -n "$backend" ]]; then args+=(--backend "$backend"); fi
  if [[ -n "$cfg" ]]; then args+=(--config-name "$cfg"); fi
  exec bash scripts/reverse_proxy.sh "${args[@]}"
  ' _ {{host}} {{cert}} {{key}} {{backend}} {{config_name}}

proxy-nginx-start host='localhost' config_name='':
  bash -ceu '
  host="$1"; cfg="$2"; args=(nginx start);
  args+=(--host "$host");
  if [[ -n "$cfg" ]]; then args+=(--config-name "$cfg"); fi
  exec bash scripts/reverse_proxy.sh "${args[@]}"
  ' _ {{host}} {{config_name}}

proxy-nginx-stop host='localhost' config_name='':
  bash -ceu '
  host="$1"; cfg="$2"; args=(nginx stop);
  args+=(--host "$host");
  if [[ -n "$cfg" ]]; then args+=(--config-name "$cfg"); fi
  exec bash scripts/reverse_proxy.sh "${args[@]}"
  ' _ {{host}} {{config_name}}

# Generate Feature Matrix (docs/reference/feature_matrix.md)
features-gen:
  python3 scripts/gen_feature_matrix.py

# Generate Universal Feature Catalog (docs/reference/feature_catalog.md)
feature-catalog-gen:
  python3 scripts/gen_feature_catalog.py

# Generate mini-agent catalog (interfaces/mini_agents.json)
mini-catalog-gen:
  python3 scripts/gen_mini_catalog.py

# Check mini-agent catalog without writing
mini-catalog-check:
  python3 scripts/gen_mini_catalog.py --check

# Validate feature registry integrity
features-validate:
  python3 scripts/check_feature_integrity.py

# Stamp docs with Updated: YYYY-MM-DD from git history
docs-stamp:
  python3 scripts/stamp_docs_updated.py

# Stamp Type: Tutorial/How‑to/Reference/Explanation across docs
docs-type-stamp:
  python3 scripts/stamp_docs_type.py

# Tail SSE events (curl+jq helper)
sse-tail prefixes='service.,state.read.model.patch' replay='25' store='.arw/last-event-id' base='http://127.0.0.1:8091':
  BASE={{base}} ARW_ADMIN_TOKEN="${ARW_ADMIN_TOKEN:-}" \
    bash scripts/sse_tail.sh --prefix {{prefixes}} --replay {{replay}} --store {{store}}

# Tag TypeScript client version based on clients/typescript/package.json
ts-client-tag:
  bash -ceu '
  ver=$(jq -r .version clients/typescript/package.json);
  [[ -n "$ver" && "$ver" != "null" ]] || { echo "could not read version" >&2; exit 1; }
  tag="ts-client-v$ver";
  if git rev-parse "$tag" >/dev/null 2>&1; then echo "Tag $tag already exists"; exit 0; fi;
  git tag "$tag"; echo "Created tag $tag"; echo "Run: git push origin $tag";
  '

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
  # Use the official Spectral CLI package; avoid relying on a global alias
  npx --yes @stoplight/spectral-cli lint -r quality/openapi-spectral.yaml spec/openapi.yaml
  npx --yes @stoplight/spectral-cli lint -r quality/openapi-spectral.yaml spec/asyncapi.yaml

interfaces-diff base="main":
  mkdir -p /tmp/ifc
  if git show origin/{{base}}:spec/openapi.yaml >/tmp/ifc/base.yaml 2>/dev/null; then :; else echo 'missing base OpenAPI in origin/{{base}}' >&2; exit 1; fi
  cp spec/openapi.yaml /tmp/ifc/rev.yaml
  # Prefer oasdiff in Docker when available; else fall back to openapi-diff (JSON)
  if command -v docker >/dev/null 2>&1; then docker run --rm -v /tmp/ifc:/tmp -w /tmp tufin/oasdiff:latest breaking /tmp/base.yaml /tmp/rev.yaml -f markdown -o ERR || true; else echo 'docker missing; falling back to openapi-diff via npx (JSON only)'; python3 scripts/yaml_to_json.py /tmp/ifc/base.yaml /tmp/ifc/base.json; python3 scripts/yaml_to_json.py /tmp/ifc/rev.yaml /tmp/ifc/rev.json; npx --yes openapi-diff /tmp/ifc/base.json /tmp/ifc/rev.json || true; fi

# Generate OpenAPI + schemas + JSON snapshot
openapi-gen:
  cargo build --release --no-default-features -p arw-server
  OPENAPI_OUT=spec/openapi.yaml target/release/arw-server
  # ensure_openapi_descriptions.py returns 1 when it modifies the spec; keep going
  python3 scripts/ensure_openapi_descriptions.py || true
  # apply curated summaries/descriptions from spec/operation_docs.yaml (exit 1 when it edits)
  python3 scripts/apply_operation_docs.py || true
  python3 scripts/generate_openapi_json.py

check-enums:
  python3 scripts/check_models_progress_enums.py

docs-deprecations:
  python3 scripts/generate_deprecations.py

docs-release-notes base="origin/main":
  BASE_REF={{base}} python3 scripts/generate_interface_release_notes.py

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

# Meta: manual workspace verify (fmt, clippy, tests, docs, event kinds)
verify-manual:
  cargo fmt --all -- --check
  cargo clippy --workspace --all-targets -- -D warnings
  cargo test --workspace --locked
  node apps/arw-launcher/src-tauri/ui/read_store.test.js
  python3 scripts/check_operation_docs_sync.py
  python3 scripts/gen_topics_doc.py --check
  python3 scripts/lint_event_names.py
  python3 scripts/docs_check.py || python scripts/docs_check.py

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
# Dependency lockfiles for pip
requirements-lock:
  if command -v pip-compile >/dev/null 2>&1; then \
    pip-compile --generate-hashes --output-file requirements/docs.txt requirements/docs.in; \
    pip-compile --generate-hashes --output-file requirements/interfaces.txt requirements/interfaces.in; \
  elif command -v python3 >/dev/null 2>&1 && python3 -m piptools --help >/dev/null 2>&1; then \
    python3 -m piptools compile --generate-hashes --output-file requirements/docs.txt requirements/docs.in; \
    python3 -m piptools compile --generate-hashes --output-file requirements/interfaces.txt requirements/interfaces.in; \
  elif command -v python >/dev/null 2>&1 && python -m piptools --help >/dev/null 2>&1; then \
    python -m piptools compile --generate-hashes --output-file requirements/docs.txt requirements/docs.in; \
    python -m piptools compile --generate-hashes --output-file requirements/interfaces.txt requirements/interfaces.in; \
  elif command -v py >/dev/null 2>&1 && py -3 -m piptools --help >/dev/null 2>&1; then \
    py -3 -m piptools compile --generate-hashes --output-file requirements/docs.txt requirements/docs.in; \
    py -3 -m piptools compile --generate-hashes --output-file requirements/interfaces.txt requirements/interfaces.in; \
  else \
    echo "error: pip-tools is not installed. Install it with \"python -m pip install pip-tools\" (add --break-system-packages on Debian/Ubuntu) and re-run." >&2; \
    exit 1; \
  fi
