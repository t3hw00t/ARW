#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'USAGE'
Usage: scripts/export_ops_assets.sh [--out DIR]

Extract Prometheus rule snippets and Grafana dashboard JSON from the docs
snippets into ready-to-apply files under DIR (default: ./ops/out).

Produces:
  DIR/prometheus_recording_rules.yaml
  DIR/prometheus_alerting_rules.yaml
  DIR/grafana_quick_panels.json

Environment:
  ARW_EXPORT_OUTDIR  Override destination directory.
USAGE
}

OUTDIR="${ARW_EXPORT_OUTDIR:-}"

while [[ $# -gt 0 ]]; do
  case "$1" in
    --out)
      OUTDIR="$2"
      shift 2
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      echo "Unknown option: $1" >&2
      usage >&2
      exit 1
      ;;
  esac
done

if [[ -z "${OUTDIR}" ]]; then
  OUTDIR="ops/out"
fi

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}" )/.." && pwd)"
mkdir -p "${ROOT}/${OUTDIR}"

extract_block() {
  local src="$1" kind="$2" dest="$3"
  python3 - "$ROOT/$src" "$kind" >"$dest" <<'PY'
import sys
from pathlib import Path

path = Path(sys.argv[1])
kind = sys.argv[2]
start = f"```{kind}"
out_lines = []
collect = False
found = False
for line in path.read_text().splitlines():
    stripped = line.strip()
    if stripped == start:
        if collect:
            break
        collect = True
        found = True
        continue
    if stripped == "```" and collect:
        collect = False
        break
    if collect:
        out_lines.append(line)

if not found or not out_lines:
    raise SystemExit(f"could not extract {kind} block from {path}")

sys.stdout.write("\n".join(out_lines) + "\n")
PY
}

extract_block "docs/snippets/prometheus_recording_rules.md" "yaml" "${ROOT}/${OUTDIR}/prometheus_recording_rules.yaml"
extract_block "docs/snippets/prometheus_alerting_rules.md" "yaml" "${ROOT}/${OUTDIR}/prometheus_alerting_rules.yaml"
extract_block "docs/snippets/grafana_quick_panels.md" "json" "${ROOT}/${OUTDIR}/grafana_quick_panels.json"

echo "[export-ops] Assets written to ${OUTDIR}"
