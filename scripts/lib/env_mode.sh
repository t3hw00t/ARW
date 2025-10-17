#!/usr/bin/env bash
# Shared helpers for managing per-platform development environments.

if [[ -n "${ARW_ENV_LIB_LOADED:-}" ]]; then
  return 0
fi
ARW_ENV_LIB_LOADED=1

if [[ -z "${REPO_ROOT:-}" ]]; then
  echo "[env-mode] REPO_ROOT is not set; source env_mode.sh from scripts that define REPO_ROOT first." >&2
  exit 1
fi

ARW_ENV_FILE="${ARW_ENV_FILE:-$REPO_ROOT/.arw-env}"
ARW_ENV_ALLOWED_MODES=("linux" "windows-host" "windows-wsl" "mac")

_arw_env_timestamp() {
  date +"%Y%m%d%H%M%S"
}

arw_detect_host_mode() {
  local uname_out
  uname_out="$(uname -s)"
  case "$uname_out" in
    Linux*)
      if grep -qi microsoft /proc/version 2>/dev/null; then
        echo "windows-wsl"
      else
        echo "linux"
      fi
      ;;
    Darwin*)
      echo "mac"
      ;;
    MINGW*|MSYS*|CYGWIN*)
      echo "windows-host"
      ;;
    *)
      echo "unknown"
      ;;
  esac
}

arw_mode_valid() {
  local candidate="$1"
  for mode in "${ARW_ENV_ALLOWED_MODES[@]}"; do
    if [[ "$candidate" == "$mode" ]]; then
      return 0
    fi
  done
  return 1
}

arw_read_mode_file() {
  if [[ ! -f "$ARW_ENV_FILE" ]]; then
    return 1
  fi
  local line mode
  while IFS= read -r line || [[ -n "$line" ]]; do
    if [[ "$line" == MODE=* ]]; then
      mode="${line#MODE=}"
      mode="${mode//[$'\r\n']/}"
      if [[ -n "$mode" ]]; then
        echo "$mode"
        return 0
      fi
    fi
  done <"$ARW_ENV_FILE"
  return 1
}

arw_write_mode_file() {
  local mode="$1"
  printf 'MODE=%s\n' "$mode" >"$ARW_ENV_FILE"
}

_arw_mv_with_backup() {
  local src="$1"
  local dest="$2"
  if [[ ! -e "$src" ]]; then
    return 0
  fi
  local final_dest="$dest"
  if [[ -e "$final_dest" || -L "$final_dest" ]]; then
    final_dest="${dest}.$(_arw_env_timestamp)"
  fi
  local rel_src="${src#$REPO_ROOT/}"
  local rel_dest="${final_dest#$REPO_ROOT/}"
  echo "[env-mode] moving ${rel_src:-$src} -> ${rel_dest:-$final_dest}" >&2
  mv "$src" "$final_dest"
}

_arw_sync_workspace_slot() {
  local path="$1"
  local mode="$2"
  local kind="$3"

  local sentinel="$path/.arw-mode"
  if [[ -e "$path" ]]; then
    local recorded=""
    if [[ -f "$sentinel" ]]; then
      recorded="$(<"$sentinel")"
    fi
    if [[ "$recorded" == "$mode" ]]; then
      return 0
    fi
    if [[ -n "$recorded" ]]; then
      _arw_mv_with_backup "$path" "${path}.${recorded}"
    else
      _arw_mv_with_backup "$path" "${path}.unmanaged"
    fi
  fi

  local staged="${path}.${mode}"
  if [[ -e "$staged" || -L "$staged" ]]; then
    _arw_mv_with_backup "$staged" "$path"
  fi

  if [[ ! -e "$path" ]]; then
    mkdir -p "$path"
  fi
  printf '%s\n' "$mode" >"$sentinel"

  if [[ "$kind" == "venv" ]]; then
    # Ensure Scripts/ and bin/ are preserved when moving between platforms.
    :
  fi
}

arw_activate_mode() {
  local mode="$1"
  _arw_sync_workspace_slot "$REPO_ROOT/target" "$mode" "target"
  _arw_sync_workspace_slot "$REPO_ROOT/.venv" "$mode" "venv"
}

arw_env_init() {
  local host_mode desired_mode source_file=1
  host_mode="$(arw_detect_host_mode)"
  if [[ "$host_mode" == "unknown" ]]; then
    echo "[env-mode] Unable to detect host platform (uname=$(uname -s)). Set MODE manually in .arw-env." >&2
    exit 1
  fi
  local forced_mode="${ARW_ENV_MODE_FORCE:-}"
  if [[ -n "$forced_mode" ]]; then
    desired_mode="$forced_mode"
    source_file=0
  else
    desired_mode="$(arw_read_mode_file || true)"
  fi

  if [[ -z "$desired_mode" ]]; then
    desired_mode="$host_mode"
    arw_write_mode_file "$desired_mode"
    source_file=1
  fi

  if ! arw_mode_valid "$desired_mode"; then
    echo "[env-mode] Invalid MODE \"$desired_mode\" recorded in .arw-env. Supported: ${ARW_ENV_ALLOWED_MODES[*]}" >&2
    exit 1
  fi

  if [[ "$desired_mode" != "$host_mode" ]]; then
    cat >&2 <<EOF
[env-mode] Active environment mismatch:
  host:    $host_mode
  current: $desired_mode (.arw-env)
Run: bash scripts/env/switch.sh $host_mode
EOF
    exit 1
  fi

  arw_activate_mode "$desired_mode"

  export ARW_ENV_MODE="$desired_mode"
  if [[ $source_file -eq 1 && -z "$forced_mode" ]]; then
    export ARW_ENV_SOURCE=".arw-env"
  elif [[ -n "$forced_mode" ]]; then
    export ARW_ENV_SOURCE="ARW_ENV_MODE_FORCE"
  else
    export ARW_ENV_SOURCE="implicit"
  fi
  case "$desired_mode" in
    windows-*)
      export ARW_EXE_SUFFIX=".exe"
      ;;
    *)
      export ARW_EXE_SUFFIX=""
      ;;
  esac
}

arw_target_profile_dir() {
  local profile="${1:-debug}"
  printf '%s\n' "$REPO_ROOT/target/$profile"
}

arw_target_bin_path() {
  local profile="$1"
  local bin="$2"
  printf '%s\n' "$REPO_ROOT/target/$profile/$bin$ARW_EXE_SUFFIX"
}
