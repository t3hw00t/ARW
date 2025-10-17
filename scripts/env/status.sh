#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"

source "$REPO_ROOT/scripts/lib/env_mode.sh"
arw_env_init

printf 'Mode: %s\n' "${ARW_ENV_MODE}"
printf 'Source: %s\n' "${ARW_ENV_SOURCE:-unknown}"
printf 'Repo: %s\n' "${REPO_ROOT}"
printf 'Target dir: %s\n' "${REPO_ROOT}/target"
printf 'Virtualenv: %s\n' "${REPO_ROOT}/.venv"
printf 'Binary suffix: %s\n' "${ARW_EXE_SUFFIX:-""}"
