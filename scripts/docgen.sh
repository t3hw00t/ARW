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
        $ref: '#/components/messages/ServiceStart'
  Service.Health:
    subscribe:
      message:
        $ref: '#/components/messages/ServiceHealth'
  Service.Test:
    subscribe:
      message:
        $ref: '#/components/messages/ServiceTest'
  Governor.Changed:
    subscribe:
      message:
        $ref: '#/components/messages/GovernorChanged'
  Memory.Applied:
    subscribe:
      message:
        $ref: '#/components/messages/MemoryApplied'
  Models.Changed:
    subscribe:
      message:
        $ref: '#/components/messages/ModelsChanged'
  Models.DownloadProgress:
    subscribe:
      message:
        $ref: '#/components/messages/ModelsDownloadProgress'
  Models.ManifestWritten:
    subscribe:
      message:
        $ref: '#/components/messages/ModelsManifestWritten'
  Models.CasGc:
    subscribe:
      message:
        $ref: '#/components/messages/ModelsCasGc'
  Egress.Preview:
    subscribe:
      message:
        $ref: '#/components/messages/EgressPreview'
  Egress.Ledger.Appended:
    subscribe:
      message:
        $ref: '#/components/messages/EgressLedgerAppended'
  Tool.Ran:
    subscribe:
      message:
        $ref: '#/components/messages/ToolRan'
  Feedback.Signal:
    subscribe:
      message:
        $ref: '#/components/messages/FeedbackSignal'
  Feedback.Suggested:
    subscribe:
      message:
        $ref: '#/components/messages/FeedbackSuggested'
  Feedback.Applied:
    subscribe:
      message:
        $ref: '#/components/messages/FeedbackApplied'
components:
  messages:
    ServiceStart:
      name: Service.Start
      payload:
        type: object
        additionalProperties: true
    ServiceHealth:
      name: Service.Health
      payload:
        type: object
        properties:
          ok: { type: boolean }
    ServiceTest:
      name: Service.Test
      payload:
        type: object
        additionalProperties: true
    GovernorChanged:
      name: Governor.Changed
      payload:
        type: object
        properties:
          profile: { type: string }
    MemoryApplied:
      name: Memory.Applied
      payload:
        type: object
        additionalProperties: true
    ModelsChanged:
      name: Models.Changed
      payload:
        type: object
        additionalProperties: true
    ModelsDownloadProgress:
      name: Models.DownloadProgress
      payload:
        type: object
        properties:
          id: { type: string }
          status: { type: string }
          code: { type: string }
          error: { type: string }
          progress: { type: integer }
          downloaded: { type: integer }
          total: { type: integer }
          file: { type: string }
          provider: { type: string }
          cas_file: { type: string }
          budget:
            type: object
            properties:
              soft_ms: { type: integer }
              hard_ms: { type: integer }
              spent_ms: { type: integer }
              remaining_soft_ms: { type: integer }
              remaining_hard_ms: { type: integer }
          disk:
            type: object
            properties:
              available: { type: integer }
              total: { type: integer }
              reserve: { type: integer }
        additionalProperties: true
    ModelsManifestWritten:
      name: Models.ManifestWritten
      payload:
        type: object
        properties:
          id: { type: string }
          manifest_path: { type: string }
          sha256: { type: ["string","null"] }
          cas: { type: ["string","null"] }
    ModelsCasGc:
      name: Models.CasGc
      payload:
        type: object
        properties:
          scanned: { type: integer }
          kept: { type: integer }
          deleted: { type: integer }
          deleted_bytes: { type: integer }
          ttl_days: { type: integer }
    EgressPreview:
      name: Egress.Preview
      payload:
        type: object
        properties:
          id: { type: string }
          url: { type: string }
          dest:
            type: object
            properties:
              host: { type: string }
              port: { type: integer }
              protocol: { type: string }
          provider: { type: string }
          corr_id: { type: string }
        additionalProperties: true
    EgressLedgerAppended:
      name: Egress.Ledger.Appended
      payload:
        type: object
        properties:
          decision: { type: string }
          reason_code: { type: string }
          posture: { type: string }
          project_id: { type: string }
          episode_id: { type: ["string","null"] }
          corr_id: { type: string }
          node_id: { type: ["string","null"] }
          tool_id: { type: string }
          dest:
            type: object
            properties:
              host: { type: string }
              port: { type: integer }
              protocol: { type: string }
          bytes_out: { type: integer }
          bytes_in: { type: integer }
          duration_ms: { type: integer }
    ToolRan:
      name: Tool.Ran
      payload:
        type: object
        properties:
          id: { type: string }
          output: { type: object }
    FeedbackSignal:
      name: Feedback.Signal
      payload:
        type: object
        properties:
          signal:
            type: object
            properties:
              id: { type: string }
              kind: { type: string }
              target: { type: string }
              confidence: { type: number }
              severity: { type: integer }
    FeedbackSuggested:
      name: Feedback.Suggested
      payload:
        type: object
        properties:
          version: { type: integer }
          suggestions:
            type: array
            items:
              type: object
              properties:
                id: { type: string }
                action: { type: string }
                params: { type: object }
                rationale: { type: string }
                confidence: { type: number }
    FeedbackApplied:
      name: Feedback.Applied
      payload:
        type: object
        properties:
          id: { type: string }
          action: { type: string }
          params: { type: object }
YAML
info "Wrote $spec_dir/asyncapi.yaml"
