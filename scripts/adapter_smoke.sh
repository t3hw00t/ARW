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
OUT_PATH="${ADAPTER_SMOKE_OUT:-}"

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
json_rows=()
for f in "${files[@]}"; do
  printf ' - %s\n' "$f"
  args=(run -q -p arw-cli -- adapters validate --manifest "$f" --json)
  # measure validation time
  t0=$(date +%s%3N 2>/dev/null || echo 0)
  if is_truthy "$STRICT"; then args+=(--strict-warnings); fi
  set +e; out="$("$CARGO" "${args[@]}")"; code=$?; set -e
  t1=$(date +%s%3N 2>/dev/null || echo 0)
  validate_ms=0; if [[ "$t0" != 0 && "$t1" != 0 ]]; then validate_ms=$((t1 - t0)); fi
  if [[ $code -ne 0 ]]; then
    echo "   -> FAIL ($code)"
  else
    echo "   -> OK"
  fi
  # compute advisories (non-fatal recommendations) and counts via python
  row=$(ARW_ADAPTER_SMOKE_RAW="$out" ARW_ADAPTER_SMOKE_FILE="$f" ARW_ADAPTER_SMOKE_STRICT="$STRICT" ARW_ADAPTER_VALIDATE_MS="$validate_ms" python3 - <<'PY'
import json,sys,os
RAW=os.environ.get('ARW_ADAPTER_SMOKE_RAW','')
PATH=os.environ.get('ARW_ADAPTER_SMOKE_FILE','')
STRICT=os.environ.get('ARW_ADAPTER_SMOKE_STRICT','0')
VAL_MS=int(os.environ.get('ARW_ADAPTER_VALIDATE_MS','0') or '0')
try:
  data=json.loads(RAW)
except Exception:
  print('{}')
  sys.exit(0)
manifest=data.get('manifest',{})
report=data.get('report',{})
advisories=[]
if not manifest.get('description'):
  advisories.append({'field':'description','message':'Adding a short description helps discovery.'})
consent=manifest.get('consent') or {}
if not consent.get('summary'):
  advisories.append({'field':'consent.summary','message':'Provide an operator-facing summary of capability needs.'})
if not manifest.get('tags'):
  advisories.append({'field':'tags','message':'Consider adding tags for discovery (vendor, modality, profile).'})
ok=(len(report.get('errors') or [])==0 and (not STRICT in ('1','true','yes','on') or len(report.get('warnings') or [])==0))
row={
  'path': PATH,
  'id': manifest.get('id'),
  'version': manifest.get('version'),
  'modalities': manifest.get('modalities') or [],
  'accelerator': (manifest.get('resources') or {}).get('accelerator'),
  'ok': ok,
  'errors': report.get('errors') or [],
  'warnings': report.get('warnings') or [],
  'advisories': advisories,
  'validate_ms': VAL_MS,
}
print(json.dumps(row,separators=(',',':')))
PY
  )
  json_rows+=("$row")
  # Early exit on hard failure
  if [[ $code -ne 0 ]]; then exit $code; fi
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
      # measure health probe time
      h0=$(date +%s%3N 2>/dev/null || echo 0)
      set +e; curl -fsS -m 2 "$url" >/dev/null; code=$?; set -e
      h1=$(date +%s%3N 2>/dev/null || echo 0)
      health_ms=0; if [[ "$h0" != 0 && "$h1" != 0 ]]; then health_ms=$((h1 - h0)); fi
      if [[ $code -ne 0 ]]; then
        log "Probe failed (non-fatal): $url"
        probe_status="fail"
      else
        log "Probe OK: $url"
        probe_status="ok"
      fi
      # augment last row with health info
      if [[ ${#json_rows[@]} -gt 0 ]]; then
        last="${json_rows[-1]}"
        augmented=$(python3 - <<'PY'
import json,sys,os
row=json.loads(sys.stdin.read())
row['health_probe_ms']=int(os.environ.get('ARW_ADAPTER_HEALTH_MS','0') or '0')
row['health_status']=os.environ.get('ARW_ADAPTER_HEALTH_STATUS','')
print(json.dumps(row,separators=(',',':')))
PY
        <<<"$last" ARW_ADAPTER_HEALTH_MS="$health_ms" ARW_ADAPTER_HEALTH_STATUS="$probe_status")
        json_rows[-1]="$augmented"
      fi
    fi
  done
fi

report_json="[$(IFS=,; echo "${json_rows[*]}")]"

if [[ -n "$OUT_PATH" ]]; then
  tmp="$(mktemp)"; printf '%s\n' "$report_json" > "$tmp"
  mkdir -p "$(dirname "$OUT_PATH")" || true
  mv "$tmp" "$OUT_PATH"; log "Wrote JSON report to $OUT_PATH"
fi

# Print Markdown summary and GitHub annotations when on CI
python3 - <<'PY'
import json,os,sys
data=json.loads(os.environ.get('ARW_ADAPTER_REPORT','[]'))
def cnt(x): return len(x) if isinstance(x,list) else 0
total=len(data)
oks=sum(1 for r in data if r.get('ok'))
errs=sum(cnt(r.get('errors')) for r in data)
warns=sum(cnt(r.get('warnings')) for r in data)
advs=sum(cnt(r.get('advisories')) for r in data)
print("| manifest | ok | errors | warnings | advisories | validate_ms | health_ms |\n|---|---:|---:|---:|---:|---:|---:|")
for r in data:
  path=r.get('path','')
  ok='yes' if r.get('ok') else 'no'
  vm=r.get('validate_ms') or 0
  hm=r.get('health_probe_ms') or 0
  print(f"| {path} | {ok} | {cnt(r.get('errors'))} | {cnt(r.get('warnings'))} | {cnt(r.get('advisories'))} | {vm} | {hm} |")
print(f"\nSummary: files={total} ok={oks} errors={errs} warnings={warns} advisories={advs}")
if os.environ.get('GITHUB_ACTIONS'):
  print(f"::notice::adapter smoke summary files={total} ok={oks} errors={errs} warnings={warns} advisories={advs}")
PY
ARW_ADAPTER_REPORT="$report_json"

log "Adapter smoke completed."
exit 0
