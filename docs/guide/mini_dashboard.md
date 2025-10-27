---
title: Mini Dashboard (TUI)
---

# Mini Dashboard (TUI)

Updated: 2025-10-27
Type: How‑to

The mini dashboard is a tiny, offline‑friendly CLI that watches a read‑model via SSE (state.read.model.patch), hydrates an initial snapshot, and prints compact live updates. It's useful on low‑spec or headless machines.

## Run

- Just (repo root):
  - `just mini-dashboard base=http://127.0.0.1:8091`
- Mise:
  - `mise run mini:dashboard BASE=http://127.0.0.1:8091`

Flags/env:
- `--base` (or `BASE`): server base URL (default `http://127.0.0.1:8091`)
- `--admin-token` (or `ARW_ADMIN_TOKEN`): admin token if your server requires it
- `--id`: read-model id to watch (default `economy_ledger`)
- `--limit`: initial snapshot depth for known routes (e.g., economy)
- `--snapshot`: optional explicit snapshot route (e.g., `/state/actions?state=completed`)
- `--json`: print full snapshot JSON on every update
- `--once`: print the initial snapshot/summary and exit
- `--sse`: periodically print SSE counters from `/metrics` (connections/sent/errors)

Examples:
- Route stats (once, JSON):
  - `cargo run -p arw-mini-dashboard -- --base http://127.0.0.1:8091 --id route_stats --json --once`

- Economy watcher (default):
  - `just mini-dashboard`
- Generic read-model + snapshot route:
  - `mise run mini:dashboard ID=actions SNAPSHOT=/state/actions?state=completed`

- SSE counters alongside updates:
  - `mise run mini:dashboard SSE=1`

## Notes
- The dashboard hydrates an initial snapshot, then applies JSON Patch from `state.read.model.patch`.
- For known models like `economy_ledger`, it prints version/entries/totals. If a snapshot includes an `items` array, it prints its length.
- With `--sse`, it polls `/metrics` every ~10s and prints `sse conn=… sent=… err=…`.
- It’s meant for quick terminal checks; for richer TS usage, see the client helpers and examples.

Related:
- [Economy Ledger](./economy_ledger.md)
- [Daily Brief](./daily_brief.md)
- TS client: `clients/typescript/README.md`
