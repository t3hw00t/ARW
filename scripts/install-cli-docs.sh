#!/usr/bin/env bash
set -euo pipefail

# Install arw-cli completions and man pages into user-local directories.
# Usage: scripts/install-cli-docs.sh [--bin /path/to/arw-cli] [--shells bash,zsh,fish] [--man-dir DIR]

BIN="${ARW_CLI_BIN:-}"
SHELLS="bash,zsh,fish"
MAN_DIR="${HOME}/.local/share/man/man1"

while [[ $# -gt 0 ]]; do
  case "$1" in
    --bin) BIN="$2"; shift 2;;
    --shells) SHELLS="$2"; shift 2;;
    --man-dir) MAN_DIR="$2"; shift 2;;
    *) echo "unknown arg: $1"; exit 2;;
  esac
done

if [[ -z "${BIN}" ]]; then
  if command -v arw-cli >/dev/null 2>&1; then
    BIN="$(command -v arw-cli)"
  else
    echo "arw-cli not found in PATH; trying cargo run (debug build)" >&2
    BIN="cargo run -q -p arw-cli --"
  fi
fi

run_cli(){
  # shellcheck disable=SC2086
  ${BIN} "$@"
}

echo "Installing man page to ${MAN_DIR}"
mkdir -p "${MAN_DIR}"
run_cli man --out-dir "${MAN_DIR}"

IFS=',' read -r -a arr <<< "${SHELLS}"
for sh in "${arr[@]}"; do
  case "$sh" in
    bash)
      dir="${HOME}/.local/share/bash-completion/completions";
      ;;
    zsh)
      dir="${HOME}/.local/share/zsh/site-functions";
      ;;
    fish)
      dir="${HOME}/.config/fish/completions";
      ;;
    powershell)
      dir="${HOME}/.local/share/powershell/Completions";
      ;;
    elvish)
      dir="${HOME}/.local/share/elvish/lib";
      ;;
    *) echo "unknown shell: $sh"; continue;;
  esac
  echo "Generating ${sh} completions to ${dir}"
  mkdir -p "${dir}"
  run_cli completions "${sh}" --out-dir "${dir}"
done

echo "Done. You may need to open a new terminal for completions/man to load."
