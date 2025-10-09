# Telemetry & Privacy
Updated: 2025-10-09
Type: Reference

Microsummary: What’s logged in the event journal, how to disable, and retention defaults. Beta.

- Local‑first: no automatic network egress; logs stay local by default.
- Event journal: records episodes and key events for debugging; can be disabled or rotated.
- Inspect: `GET /admin/events/journal?limit=200&prefix=topic.` returns the latest entries persisted in the kernel’s SQLite journal (admin only).
- CLI: `arw-cli events journal --limit 100 --prefix chat.` tails the same endpoint with a text summary (`--follow` keeps polling, `--after 2025-10-02T17:15:00Z` or `--after-relative 15m` skips older entries, `--payload-width 0` hides payload bodies, `--json` prints raw output). The summary includes an Age column that mirrors the observations/actions views.
- Observations: `arw-cli events observations --limit 50 --kind-prefix service. --since-relative 15m` fetches the `/state/observations` read-model with optional filtering, matching the new API parameters (use `--json` for raw output, `--payload-width 0` to hide payloads, and `--since`/`--since-relative` to skip older envelopes). The text summary includes an Age column so you can spot the freshest envelopes instantly.
- Actions: `arw-cli state actions --state completed --kind-prefix chat.` pulls the `/state/actions` snapshot with built-in server filters (`--updated-since RFC3339` or `--updated-relative 30m` narrows by time; `--json/--pretty` mirror the API response; `--watch` keeps a live SSE stream open so you capture new updates as they land). The text summary includes an Age column beside the updated timestamp for fast triage.
- Disable/Configure: journaling is available whenever `ARW_KERNEL_ENABLE=1`; disabling the kernel stops journaling. See [Configuration](../CONFIGURATION.md) for kernel controls and `ARW_DEBUG` notes.
- Retention: entries live inside `{state_dir}/events.sqlite` alongside other kernel tables. Use standard SQLite maintenance (VACUUM, backups, exports) to enforce retention windows until dedicated pruning lands.
