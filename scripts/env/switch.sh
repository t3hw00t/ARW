#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"

usage() {
  cat <<'EOF'
Usage: bash scripts/env/switch.sh <mode>

Modes:
  linux          Native Linux workstation or container
  windows-host   Windows host (Git Bash / PowerShell)
  windows-wsl    Windows Subsystem for Linux (Ubuntu, etc.)
  mac            macOS host

Run this command *inside* the environment you wish to activate.
The script updates .arw-env, swaps the active target/venv directories,
and records the current mode so helper scripts stay consistent.
EOF
}

if [[ $# -lt 1 ]]; then
  usage
  exit 2
fi

MODE="$1"

source "$REPO_ROOT/scripts/lib/env_mode.sh"

if ! arw_mode_valid "$MODE"; then
  echo "[env-switch] Unsupported mode '$MODE'. Allowed: ${ARW_ENV_ALLOWED_MODES[*]}" >&2
  exit 1
fi

HOST_MODE="$(arw_detect_host_mode)"
if [[ "$HOST_MODE" == "unknown" ]]; then
  echo "[env-switch] Unable to detect host platform (uname=$(uname -s)). Aborting." >&2
  exit 1
fi

if [[ "$HOST_MODE" != "$MODE" ]]; then
  cat >&2 <<EOF
[env-switch] Environment mismatch detected.
  mode requested: $MODE
  host detected : $HOST_MODE
Run this command inside the $MODE environment.
EOF
  exit 1
fi

arw_write_mode_file "$MODE"
arw_activate_mode "$MODE"

printf '[env-switch] Activated %s mode at %s\n' "$MODE" "$REPO_ROOT"
printf '[env-switch] Active target dir: %s\n' "$REPO_ROOT/target"
printf '[env-switch] Active venv dir:   %s\n' "$REPO_ROOT/.venv"
