#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'USAGE'
Usage: scripts/package_ops_assets.sh [--out DIR] [--dest FILE]

Regenerates Prometheus/Grafana ops assets and bundles them into a tarball for
transport to monitoring hosts.

Options:
  --out DIR    Directory to place exported assets (default: ops/out-package)
  --dest FILE  Destination tarball (default: dist/ops-assets.tar.gz)
USAGE
}

OUTDIR="ops/out-package"
DEST="dist/ops-assets.tar.gz"

while [[ $# -gt 0 ]]; do
  case "$1" in
    --out)
      OUTDIR="$2"
      shift 2
      ;;
    --dest)
      DEST="$2"
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

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

mkdir -p "$(dirname "$DEST")"

if command -v python >/dev/null 2>&1; then
  python scripts/export_ops_assets.py --out "$OUTDIR"
else
  scripts/export_ops_assets.sh --out "$OUTDIR"
fi

tar -czf "$DEST" -C "$OUTDIR" .

echo "[ops-package] assets exported to $OUTDIR and archived at $DEST"
