#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'EOF'
Usage: bash scripts/update_mise_hash.sh [--url <installer-url>]

Downloads the mise installer script, prints its SHA256, and leaves the file in a temp directory for verification.

Options:
  --url URL   Override the installer URL (default: https://mise.jdx.dev/install.sh)
  -h, --help  Show this help message.
EOF
}

INSTALL_URL="https://mise.jdx.dev/install.sh"

while [[ $# -gt 0 ]]; do
  case "$1" in
    --url)
      INSTALL_URL="$2"
      shift 2
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      echo "[mise-hash] Unknown option: $1" >&2
      usage
      exit 2
      ;;
  esac
done

tmp_dir="$(mktemp -d)"
trap 'rm -rf "$tmp_dir"' EXIT

installer="$tmp_dir/mise-install.sh"

echo "[mise-hash] Downloading installer from $INSTALL_URL"
curl -fsSL "$INSTALL_URL" -o "$installer"

hash="$(shasum -a 256 "$installer" | awk '{print $1}')"
echo "[mise-hash] SHA256: $hash"
echo "[mise-hash] (Update .github/workflows/ci.yml -> MISE_INSTALL_SHA256 with this value)"
