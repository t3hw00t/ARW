#!/usr/bin/env bash
set -euo pipefail

if [[ "${ARW_SKIP_RELEASE_BLOCKER_CHECK:-0}" == "1" ]]; then
  exit 0
fi

label="${ARW_RELEASE_BLOCKER_LABEL:-release-blocker:restructure}"

github_repo="${GITHUB_REPOSITORY:-}"
if [[ -z "$github_repo" ]]; then
  remote_url=$(git config --get remote.origin.url 2>/dev/null || true)
  if [[ "$remote_url" =~ github.com[:/]{1}([^/]+/[A-Za-z0-9._-]+)(\.git)?$ ]]; then
    github_repo="${BASH_REMATCH[1]}"
  fi
fi

if [[ -z "$github_repo" ]]; then
  echo "[release-gate] Unable to determine GitHub repository. Set GITHUB_REPOSITORY or ensure 'origin' points at github.com." >&2
  exit 1
fi

if ! command -v python3 >/dev/null 2>&1; then
  echo "[release-gate] Missing dependency: python3" >&2
  exit 1
fi

ARW_RELEASE_BLOCKER_REPO="$github_repo" \
ARW_RELEASE_BLOCKER_LABEL_RESOLVED="$label" \
python3 - <<'PY'
import json
import os
import sys
import urllib.error
import urllib.parse
import urllib.request

label = os.environ["ARW_RELEASE_BLOCKER_LABEL_RESOLVED"]
repo = os.environ["ARW_RELEASE_BLOCKER_REPO"]
token = os.environ.get("GH_TOKEN") or os.environ.get("GITHUB_TOKEN")
per_page = 100

headers = {
    "Accept": "application/vnd.github+json",
    "User-Agent": "check-release-blockers-script",
}
if token:
    headers["Authorization"] = f"Bearer {token}"

issues = []
page = 1
rate_remaining = None

while True:
    query = urllib.parse.urlencode(
        {
            "state": "open",
            "labels": label,
            "per_page": per_page,
            "page": page,
        }
    )
    url = f"https://api.github.com/repos/{repo}/issues?{query}"
    req = urllib.request.Request(url, headers=headers)

    try:
        with urllib.request.urlopen(req) as resp:
            payload = json.load(resp)
            rate_remaining = resp.headers.get("X-RateLimit-Remaining")
    except urllib.error.HTTPError as exc:
        detail = exc.read().decode() if hasattr(exc, "read") else ""
        message = detail or exc.reason
        print(
            f"[release-gate] GitHub API error (status {exc.code}): {message}",
            file=sys.stderr,
        )
        if exc.code == 403 and not token:
            print(
                "[release-gate] Provide GH_TOKEN or GITHUB_TOKEN to increase rate limits.",
                file=sys.stderr,
            )
        sys.exit(1)
    except urllib.error.URLError as exc:
        print(f"[release-gate] Network error: {exc.reason}", file=sys.stderr)
        sys.exit(1)

    if isinstance(payload, dict) and payload.get("message"):
        print(f"[release-gate] GitHub API error: {payload['message']}", file=sys.stderr)
        documentation = payload.get("documentation_url")
        if documentation:
            print(f"[release-gate] See: {documentation}", file=sys.stderr)
        sys.exit(1)

    if not isinstance(payload, list):
        print("[release-gate] Unexpected GitHub response shape.", file=sys.stderr)
        sys.exit(1)

    for item in payload:
        if item.get("pull_request"):
            continue
        issues.append(f"#{item.get('number')} ({item.get('title')})")

    if len(payload) < per_page:
        break
    page += 1

if issues:
    joined = ", ".join(issues)
    print(f"[release-gate] Open {label} issues: {joined}", file=sys.stderr)
    sys.exit(1)

message = f"[release-gate] No open {label} issues."
if token is None:
    message += " (unauthenticated request)"
elif rate_remaining is not None:
    message += f" (remaining rate limit: {rate_remaining})"
print(message)
PY
