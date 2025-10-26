#!/usr/bin/env bash
set -euo pipefail

# Print effective performance preset and key knobs from /about,
# and list any local env overrides if set.

BASE="${BASE:-http://127.0.0.1:8091}"
TOKEN="${ARW_ADMIN_TOKEN:-${TOKEN:-}}"

hdr=()
if [[ -n "$TOKEN" ]]; then hdr=(-H "Authorization: Bearer $TOKEN"); fi

about_json=$(curl -fsS "${hdr[@]}" "$BASE/about" || true)
if [[ -z "$about_json" ]]; then
  echo "[preset-diag] error: cannot fetch $BASE/about" >&2
  exit 1
fi

parse_py=$(python3 - <<'PY'
import json,sys,os
try:
  data=json.load(sys.stdin)
except Exception as e:
  print('error: invalid JSON', e)
  sys.exit(2)
p=data.get('perf_preset') or {}
tier=p.get('tier') or ''
hmc=p.get('http_max_conc')
aqm=p.get('actions_queue_max')
print(json.dumps({'tier':tier,'http_max_conc':hmc,'actions_queue_max':aqm}))
PY
)
read -r tier hmc aqm < <(python3 - <<'PY'
import json,sys
obj=json.loads(sys.stdin.read())
print(obj.get('tier') or '')
print(obj.get('http_max_conc') if obj.get('http_max_conc') is not None else '')
print(obj.get('actions_queue_max') if obj.get('actions_queue_max') is not None else '')
PY
<<< "$parse_py")

echo "Preset tier: ${tier:-unknown}"
if [[ -n "${hmc:-}" ]]; then echo "HTTP concurrency (about): $hmc"; fi
if [[ -n "${aqm:-}" ]]; then echo "Actions queue max (about): $aqm"; fi

echo "Local env overrides (if set):"
keys=(
  ARW_HTTP_MAX_CONC ARW_WORKERS ARW_WORKERS_MAX ARW_ACTIONS_QUEUE_MAX
  ARW_TOOLS_CACHE_TTL_SECS ARW_TOOLS_CACHE_CAP
  ARW_PREFER_LOW_POWER ARW_LOW_POWER ARW_OCR_PREFER_LOW_POWER ARW_OCR_LOW_POWER
  ARW_ACCESS_LOG ARW_ACCESS_UA ARW_ACCESS_UA_HASH ARW_ACCESS_REF
  ARW_EVENTS_SSE_DECORATE ARW_RUNTIME_WATCHER_COOLDOWN_MS
  ARW_MEMORY_EMBED_BACKFILL_BATCH ARW_MEMORY_EMBED_BACKFILL_IDLE_SEC
)
any=0
for k in "${keys[@]}"; do
  v="${!k-}"
  if [[ -n "${v:-}" ]]; then printf "  %s=%s\n" "$k" "$v"; any=1; fi
done
if [[ $any -eq 0 ]]; then echo "  (none)"; fi

exit 0

