---
title: Daily Brief
---

# Daily Brief
Updated: 2025-10-26
Type: Reference

The Daily Brief provides a consolidated snapshot across runtime, economy, persona, memory, and autonomy, with a concise summary and attention cues. A publish event is emitted so UIs can refresh without polling.

- Endpoint: `GET /state/briefs/daily`
  - Query params: `proj` (optional; reserved for future project scoping)
  - Response: `{ generated_at, summary, economy?, runtime?, persona?, memory?, autonomy?, attention[] }`
  - Requires admin token locally unless running with `ARW_DEBUG=1`.

- Publish Event: `brief.daily.published`
  - Topic: emitted on `/events` with the full brief snapshot as payload
  - Clients can listen for this event and update their view immediately

## TypeScript Client

```ts
import { ArwClient } from '@arw/client';

const client = new ArwClient('http://127.0.0.1:8091', process.env.ARW_ADMIN_TOKEN);

// Snapshot
const brief = await client.state.dailyBrief();
console.log('summary', brief.summary);

// Live updates via publish event
const sub = client.state.watchDailyBrief({
  loadInitial: async () => brief,
  onUpdate: (next) => console.log('updated', next?.generated_at, next?.summary),
});

// later
sub.close();
```

Example: `clients/typescript/examples/daily_brief.ts`.

## Task Shortcuts (Repo Root)

- Mise
  - `mise run ts:daily:brief BASE=http://127.0.0.1:8091 TIMEOUT=8000`

## Notes

- Launcher already refreshes the Daily Brief panel on `brief.daily.published` and shows a placeholder until the first snapshot is generated.
- The server generates and publishes a brief at startup and then periodically (default hourly); consumers should subscribe for updates and/or fetch on demand.
