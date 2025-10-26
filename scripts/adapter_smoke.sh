#!/usr/bin/env bash
set -euo pipefail

# Minimal adapter smoke harness.
# - Validates adapter manifests using arw-cli (strict warnings optional).
# - Optional: when ADAPTER_SMOKE_HEALTH=1, attempts basic health probes for manifests
#   that declare a metadata.upstream and health.status_endpoint (non-fatal by default).

PROJECT_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
DIR="${ADAPTERS_DIR:-${PROJECT_ROOT}/adapters}"
FILES_ENV="${ADAPTER_SMOKE_FILES:-}"
STRICT="${ADAPTER_SMOKE_STRICT_WARNINGS:-1}"
DO_HEALTH="${ADAPTER_SMOKE_HEALTH:-0}"

log() { printf '[adapters-smoke] %s\n' "$*"; }
is_truthy() { case "${1:-}" in 1|true|yes|on) return 0 ;; *) return 1 ;; esac; }

# Resolve cargo path (mirrors scripts/lint_adapters.sh behavior)
if [[ -f "$HOME/.cargo/env" ]]; then source "$HOME/.cargo/env"; fi
if [[ -n "${USERPROFILE:-}" && -f "$USERPROFILE/.cargo/env" ]]; then source "$USERPROFILE/.cargo/env"; fi
if command -v cargo >/dev/null 2>&1; then CARGO="cargo"
elif [[ -x "$HOME/.cargo/bin/cargo" ]]; then CARGO="$HOME/.cargo/bin/cargo"
elif [[ -n "${USERPROFILE:-}" && -x "$USERPROFILE/.cargo/bin/cargo.exe" ]]; then CARGO="$USERPROFILE/.cargo/bin/cargo.exe"
else
  echo "[adapters-smoke] cargo not found in PATH or expected locations" >&2
  exit 127
fi

if [[ ! -d "$DIR" ]]; then
  log "No adapters directory: $DIR (nothing to smoke)."
  exit 0
fi

collect_files() {
  if [[ -n "$FILES_ENV" ]]; then
    while IFS= read -r f; do [[ -n "$f" && -f "$f" ]] && printf '%s\n' "$f"; done < <(printf '%s\n' $FILES_ENV)
  else
    find "$DIR" -type f \( -name "*.json" -o -name "*.toml" \) | sort
  fi
}

files=( $(collect_files) )
if [[ ${#files[@]} -eq 0 ]]; then
  log "No adapter manifests found under '$DIR' — skipping."
  exit 0
fi

log "Validating ${#files[@]} manifest(s) (strict=${STRICT})."
for f in "${files[@]}"; do
  printf ' - %s\n' "$f"
  args=(run -q -p arw-cli -- adapters validate --manifest "$f")
  if is_truthy "$STRICT"; then args+=(--strict-warnings); fi
  set +e; "$CARGO" "${args[@]}" >/dev/null; code=$?; set -e
  if [[ $code -ne 0 ]]; then
    echo "   -> FAIL ($code)"; exit $code
  else
    echo "   -> OK"
  fi
done

if is_truthy "$DO_HEALTH"; then
  log "Health probes enabled (ADAPTER_SMOKE_HEALTH=1)."
  have_curl=0; command -v curl >/dev/null 2>&1 && have_curl=1
  if [[ $have_curl -ne 1 ]]; then
    log "curl not found; skipping health probes."; exit 0
  fi
  # Best-effort: probe (metadata.upstream + health.status_endpoint)
  for f in "${files[@]}"; do
    # parse JSON with Python to avoid jq dependency
    if [[ "${f##*.}" != "json" ]]; then continue; fi
    read -r base path < <(python3 - "$f" <<'PY'
import json,sys
path=sys.argv[1]
try:
  with open(path,'r',encoding='utf-8') as fh:
    data=json.load(fh)
except Exception:
  print('','',end='')
  sys.exit(0)
up = (data.get('metadata') or {}).get('upstream') or ''
se = (data.get('health') or {}).get('status_endpoint') or ''
print(up or '', se or '')
PY
    ) || true
    if [[ -n "$base" && -n "$path" ]]; then
      url="${base%/}${path}"
      log "Probing: $url"
      set +e; curl -fsS -m 2 "$url" >/dev/null; code=$?; set -e
      if [[ $code -ne 0 ]]; then
        log "Probe failed (non-fatal): $url"
      else
        log "Probe OK: $url"
      fi
    fi
  done
fi

log "Adapter smoke completed."
exit 0

