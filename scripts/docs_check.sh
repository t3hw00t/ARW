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
  # Accept either Updated:, Generated:, the stable generator headers used by
  # our docgen scripts ("_Generated ..."), and spec diff header ("Base: ")
  head -n 40 "$f" | grep -Eq '^(Updated:|Generated:|_Last updated:|_Generated |Base: )' || {
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

# Banned legacy flags or terminology creeping back in
info "Checking for banned legacy flags"
python3 - <<'PY' "$docs_dir" "$root_dir/README.md"
import sys, pathlib
root_docs = pathlib.Path(sys.argv[1])
readme = pathlib.Path(sys.argv[2])
targets = list(root_docs.rglob('*.md')) + [readme]
hits = []
for path in targets:
    try:
        text = path.read_text(encoding='utf-8')
    except Exception as exc:
        print(f"[error] {path}: cannot read ({exc})")
        continue
    for idx, line in enumerate(text.splitlines(), 1):
        if '--Legacy' in line or ' -Legacy' in line:
            hits.append((path, idx, line.strip()))
if hits:
    print('[error] Deprecated legacy flags detected:')
    for path, line_no, line in hits:
        rel = path.relative_to(pathlib.Path.cwd())
        print(f"  {rel}:{line_no}: {line}")
    # communicate error count via sentinel
    print(f"__DOCS_CHECK_LEGACY_HITS__={len(hits)}")
PY
legacy_hits=$(grep -oE '__DOCS_CHECK_LEGACY_HITS__=[0-9]+' - | cut -d= -f2 || echo 0)
if [[ "${legacy_hits:-0}" -gt 0 ]]; then
  errors=$((errors+legacy_hits))
fi

# Guard against reintroducing removed legacy admin routes outside docs/spec
info "Scanning code for legacy admin route references"
python3 - <<'PY' "$root_dir"
import sys, pathlib

root = pathlib.Path(sys.argv[1])
blocked = ['/' + 'admin' + '/state/', '/' + 'admin' + '/projects/']
skip_dirs = {".git", ".arw", "docs", "spec", "target", "site", "vendor", "sandbox"}
allowed_suffixes = {
    ".rs", ".js", ".ts", ".tsx", ".jsx", ".mjs", ".cjs", ".json",
    ".toml", ".yaml", ".yml", ".py", ".sh", ".bash", ".zsh", ".fish",
    ".go", ".rb", ".kt", ".swift", ".java", ".cs", ".html", ".css",
    ".scss", ".sass", ".less", ".mdx", ".txt"
}

def should_skip(path: pathlib.Path) -> bool:
    parts = set(p.name for p in path.parents)
    return bool(skip_dirs & parts)

hits = []
for file in root.rglob('*'):
    if not file.is_file():
        continue
    if should_skip(file):
        continue
    if file.suffix and file.suffix.lower() not in allowed_suffixes:
        continue
    try:
        text = file.read_text(encoding='utf-8')
    except Exception:
        continue
    for idx, line in enumerate(text.splitlines(), 1):
        for pattern in blocked:
            if pattern in line:
                hits.append((file, idx, line.strip()))

if hits:
    print('[error] Legacy admin route references detected:')
    for path, line_no, line in hits[:50]:
        rel = path.relative_to(root)
        print(f"  {rel}:{line_no}: {line}")
    print(f"__DOCS_CHECK_LEGACY_ROUTES__={len(hits)}")
PY
legacy_routes=$(grep -oE '__DOCS_CHECK_LEGACY_ROUTES__=[0-9]+' - | cut -d= -f2 || echo 0)
if [[ "${legacy_routes:-0}" -gt 0 ]]; then
  errors=$((errors+legacy_routes))
fi

info "Ensuring legacy capsule header is not reintroduced"
python3 - <<'PY' "$root_dir"
import sys, pathlib

root = pathlib.Path(sys.argv[1])
blocked = "X-ARW-Gate"
allowed_dirs = {root / "docs"}
allowed_files = {
    root / "apps" / "arw-server" / "src" / "capsule_guard.rs",
    root / "scripts" / "docs_check.sh",
    root / "CHANGELOG.md",
}
skip_names = {".git", ".arw", "target", "site", "vendor", "sandbox", "node_modules"}

def should_skip(path: pathlib.Path) -> bool:
    return any(part.name in skip_names for part in path.parents)

hits = []
for file in root.rglob('*'):
    if not file.is_file():
        continue
    if file in allowed_files:
        continue
    if should_skip(file):
        continue
    if any(parent in allowed_dirs for parent in file.parents):
        continue
    try:
        text = file.read_text(encoding='utf-8')
    except Exception:
        continue
    if blocked in text:
        hits.append(file.relative_to(root))

if hits:
    print('[error] Legacy capsule header detected:')
    for path in hits[:50]:
        print(f"  {path}")
    print(f"__DOCS_CHECK_CAPSULE_HEADER__={len(hits)}")
PY
legacy_capsule=$(grep -oE '__DOCS_CHECK_CAPSULE_HEADER__=[0-9]+' - | cut -d= -f2 || echo 0)
if [[ "${legacy_capsule:-0}" -gt 0 ]]; then
  errors=$((errors+legacy_capsule))
fi

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
