#!/usr/bin/env bash
# Example cron helper for ARW maintenance (place in /etc/cron.weekly/)
set -euo pipefail

WORKDIR=${WORKDIR:-/opt/arw}
STATE_DIR=${STATE_DIR:-/var/arw/state}
CONSENT=${CONSENT:-private}
SERVICE=${SERVICE:-arw-server.service}
LOG_FILE=${LOG_FILE:-/var/log/arw-maintenance.log}

cd "$WORKDIR"

systemctl stop "$SERVICE"
"$WORKDIR"/scripts/maintenance.sh --state-dir "$STATE_DIR" --pointer-consent "$CONSENT" \
  >>"$LOG_FILE" 2>&1
systemctl start "$SERVICE"
