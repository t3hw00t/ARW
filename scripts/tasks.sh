#!/usr/bin/env bash
set -euo pipefail

DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd "$DIR/.." && pwd)"
TASKS="$ROOT/.arw/tasks.json"

title(){ echo -e "\033[36m\n=== $* ===\033[0m"; }
info(){ echo -e "\033[36m[tasks]\033[0m $*"; }
die(){ echo "error: $*" >&2; exit 1; }

ensure_tasks(){
  mkdir -p "$ROOT/.arw"
  if [ ! -f "$TASKS" ]; then
    printf '{"version":1,"updated":"","tasks":[]}' > "$TASKS"
  fi
}

now_utc(){ date -u +"%Y-%m-%d %H:%M:%S UTC"; }
gen_id(){ printf "t-%s-%04d" "$(date +%y%m%d%H%M%S)" "$((RANDOM%10000))"; }

list(){
  ensure_tasks
  if ! command -v jq >/dev/null 2>&1; then die "jq not installed"; fi
  echo "Status | Updated | ID | Title"
  echo "-------|---------|----|------"
  jq -r '.tasks | sort_by(.updated // "") | reverse | .[] | ( .status // "todo") + " | " + (.updated // "") + " | " + (.id // "?") + " | " + (.title // "(untitled)")' "$TASKS"
}

add(){
  ensure_tasks
  if ! command -v jq >/dev/null 2>&1; then die "jq not installed"; fi
  local title="${1:-}"; shift || true
  [ -n "$title" ] || die "usage: $0 add \"Title\" [--desc text]"
  local desc=""
  while [[ $# -gt 0 ]]; do
    case "$1" in
      --desc) desc="${2:-}"; shift 2;;
      *) die "unknown option: $1";;
    esac
  done
  local id; id=$(gen_id)
  local now; now=$(now_utc)
  tmp=$(mktemp)
  jq --arg id "$id" --arg title "$title" --arg desc "$desc" --arg now "$now" '
      .tasks += [{id:$id,title:$title,desc:$desc,status:"todo",notes:[],updated:$now}] | .updated=$now' "$TASKS" > "$tmp" && mv "$tmp" "$TASKS"
  info "Added task $id: $title"
  bash "$DIR/docgen.sh" || true
}

set_status(){
  ensure_tasks
  local id="${1:-}"; local st="${2:-}"
  [ -n "$id" ] && [ -n "$st" ] || die "usage: $0 <start|pause|done|todo> <task-id>"
  local now; now=$(now_utc)
  tmp=$(mktemp)
  jq --arg id "$id" --arg st "$st" --arg now "$now" '
    .tasks = (.tasks | map(if .id==$id then .status=$st | .updated=$now | (if $st=="done" then (.done_at=$now) else . end) else . end))
    | .updated=$now' "$TASKS" > "$tmp" && mv "$tmp" "$TASKS"
  info "Set $id -> $st"
  bash "$DIR/docgen.sh" || true
}

note(){
  ensure_tasks
  local id="${1:-}"; shift || true
  local text="${*:-}"
  [ -n "$id" ] && [ -n "$text" ] || die "usage: $0 note <task-id> \"note text\""
  local now; now=$(now_utc)
  tmp=$(mktemp)
  jq --arg id "$id" --arg now "$now" --arg text "$text" '
    .tasks = (.tasks | map(if .id==$id then .notes = ((.notes // []) + [{time:$now,text:$text}]) | .updated=$now else . end))
    | .updated=$now' "$TASKS" > "$tmp" && mv "$tmp" "$TASKS"
  info "Noted $id: $text"
  bash "$DIR/docgen.sh" || true
}

case "${1:-}" in
  list) shift; list "$@" ;;
  add) shift; add "$@" ;;
  start) shift; set_status "${1:-}" in_progress ;;
  pause) shift; set_status "${1:-}" paused ;;
  done) shift; set_status "${1:-}" "done" ;;
  todo) shift; set_status "${1:-}" todo ;;
  note) shift; note "${1:-}" "${2:-}" ;;
  *)
    cat << USAGE
Usage: $0 <command> [args]
Commands:
  list                       List tasks
  add "Title" [--desc txt]    Add a new task
  start <id>                 Mark task in_progress
  pause <id>                 Mark task paused
  done <id>                  Mark task done
  todo <id>                  Mark task todo
  note <id> "text"            Append a note
USAGE
    ;;
esac
