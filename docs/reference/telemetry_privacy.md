# Telemetry & Privacy
Updated: 2025-09-16
Type: Reference

Microsummary: What’s logged in the event journal, how to disable, and retention defaults. Beta.

- Local‑first: no automatic network egress; logs stay local by default.
- Event journal: records episodes and key events for debugging; can be disabled or rotated.
- Inspect: `GET /admin/events/journal?limit=200&prefix=topic.` returns the latest JSONL entries (admin only).
- CLI: `arw-cli events journal --limit 100 --prefix chat.` tails the same endpoint with a text summary (`--follow` keeps polling, `--after 2025-10-02T17:15:00Z` skips older entries, `--json` prints raw output).
- Disable/Configure: see [Configuration](../CONFIGURATION.md) (journal section) and `ARW_DEBUG` notes.
- Retention: defaults to per‑user storage path; configure size/time limits in configuration.
