#!/usr/bin/env bash
set -euo pipefail

# Ubuntu 24.04 WSL cleanup utility: trims caches, logs, temp files, and stale processes.

readonly PROG_NAME="$(basename "$0")"

DRY_RUN=0
SKIP_KILL=0
SKIP_APT=0
SKIP_LOGS=0
SKIP_USER_CACHE=0
ALL_USERS=0
TARGET_USER=""
MIN_IDLE_SECONDS=900
TMP_RETENTION_HOURS=24
LOG_RETENTION_DAYS=14
KILL_LISTENERS=0
LISTENER_PORT_MIN=1024
LISTENER_PORT_MAX=65535
LISTENER_INCLUDE_REGEX_DEFAULT='(node|npm|yarn|pnpm|bun|deno|python|uvicorn|gunicorn|pytest|php|ruby|rails|django|java|gradle|dotnet|cargo|go|webpack|vite)'
LISTENER_EXCLUDE_REGEX_DEFAULT='(ssh|sshd|systemd|docker|containerd|wslhost|code)'
LISTENER_INCLUDE_REGEX="$LISTENER_INCLUDE_REGEX_DEFAULT"
LISTENER_EXCLUDE_REGEX="$LISTENER_EXCLUDE_REGEX_DEFAULT"

EXCLUDE_COMM_REGEX_DEFAULT='^(systemd|init|dbus-daemon|cron|atd|rsyslogd|ssh-agent|tmux|[(]?sd-pam[)]?|gnome-keyring|gnome-keyring-d|at-spi-bus-laun|at-spi2-registr|gvfsd)$'
EXCLUDE_COMM_REGEX="${EXCLUDE_COMM_REGEX:-$EXCLUDE_COMM_REGEX_DEFAULT}"

timestamp() { date '+%Y-%m-%dT%H:%M:%S%z'; }
log_info() { printf '%s [INFO] %s\n' "$(timestamp)" "$*"; }
log_warn() { printf '%s [WARN] %s\n' "$(timestamp)" "$*" >&2; }
log_error() { printf '%s [ERR ] %s\n' "$(timestamp)" "$*" >&2; }

usage() {
    cat <<EOF
Usage: sudo $PROG_NAME [options]

Options:
  -n, --dry-run                  Show what would be done without changing anything.
  --skip-kill                    Skip stale background process cleanup.
  --kill-listeners               Kill user-owned TCP listener processes (matches include regex).
  --skip-apt                     Skip apt cache cleanup.
  --skip-logs                    Skip log cleanup and journald vacuum.
  --skip-user-caches             Skip per-user cache cleanup.
  --all-users                    Operate on all non-system users (UID >= 1000).
  --user <name>                  Target a specific user.
  --min-idle-seconds=<seconds>   Process idle threshold (default: ${MIN_IDLE_SECONDS}).
  --tmp-retention-hours=<hrs>    Remove /tmp entries older than this (default: ${TMP_RETENTION_HOURS}).
  --log-retention-days=<days>    Keep logs for this many days (default: ${LOG_RETENTION_DAYS}).
  --exclude-comm=<regex>         Add commands (regex) to preserve from kill list.
  --listener-port-min=<port>     Only target listeners on TCP ports >= value (default: ${LISTENER_PORT_MIN}).
  --listener-port-max=<port>     Only target listeners on TCP ports <= value (default: ${LISTENER_PORT_MAX}).
  --listener-include=<regex>     Override listener include regex (default: ${LISTENER_INCLUDE_REGEX_DEFAULT}).
  --listener-exclude=<regex>     Extend listener exclude regex to preserve processes.
  -h, --help                     Show this help message.

Environment overrides:
  EXCLUDE_COMM_REGEX             Override the process command exclusion regex completely.
EOF
}

require_arg() {
    local opt="$1" val="$2"
    if [[ -z "$val" ]]; then
        log_error "Option '$opt' expects a value."
        exit 1
    fi
}

is_unsigned_int() { [[ "$1" =~ ^[0-9]+$ ]]; }

report_disk_usage() {
    local label="$1"
    log_info "Filesystem usage (${label}):"
    df -hT / 2>/dev/null | awk 'NR==1 {printf "  %s\n",$0; next} {printf "  %s\n",$0}'
}

cleanup_tmp() {
    local minutes="$1"
    local dirs=("/tmp" "/var/tmp")
    log_info "Cleaning temporary directories (older than ${minutes} minutes)."
    for dir in "${dirs[@]}"; do
        [[ -d "$dir" ]] || continue
        log_info "  -> $dir"
        if (( DRY_RUN )); then
            find "$dir" -mindepth 1 -mmin "+${minutes}" -print
        else
            find "$dir" -mindepth 1 -mmin "+${minutes}" -exec rm -rf {} + 2>/dev/null
        fi
    done
}

cleanup_crash_dumps() {
    local crash_dir="/var/crash"
    [[ -d "$crash_dir" ]] || return
    log_info "Removing crash dumps from $crash_dir"
    if (( DRY_RUN )); then
        find "$crash_dir" -mindepth 1 -print
    else
        find "$crash_dir" -mindepth 1 -delete
    fi
}

cleanup_coredumps() {
    local core_dir="/var/lib/systemd/coredump"
    [[ -d "$core_dir" ]] || return
    log_info "Removing systemd coredumps from $core_dir"
    if (( DRY_RUN )); then
        find "$core_dir" -mindepth 1 -print
    else
        find "$core_dir" -mindepth 1 -delete
    fi
}

cleanup_user_cache() {
    local user="$1" home="$2" minutes="$3"
    [[ -d "$home" ]] || { log_warn "Home directory $home not found for user $user; skipping."; return; }

    log_info "Cleaning caches for $user ($home)"
    local paths=(
        "$home/.cache/pip"
        "$home/.cache/pipenv"
        "$home/.cache/pnpm"
        "$home/.cache/npm"
        "$home/.npm/_cacache"
        "$home/.cache/node-gyp"
        "$home/.cache/thumbnails"
        "$home/.local/share/Trash/files"
        "$home/.local/share/Trash/info"
    )
    for path in "${paths[@]}"; do
        [[ -e "$path" ]] || continue
        log_info "  -> Removing $path"
        if (( DRY_RUN )); then
            log_info "[dry-run] rm -rf $path"
        else
            rm -rf "$path"
        fi
    done

    if [[ -d "$home/.cache" ]]; then
        log_info "  -> Purging entries in $home/.cache older than ${minutes} minutes"
        if (( DRY_RUN )); then
            find "$home/.cache" -mindepth 1 -mmin "+${minutes}" -print
        else
            find "$home/.cache" -mindepth 1 -mmin "+${minutes}" -exec rm -rf {} + 2>/dev/null
        fi
    fi

    log_info "  -> Removing __pycache__ directories older than 7 days"
    if (( DRY_RUN )); then
        find "$home" -path "$home/.cache" -prune -o -type d -name '__pycache__' -mtime +7 -print
    else
        find "$home" -path "$home/.cache" -prune -o -type d -name '__pycache__' -mtime +7 -exec rm -rf {} + 2>/dev/null
    fi
}

cleanup_apt() {
    log_info "Cleaning apt caches and unused packages"
    if (( DRY_RUN )); then
        log_info "[dry-run] apt-get -y autoremove --purge"
        log_info "[dry-run] apt-get autoclean"
        log_info "[dry-run] apt-get clean"
    else
        export DEBIAN_FRONTEND=noninteractive
        apt-get -y autoremove --purge
        apt-get autoclean
        apt-get clean
    fi
}

cleanup_logs() {
    log_info "Pruning system logs older than ${LOG_RETENTION_DAYS} days"
    if (( DRY_RUN )); then
        find /var/log -type f \( -name '*.gz' -o -name '*.xz' -o -name '*.old' -o -name '*.1' \) -mtime "+${LOG_RETENTION_DAYS}" -print
    else
        find /var/log -type f \( -name '*.gz' -o -name '*.xz' -o -name '*.old' -o -name '*.1' \) -mtime "+${LOG_RETENTION_DAYS}" -delete
    fi

    if command -v journalctl >/dev/null 2>&1; then
        if (( DRY_RUN )); then
            log_info "[dry-run] journalctl --vacuum-time=${LOG_RETENTION_DAYS}d"
        else
            if ! journalctl --vacuum-time="${LOG_RETENTION_DAYS}d"; then
                log_warn "journalctl vacuum failed (systemd may be disabled in this WSL instance)."
            fi
        fi
    else
        log_info "journalctl not available; skipping journal vacuum."
    fi

    for file in /var/log/wtmp /var/log/btmp /var/log/lastlog; do
        [[ -f "$file" ]] || continue
        if (( DRY_RUN )); then
            log_info "[dry-run] truncate -s 0 $file"
        else
            : > "$file"
        fi
    done
}

kill_stale_processes() {
    local user="$1" min="$2"
    log_info "Scanning for stale background processes owned by $user (idle >= ${min}s, no TTY)"
    local found=0
    local script_pid="$$"
    local script_ppid="${PPID:-}"
    while IFS=$'\t' read -r pid etimes stat comm; do
        [[ "$pid" == "$script_pid" || "$pid" == "$script_ppid" ]] && continue
        found=1
        log_info "  -> PID $pid ($comm) idle ${etimes}s: sending SIGTERM"
        if (( DRY_RUN )); then
            log_info "[dry-run] kill -15 $pid"
            continue
        fi
        if kill -15 "$pid" 2>/dev/null; then
            sleep 1
            if kill -0 "$pid" 2>/dev/null; then
                log_warn "PID $pid survived SIGTERM; sending SIGKILL"
                kill -9 "$pid" 2>/dev/null || true
            fi
        else
            log_warn "PID $pid no longer present."
        fi
    done < <(
        ps -eo pid=,user=,etimes=,tty=,stat=,comm= \
        | awk -v target="$user" -v min="$min" -v pattern="$EXCLUDE_COMM_REGEX" '
            $2 == target && $3 >= min && $4 == "?" && $5 !~ /[RZ]/ && $6 !~ pattern {
                printf "%s\t%s\t%s\t%s\n", $1, $3, $5, $6
            }'
    )
    (( found == 0 )) && log_info "No stale processes found for user $user."
}

kill_listening_ports() {
    command -v lsof >/dev/null 2>&1 || { log_warn "lsof not available; skipping listener cleanup."; return; }
    (( ${#USERS_TO_CLEAN[@]} == 0 )) && { log_info "No user context for listener cleanup; skipping."; return; }

    log_info "Scanning for user-owned TCP listeners on ports ${LISTENER_PORT_MIN}-${LISTENER_PORT_MAX}"

    local -A TARGET_USER_SET=()
    for user in "${USERS_TO_CLEAN[@]}"; do
        TARGET_USER_SET["$user"]=1
    done

    local -A PROCESSED_PIDS=()
    local matched=0
    local record_pid="" record_cmd="" record_user="" record_addr=""

    while IFS= read -r line; do
        case "$line" in
            p*) record_pid="${line#p}" ;;
            c*) record_cmd="${line#c}" ;;
            L*) record_user="${line#L}" ;;
            n*)
                record_addr="${line#n}"
                [[ -n "$record_pid" && -n "$record_cmd" && -n "$record_user" ]] || continue
                [[ -n "${TARGET_USER_SET[$record_user]:-}" ]] || continue
                local port="${record_addr##*:}"
                [[ "$port" =~ ^[0-9]+$ ]] || continue
                (( port < LISTENER_PORT_MIN || port > LISTENER_PORT_MAX )) && continue
                if [[ -n "$LISTENER_INCLUDE_REGEX" ]] && [[ ! "$record_cmd" =~ $LISTENER_INCLUDE_REGEX ]]; then
                    continue
                fi
                if [[ -n "$LISTENER_EXCLUDE_REGEX" ]] && [[ "$record_cmd" =~ $LISTENER_EXCLUDE_REGEX ]]; then
                    continue
                fi
                if [[ -n "${PROCESSED_PIDS[$record_pid]:-}" ]]; then
                    continue
                fi
                PROCESSED_PIDS["$record_pid"]=1
                matched=1
                log_info "  -> Terminating $record_cmd (PID $record_pid) listening on $record_addr"
                if (( DRY_RUN )); then
                    log_info "[dry-run] kill -15 $record_pid"
                else
                    if kill -15 "$record_pid" 2>/dev/null; then
                        sleep 1
                        if kill -0 "$record_pid" 2>/dev/null; then
                            log_warn "Process $record_pid survived SIGTERM; sending SIGKILL"
                            kill -9 "$record_pid" 2>/dev/null || true
                        fi
                    else
                        log_warn "Process $record_pid already exited."
                    fi
                fi
                ;;
        esac
    done < <(lsof -nP -iTCP -sTCP:LISTEN -FpctLn)

    (( matched == 0 )) && log_info "No matching TCP listeners found for termination."
}

main() {
    while [[ $# -gt 0 ]]; do
        case "$1" in
            -n|--dry-run) DRY_RUN=1 ;;
            --skip-kill) SKIP_KILL=1 ;;
            --kill-listeners) KILL_LISTENERS=1 ;;
            --skip-apt) SKIP_APT=1 ;;
            --skip-logs) SKIP_LOGS=1 ;;
            --skip-user-caches) SKIP_USER_CACHE=1 ;;
            --all-users) ALL_USERS=1 ;;
            --user)
                shift
                require_arg "--user" "${1:-}"
                TARGET_USER="$1"
                ;;
            --min-idle-seconds=*)
                MIN_IDLE_SECONDS="${1#*=}"
                ;;
            --min-idle-seconds)
                shift
                require_arg "--min-idle-seconds" "${1:-}"
                MIN_IDLE_SECONDS="$1"
                ;;
            --tmp-retention-hours=*)
                TMP_RETENTION_HOURS="${1#*=}"
                ;;
            --tmp-retention-hours)
                shift
                require_arg "--tmp-retention-hours" "${1:-}"
                TMP_RETENTION_HOURS="$1"
                ;;
            --log-retention-days=*)
                LOG_RETENTION_DAYS="${1#*=}"
                ;;
            --log-retention-days)
                shift
                require_arg "--log-retention-days" "${1:-}"
                LOG_RETENTION_DAYS="$1"
                ;;
            --exclude-comm=*)
                EXCLUDE_COMM_REGEX="(${EXCLUDE_COMM_REGEX})|(${1#*=})"
                ;;
            --exclude-comm)
                shift
                require_arg "--exclude-comm" "${1:-}"
                EXCLUDE_COMM_REGEX="(${EXCLUDE_COMM_REGEX})|($1)"
                ;;
            --listener-port-min=*)
                LISTENER_PORT_MIN="${1#*=}"
                ;;
            --listener-port-min)
                shift
                require_arg "--listener-port-min" "${1:-}"
                LISTENER_PORT_MIN="$1"
                ;;
            --listener-port-max=*)
                LISTENER_PORT_MAX="${1#*=}"
                ;;
            --listener-port-max)
                shift
                require_arg "--listener-port-max" "${1:-}"
                LISTENER_PORT_MAX="$1"
                ;;
            --listener-include=*)
                LISTENER_INCLUDE_REGEX="${1#*=}"
                ;;
            --listener-include)
                shift
                require_arg "--listener-include" "${1:-}"
                LISTENER_INCLUDE_REGEX="$1"
                ;;
            --listener-exclude=*)
                LISTENER_EXCLUDE_REGEX="(${LISTENER_EXCLUDE_REGEX})|(${1#*=})"
                ;;
            --listener-exclude)
                shift
                require_arg "--listener-exclude" "${1:-}"
                LISTENER_EXCLUDE_REGEX="(${LISTENER_EXCLUDE_REGEX})|($1)"
                ;;
            -h|--help)
                usage
                exit 0
                ;;
            *)
                log_error "Unknown option: $1"
                usage
                exit 1
                ;;
        esac
        shift || true
    done

    if [[ $(id -u) -ne 0 ]]; then
        log_error "Must run as root. Try: sudo bash $PROG_NAME"
        exit 1
    fi

    ! is_unsigned_int "$MIN_IDLE_SECONDS" && { log_error "min-idle-seconds must be a positive integer."; exit 1; }
    ! is_unsigned_int "$TMP_RETENTION_HOURS" && { log_error "tmp-retention-hours must be a positive integer."; exit 1; }
    ! is_unsigned_int "$LOG_RETENTION_DAYS" && { log_error "log-retention-days must be a positive integer."; exit 1; }
    ! is_unsigned_int "$LISTENER_PORT_MIN" && { log_error "listener-port-min must be a positive integer."; exit 1; }
    ! is_unsigned_int "$LISTENER_PORT_MAX" && { log_error "listener-port-max must be a positive integer."; exit 1; }

    (( MIN_IDLE_SECONDS < 60 )) && { log_warn "Raising min idle to 60 seconds for safety."; MIN_IDLE_SECONDS=60; }
    (( TMP_RETENTION_HOURS < 1 )) && { log_warn "Raising temp retention to 1 hour for safety."; TMP_RETENTION_HOURS=1; }
    (( LOG_RETENTION_DAYS < 1 )) && { log_warn "Raising log retention to 1 day for safety."; LOG_RETENTION_DAYS=1; }
    (( LISTENER_PORT_MIN < 1 )) && { log_warn "Raising listener port minimum to 1."; LISTENER_PORT_MIN=1; }
    (( LISTENER_PORT_MAX < LISTENER_PORT_MIN )) && { log_warn "Adjusting listener port range to remain valid."; LISTENER_PORT_MAX="$LISTENER_PORT_MIN"; }

    local TMP_RETENTION_MINUTES=$(( TMP_RETENTION_HOURS * 60 ))

    declare -a USERS_TO_CLEAN=()
    if (( ALL_USERS )); then
        mapfile -t USERS_TO_CLEAN < <(getent passwd | awk -F: '$3 >= 1000 && $6 ~ /^\/home\// {print $1}')
    elif [[ -n "$TARGET_USER" ]]; then
        USERS_TO_CLEAN=("$TARGET_USER")
    elif [[ -n "${SUDO_USER:-}" ]]; then
        USERS_TO_CLEAN=("$SUDO_USER")
    else
        mapfile -t USERS_TO_CLEAN < <(getent passwd | awk -F: '$3 >= 1000 && $6 ~ /^\/home\// {print $1}')
    fi

    if (( ${#USERS_TO_CLEAN[@]} == 0 )); then
        log_warn "No non-system users detected; skipping per-user cache cleanup."
        SKIP_USER_CACHE=1
    else
        # Verify users exist
        declare -a VERIFIED_USERS=()
        for user in "${USERS_TO_CLEAN[@]}"; do
            if getent passwd "$user" >/dev/null; then
                VERIFIED_USERS+=("$user")
            else
                log_warn "Skipping unknown user '$user'."
            fi
        done
        USERS_TO_CLEAN=("${VERIFIED_USERS[@]}")
        if (( ${#USERS_TO_CLEAN[@]} == 0 )); then
            log_warn "No valid users remaining after verification; skipping per-user cache cleanup."
            SKIP_USER_CACHE=1
        fi
    fi

    (( DRY_RUN )) && log_info "Dry-run mode enabled; no changes will be applied."

    report_disk_usage "before cleanup"

    if (( ! SKIP_KILL )); then
        for user in "${USERS_TO_CLEAN[@]}"; do
            kill_stale_processes "$user" "$MIN_IDLE_SECONDS"
        done
    else
        log_info "Skipping process cleanup (--skip-kill)."
    fi

    if (( KILL_LISTENERS )); then
        kill_listening_ports
    else
        log_info "Skipping TCP listener cleanup."
    fi

    cleanup_tmp "$TMP_RETENTION_MINUTES"
    cleanup_crash_dumps
    cleanup_coredumps

    if (( ! SKIP_USER_CACHE )) && (( ${#USERS_TO_CLEAN[@]} > 0 )); then
        for user in "${USERS_TO_CLEAN[@]}"; do
            local home
            home=$(getent passwd "$user" | cut -d: -f6)
            cleanup_user_cache "$user" "$home" "$TMP_RETENTION_MINUTES"
        done
    else
        log_info "Skipping per-user cache cleanup."
    fi

    (( ! SKIP_APT )) && cleanup_apt || log_info "Skipping apt cleanup."
    (( ! SKIP_LOGS )) && cleanup_logs || log_info "Skipping log cleanup."

    if (( ! DRY_RUN )); then
        sync
    fi

    report_disk_usage "after cleanup"
    log_info "Cleanup complete."
}

main "$@"
