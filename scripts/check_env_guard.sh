#!/usr/bin/env bash
set -euo pipefail

# Check for raw std::env::set_var/remove_var in test modules and suggest using EnvGuard/begin_state_env.
# Allows occurrences in test support modules and non-test runtime code.

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")"/.. && pwd)"
cd "$ROOT_DIR"

violations=0

while IFS= read -r -d '' file; do
  # skip known helpers
  base=$(basename "$file")
  case "$file" in
    */test_support/*) continue ;;
  esac
  case "$base" in
    test_support.rs) continue ;;
  esac

  # Determine start of tests region; prefer `mod tests` if present
  start_line=$(rg -n "^\s*mod\s+tests\b" "$file" | head -n1 | cut -d: -f1)
  if [[ -z "$start_line" ]]; then
    start_line=$(rg -n "#\\[cfg\\(test\\)\\]" "$file" | head -n1 | cut -d: -f1)
  fi
  if [[ -z "$start_line" ]]; then
    continue
  fi
  if tail -n +"$start_line" "$file" | rg -n "std::env::(set_var|remove_var)\(" >/dev/null; then
    echo "Found raw std::env mutations in test module: $file"
    tail -n +"$start_line" "$file" | rg -n "std::env::(set_var|remove_var)\(" || true
    violations=$((violations+1))
  fi
done < <(rg -l "#\[cfg\(test\)\]" apps crates -g "**/*.rs" -0)

if [[ $violations -gt 0 ]]; then
  echo "\nPlease use test EnvGuard/begin_state_env instead of raw std::env mutations in tests." >&2
  if [[ "${ENFORCE_ENV_GUARD:-0}" == "1" ]]; then
    exit 1
  fi
else
  echo "EnvGuard check passed."
fi
