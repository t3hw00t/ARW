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
echo "[pre-commit] Generate interface index & deprecations"
if command -v python3 >/dev/null 2>&1; then
  python3 scripts/interfaces_generate_index.py || true
  python3 scripts/generate_deprecations.py || true
  python3 scripts/ensure_openapi_descriptions.py || true
  python3 scripts/generate_openapi_json.py || true
  if ! git diff --quiet -- interfaces/index.yaml; then
    echo "::error::interfaces/index.yaml changed; commit the updated index"; git --no-pager diff -- interfaces/index.yaml | sed -n '1,120p'; exit 1; fi
  if ! git diff --quiet -- docs/reference/deprecations.md; then
    echo "::error::deprecations doc changed; commit the update"; git --no-pager diff -- docs/reference/deprecations.md | sed -n '1,120p'; exit 1; fi
  if ! git diff --quiet -- spec/openapi.yaml; then
    echo "::error::spec/openapi.yaml updated with placeholder descriptions/tags; commit the changes"; git --no-pager diff -- spec/openapi.yaml | sed -n '1,160p'; exit 1; fi
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
  OPENAPI_OUT="$tmp/openapi.yaml" cargo run -p arw-svc --bin arw-svc --quiet || true
)
if [[ ! -s "$tmp/openapi.yaml" ]]; then
  echo "::error::Failed to generate OpenAPI via arw-svc (OPENAPI_OUT)" >&2
  exit 1
fi
py=$(command -v python3 || true)
if [[ -n "$py" ]]; then
  "$py" - "$tmp/openapi.yaml" << 'PY' > "$tmp/spec.norm.yaml"
import sys, yaml
gen = yaml.safe_load(open(sys.argv[1]))
def strip_some(obj):
  if isinstance(obj, dict):
    # remove noisy differences not owned by codegen
    for k in ['deprecated','description','summary','x-sunset','info','tags','components']:
      obj.pop(k, None)
    return {k: strip_some(v) for k,v in obj.items()}
  if isinstance(obj, list):
    return [strip_some(x) for x in obj]
  return obj
cur = strip_some(yaml.safe_load(open('spec/openapi.yaml')))
gen = strip_some(gen)
yaml.safe_dump(cur, sys.stdout, sort_keys=True)
PY
  "$py" - "$tmp/openapi.yaml" << 'PY' > "$tmp/gen.norm.yaml"
import sys, yaml
def strip_some(obj):
  if isinstance(obj, dict):
    for k in ['deprecated','description','summary','x-sunset','info','tags','components']:
      obj.pop(k, None)
    return {k: strip_some(v) for k,v in obj.items()}
  if isinstance(obj, list):
    return [strip_some(x) for x in obj]
  return obj
gen = strip_some(yaml.safe_load(open(sys.argv[1])))
yaml.safe_dump(gen, sys.stdout, sort_keys=True)
PY
  if ! diff -u "$tmp/spec.norm.yaml" "$tmp/gen.norm.yaml" >/dev/null; then
    echo "::error::OpenAPI spec out of sync with code-generated output" >&2
    diff -u "$tmp/spec.norm.yaml" "$tmp/gen.norm.yaml" | sed -n '1,200p'
    exit 1
  fi
else
  echo "[pre-push] python3 not found; performing raw diff"
  if ! diff -u spec/openapi.yaml "$tmp/openapi.yaml" >/dev/null; then
    echo "::error::OpenAPI spec out of sync with code-generated output" >&2
    diff -u spec/openapi.yaml "$tmp/openapi.yaml" | sed -n '1,200p'
    exit 1
  fi
fi
echo "[pre-push] OpenAPI sync OK"

# Optional: AsyncAPI diff (best-effort)
if command -v npx >/dev/null 2>&1; then
  base_ref=${BASE_REF:-origin/main}
  if git show "$base_ref:spec/asyncapi.yaml" > "$tmp/a_base.yaml" 2>/dev/null; then
    cp spec/asyncapi.yaml "$tmp/a_head.yaml"
    npx --yes @asyncapi/diff "$tmp/a_base.yaml" "$tmp/a_head.yaml" --markdown | sed -n '1,200p' || true
  fi
fi

# Spectral lint (always on pre-push)
echo "[pre-push] Spectral lint OpenAPI/AsyncAPI"
if command -v npx >/dev/null 2>&1; then
  # Lint repo spec
  npx --yes @stoplight/spectral-cli lint -r quality/openapi-spectral.yaml spec/openapi.yaml || exit 1
  npx --yes @stoplight/spectral-cli lint -r quality/openapi-spectral.yaml spec/asyncapi.yaml || exit 1
  # Lint code-generated OpenAPI as well (style parity)
  npx --yes @stoplight/spectral-cli lint -r quality/openapi-spectral.yaml "$tmp/openapi.yaml" || exit 1
else
  echo "[pre-push] npx unavailable; skipping spectral"
fi

# Generate interface release notes and warn if changed
echo "[pre-push] Generating Interface Release Notes"
if command -v python3 >/dev/null 2>&1; then
  BASE_REF=${BASE_REF:-origin/main} python3 scripts/generate_interface_release_notes.py || true
  if ! git diff --quiet -- docs/reference/interface-release-notes.md; then
    echo "::warning::interface-release-notes changed; commit if you want it in this push"
    git --no-pager diff -- docs/reference/interface-release-notes.md | sed -n '1,160p'
  fi
else
  echo "[pre-push] python3 unavailable; skipping release notes generation"
fi
EOF
chmod +x .git/hooks/pre-push
echo "[hooks] Installed .git/hooks/pre-push"
