#!/usr/bin/env bash
set -euo pipefail

root_dir="$(cd "$(dirname "$0")/.." && pwd)"
docs_dir="$root_dir/docs"
mkdocs_yml="$root_dir/mkdocs.yml"

blue='\033[34m'; red='\033[31m'; yellow='\033[33m'; reset='\033[0m'
info() { echo -e "${blue}[docs-check]${reset} $*"; }
warn() { echo -e "${yellow}[warn]${reset} $*"; }
err()  { echo -e "${red}[error]${reset} $*"; }

errors=0; warnings=0

info "Building docs with mkdocs --strict to catch nav issues"
if command -v "$root_dir/.venv/bin/mkdocs" >/dev/null 2>&1; then
  "$root_dir/.venv/bin/mkdocs" build --strict -f "$mkdocs_yml" >/dev/null
elif command -v mkdocs >/dev/null 2>&1; then
  mkdocs build --strict -f "$mkdocs_yml" >/dev/null
else
  warn "mkdocs not found; skipping build check"
  warnings=$((warnings+1))
fi

info "Scanning markdown files under docs/"

# Collect list of .md files
mapfile -t files < <(find "$docs_dir" -type f -name "*.md" | sort)

# Simple heading/title checks and Updated line
for f in "${files[@]}"; do
  rel="${f#"$docs_dir"/}"
  # Extract front-matter title if present
  fm_title=""
  if head -n 1 "$f" | grep -q '^---$'; then
    fm_title=$(awk '/^---$/{c++;next} c==1 && /^title:/ {sub(/^title:[ ]*/,""); print; exit}' "$f" || true)
  fi
  h1=$(grep -m1 -E '^# ' "$f" | sed 's/^# \s*//') || true
  if [[ -n "$fm_title" && -n "$h1" ]]; then
    # normalize: strip punctuation and lowercase
    n1=$(echo "$fm_title" | tr '[:upper:]' '[:lower:]' | tr -cd '[:alnum:] ')
    n2=$(echo "$h1"       | tr '[:upper:]' '[:lower:]' | tr -cd '[:alnum:] ')
    if [[ "$n1" != "$n2" ]]; then
      warn "$rel: title/front-matter and H1 differ — '$fm_title' vs '$h1'"
      warnings=$((warnings+1))
    fi
  fi
  # Updated or Generated line near top (first 40 lines)
  head -n 40 "$f" | grep -Eq '^(Updated:|Generated:)' || {
    warn "$rel: missing Updated:/Generated: information"
    warnings=$((warnings+1))
  }
  # Heading case scan: flag lowercase starts for H2/H3
  while IFS= read -r line; do
    text="${line### }"; text="${text#### }"
    first=${text:0:1}
    if [[ "$first" =~ [a-z] ]]; then
      warn "$rel: heading not Title Case → '$text'"
      warnings=$((warnings+1))
    fi
  done < <(grep -E '^(##|###) ' "$f" || true)
done

# Link check for relative .md references
info "Checking relative links to .md files"
python3 - << 'PY' "$docs_dir"
import os, re, sys
docs_dir = sys.argv[1]
errors = 0
for root, _, files in os.walk(docs_dir):
  for name in files:
    if not name.endswith('.md'): continue
    path = os.path.join(root, name)
    rel  = os.path.relpath(path, docs_dir)
    try:
      text = open(path, 'r', encoding='utf-8').read()
    except Exception as e:
      print(f"[error] {rel}: cannot read: {e}")
      errors += 1; continue
    for m in re.finditer(r'\[[^\]]+\]\(([^)]+\.md)(#[^)]+)?\)', text):
      href = m.group(1)
      if href.startswith('http://') or href.startswith('https://'):
        continue
      # Resolve relative to current file
      target = os.path.normpath(os.path.join(os.path.dirname(path), href))
      if not os.path.isabs(target):
        target = os.path.normpath(os.path.join(docs_dir, os.path.relpath(target, docs_dir)))
      if not os.path.exists(target):
        print(f"[error] {rel}: broken link → {href}")
        errors += 1
print(f"__DOCS_CHECK_LINK_ERRORS__={errors}")
PY
link_errors=$(grep -oE '__DOCS_CHECK_LINK_ERRORS__=[0-9]+' - | cut -d= -f2 || echo 0)

if [[ "${link_errors:-0}" -gt 0 ]]; then
  errors=$((errors+link_errors))
fi

info "Done. warnings=$warnings errors=$errors"

# By default, do not fail on warnings; fail only on errors (e.g., broken links)
if [[ ${errors} -gt 0 ]]; then
  err "Found ${errors} errors"
  exit 1
fi
exit 0
