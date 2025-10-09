#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'EOF'
Usage: bash scripts/build_docs_wheels.sh [options]

Download the MkDocs/doc requirements wheels defined in requirements/docs.txt for offline use.
The wheels are stored in a local cache directory and (optionally) archived for transport.

Options:
  --output DIR     Directory to store downloaded wheels (default: cache/docs-wheels).
  --archive PATH   Optional tar.gz archive to create from the downloaded wheels.
  --python BIN     Python interpreter to use (default: python3 or python).
  -h, --help       Show this message.
EOF
}

OUTPUT_DIR=""
ARCHIVE_PATH=""
PYTHON_BIN=""

while [[ $# -gt 0 ]]; do
  case "$1" in
    --output)
      OUTPUT_DIR="$2"
      shift 2
      ;;
    --archive)
      ARCHIVE_PATH="$2"
      shift 2
      ;;
    --python)
      PYTHON_BIN="$2"
      shift 2
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      echo "[docs-wheels] Unknown option: $1" >&2
      usage
      exit 2
      ;;
  esac
done

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
REQ_FILE="$ROOT_DIR/requirements/docs.txt"

if [[ ! -f "$REQ_FILE" ]]; then
  echo "[docs-wheels] requirements/docs.txt not found" >&2
  exit 1
fi

if [[ -z "$PYTHON_BIN" ]]; then
  PYTHON_BIN="$(command -v python3 || command -v python || true)"
fi

if [[ -z "$PYTHON_BIN" ]]; then
  echo "[docs-wheels] python3/python not found in PATH" >&2
  exit 1
fi

if [[ -z "$OUTPUT_DIR" ]]; then
  OUTPUT_DIR="$ROOT_DIR/cache/docs-wheels"
fi
mkdir -p "$OUTPUT_DIR"

echo "[docs-wheels] Downloading requirements to $OUTPUT_DIR"
"$PYTHON_BIN" -m pip download --require-hashes -r "$REQ_FILE" -d "$OUTPUT_DIR"

if [[ -n "$ARCHIVE_PATH" ]]; then
  mkdir -p "$(dirname "$ARCHIVE_PATH")"
  echo "[docs-wheels] Creating archive $ARCHIVE_PATH"
  tar -C "$OUTPUT_DIR" -czf "$ARCHIVE_PATH" .
fi

echo "[docs-wheels] Done."
