#!/usr/bin/env bash
set -euo pipefail

ROOT=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")"/../.. && pwd)
cd "$ROOT"

if [[ ! -d .git ]]; then
  echo "[hooks] Not a git repository: $ROOT" >&2
  exit 1
fi

mkdir -p .git/hooks
cat > .git/hooks/pre-commit << 'EOF'
#!/usr/bin/env bash
set -euo pipefail
echo "[pre-commit] cargo fmt --check"
cargo fmt --all -- --check
echo "[pre-commit] cargo clippy -D warnings"
cargo clippy --workspace --all-targets -- -D warnings
if command -v python3 >/dev/null 2>&1; then
  echo "[pre-commit] Event naming guard (dot.case)"
  python3 scripts/lint_event_names.py
else
  echo "[pre-commit] python3 unavailable; skipping event lint"
fi
echo "[pre-commit] Generate interface index & deprecations"
if command -v python3 >/dev/null 2>&1; then
  python3 scripts/interfaces_generate_index.py || true
  python3 scripts/generate_deprecations.py || true
  python3 scripts/ensure_openapi_descriptions.py || true
  python3 scripts/apply_operation_docs.py || true
  python3 scripts/generate_openapi_json.py || true
  if ! git diff --quiet -- interfaces/index.yaml; then
    echo "::error::interfaces/index.yaml changed; commit the updated index"; git --no-pager diff -- interfaces/index.yaml | sed -n '1,120p'; exit 1; fi
  if ! git diff --quiet -- docs/reference/deprecations.md; then
    echo "::error::deprecations doc changed; commit the update"; git --no-pager diff -- docs/reference/deprecations.md | sed -n '1,120p'; exit 1; fi
  if ! git diff --quiet -- spec/openapi.yaml; then
    echo "::error::spec/openapi.yaml updated (curated summaries/descriptions or placeholders changed); commit the regeneration"; git --no-pager diff -- spec/openapi.yaml | sed -n '1,160p'; exit 1; fi
  if ! git diff --quiet -- docs/static/openapi.json; then
    echo "::error::docs/static/openapi.json changed; commit the regenerated file"; git --no-pager diff -- docs/static/openapi.json | sed -n '1,160p'; exit 1; fi
else
  echo "[pre-commit] python3 unavailable; skipping index/deprecations generation"
fi
echo "[pre-commit] Spectral lint (OpenAPI/AsyncAPI)"
if command -v npx >/dev/null 2>&1; then
  if git diff --cached --name-only | grep -E '^(spec/|quality/openapi-spectral.yaml)' >/dev/null 2>&1; then
    npx --yes @stoplight/spectral-cli lint -r quality/openapi-spectral.yaml spec/openapi.yaml || exit 1
    npx --yes @stoplight/spectral-cli lint -r quality/openapi-spectral.yaml spec/asyncapi.yaml || exit 1
  else
    echo "[pre-commit] no spec changes; skipping spectral"
  fi
else
  echo "[pre-commit] npx unavailable; skipping spectral"
fi
if command -v cargo-nextest >/dev/null 2>&1; then
  echo "[pre-commit] cargo nextest run"
  cargo nextest run --workspace
else
  echo "[pre-commit] cargo test"
  cargo test --workspace --locked
fi

# Docs: always validate registry-generated docs are up-to-date when
# registry or generator files change (drift guard), otherwise only when docs changed
if git diff --cached --name-only | grep -E '^(interfaces/|scripts/gen_|scripts/check_system_components_integrity.py|scripts/check_feature_integrity.py)' >/dev/null 2>&1; then
  echo "[pre-commit] Registry or generators changed — regenerating and verifying docs"
  if command -v python3 >/dev/null 2>&1; then
    python3 scripts/gen_feature_matrix.py
    python3 scripts/gen_feature_catalog.py
    python3 scripts/gen_system_components.py
  fi
  if ! git diff --quiet -- docs/reference/feature_matrix.md docs/reference/feature_catalog.md docs/reference/system_components.md; then
    echo "::error::Generated docs changed; commit the regenerated files" >&2
    git --no-pager diff -- docs/reference/feature_matrix.md | sed -n '1,80p'
    git --no-pager diff -- docs/reference/feature_catalog.md | sed -n '1,80p'
    git --no-pager diff -- docs/reference/system_components.md | sed -n '1,80p'
    exit 1
  fi
fi

if [[ -z "${ARW_SKIP_DOC_STAMP:-}" ]]; then
  if git diff --cached --name-only | grep -E '^(docs/|mkdocs.yml)' >/dev/null 2>&1; then
    echo "[pre-commit] Docs changed — stamping metadata and building"
    if command -v python3 >/dev/null 2>&1; then
      python3 scripts/stamp_docs_updated.py || true
      python3 scripts/stamp_docs_type.py || true
      git add docs/**/*.md 2>/dev/null || true
    fi
    if command -v python3 >/dev/null 2>&1; then
      python3 scripts/docs_check.py
    elif command -v python >/dev/null 2>&1; then
      python scripts/docs_check.py
    elif command -v mkdocs >/dev/null 2>&1; then
      mkdocs build --strict -f mkdocs.yml
    else
      echo "[pre-commit] docs_check.py skipped (missing Python/mkdocs)"
    fi
  fi
else
  echo "[pre-commit] Doc stamp skipped (ARW_SKIP_DOC_STAMP set)"
fi
EOF
chmod +x .git/hooks/pre-commit
echo "[hooks] Installed .git/hooks/pre-commit"

# Pre-push: heavy interface checks (OpenAPI sync, optional diffs)
cat > .git/hooks/pre-push << 'EOF'
#!/usr/bin/env bash
set -euo pipefail
echo "[pre-push] OpenAPI codegen sync check"
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
(
  # Ensure codegen runs from the repo root so Cargo and relative paths resolve
  ROOT=$(git rev-parse --show-toplevel 2>/dev/null || pwd)
  cd "$ROOT"
  SERVER_BIN="$ROOT/target/release/arw-server"
  if [[ "$OS" == "Windows_NT" ]]; then
    SERVER_BIN="${SERVER_BIN}.exe"
  fi
  if [[ ! -x "$SERVER_BIN" ]]; then
    echo "[pre-push] building arw-server (release)" >&2
    if ! cargo build --release -p arw-server >/tmp/arw-prepush-build.log 2>&1; then
      sed 's/^/[build] /' /tmp/arw-prepush-build.log >&2 || true
      exit 1
    fi
    rm -f /tmp/arw-prepush-build.log || true
  fi
  OPENAPI_OUT="$tmp/codegen.yaml" "$SERVER_BIN"
)
if [[ ! -s "$tmp/codegen.yaml" ]]; then
  echo "::error::Failed to generate OpenAPI via arw-server (OPENAPI_OUT)" >&2
  exit 1
fi
py=$(command -v python3 || true)
if [[ -n "$py" ]]; then
  # Merge curated fields into codegen and validate path parity
  "$py" scripts/openapi_overlay.py "$tmp/codegen.yaml" spec/openapi.yaml "$tmp/merged.yaml" || {
    echo "::error::OpenAPI path parity mismatch (see error above)" >&2
    exit 1
  }
  # Normalize key order before diff to avoid spurious ordering diffs
  "$py" - << 'PY' spec/openapi.yaml > "$tmp/spec.norm.yaml"
import sys, yaml
print(yaml.safe_dump(yaml.safe_load(open(sys.argv[1])), sort_keys=True), end='')
PY
  "$py" - << 'PY' "$tmp/merged.yaml" > "$tmp/merged.norm.yaml"
import sys, yaml
print(yaml.safe_dump(yaml.safe_load(open(sys.argv[1])), sort_keys=True), end='')
PY
  if ! diff -u "$tmp/spec.norm.yaml" "$tmp/merged.norm.yaml" >/dev/null; then
    echo "::error::OpenAPI spec out of sync with code-generated output" >&2
    diff -u "$tmp/spec.norm.yaml" "$tmp/merged.norm.yaml" | sed -n '1,200p'
    exit 1
  fi
else
  echo "::warning::python3 not found; skipping overlay merge and raw-diffing codegen vs spec"
  if ! diff -u spec/openapi.yaml "$tmp/codegen.yaml" >/dev/null; then
    echo "::error::OpenAPI spec out of sync with code-generated output (raw)" >&2
    diff -u spec/openapi.yaml "$tmp/codegen.yaml" | sed -n '1,200p'
    exit 1
  fi
fi
echo "[pre-push] OpenAPI sync OK"

# Optional: AsyncAPI diff (best-effort)
if command -v npx >/dev/null 2>&1; then
  base_ref=${BASE_REF:-origin/main}
  if git show "$base_ref:spec/asyncapi.yaml" > "$tmp/a_base.yaml" 2>/dev/null; then
    cp spec/asyncapi.yaml "$tmp/a_head.yaml"
    # Use AsyncAPI CLI for diff output in markdown format
    npx --yes @asyncapi/cli diff "$tmp/a_base.yaml" "$tmp/a_head.yaml" -f md | sed -n '1,200p' || true
  fi
fi

# Spectral lint (always on pre-push)
echo "[pre-push] Spectral lint OpenAPI/AsyncAPI"
if command -v npx >/dev/null 2>&1; then
  # Lint repo spec
  npx --yes @stoplight/spectral-cli@6 lint -r quality/openapi-spectral.yaml spec/openapi.yaml || exit 1
  # AsyncAPI is kept minimal; treat errors as warnings for now
  base_ref=${BASE_REF:-origin/main}
  if git diff --quiet "$base_ref"..HEAD -- spec/asyncapi.yaml 2>/dev/null; then
    echo "[pre-push] no AsyncAPI changes; skipping spectral"
  else
    npx --yes @stoplight/spectral-cli@6 lint -r quality/openapi-spectral.yaml --fail-severity=warn spec/asyncapi.yaml || true
    # Tiny build step: extract channel keys and lint with a simple rule
    if command -v python3 >/dev/null 2>&1; then
      chan_json="$tmp/asyncapi.channels.json"
      python3 scripts/extract_asyncapi_channels.py "$chan_json" || {
        echo "::error::failed to extract AsyncAPI channels" >&2; exit 1; }
      npx --yes @stoplight/spectral-cli@6 lint -r quality/asyncapi-channels-spectral.yaml "$chan_json" || exit 1
    else
      echo "::warning::python3 not found; skipping asyncapi channel key lint"
    fi
  fi
  # Lint merged OpenAPI (codegen + curated overlay) for style parity
  npx --yes @stoplight/spectral-cli@6 lint -r quality/openapi-spectral.yaml "$tmp/merged.yaml" || exit 1
else
  echo "[pre-push] npx unavailable; skipping spectral"
fi

EOF
chmod +x .git/hooks/pre-push
echo "[hooks] Installed .git/hooks/pre-push"
