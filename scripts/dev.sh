#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

command="${1:-help}"
if [[ $# -gt 0 ]]; then
  shift
fi

show_help() {
  cat <<'EOF'
ARW Dev Utility (scripts/dev.sh)

Usage:
  bash scripts/dev.sh <command> [options]

Commands:
  help             Show this message.
  setup            Run repo setup (adds --headless by default).
  setup-agent      Headless/minimal setup tuned for autonomous agents.
  build            Build workspace (headless by default).
  build-launcher   Build workspace including the launcher.
  clean            Remove cargo/venv artifacts via scripts/clean.sh.
  fmt              Run cargo fmt --all.
  fmt-check        Run cargo fmt --all -- --check.
  lint             Run cargo clippy with -D warnings.
  lint-fix         Run cargo clippy --fix (best-effort).
  lint-events      Run event-name linter (python).
  test             Run workspace tests (prefers cargo-nextest).
  test-fast        Alias for cargo nextest run --workspace.
  docs             Regenerate docs (docgen + mkdocs build --strict when available).
  docs-check       Run docs checks (docgen + docs_check.sh if bash available).
  verify           Run fmt → clippy → tests → docs guardrail sequence.
  hooks            Install git hooks (delegates to scripts/hooks/install_hooks.sh).
  status           Generate workspace status page (docgen).

Additional options after the command are forwarded to the underlying script.
EOF
}

to_switch() {
  local value="${1#/}"
  value="${value#-}"
  value="${value#-}"
  value="${value%%=*}"
  printf '%s' "$value" | tr '[:upper:]' '[:lower:]'
}

has_switch() {
  local target="$1"
  shift || true
  local arg
  for arg in "$@"; do
    if [[ "$(to_switch "$arg")" == "$target" ]]; then
      return 0
    fi
  done
  return 1
}

ensure_tool() {
  local tool="$1"
  if ! command -v "$tool" >/dev/null 2>&1; then
    echo "[dev] '$tool' not found in PATH" >&2
    exit 1
  fi
}

run_verify() {
  local ok=0
  set +e

  echo "[verify] cargo fmt --all -- --check"
  if ! cargo fmt --all -- --check; then ok=1; fi

  echo "[verify] cargo clippy --workspace --all-targets -- -D warnings"
  if ! cargo clippy --workspace --all-targets -- -D warnings; then ok=1; fi

  if command -v cargo-nextest >/dev/null 2>&1; then
    echo "[verify] cargo nextest run --workspace"
    if ! cargo nextest run --workspace; then ok=1; fi
  else
    echo "[verify] cargo-nextest not found; falling back to cargo test --workspace --locked"
    if ! cargo test --workspace --locked; then ok=1; fi
  fi

  if command -v node >/dev/null 2>&1; then
    echo "[verify] node apps/arw-launcher/src-tauri/ui/read_store.test.js"
    if ! node "$REPO_ROOT/apps/arw-launcher/src-tauri/ui/read_store.test.js"; then ok=1; fi
  else
    local node_fallback="/c/Program Files/nodejs/node.exe"
    if [[ -x "$node_fallback" ]]; then
      echo "[verify] node fallback apps/arw-launcher/src-tauri/ui/read_store.test.js"
      if ! "$node_fallback" "$REPO_ROOT/apps/arw-launcher/src-tauri/ui/read_store.test.js"; then ok=1; fi
    else
      echo "[verify] skipping node UI tests (node not found)"
    fi
  fi

  PYTHON="$(command -v python3 || command -v python || true)"
  if [[ -n "$PYTHON" ]]; then
    if "$PYTHON" - <<'PY' >/dev/null 2>&1
import importlib.util, sys
sys.exit(0 if importlib.util.find_spec("yaml") else 1)
PY
    then
      echo "[verify] python check_operation_docs_sync.py"
      if ! "$PYTHON" "$REPO_ROOT/scripts/check_operation_docs_sync.py"; then ok=1; fi
    else
      echo "[verify] skipping operation docs sync check (PyYAML missing; run 'python3 -m pip install --user --break-system-packages pyyaml')"
    fi

    echo "[verify] python scripts/gen_topics_doc.py --check"
    if ! "$PYTHON" "$REPO_ROOT/scripts/gen_topics_doc.py" --check; then ok=1; fi

    echo "[verify] python scripts/lint_event_names.py"
    if ! "$PYTHON" "$REPO_ROOT/scripts/lint_event_names.py"; then ok=1; fi
  else
    echo "[verify] python not found; skipping python-based checks"
  fi

  if [[ -x "$REPO_ROOT/scripts/docs_check.sh" ]]; then
    echo "[verify] bash scripts/docs_check.sh"
    if ! bash "$REPO_ROOT/scripts/docs_check.sh"; then ok=1; fi
  elif command -v mkdocs >/dev/null 2>&1; then
    echo "[verify] docs_check.sh unavailable; running mkdocs build --strict"
    if ! mkdocs build --strict -f "$REPO_ROOT/mkdocs.yml"; then ok=1; fi
  else
    echo "[verify] skipping docs lint (docs_check.sh & mkdocs missing)"
  fi

  set -e
  if [[ $ok -ne 0 ]]; then
    echo "[verify] One or more checks failed" >&2
    return 1
  fi
  return 0
}

case "$command" in
  help)
    show_help
    ;;
  setup)
    args=("$@")
    if ! has_switch "headless" "${args[@]}" && ! has_switch "withlauncher" "${args[@]}"; then
      args=(--headless "${args[@]}")
    fi
    if ! has_switch "yes" "${args[@]}"; then
      args=(--yes "${args[@]}")
    fi
    bash "$SCRIPT_DIR/setup.sh" "${args[@]}"
    ;;
  setup-agent)
    env ARW_DOCGEN_SKIP_BUILDS=1 ARW_SETUP_AGENT=1 ARW_BUILD_MODE=debug bash "$SCRIPT_DIR/setup.sh" --yes --headless --minimal --no-docs "$@"
    ;;
  build)
    args=("$@")
    if ! has_switch "headless" "${args[@]}" && ! has_switch "withlauncher" "${args[@]}"; then
      args=(--headless "${args[@]}")
    fi
    bash "$SCRIPT_DIR/build.sh" "${args[@]}"
    ;;
  build-launcher)
    bash "$SCRIPT_DIR/build.sh" --with-launcher "$@"
    ;;
  clean)
    bash "$SCRIPT_DIR/clean.sh" "$@"
    ;;
  fmt)
    ensure_tool cargo
    cargo fmt --all "$@"
    ;;
  fmt-check)
    ensure_tool cargo
    cargo fmt --all -- --check "$@"
    ;;
  lint)
    ensure_tool cargo
    cargo clippy --workspace --all-targets -- -D warnings "$@"
    ;;
  lint-fix)
    ensure_tool cargo
    cargo clippy --workspace --all-targets --fix -Z unstable-options --allow-dirty --allow-staged "$@"
    ;;
  lint-events)
    PYTHON="$(command -v python3 || command -v python || true)"
    if [[ -z "$PYTHON" ]]; then
      echo "[dev] python not found; install Python 3.11+ to lint events" >&2
      exit 1
    fi
    "$PYTHON" "$REPO_ROOT/scripts/lint_event_names.py" "$@"
    ;;
  test)
    bash "$SCRIPT_DIR/test.sh" "$@"
    ;;
  test-fast)
    if command -v cargo-nextest >/dev/null 2>&1; then
      cargo nextest run --workspace "$@"
    else
      echo "[dev] cargo-nextest not found; running cargo test --workspace --locked"
      cargo test --workspace --locked "$@"
    fi
    ;;
  docs)
    bash "$SCRIPT_DIR/docgen.sh" "$@"
    if command -v mkdocs >/dev/null 2>&1; then
      mkdocs build --strict -f "$REPO_ROOT/mkdocs.yml"
    else
      echo "[dev] mkdocs not found; skipping mkdocs build. Install via 'pip install mkdocs-material'."
    fi
    ;;
  docs-check)
    bash "$SCRIPT_DIR/docgen.sh" "$@"
    if [[ -x "$REPO_ROOT/scripts/docs_check.sh" ]]; then
      bash "$REPO_ROOT/scripts/docs_check.sh"
    elif command -v mkdocs >/dev/null 2>&1; then
      echo "[dev] docs_check.sh unavailable; running mkdocs build --strict instead"
      mkdocs build --strict -f "$REPO_ROOT/mkdocs.yml"
    else
      echo "[dev] skipping docs checks (missing docs_check.sh & mkdocs)"
    fi
    ;;
  verify)
    run_verify
    ;;
  hooks)
    bash "$SCRIPT_DIR/hooks/install_hooks.sh" "$@"
    ;;
  status)
    bash "$SCRIPT_DIR/docgen.sh" "$@"
    ;;
  *)
    echo "[dev] Unknown command '$command'. Run 'bash scripts/dev.sh help' for usage." >&2
    exit 1
    ;;
esac
