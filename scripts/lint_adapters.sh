#!/usr/bin/env bash
set -euo pipefail

DIR="${ADAPTERS_DIR:-adapters}"
STRICT="${ADAPTERS_LINT_STRICT_WARNINGS:-0}"
FILES_ENV="${ADAPTERS_FILES:-}"

# Ensure cargo is available (Linux/macOS/Windows bash)
if [[ -f "$HOME/.cargo/env" ]]; then
  # shellcheck disable=SC1090
  source "$HOME/.cargo/env"
elif [[ -n "${USERPROFILE:-}" && -f "$USERPROFILE/.cargo/env" ]]; then
  # shellcheck disable=SC1090
  source "$USERPROFILE/.cargo/env"
fi
if command -v cargo >/dev/null 2>&1; then
  CARGO="cargo"
elif [[ -x "$HOME/.cargo/bin/cargo" ]]; then
  CARGO="$HOME/.cargo/bin/cargo"
elif [[ -n "${USERPROFILE:-}" && -x "$USERPROFILE/.cargo/bin/cargo.exe" ]]; then
  CARGO="$USERPROFILE/.cargo/bin/cargo.exe"
else
  echo "[adapters-lint] cargo not found in PATH or expected locations" >&2
  exit 127
fi

if [[ ! -d "$DIR" ]]; then
  echo "[adapters-lint] No directory '$DIR' found; skipping."
  exit 0
fi

FILES=()
if [[ -n "$FILES_ENV" ]]; then
  # Use newline or space separated list from env and filter existing files only
  while IFS= read -r f; do
    [[ -n "$f" && -f "$f" ]] && FILES+=("$f")
  done < <(printf '%s\n' $FILES_ENV)
  if [[ ${#FILES[@]} -eq 0 ]]; then
    echo "[adapters-lint] No changed adapter manifests to lint; skipping."
    exit 0
  fi
  echo "[adapters-lint] Validating ${#FILES[@]} changed manifest(s)..."
else
  mapfile -t FILES < <(find "$DIR" -type f \( -name "*.json" -o -name "*.toml" \) | sort)
  if [[ ${#FILES[@]} -eq 0 ]]; then
    echo "[adapters-lint] No manifests found under '$DIR'; skipping."
    exit 0
  fi
  echo "[adapters-lint] Validating ${#FILES[@]} manifest(s) under '$DIR'..."
fi
FAILED=0
for f in "${FILES[@]}"; do
  echo "- $f"
  set +e
  if [[ "$STRICT" == "1" ]]; then
    "$CARGO" run -q -p arw-cli -- adapters validate --manifest "$f" --strict-warnings >/dev/null
  else
    "$CARGO" run -q -p arw-cli -- adapters validate --manifest "$f" >/dev/null
  fi
  code=$?
  set -e
  if [[ $code -ne 0 ]]; then
    echo "  -> FAIL ($code)"
    FAILED=1
  else
    echo "  -> OK"
  fi
done

if [[ $FAILED -ne 0 ]]; then
  echo "[adapters-lint] One or more manifests failed validation."
  exit 1
fi

echo "[adapters-lint] All manifests validated successfully."
exit 0
