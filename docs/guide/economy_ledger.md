---
title: Economy Ledger
---

# Economy Ledger
Updated: 2025-10-26
Type: Reference

ARW exposes an economy ledger snapshot with totals and attention flags, and streams incremental updates over SSE so UIs can refresh without polling.

- Endpoint: `GET /state/economy/ledger`
  - Query params: `limit` (max entries, recent first), `offset` (pagination), `currency` (filter by code; use `unitless` for entries without a currency)
  - Response: `{ version, generated?, entries[], totals[], attention[], usage }`

- SSE Read-Model: id `economy_ledger`
  - Topic: `state.read.model.patch`
  - Clients apply JSON Patch deltas to a local snapshot
  - A companion summary event `economy.ledger.updated` is also published for coarse listeners

## TypeScript Client

```ts
import { ArwClient } from '@arw/client';

const client = new ArwClient('http://127.0.0.1:8091', process.env.ARW_ADMIN_TOKEN);

// Snapshot (with pagination)
const snap = await client.state.economyLedger({ limit: 25 });

// Live patches (id = economy_ledger)
const sub = client.state.watchEconomyLedger({
  loadInitial: async () => snap,
  onUpdate: (next) => console.log('version', next?.version),
});

// later
sub.close();
```

Example: `clients/typescript/examples/economy_ledger.ts`.

## Task Shortcuts (Repo Root)

- Just
  - `just ts-economy-smoke base=http://127.0.0.1:8091 timeout=10000` — build client and run the ledger SSE smoke.
  - `just ts-events-patches structured=true` — tail `state.read.model.patch` with resume storage.

- Mise
  - `mise run ts:economy:smoke BASE=http://127.0.0.1:8091 TIMEOUT=10000`
  - `mise run ts:events:patches BASE=http://127.0.0.1:8091 REPLAY=25 STORE=.arw/last-event-id`

### Smoke Flags

Economy smoke scripts accept helpful flags to make CI/local checks reliable:

- Script flags (Node example):
  - `--timeout <ms>`: how long to wait before exiting
  - `--trigger`: submit a small demo action to nudge contributions so a patch is likely observed
  - `--require-update`: fail if no patch is observed during the timeout

- Mise task env (ts:economy:smoke):
  - `TIMEOUT` (ms), `TRIGGER=1|0`, `REQUIRE_UPDATE=1|0`

## CLI

- Snapshot (text):
  - `arw-cli state economy-ledger --base http://127.0.0.1:8091 --limit 25`
  - Filter by currency: `--currency USD` (use `unitless` when entries lack a currency)

- JSON/CSV:
  - `arw-cli state economy-ledger --json --pretty`
  - `arw-cli state economy-ledger --csv > ledger.csv`
