#!/usr/bin/env bash
set -euo pipefail

# Agent Hub (ARW) — Supply-chain and code audit helper
# Runs cargo-audit and cargo-deny with a simple interactive mode.

DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd "$DIR/.." && pwd)"

INSTALL_TOOLS=auto   # auto|yes|no
INTERACTIVE=0
RUN_AUDIT=1         # cargo-audit
RUN_DENY=1          # cargo-deny (advisories,bans,sources,licenses)
STRICT=0            # exit non-zero on any failure

usage() {
  cat <<'EOF'
ARW audit helper

Usage: scripts/audit.sh [options]

Options
  -i, --interactive     Interactive menu
  --install-tools=MODE  Install missing tools: auto|yes|no (default: auto)
  --no-audit            Skip cargo-audit
  --no-deny             Skip cargo-deny
  --strict              Fail on warnings/errors from checks
  -h, --help            Show help

Examples
  scripts/audit.sh --strict
  scripts/audit.sh --interactive
EOF
}

for arg in "$@"; do
  case "$arg" in
    -i|--interactive) INTERACTIVE=1 ;;
    --install-tools=auto) INSTALL_TOOLS=auto ;;
    --install-tools=yes) INSTALL_TOOLS=yes ;;
    --install-tools=no) INSTALL_TOOLS=no ;;
    --no-audit) RUN_AUDIT=0 ;;
    --no-deny) RUN_DENY=0 ;;
    --strict) STRICT=1 ;;
    -h|--help) usage; exit 0 ;;
    *) echo "Unknown option: $arg" >&2; usage; exit 2 ;;
  esac
done

info(){ echo -e "\033[36m[audit]\033[0m $*"; }
warn(){ echo -e "\033[33m[audit]\033[0m $*"; }
err(){ echo -e "\033[31m[audit]\033[0m $*"; }

need_tool() { # name, install_cmd...
  local name="$1"; shift || true
  if command -v "$name" >/dev/null 2>&1; then return 0; fi
  case "$INSTALL_TOOLS" in
    no) warn "$name not found and --install-tools=no"; return 1 ;;
    auto|yes)
      if [[ "$INSTALL_TOOLS" == yes ]]; then info "Installing $name (forced)"; else info "Installing missing tool: $name"; fi
      if [[ "$name" == cargo-audit ]]; then
        cargo install --locked cargo-audit || { warn "cargo-audit install failed"; return 1; }
      elif [[ "$name" == cargo-deny ]]; then
        cargo install --locked cargo-deny || { warn "cargo-deny install failed"; return 1; }
      else
        if (($#>0)); then "$@" || return 1; else return 1; fi
      fi
      command -v "$name" >/dev/null 2>&1
      ;;
  esac
}

run_cargo_audit() {
  if [[ $RUN_AUDIT -eq 0 ]]; then return 0; fi
  if ! need_tool cargo-audit; then warn "Skipping cargo-audit"; return 0; fi
  info "cargo audit (this updates advisory DB as needed)"
  local args=()
  # Temporary ignore for glib<0.20 (RUSTSEC-2024-0429) — removed automatically when resolved
  if ! glib_version_ge_020; then args+=(--ignore RUSTSEC-2024-0429); fi
  # Bench-only arrow2 pulls lexical-core 0.8.x (RUSTSEC-2023-0086) and arrow2 OOB (RUSTSEC-2025-0038).
  # Ignore if arrow2 present in lock.
  if rg -q "^name = \"arrow2\"$" "$ROOT/Cargo.lock"; then args+=(--ignore RUSTSEC-2023-0086 --ignore RUSTSEC-2025-0038); fi
  (cd "$ROOT" && cargo audit "${args[@]}" || true)
}

run_cargo_deny() {
  if [[ $RUN_DENY -eq 0 ]]; then return 0; fi
  if ! need_tool cargo-deny; then warn "Skipping cargo-deny"; return 0; fi
  info "cargo deny check advisories bans sources licenses"
  (cd "$ROOT" && cargo deny check advisories bans sources licenses || true)
}

glib_version_ge_020() {
  local lock="$ROOT/Cargo.lock"
  [[ -f "$lock" ]] || return 1
  local ver line
  # find the package block for glib and capture its version
  ver=$(awk '/^\[\[package\]\]/{inpkg=1; name=""; ver=""} inpkg && /^name =/ {name=$0} inpkg && /^version =/ {ver=$0} inpkg && name ~ /"glib"/ && ver {print ver; exit}' "$lock" | awk -F '"' '{print $2}') || true
  [[ -n "$ver" ]] || return 1
  local major minor patch
  IFS='.' read -r major minor patch <<<"$ver"
  # >= 0.20.0 if major>0 or (major==0 and minor>=20)
  if [[ ${major:-0} -gt 0 ]]; then return 0; fi
  if [[ ${major:-0} -eq 0 && ${minor:-0} -ge 20 ]]; then return 0; fi
  return 1
}

auto_clean_ignored_advisory() {
  local deny="$ROOT/deny.toml"
  [[ -f "$deny" ]] || return 0
  if glib_version_ge_020 && rg -q "RUSTSEC-2024-0429" "$deny"; then
    info "glib >= 0.20 detected. Removing temporary ignore RUSTSEC-2024-0429 from deny.toml"
    # Remove the line containing the advisory; preserve surrounding formatting
    tmp=$(mktemp)
    awk 'index($0,"RUSTSEC-2024-0429")==0 {print $0}' "$deny" > "$tmp" && mv "$tmp" "$deny"
  fi
}

summary_and_exit() {
  local rc=$1
  if [[ $rc -eq 0 ]]; then info "Audit complete"; else err "Audit failed (strict)"; fi
  auto_clean_ignored_advisory || true
  exit "$rc"
}

interactive_menu() {
  while true; do
    echo ""
    echo "Agent Hub (ARW) — Audit Menu"
    echo "  Root: $ROOT"
    echo "  Tools: cargo-audit=$(command -v cargo-audit >/dev/null 2>&1 && echo ok || echo missing), cargo-deny=$(command -v cargo-deny >/dev/null 2>&1 && echo ok || echo missing)"
    echo "  Checks: audit=$RUN_AUDIT deny=$RUN_DENY strict=$STRICT"
    cat <<EOF
  1) Toggle cargo-audit
  2) Toggle cargo-deny
  3) Install missing tools (now)
  4) Run selected checks
  0) Exit
EOF
    read -r -p "Select: " pick || true
    case "$pick" in
      1) RUN_AUDIT=$((1-RUN_AUDIT)) ;;
      2) RUN_DENY=$((1-RUN_DENY)) ;;
      3) need_tool cargo-audit || true; need_tool cargo-deny || true ;;
      4) run_cargo_audit; run_cargo_deny;;
      0|'') break ;;
      *) : ;;
    esac
  done
}

# Preconditions
if ! command -v cargo >/dev/null 2>&1; then
  err "Rust 'cargo' not found in PATH. Install rustup first: https://rustup.rs"
  exit 1
fi

if [[ $INTERACTIVE -eq 1 ]]; then
  interactive_menu
  summary_and_exit 0
fi

# Standard mode
run_cargo_audit
run_cargo_deny

if [[ $STRICT -eq 1 ]]; then
  # Re-run with strict flags where possible to derive result codes
  strict_rc=0
  if [[ $RUN_AUDIT -eq 1 ]]; then
    audit_args=()
    $(! glib_version_ge_020) && audit_args+=(--ignore RUSTSEC-2024-0429)
    if rg -q "^name = \"arrow2\"$" "$ROOT/Cargo.lock"; then audit_args+=(--ignore RUSTSEC-2023-0086 --ignore RUSTSEC-2025-0038); fi
    (cd "$ROOT" && cargo audit "${audit_args[@]}") || strict_rc=1
  fi
  if [[ $RUN_DENY -eq 1 ]]; then (cd "$ROOT" && cargo deny check advisories bans sources licenses) || strict_rc=1; fi
  summary_and_exit $strict_rc
else
  summary_and_exit 0
fi
