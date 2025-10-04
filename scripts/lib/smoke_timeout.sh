#!/usr/bin/env bash
# shellcheck shell=bash

# Reusable timeout guard helpers for smoke scripts.
#
# Usage:
#   source "$(dirname "${BASH_SOURCE[0]}")/lib/smoke_timeout.sh"
#   smoke_timeout::init "tag" 600 "SPECIFIC_ENV"
#   ...
#   status=$(smoke_timeout::cleanup "$status")

if [[ -n "${SMOKE_TIMEOUT_LIB_SOURCED:-}" ]]; then
  return 0
fi
SMOKE_TIMEOUT_LIB_SOURCED=1

_smoke_timeout_main_pid=""
_smoke_timeout_guard_pid=""
_smoke_timeout_children=()
_smoke_timeout_limit=""
_smoke_timeout_tag="smoke"
_smoke_timeout_timed_out=0

smoke_timeout::init() {
  local tag=${1:-smoke}
  local default_limit=${2:-600}
  local specific_env=${3:-}

  _smoke_timeout_main_pid=$$
  _smoke_timeout_tag=$tag
  _smoke_timeout_guard_pid=""
  _smoke_timeout_children=()
  _smoke_timeout_timed_out=0

  local fallback=$default_limit
  if [[ -n "${SMOKE_TIMEOUT_SECS:-}" ]]; then
    if [[ ${SMOKE_TIMEOUT_SECS} =~ ^[0-9]+$ ]]; then
      fallback=${SMOKE_TIMEOUT_SECS}
    else
      printf '[%s] warning: SMOKE_TIMEOUT_SECS="%s" is not an integer; using %s\n' \
        "$tag" "${SMOKE_TIMEOUT_SECS}" "$fallback" >&2
    fi
  fi

  local specific_limit=""
  if [[ -n "$specific_env" && -n "${!specific_env:-}" ]]; then
    local candidate=${!specific_env}
    if [[ $candidate =~ ^[0-9]+$ ]]; then
      specific_limit=$candidate
    else
      printf '[%s] warning: %s="%s" is not an integer; falling back to %s\n' \
        "$tag" "$specific_env" "$candidate" "$fallback" >&2
    fi
  fi

  if [[ -n $specific_limit ]]; then
    _smoke_timeout_limit=$specific_limit
  else
    _smoke_timeout_limit=$fallback
  fi

  trap 'smoke_timeout::on_usr1' USR1
  trap 'smoke_timeout::on_term' TERM
  smoke_timeout::start_guard
}

smoke_timeout::start_guard() {
  local limit=${_smoke_timeout_limit:-}
  if [[ -z $limit || $limit == 0 ]]; then
    return
  fi
  (
    sleep "$limit"
    kill -s USR1 "$_smoke_timeout_main_pid" 2>/dev/null || exit 0
    sleep 1
    kill -s TERM "$_smoke_timeout_main_pid" 2>/dev/null || exit 0
  ) &
  _smoke_timeout_guard_pid=$!
}

smoke_timeout::on_usr1() {
  _smoke_timeout_timed_out=1
  printf '[%s] timeout guard triggered after %ss\n' \
    "${_smoke_timeout_tag}" "${_smoke_timeout_limit}" >&2
}

smoke_timeout::on_term() {
  _smoke_timeout_timed_out=1
  exit 124
}

smoke_timeout::register_child() {
  local pid=$1
  if [[ -n $pid ]]; then
    _smoke_timeout_children+=("$pid")
  fi
}

smoke_timeout::unregister_child() {
  local pid=$1
  if [[ -z $pid ]]; then
    return
  fi
  local tmp=()
  local current
  for current in "${_smoke_timeout_children[@]}"; do
    if [[ $current != "$pid" ]]; then
      tmp+=("$current")
    fi
  done
  _smoke_timeout_children=("${tmp[@]}")
}

smoke_timeout::cleanup() {
  local status=$1

  if [[ -n ${_smoke_timeout_guard_pid:-} ]]; then
    kill "$_smoke_timeout_guard_pid" 2>/dev/null || true
    wait "$_smoke_timeout_guard_pid" 2>/dev/null || true
    _smoke_timeout_guard_pid=""
  fi

  local pid
  for pid in "${_smoke_timeout_children[@]}"; do
    kill "$pid" 2>/dev/null || true
    wait "$pid" 2>/dev/null || true
  done
  _smoke_timeout_children=()

  if (( _smoke_timeout_timed_out )); then
    status=124
    printf '[%s] timed out after %ss\n' \
      "${_smoke_timeout_tag}" "${_smoke_timeout_limit}" >&2
  fi

  printf '%s' "$status"
}
