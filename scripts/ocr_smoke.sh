#!/usr/bin/env bash
set -euo pipefail

# Minimal OCR smoke: triggers pre-OCR pipeline and verifies metrics presence.
# Env:
#   BASE     - server base URL (default http://127.0.0.1:8103)
#   TOKEN    - admin token for Authorization (default $ARW_ADMIN_TOKEN or 'test-admin-token')
#   TIMEOUT  - curl max time in seconds (default 8)

BASE=${BASE:-http://127.0.0.1:8103}
TOKEN=${TOKEN:-${ARW_ADMIN_TOKEN:-test-admin-token}}
TIMEOUT=${TIMEOUT:-8}

tmp_png="/tmp/ocr-smoke.png"
tmp_resp="/tmp/ocr-smoke-response.json"
tmp_metrics="/tmp/ocr-smoke-metrics.txt"

# 1x1 white PNG
cat >"${tmp_png}" <<'PNG'
iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAuMBgq0M3yMAAAAASUVORK5CYII=
PNG
base64 -d "${tmp_png}" > "${tmp_png}.bin" && mv "${tmp_png}.bin" "${tmp_png}"

# Fire OCR (expect 200 or 5xx depending on features; pre-OCR should still run when quality=lite)
code=$(curl -sS -w '%{http_code}' -o "${tmp_resp}" \
  -m "${TIMEOUT}" -X POST \
  -H "authorization: Bearer ${TOKEN}" -H 'content-type: application/json' \
  "${BASE}/tools/run" \
  -d '{"id":"ui.screenshot.ocr","input":{"path":"/tmp/ocr-smoke.png","lang":"eng"}}') || true
echo "[ocr-smoke] tools/run status=${code}"

# Pull metrics and check pre-OCR signals
curl -sS -m "${TIMEOUT}" "${BASE}/metrics" >"${tmp_metrics}"
if grep -q '^arw_ocr_preprocess_total' "${tmp_metrics}" || grep -q '^arw_ocr_preprocess_ms' "${tmp_metrics}"; then
  echo "[ocr-smoke] OK: pre-OCR metrics observed"
  exit 0
fi

echo "[ocr-smoke] ERROR: pre-OCR metrics not found" >&2
exit 1

