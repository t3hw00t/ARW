#!/usr/bin/env bash
set -euo pipefail

DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd "$DIR/.." && pwd)"
LOG="$ROOT/.install.log"

if [[ ! -f "$LOG" ]]; then
  echo "[uninstall] No install log found; nothing to do."
  exit 0
fi

removed=()
left=()
pycmd="python3"
if ! command -v python3 >/dev/null 2>&1 && command -v python >/dev/null 2>&1; then
  pycmd="python"
fi

while read -r type item; do
  [[ -z "$type" || "$type" == \#* ]] && continue
  case "$type" in
    DIR)
      path="$ROOT/$item"
      if [[ -e "$path" ]]; then
        rm -rf "$path"
        removed+=("$item")
      else
        left+=("$item (missing)")
      fi
      ;;
    PIP)
      if command -v "$pycmd" >/dev/null 2>&1; then
        if "$pycmd" -m pip uninstall -y "$item" >/dev/null 2>&1; then
          removed+=("pip package $item")
        else
          left+=("pip package $item")
        fi
      else
        left+=("pip package $item (python not found)")
      fi
      ;;
  esac
done < "$LOG"

rm -f "$LOG"

echo "[uninstall] Removed:"
for r in "${removed[@]}"; do
  echo "  - $r"
done
if [[ ${#left[@]} -gt 0 ]]; then
  echo "[uninstall] Left on system:"
  for k in "${left[@]}"; do
    echo "  - $k"
  done
fi

echo "[uninstall] Done."
