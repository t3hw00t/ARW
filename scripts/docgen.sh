#!/usr/bin/env bash
set -euo pipefail

info() { echo -e "\033[36m[docgen]\033[0m $*"; }
die()  { echo "error: $*" >&2; exit 1; }

command -v cargo >/dev/null 2>&1 || die "Rust 'cargo' not found in PATH"

info "Collecting cargo metadata"
json=$(cargo metadata --no-deps --locked --format-version 1)

title='---
title: Workspace Status
---

# Workspace Status

Generated: '$(date -u +"%Y-%m-%d %H:%M")' UTC
'

libs=$(echo "$json" | jq -r '.packages[] | {name, version, path: .manifest_path, kinds: ([.targets[].kind[]] | unique)} | select((.kinds | tostring) | test("lib")) | "- **\(.name)**: \(.version) — `\(.path)`"' || true)
bins=$(echo "$json" | jq -r '.packages[] | {name, version, path: .manifest_path, kinds: ([.targets[].kind[]] | unique)} | select((.kinds | tostring) | test("bin")) | "- **\(.name)**: \(.version) — `\(.path)`"' || true)

out="$title\n\n## Libraries\n${libs:-_none_}\n\n## Binaries\n${bins:-_none_}\n"

dest="$(cd "$(dirname "$0")/.." && pwd)/docs/developer/status.md"
info "Writing $dest"
printf "%s" "$out" > "$dest"
info "Done."

# --- Tasks page generation ---
root_dir="$(cd "$(dirname "$0")/.." && pwd)"
tasks_json="$root_dir/.arw/tasks.json"
tasks_md="$root_dir/docs/developer/tasks.md"

if [ -f "$tasks_json" ] && command -v jq >/dev/null 2>&1; then
  info "Generating tasks page from $tasks_json"
  # Normalize and ensure updated field
  now_ts="$(date -u +"%Y-%m-%d %H:%M") UTC"
  tmp=$(mktemp)
  jq --arg now "$now_ts" '.updated = $now | .tasks = (.tasks // [])' "$tasks_json" > "$tmp" && mv "$tmp" "$tasks_json"

  # Build markdown grouped by status
  header='---
title: Tasks Status
---

# Tasks Status

Updated: '"$now_ts"'
'
  make_sec() {
    sec="$1"; title="$2"
    echo "## $title"
    jq -r --arg st "$sec" '
      .tasks | map(select((.status // "todo") == $st)) |
      sort_by((.updated // "")) | reverse |
      .[] | "- [" + (.id // "?") + "] " + (.title // "(untitled)") +
             " — " + ($st) +
             (if .updated then " (updated: " + .updated + ")" else "" end)
             + (if (.notes // []) | length > 0 then "\n  " + ((.notes // []) | map("  - " + (.time // "") + ": " + (.text // "")) | join("\n")) else "" end)
      ' "$tasks_json"
  }

  {
    printf "%s\n\n" "$header"
    make_sec todo "To Do"; echo
    make_sec in_progress "In Progress"; echo
    make_sec paused "Paused"; echo
    make_sec done "Done"; echo
  } > "$tasks_md"
  info "Wrote $tasks_md"
else
  # Ensure a placeholder exists
  if [ ! -f "$tasks_md" ]; then
    printf "---\ntitle: Tasks Status\n---\n\n# Tasks Status\n\n_no tasks file found (.arw/tasks.json)_\n" > "$tasks_md"
    info "Wrote placeholder $tasks_md"
  fi
fi

# --- Spec generation: MCP tools and AsyncAPI ---
info "Generating specs"
root_dir="$(cd "$(dirname "$0")/.." && pwd)"
spec_dir="$root_dir/spec"
mkdir -p "$spec_dir"

# MCP tools via arw-cli
if cargo build -p arw-cli --release >/dev/null 2>&1; then
  if "$root_dir/target/release/arw-cli" tools > "$spec_dir/mcp-tools.json" 2>/dev/null; then
    info "Wrote $spec_dir/mcp-tools.json"
  else
    warn "failed to generate mcp-tools.json"
  fi
else
  warn "arw-cli build failed; skipping mcp-tools.json"
fi

# AsyncAPI (minimal for known events)
cat > "$spec_dir/asyncapi.yaml" << 'YAML'
asyncapi: 2.6.0
info:
  title: "arw-svc events"
  version: "0.1.0"
defaultContentType: application/json
channels:
  Service.Start:
    subscribe:
      message:
        name: Service.Start
  Service.Health:
    subscribe:
      message:
        name: Service.Health
  Service.Test:
    subscribe:
      message:
        name: Service.Test
  Governor.Changed:
    subscribe:
      message:
        name: Governor.Changed
  Memory.Applied:
    subscribe:
      message:
        name: Memory.Applied
  Models.Changed:
    subscribe:
      message:
        name: Models.Changed
  Models.DownloadProgress:
    subscribe:
      message:
        name: Models.DownloadProgress
  Tool.Ran:
    subscribe:
      message:
        name: Tool.Ran
  Feedback.Signal:
    subscribe:
      message:
        name: Feedback.Signal
  Feedback.Suggested:
    subscribe:
      message:
        name: Feedback.Suggested
  Feedback.Applied:
    subscribe:
      message:
        name: Feedback.Applied
YAML
info "Wrote $spec_dir/asyncapi.yaml"
