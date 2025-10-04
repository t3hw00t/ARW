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

info "Regenerating event topics reference"
python3 scripts/gen_topics_doc.py

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

# Build release binaries once so downstream steps can reuse them
build_ok=0
if cargo build --release -p arw-server -p arw-cli >/dev/null 2>&1; then
  build_ok=1
else
  warn "cargo build --release (arw-server/arw-cli) failed"
fi

if [ "$build_ok" -eq 1 ]; then
  info "Rendering gating references"
  if "$root_dir/target/release/arw-cli" gate keys --json --pretty > "$root_dir/docs/GATING_KEYS.json"; then
    info "Wrote docs/GATING_KEYS.json"
  else
    warn "failed to generate GATING_KEYS.json"
  fi
  if "$root_dir/target/release/arw-cli" gate keys --doc > "$root_dir/docs/GATING_KEYS.md"; then
    info "Wrote docs/GATING_KEYS.md"
  else
    warn "failed to render GATING_KEYS.md"
  fi
  if "$root_dir/target/release/arw-cli" gate config schema --pretty > "$root_dir/docs/reference/gating_config.schema.json"; then
    info "Wrote docs/reference/gating_config.schema.json"
  else
    warn "failed to generate gating_config.schema.json"
  fi
  if "$root_dir/target/release/arw-cli" gate config doc > "$root_dir/docs/reference/gating_config.md"; then
    info "Wrote docs/reference/gating_config.md"
  else
    warn "failed to render gating_config.md"
  fi
fi

# OpenAPI output directly from annotated ApiDoc
if [ "$build_ok" -eq 1 ]; then
  info "Generating OpenAPI from annotations"
  if OPENAPI_OUT="$spec_dir/openapi.yaml" "$root_dir/target/release/arw-server"; then
    info "Wrote $spec_dir/openapi.yaml"
    if command -v python3 >/dev/null 2>&1; then
      set +e
      python3 "$root_dir/scripts/ensure_openapi_descriptions.py"
      normalize_status=$?
      set -e
      if [ "$normalize_status" -eq 0 ]; then
        :
      elif [ "$normalize_status" -eq 1 ]; then
        info "Normalized OpenAPI descriptions"
      else
        warn "failed to normalize openapi descriptions"
      fi
    fi
  else
    warn "failed to generate openapi.yaml"
  fi
else
  warn "skipping OpenAPI generation (arw-server build failed)"
fi

# MCP tools via arw-cli (requires successful build)
if [ "$build_ok" -eq 1 ]; then
  if "$root_dir/target/release/arw-cli" tools > "$spec_dir/mcp-tools.json" 2>/dev/null; then
    info "Wrote $spec_dir/mcp-tools.json"
  else
    warn "failed to generate mcp-tools.json"
  fi
else
  warn "arw-cli build failed; skipping mcp-tools.json"
fi

# AsyncAPI generation driven from arw-topics
info "Generating AsyncAPI"
if python3 "$root_dir/scripts/gen_asyncapi.py" >/dev/null 2>&1; then
  info "Wrote $spec_dir/asyncapi.yaml"
else
  warn "failed to generate asyncapi.yaml"
fi
# OpenAPI JSON (for quick preview/static consumption)
if command -v python3 >/dev/null 2>&1; then
  python3 "$root_dir/scripts/generate_openapi_json.py" || true
fi

info "Generating interface release notes"
if command -v python3 >/dev/null 2>&1; then
  base_ref="${BASE_REF:-origin/main}"
  BASE_REF="$base_ref" python3 "$root_dir/scripts/generate_interface_release_notes.py" || \
    warn "interface release notes generation failed"
else
  warn "python3 unavailable; skipping interface release notes"
fi
