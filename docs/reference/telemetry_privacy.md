# Telemetry & Privacy
Updated: 2025-09-16
Type: Reference

Microsummary: What’s logged in the event journal, how to disable, and retention defaults. Beta.

- Local‑first: no automatic network egress; logs stay local by default.
- Event journal: records episodes and key events for debugging; can be disabled or rotated.
- Inspect: `GET /admin/events/journal?limit=200&prefix=topic.` returns the latest JSONL entries (admin only).
- CLI: `arw-cli events journal --limit 100 --prefix chat.` tails the same endpoint with a text summary (`--follow` keeps polling, `--after 2025-10-02T17:15:00Z` skips older entries, `--json` prints raw output).
- Observations: `arw-cli events observations --limit 50 --kind-prefix service. --since-relative 15m` fetches the `/state/observations` read-model with optional filtering, matching the new API parameters (use `--json` for raw output, `--payload-width 0` to hide payloads, and `--since`/`--since-relative` to skip older envelopes).
- Actions: `arw-cli state actions --state completed --kind-prefix chat.` pulls the `/state/actions` snapshot with built-in server filters (`--updated-since RFC3339` narrows by time; `--json/--pretty` mirror the API response).
- Disable/Configure: see [Configuration](../CONFIGURATION.md) (journal section) and `ARW_DEBUG` notes.
- Retention: defaults to per‑user storage path; configure size/time limits in configuration.
