#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

# Ensure cargo is on PATH; when invoked under bare bash shells (e.g., CI or Windows),
# sourcing the Rust env file avoids "cargo: command not found" failures during verify.
if [[ -f "$HOME/.cargo/env" ]]; then
  # shellcheck disable=SC1090
  source "$HOME/.cargo/env"
elif [[ -n "${USERPROFILE:-}" && -f "$USERPROFILE/.cargo/env" ]]; then
  # shellcheck disable=SC1090
  source "$USERPROFILE/.cargo/env"
fi

source "$REPO_ROOT/scripts/lib/env_mode.sh"
arw_env_init
# Preflight: emit environment mode summary for users and agents
echo "[env] mode=${ARW_ENV_MODE} source=${ARW_ENV_SOURCE:-unknown} exe=${ARW_EXE_SUFFIX:-}"

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
  docs-check       Run docs checks (docgen + docs_check.py).
  docs-cache       Build offline docs wheel cache (writes dist/docs-wheels.tar.gz).
  verify           Run fmt → clippy → tests → docs guardrail sequence.
                   Flags: --fast (skip docs/UI), --with-launcher (include Tauri crate), --ci (CI parity: registries, docgens --check, env-guard, smokes)
                   Tip: new to the repo? Start with --fast and follow docs/guide/quick_smoke.md.
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
  local fast=0
  local skip_docs=0
  local skip_ui=0
  local skip_docs_python=0
  local include_launcher=0
  local require_docs=0
  local ci_mode=0
  local is_linux=0

  while [[ $# -gt 0 ]]; do
    case "$1" in
      --fast)
        fast=1
        skip_docs=1
        skip_ui=1
        skip_docs_python=1
        shift
        ;;
      --skip-docs)
        skip_docs=1
        shift
        ;;
      --skip-ui)
        skip_ui=1
        shift
        ;;
      --skip-doc-python)
        skip_docs_python=1
        shift
        ;;
      --with-launcher|--include-launcher)
        include_launcher=1
        shift
        ;;
      --ci)
        ci_mode=1
        shift
        ;;
      *)
        echo "[verify] Unknown option '$1'" >&2
        return 2
        ;;
    esac
  done

  case "$(uname -s 2>/dev/null)" in
    Linux|Linux-*) is_linux=1 ;;
  esac

  if [[ $ci_mode -eq 1 ]]; then
    if [[ $fast -eq 1 ]]; then
      echo "[verify] --ci overrides --fast; running full suite."
      fast=0
      skip_docs=0
      skip_ui=0
      skip_docs_python=0
    fi
    if [[ $skip_docs -eq 1 || $skip_ui -eq 1 || $skip_docs_python -eq 1 ]]; then
      echo "[verify] --ci ignores skip flags to align with CI coverage."
      skip_docs=0
      skip_ui=0
      skip_docs_python=0
    fi
    require_docs=1
  fi

  if [[ "${ARW_VERIFY_REQUIRE_DOCS:-}" =~ ^(1|true|yes)$ ]]; then
    require_docs=1
  fi

  set +e

  if [[ $fast -eq 1 ]]; then
    echo "[verify] fast mode enabled (skipping doc sync, docs lint, launcher UI tests)."
  fi

  if [[ $include_launcher -eq 1 || "${ARW_VERIFY_INCLUDE_LAUNCHER:-}" =~ ^(1|true|yes)$ ]]; then
    include_launcher=1
  fi
  if [[ $include_launcher -eq 1 ]]; then
    echo "[verify] including arw-launcher targets (per request)"
  else
    echo "[verify] skipping arw-launcher crate (headless default; pass --with-launcher or set ARW_VERIFY_INCLUDE_LAUNCHER=1 to include)"
  fi

  echo "[verify] cargo fmt --all -- --check"
  if ! cargo fmt --all -- --check; then ok=1; fi

  local clippy_args=(--workspace --all-targets)
  if [[ $include_launcher -ne 1 || $is_linux -eq 1 ]]; then
    clippy_args+=(--exclude arw-launcher)
  fi
  echo "[verify] cargo clippy ${clippy_args[*]} -- -D warnings"
  if ! cargo clippy "${clippy_args[@]}" -- -D warnings; then ok=1; fi
  if [[ $include_launcher -eq 1 && $is_linux -eq 1 ]]; then
    local launcher_clippy_args=(-p arw-launcher --all-targets --features launcher-linux-ui)
    echo "[verify] cargo clippy ${launcher_clippy_args[*]} -- -D warnings"
    if ! cargo clippy "${launcher_clippy_args[@]}" -- -D warnings; then ok=1; fi
  fi

  if command -v cargo-nextest >/dev/null 2>&1; then
    local nextest_args=(run --workspace --test-threads=1)
    if [[ $include_launcher -ne 1 || $is_linux -eq 1 ]]; then
      nextest_args+=(--exclude arw-launcher)
    fi
    echo "[verify] cargo nextest ${nextest_args[*]}"
    if ! cargo nextest "${nextest_args[@]}"; then ok=1; fi
    if [[ $include_launcher -eq 1 && $is_linux -eq 1 ]]; then
      local launcher_list_args=(list -p arw-launcher --features launcher-linux-ui --message-format json)
      local launcher_list_out=""
      if launcher_list_out="$(cargo nextest "${launcher_list_args[@]}" 2>&1)"; then
        if printf '%s' "$launcher_list_out" | grep -q '"type":"test"'; then
          local launcher_nextest_args=(run -p arw-launcher --test-threads=1 --features launcher-linux-ui)
          echo "[verify] cargo nextest ${launcher_nextest_args[*]}"
          if ! cargo nextest "${launcher_nextest_args[@]}"; then ok=1; fi
        else
          echo "[verify] no launcher tests registered; skipping cargo nextest -p arw-launcher"
        fi
      else
        if printf '%s' "$launcher_list_out" | grep -qi 'no library targets found'; then
          echo "[verify] no launcher testable targets found; skipping cargo nextest -p arw-launcher"
        else
          printf '%s\n' "$launcher_list_out"
          ok=1
        fi
      fi
    fi
  else
    echo "[verify] cargo-nextest not found; falling back to cargo test --workspace --locked"
    local test_args=(--workspace --locked)
    if [[ $include_launcher -ne 1 || $is_linux -eq 1 ]]; then
      test_args+=(--exclude arw-launcher)
    fi
    local test_trailer=(-- --test-threads=1)
    if ! cargo test "${test_args[@]}" "${test_trailer[@]}"; then ok=1; fi
    if [[ $include_launcher -eq 1 && $is_linux -eq 1 ]]; then
      local launcher_list_out=""
      if launcher_list_out="$(cargo test -p arw-launcher --features launcher-linux-ui -- --list 2>&1)"; then
        local launcher_test_args=(-p arw-launcher --features launcher-linux-ui -- --test-threads=1)
        echo "[verify] cargo test ${launcher_test_args[*]}"
        if ! cargo test "${launcher_test_args[@]}"; then ok=1; fi
      elif printf '%s' "$launcher_list_out" | grep -qi 'no library targets found'; then
        echo "[verify] no launcher testable targets found; skipping cargo test -p arw-launcher"
      else
        printf '%s\n' "$launcher_list_out"
        ok=1
      fi
    fi
  fi

  if [[ $skip_ui -eq 1 ]]; then
    echo "[verify] skipping launcher UI smoke (requested)"
  elif [[ $include_launcher -ne 1 ]]; then
    echo "[verify] launcher UI smoke skipped (headless default; pass --with-launcher to include)"
  else
    if command -v node >/dev/null 2>&1; then
      echo "[verify] node apps/arw-launcher/src-tauri/ui/read_store.test.js"
      if ! node "$REPO_ROOT/apps/arw-launcher/src-tauri/ui/read_store.test.js"; then ok=1; fi
      echo "[verify] node apps/arw-launcher/src-tauri/ui/persona_preview.test.js"
      if ! node "$REPO_ROOT/apps/arw-launcher/src-tauri/ui/persona_preview.test.js"; then ok=1; fi
    else
      local node_fallback="/c/Program Files/nodejs/node.exe"
      if [[ -x "$node_fallback" ]]; then
        echo "[verify] node fallback apps/arw-launcher/src-tauri/ui/read_store.test.js"
        if ! "$node_fallback" "$REPO_ROOT/apps/arw-launcher/src-tauri/ui/read_store.test.js"; then ok=1; fi
        echo "[verify] node fallback apps/arw-launcher/src-tauri/ui/persona_preview.test.js"
        if ! "$node_fallback" "$REPO_ROOT/apps/arw-launcher/src-tauri/ui/persona_preview.test.js"; then ok=1; fi
      else
        echo "[verify] launcher UI smoke skipped (Node.js 18+ not found; install Node.js or pass --skip-ui/--fast to suppress this notice)"
      fi
    fi
  fi

  # TypeScript client build (optional; skips in --fast)
  if [[ $fast -eq 1 ]]; then
    echo "[verify] skipping TypeScript client build (--fast)"
  else
    if command -v node >/dev/null 2>&1 && command -v npm >/dev/null 2>&1; then
      echo "[verify] building TypeScript client (clients/typescript)"
      pushd "$REPO_ROOT/clients/typescript" >/dev/null || true
      # Prefer clean, fall back to install when lock is absent
      if [[ -f package-lock.json ]]; then
        if ! npm ci --no-audit --no-fund; then ok=1; fi
      else
        if ! npm install --no-audit --no-fund; then ok=1; fi
      fi
      if ! npm run build; then ok=1; fi
      popd >/dev/null || true
    else
      echo "[verify] TypeScript client build skipped (Node.js/npm not found)"
    fi
  fi

  # Optional: adapters lint (opt-in via env)
  if [[ "${ARW_VERIFY_INCLUDE_ADAPTERS:-}" =~ ^(1|true|yes)$ ]]; then
    echo "[verify] adapters lint (ARW_VERIFY_INCLUDE_ADAPTERS=1)"
    if ! bash "$REPO_ROOT/scripts/lint_adapters.sh"; then ok=1; fi
  fi

  PYTHON="$(command -v python3 || command -v python || true)"
  if [[ -n "$PYTHON" ]]; then
    local run_doc_sync=1
    if [[ $skip_docs_python -eq 1 ]]; then
      echo "[verify] skipping doc sync checks (requested)"
      run_doc_sync=0
    fi
    if [[ $run_doc_sync -eq 1 ]]; then
      if "$PYTHON" - <<'PY' >/dev/null 2>&1
import importlib.util, sys
sys.exit(0 if importlib.util.find_spec("yaml") else 1)
PY
      then
        :
      else
        if [[ $require_docs -eq 1 ]]; then
          echo "[verify] doc sync blocked (PyYAML missing; install with 'python3 -m pip install --user --break-system-packages pyyaml' or pass --skip-doc-python/--fast)"
          ok=1
        else
          echo "[verify] PyYAML missing; skipping doc sync checks (set ARW_VERIFY_REQUIRE_DOCS=1 to require doc tooling)"
        fi
        run_doc_sync=0
      fi
    fi

    if [[ $run_doc_sync -eq 1 ]]; then
      echo "[verify] python check_operation_docs_sync.py"
      if ! "$PYTHON" "$REPO_ROOT/scripts/check_operation_docs_sync.py"; then ok=1; fi

      echo "[verify] python scripts/check_tasks_sync.py"
      if ! "$PYTHON" "$REPO_ROOT/scripts/check_tasks_sync.py"; then ok=1; fi

      echo "[verify] python scripts/gen_topics_doc.py --check"
      if ! "$PYTHON" "$REPO_ROOT/scripts/gen_topics_doc.py" --check; then ok=1; fi
    fi

    echo "[verify] python scripts/lint_event_names.py"
    if ! "$PYTHON" "$REPO_ROOT/scripts/lint_event_names.py"; then ok=1; fi
  else
    if [[ $require_docs -eq 1 ]]; then
      echo "[verify] doc sync blocked (python not found; install Python 3.11+ or pass --skip-doc-python/--fast)"
      ok=1
    else
      echo "[verify] python not found; skipping doc sync + event lint (set ARW_VERIFY_REQUIRE_DOCS=1 to require doc tooling)"
    fi
  fi

  if [[ $skip_docs -eq 1 ]]; then
    echo "[verify] skipping docs lint (requested)"
  else
    if [[ -f "$REPO_ROOT/scripts/docs_check.py" ]]; then
      local py_bin=""
      if command -v python3 >/dev/null 2>&1; then
        py_bin="python3"
      elif command -v python >/dev/null 2>&1; then
        py_bin="python"
      fi
      if [[ -n "$py_bin" ]]; then
        echo "[verify] $py_bin scripts/docs_check.py"
        if ! "$py_bin" "$REPO_ROOT/scripts/docs_check.py"; then ok=1; fi
      elif command -v mkdocs >/dev/null 2>&1; then
        echo "[verify] python missing; running mkdocs build --strict"
        if ! mkdocs build --strict -f "$REPO_ROOT/mkdocs.yml"; then ok=1; fi
      else
        echo "[verify] docs lint blocked (missing Python and mkdocs; install the docs toolchain or pass --skip-docs/--fast)"
        ok=1
      fi
    elif command -v mkdocs >/dev/null 2>&1; then
      echo "[verify] docs_check.py unavailable; running mkdocs build --strict"
      if ! mkdocs build --strict -f "$REPO_ROOT/mkdocs.yml"; then ok=1; fi
    else
      echo "[verify] docs lint blocked (missing scripts/docs_check.py and mkdocs; install the docs toolchain or pass --skip-docs/--fast)"
      ok=1
    fi
  fi

  if [[ $ci_mode -eq 1 ]]; then
    echo "[verify] CI mode enabled (running extended guardrails)."
    if [[ -z "$PYTHON" ]]; then
      echo "[verify] CI checks require python 3.11+ on PATH"
      ok=1
    else
      echo "[verify] python scripts/check_feature_integrity.py"
      if ! "$PYTHON" "$REPO_ROOT/scripts/check_feature_integrity.py"; then ok=1; fi

      echo "[verify] python scripts/check_system_components_integrity.py"
      if ! "$PYTHON" "$REPO_ROOT/scripts/check_system_components_integrity.py"; then ok=1; fi

      local regen_scripts=(
        "scripts/gen_feature_matrix.py"
        "scripts/gen_feature_catalog.py"
        "scripts/gen_system_components.py"
      )
      local script_path
      for script_path in "${regen_scripts[@]}"; do
        echo "[verify] python ${script_path} --check"
        if ! "$PYTHON" "$REPO_ROOT/${script_path}" --check; then ok=1; fi
      done
    fi

    echo "[verify] ENFORCE_ENV_GUARD=1 bash scripts/check_env_guard.sh"
    if ! ENFORCE_ENV_GUARD=1 bash "$REPO_ROOT/scripts/check_env_guard.sh"; then ok=1; fi

    local py_bin=""
    if command -v python3 >/dev/null 2>&1; then
      py_bin="python3"
    elif command -v python >/dev/null 2>&1; then
      py_bin="python"
    fi
    if [[ -z "$py_bin" ]]; then
      echo "[verify] python is required to run ci_snappy_bench" >&2
      ok=1
    else
      echo "[verify] $py_bin scripts/ci_snappy_bench.py"
      if ! "$py_bin" "$REPO_ROOT/scripts/ci_snappy_bench.py"; then ok=1; fi
    fi

    local triad_timeout="${TRIAD_SMOKE_TIMEOUT_SECS:-90}"
    echo "[verify] TRIAD_SMOKE_TIMEOUT_SECS=${triad_timeout} bash scripts/triad_smoke.sh"
    if [[ "${ARW_ENV_MODE:-}" == "windows-host" || "${ARW_ENV_MODE:-}" == "windows-wsl" ]]; then
      export RUNTIME_SMOKE_USE_RELEASE="${RUNTIME_SMOKE_USE_RELEASE:-1}"
    fi
    if ! TRIAD_SMOKE_TIMEOUT_SECS="$triad_timeout" bash "$REPO_ROOT/scripts/triad_smoke.sh"; then ok=1; fi

    local ctx_py=""
    if command -v python3 >/dev/null 2>&1; then
      ctx_py="python3"
    elif command -v python >/dev/null 2>&1; then
      ctx_py="python"
    fi
    if [[ -z "$ctx_py" ]]; then
      echo "[verify] python is required to run context_ci" >&2
      ok=1
    else
      echo "[verify] $ctx_py scripts/context_ci.py"
      if ! "$ctx_py" "$REPO_ROOT/scripts/context_ci.py"; then ok=1; fi
    fi

    echo "[verify] RUNTIME_SMOKE_GPU_POLICY=${RUNTIME_SMOKE_GPU_POLICY:-simulate} bash scripts/runtime_smoke_suite.sh"
    if ! RUNTIME_SMOKE_GPU_POLICY="${RUNTIME_SMOKE_GPU_POLICY:-simulate}" bash "$REPO_ROOT/scripts/runtime_smoke_suite.sh"; then ok=1; fi

    echo "[verify] ARW_SERVER_BIN=$REPO_ROOT/target/debug/arw-server bash scripts/runtime_vision_smoke.sh"
    if ! ARW_SERVER_BIN="$REPO_ROOT/target/debug/arw-server" bash "$REPO_ROOT/scripts/runtime_vision_smoke.sh"; then ok=1; fi

    echo "[verify] ARW_LEGACY_CHECK_WAIT_SECS=30 bash scripts/check_legacy_surface.sh"
    if ! ARW_LEGACY_CHECK_WAIT_SECS=30 bash "$REPO_ROOT/scripts/check_legacy_surface.sh"; then ok=1; fi

    echo "[verify] bash scripts/verify_bundle_signatures.sh"
    if ! bash "$REPO_ROOT/scripts/verify_bundle_signatures.sh"; then ok=1; fi
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
    env ARW_DOCGEN_SKIP_BUILDS=1 ARW_SETUP_AGENT=1 ARW_BUILD_MODE=debug bash "$SCRIPT_DIR/setup.sh" --yes --headless --minimal --no-docs --skip-cli "$@"
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
      cargo nextest run --workspace --test-threads=1 "$@"
    else
      echo "[dev] cargo-nextest not found; running cargo test --workspace --locked"
      cargo test --workspace --locked "$@" -- --test-threads=1
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
    if [[ -f "$REPO_ROOT/scripts/docs_check.py" ]]; then
      if command -v python3 >/dev/null 2>&1; then
        python3 "$REPO_ROOT/scripts/docs_check.py" "$@"
      elif command -v python >/dev/null 2>&1; then
        python "$REPO_ROOT/scripts/docs_check.py" "$@"
      elif command -v mkdocs >/dev/null 2>&1; then
        echo "[dev] Python unavailable; running mkdocs build --strict instead"
        mkdocs build --strict -f "$REPO_ROOT/mkdocs.yml"
      else
        echo "[dev] skipping docs checks (missing docs_check.py & mkdocs)"
      fi
    elif command -v mkdocs >/dev/null 2>&1; then
      echo "[dev] docs_check.py unavailable; running mkdocs build --strict instead"
      mkdocs build --strict -f "$REPO_ROOT/mkdocs.yml"
    else
      echo "[dev] skipping docs checks (missing docs_check.py & mkdocs)"
    fi
    ;;
  docs-cache)
    bash "$SCRIPT_DIR/build_docs_wheels.sh" --archive "$REPO_ROOT/dist/docs-wheels.tar.gz" "$@"
    ;;
  verify)
    run_verify "$@"
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
