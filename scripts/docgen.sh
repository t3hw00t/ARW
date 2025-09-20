#!/usr/bin/env bash
set -euo pipefail

info() { echo -e "\033[36m[docgen]\033[0m $*"; }
warn() { echo -e "\033[33m[docgen]\033[0m warning: $*" >&2; }
die()  { echo "error: $*" >&2; exit 1; }

command -v cargo >/dev/null 2>&1 || die "Rust 'cargo' not found in PATH"

info "Validating feature registry"
python3 scripts/check_feature_integrity.py

info "Validating system component registry"
python3 scripts/check_system_components_integrity.py

info "Generating feature docs"
python3 scripts/gen_feature_matrix.py
python3 scripts/gen_feature_catalog.py
python3 scripts/gen_system_components.py

info "Collecting cargo metadata"
json=$(cargo metadata --no-deps --locked --format-version 1)

# Figure out repo root and blob base for links
repo_root=$(git rev-parse --show-toplevel 2>/dev/null || pwd)
# Try to infer GitHub slug from remote; fallback to known slug
remote_url=$(git config --get remote.origin.url 2>/dev/null || echo "")
if [[ "$remote_url" =~ github.com[:/]{1,2}([^/]+)/([^/.]+) ]]; then
  gh_owner="${BASH_REMATCH[1]}"
  gh_repo="${BASH_REMATCH[2]}"
  gh_slug="$gh_owner/$gh_repo"
else
  gh_slug="t3hw00t/ARW"
fi
REPO_BLOB_BASE=${REPO_BLOB_BASE:-"https://github.com/$gh_slug/blob/main/"}

title='---
title: Workspace Status
---

# Workspace Status

Generated: '$(date -u +"%Y-%m-%d %H:%M")' UTC
'

libs=$(echo "$json" | jq -r --arg root "$repo_root/" --arg base "$REPO_BLOB_BASE" '
  .packages[] | {
    name, version,
    manifest: .manifest_path,
    rel: (.manifest_path | sub("^" + $root; "")),
    kinds: ([.targets[].kind[]] | unique)
  }
  | select((.kinds | tostring) | test("lib"))
  | "- **\(.name)**: \(.version) — [\(.rel)](\($base)\(.rel))"
' || true)
bins=$(echo "$json" | jq -r --arg root "$repo_root/" --arg base "$REPO_BLOB_BASE" '
  .packages[] | {
    name, version,
    manifest: .manifest_path,
    rel: (.manifest_path | sub("^" + $root; "")),
    kinds: ([.targets[].kind[]] | unique)
  }
  | select((.kinds | tostring) | test("bin"))
  | "- **\(.name)**: \(.version) — [\(.rel)](\($base)\(.rel))"
' || true)

out="$title\n\n## Libraries\n${libs:-_none_}\n\n## Binaries\n${bins:-_none_}\n"

dest="$(cd "$(dirname "$0")/.." && pwd)/docs/developer/status.md"
info "Writing $dest"
# Interpret \n sequences in $out as real newlines
printf "%b" "$out" > "$dest"
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
    make_sec "done" "Done"; echo
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
  title: "arw-server events"
  version: "0.1.0"
  description: "Normalized dot.case event channels for the unified server."
  license:
    name: "MIT OR Apache-2.0"
  contact:
    name: "ARW"
    url: "https://github.com/t3hw00t/ARW"
    email: "noreply@example.com"
defaultContentType: application/json
tags:
  - name: CloudEvents
    description: Events include CloudEvents 1.0 metadata under `ce`.
channels:
  service.start:
    subscribe:
      operationId: service_start
      description: Service emitted start event
      message:
        $ref: '#/components/messages/ServiceStart'
  service.health:
    subscribe:
      operationId: service_health
      description: Periodic health heartbeat
      message:
        $ref: '#/components/messages/ServiceHealth'
  service.test:
    subscribe:
      operationId: service_test
      description: Test event emission
      message:
        $ref: '#/components/messages/ServiceTest'
  governor.changed:
    subscribe:
      operationId: governor_changed
      description: Governor profile changed
      message:
        $ref: '#/components/messages/GovernorChanged'
  memory.applied:
    subscribe:
      operationId: memory_applied
      description: Memory applied to working set
      message:
        $ref: '#/components/messages/MemoryApplied'
  models.changed:
    subscribe:
      operationId: models_changed
      description: Models list/default changed
      message:
        $ref: '#/components/messages/ModelsChanged'
  models.download.progress:
    subscribe:
      operationId: models_download_progress
      description: Download progress, status codes, metrics snapshots
      message:
        $ref: '#/components/messages/ModelsDownloadProgress'
  models.manifest.written:
    subscribe:
      operationId: models_manifest_written
      description: A model manifest has been written
      message:
        $ref: '#/components/messages/ModelsManifestWritten'
  models.cas.gc:
    subscribe:
      operationId: models_cas_gc
      description: CAS GC run summary
      message:
        $ref: '#/components/messages/ModelsCasGc'
  egress.preview:
    subscribe:
      operationId: egress_preview
      description: Egress preflight summary
      message:
        $ref: '#/components/messages/EgressPreview'
  egress.ledger.appended:
    subscribe:
      operationId: egress_ledger_appended
      description: Egress decision appended to ledger
      message:
        $ref: '#/components/messages/EgressLedgerAppended'
  tool.ran:
    subscribe:
      operationId: tool_ran
      description: Tool execution completed
      message:
        $ref: '#/components/messages/ToolRan'
  feedback.signal:
    subscribe:
      operationId: feedback_signal
      description: Feedback signal recorded
      message:
        $ref: '#/components/messages/FeedbackSignal'
  feedback.suggested:
    subscribe:
      operationId: feedback_suggested
      description: Feedback suggestion produced
      message:
        $ref: '#/components/messages/FeedbackSuggested'
  feedback.applied:
    subscribe:
      operationId: feedback_applied
      description: Feedback suggestion applied
      message:
        $ref: '#/components/messages/FeedbackApplied'
  rpu.trust.changed:
    subscribe:
      operationId: rpu_trust_changed
      description: Trust store changed/reloaded
      message:
        $ref: '#/components/messages/RpuTrustChanged'
components:
  messages:
    ServiceStart:
      name: service.start
      payload:
        type: object
        additionalProperties: true
    ServiceHealth:
      name: service.health
      payload:
        type: object
        properties:
          ok: { type: boolean }
    ServiceTest:
      name: service.test
      payload:
        type: object
        additionalProperties: true
    GovernorChanged:
      name: governor.changed
      payload:
        type: object
        properties:
          profile: { type: string }
    MemoryApplied:
      name: memory.applied
      payload:
        type: object
        additionalProperties: true
    ModelsChanged:
      name: models.changed
      payload:
        type: object
        additionalProperties: true
    ModelsDownloadProgress:
      name: models.download.progress
      payload:
        type: object
        properties:
          id: { type: string }
          status:
            type: string
            enum:
              - started
              - downloading
              - degraded
              - canceled
              - complete
              - no-active-job
              - error
          code:
            type: string
            enum:
              - soft-budget
              - hard-budget
              - request-timeout
              - http
              - io
              - size_limit
              - quota_exceeded
              - disk_insufficient
              - sha256_mismatch
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
      name: models.manifest.written
      payload:
        type: object
        properties:
          id: { type: string }
          manifest_path: { type: string }
          sha256: { type: ["string","null"] }
          cas: { type: ["string","null"] }
    ModelsCasGc:
      name: models.cas.gc
      payload:
        type: object
        properties:
          scanned: { type: integer }
          kept: { type: integer }
          deleted: { type: integer }
          deleted_bytes: { type: integer }
          ttl_days: { type: integer }
    EgressPreview:
      name: egress.preview
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
      name: egress.ledger.appended
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
      name: tool.ran
      payload:
        type: object
        properties:
          id: { type: string }
          output: { type: object }
    FeedbackSignal:
      name: feedback.signal
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
      name: feedback.suggested
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
      name: feedback.applied
      payload:
        type: object
        properties:
          id: { type: string }
          action: { type: string }
          params: { type: object }
    RpuTrustChanged:
      name: rpu.trust.changed
      payload:
        type: object
        properties:
          count: { type: integer }
          path: { type: string }
YAML
info "Wrote $spec_dir/asyncapi.yaml"
# OpenAPI JSON (for quick preview/static consumption)
if command -v python3 >/dev/null 2>&1; then
  python3 "$root_dir/scripts/generate_openapi_json.py" || true
fi
