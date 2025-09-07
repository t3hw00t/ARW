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

