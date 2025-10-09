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

usage() {
  cat <<'EOF'
Usage: bash scripts/docs_check.sh [options]

Options:
  --skip-mkdocs    Skip the mkdocs --strict build (or set DOCS_CHECK_SKIP_MKDOCS=1).
  --fast           Convenience alias for setting DOCS_CHECK_FAST=1 (skips mkdocs + heavy scans).
  -h, --help       Show this message.
EOF
}

skip_mkdocs=${DOCS_CHECK_SKIP_MKDOCS:-0}
fast_mode=${DOCS_CHECK_FAST:-0}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --skip-mkdocs) skip_mkdocs=1; shift ;;
    --fast) fast_mode=1; skip_mkdocs=1; shift ;;
    -h|--help) usage; exit 0 ;;
    *) err "Unknown option: $1"; usage; exit 2 ;;
  esac
done

if [[ "$fast_mode" == "1" ]]; then
  skip_mkdocs=1
  warn "DOCS_CHECK_FAST enabled: skipping mkdocs build and Python-based sweeps."
fi

rg_bin=""
have_rg=0
if command -v rg >/dev/null 2>&1; then
  rg_bin="$(command -v rg)"
  have_rg=1
else
  warn "ripgrep (rg) not found; skipping legacy term sweeps. Install via 'mise install' or 'brew install ripgrep'."
  warnings=$((warnings+1))
fi

PYTHON_BIN="${PYTHON:-${PYTHON3:-}}"
if [[ -z "$PYTHON_BIN" ]]; then
  if command -v python3 >/dev/null 2>&1; then
    PYTHON_BIN="$(command -v python3)"
  elif command -v python >/dev/null 2>&1; then
    PYTHON_BIN="$(command -v python)"
  fi
fi
if [[ -z "$PYTHON_BIN" ]]; then
  warn "python not found; skipping Python-based docs checks. Install via mise or run 'bash scripts/bootstrap_docs.sh' after adding Python."
  warnings=$((warnings+1))
fi

if [[ "$skip_mkdocs" != "1" ]]; then
  info "Building docs with mkdocs --strict to catch nav issues"
  if command -v "$root_dir/.venv/bin/mkdocs" >/dev/null 2>&1; then
    "$root_dir/.venv/bin/mkdocs" build --strict -f "$mkdocs_yml" >/dev/null
  elif command -v mkdocs >/dev/null 2>&1; then
    mkdocs build --strict -f "$mkdocs_yml" >/dev/null
  else
    warn "mkdocs not found; skipping build check"
    warn "Install docs toolchain via 'mise run bootstrap:docs' or 'bash scripts/bootstrap_docs.sh'."
    warnings=$((warnings+1))
  fi
else
  warn "Skipping mkdocs build (--skip-mkdocs or DOCS_CHECK_SKIP_MKDOCS=1)"
  warnings=$((warnings+1))
fi

info "Scanning markdown files under docs/"

# Collect list of .md files
files=()
if type mapfile >/dev/null 2>&1; then
  mapfile -t files < <(find "$docs_dir" -type f -name "*.md" | sort)
else
  while IFS= read -r file; do
    files+=("$file")
  done < <(find "$docs_dir" -type f -name "*.md" | sort)
fi

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
  if ! awk 'NR>40 {exit 0} $0 ~ /^[[:space:]]*(Updated:|Generated:|_Last updated:|_Generated |Base: )/ {found=1; exit} END {exit found?0:1}' "$f"; then
    warn "$rel: missing Updated:/Generated: information"
    warnings=$((warnings+1))
  fi
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
if [[ "$fast_mode" == "1" ]]; then
  warn "Skipping legacy flag sweep (fast mode)"
  warnings=$((warnings+1))
elif [[ $have_rg -ne 1 ]]; then
  warn "Skipping legacy flag sweep (ripgrep not installed)"
  warnings=$((warnings+1))
else
  info "Checking for banned legacy flags"
  legacy_hits=$("$rg_bin" --no-messages --with-filename --line-number --regexp '(--Legacy| -Legacy)' "$docs_dir" "$root_dir/README.md" || true)
  if [[ -n "$legacy_hits" ]]; then
    err "Deprecated legacy flags detected:"
    printf "  %s\n" "$legacy_hits"
    hits_count=$(printf "%s" "$legacy_hits" | wc -l | tr -d ' ')
    errors=$((errors+hits_count))
  fi
fi

# Guard against reintroducing removed legacy admin routes outside docs/spec
if [[ "$fast_mode" == "1" ]]; then
  warn "Skipping legacy admin route scan (fast mode)"
  warnings=$((warnings+1))
elif [[ $have_rg -ne 1 ]]; then
  warn "Skipping legacy admin route scan (ripgrep not installed)"
  warnings=$((warnings+1))
else
  info "Scanning code for legacy admin route references"
  legacy_routes=$("$rg_bin" --no-messages --with-filename --line-number --regexp '/admin/(state|projects)/' "$root_dir" \
    --glob '!docs/**' --glob '!spec/**' --glob '!.git/**' --glob '!target/**' --glob '!site/**' --glob '!vendor/**' \
    --glob '!sandbox/**' --glob '!node_modules/**' --glob '!dist/**' || true)
  if [[ -n "$legacy_routes" ]]; then
    err "Legacy admin route references detected:"
    printf "  %s\n" "$legacy_routes"
    routes_count=$(printf "%s" "$legacy_routes" | wc -l | tr -d ' ')
    errors=$((errors+routes_count))
  fi
fi

if [[ "$fast_mode" == "1" ]]; then
  warn "Skipping capsule header sweep (fast mode)"
  warnings=$((warnings+1))
elif [[ $have_rg -ne 1 ]]; then
  warn "Skipping legacy capsule header scan (ripgrep not installed)"
  warnings=$((warnings+1))
else
  info "Ensuring legacy capsule header is not reintroduced"
  capsule_hits=$("$rg_bin" --no-messages --with-filename --line-number 'X-ARW-Gate' "$root_dir" \
    --glob '!docs/**' --glob '!target/**' --glob '!site/**' --glob '!vendor/**' --glob '!sandbox/**' \
    --glob '!node_modules/**' --glob '!dist/**' --glob '!spec/**' || true)
  if [[ -n "$capsule_hits" ]]; then
    # Allow known files
    filtered=""
    while IFS= read -r line; do
      file="${line%%:*}"
      if [[ "$file" =~ ^[A-Za-z]$ && "$line" == [A-Za-z]:/* ]]; then
        drive_prefix="${line:0:2}"
        rest="${line:2}"
        file="${drive_prefix}${rest%%:*}"
      fi
      norm="${file//\\//}"
      case "$norm" in
        */apps/arw-server/src/capsule_guard.rs|*/scripts/docs_check.sh|*/CHANGELOG.md|*/scripts/check_legacy_surface.sh)
          continue
          ;;
      esac
      filtered+="${line}"$'\n'
    done <<<"$capsule_hits"
    if [[ -n "$filtered" ]]; then
      err "Legacy capsule header detected:"
      printf "  %s\n" "$filtered"
      capsule_count=$(printf "%s" "$filtered" | wc -l | tr -d ' ')
      errors=$((errors+capsule_count))
    fi
  fi
fi

if [[ "$fast_mode" == "1" ]]; then
  warn "Skipping legacy Models.* sweep (fast mode)"
  warnings=$((warnings+1))
elif [[ $have_rg -ne 1 ]]; then
  warn "Skipping legacy Models.* sweep (ripgrep not installed)"
  warnings=$((warnings+1))
else
  info "Ensuring docs avoid legacy Models.* event names"
  models_hits=$("$rg_bin" --pcre2 --no-messages --with-filename --line-number --regexp 'Models\.(?!\*)' "$docs_dir" --glob '!release_notes.md' || true)
  if [[ -n "$models_hits" ]]; then
    err "Legacy Models.* references detected:"
    printf "  %s\n" "$models_hits"
    models_count=$(printf "%s" "$models_hits" | wc -l | tr -d ' ')
    errors=$((errors+models_count))
  fi
fi

# Link check for relative .md references
if [[ "$fast_mode" == "1" ]]; then
  warn "Skipping relative link scan (fast mode)"
  warnings=$((warnings+1))
elif [[ -n "$PYTHON_BIN" ]]; then
  info "Checking relative links to .md files"
  link_output=$("$PYTHON_BIN" - <<'PY' "$docs_dir"
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
)
  printf "%s" "$link_output"
  link_errors=$(printf "%s" "$link_output" | grep -oE '__DOCS_CHECK_LINK_ERRORS__=[0-9]+' | cut -d= -f2 || echo 0)
else
  warn "python not found; skipping relative link scan"
  warnings=$((warnings+1))
  link_errors=0
fi

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
