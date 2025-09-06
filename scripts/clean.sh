#!/usr/bin/env bash
set -euo pipefail

hard=0
if [[ ${1:-} == '--hard' ]]; then hard=1; fi

echo '[clean] Removing target/ and dist/'
rm -rf target dist || true

if [[ $hard -eq 1 ]]; then
  echo '[clean] Hard mode: removing backups (*.bak_*, .backups/)'
  find . -type f -name '*.bak_*' -print0 | xargs -0 rm -f || true
  rm -rf .backups || true
fi

echo '[clean] Done.'
