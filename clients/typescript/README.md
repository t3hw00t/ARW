# ARW TypeScript Client (Minimal)

This is a lightweight, hand‑written client for `arw-server` focusing on the core API surfaces:

- `POST /actions` → submit an action
- `GET /actions/:id` → poll action state
- `GET /events` (SSE) → subscribe to events (browser and Node supported)
- `GET /state/observations` → admin snapshot with optional `limit` / `kind_prefix` / `since`
- `GET /state/beliefs`, `GET /state/intents`, `GET /state/actions`
- `client.state.watch*` helpers (Observations/Beliefs/Intents/Actions) hydrate via SSE read-model patches with automatic JSON Patch application
- `GET /about` and `GET /healthz`

It does not rely on a generator and has no runtime dependencies beyond the standard browser/Node `fetch` and `EventSource` APIs.

## Install (offline-first)

Use the helper scripts so installs default to offline when the cache exists and refresh it otherwise:

- Bash: `clients/typescript/scripts/install-offline.sh`
- PowerShell: `clients/typescript/scripts/install-offline.ps1`

Behaviour:
- If `npm-cache.tgz` is present it is expanded into `.npm-cache/` and `npm ci --offline --prefer-offline --cache .npm-cache` runs.
- If no cache is present it falls back to `npm ci --prefer-offline --cache .npm-cache` to seed the cache.
- After a successful install the cache is repacked into `npm-cache.tgz` so you can share or attach it to a release artifact.

Pass `--refresh` (`-Refresh` in PowerShell) to force a fresh cache and tarball.

If you find a checked-in `.tooling/node-*/` folder on the machine, you can temporarily add its `node.exe` directory to `PATH` to use that pinned Node/npm for installs and builds (keeps tooling consistent without requiring a global install).

Backpressure/metrics knobs:
- `events.stream`: `maxQueue` (soft cap, drops oldest), `onDrop` (notified on drops), `onStats` (pending+dropped counters), `parseJson` (default true), `signal` for cancellation.
- `events.subscribeReadModel`: `maxPendingPatches` (cap pre-hydration backlog), `maxApplyPerTick` (queue excess patches per tick), `onDrop` (drops when cap exceeded), `throttleMs` (coalesce onUpdate), `onMetrics` (pending/applied/dropped/lastEventId), plus `initial`, `loadInitial`, `reconnect*`, `inactivityTimeoutMs`.
- Watch helpers (`watch*`) accept the same backpressure/metrics options; `watchDailyBrief` also respects `maxPendingPatches`, `onDrop`, `onMetrics`, and `throttleMs`.
- Helpers: `createStreamMetricsCollector()` / `createReadModelMetricsCollector()` return reusable `onDrop`/`onStats`/`onMetrics` hooks and `snapshot()`/`reset()` accessors for quick telemetry wiring.

Example: cap/read-model metrics

```ts
import { ArwClient, createReadModelMetricsCollector } from '@arw/client';

const client = new ArwClient(process.env.BASE!, process.env.ARW_ADMIN_TOKEN);
const metrics = createReadModelMetricsCollector();

const sub = client.events.subscribeReadModel('projects', {
  maxPendingPatches: 200,
  maxApplyPerTick: 10,
  throttleMs: 50,
  onDrop: ({ dropped }) => console.warn('dropped patches', dropped),
  onMetrics: metrics.onMetrics,
  onUpdate: (snap) => console.log('version', snap?.version),
});

setInterval(() => {
  console.log('metrics', metrics.snapshot());
}, 5000);
```

Example: managed stream with defaults

```ts
import { ArwClient } from '@arw/client';

const client = new ArwClient(process.env.BASE!, process.env.ARW_ADMIN_TOKEN);

const stream = client.events.managedStream({
  topics: ['service.*'],
  replay: 10,
  maxQueue: 500,
  logDrops: true,
});

for await (const evt of stream) {
  console.log(evt.type, evt.data);
}

console.log('final stats', stream.stats());
```

## Usage

```ts
import { ArwClient } from './arw-client';

const client = new ArwClient('http://127.0.0.1:8091', process.env.ARW_ADMIN_TOKEN);

// Submit an action
const submit = await client.actions.submit({ kind: 'demo.echo', input: { msg: 'hi' } });
console.log('action id', submit.id);

// Poll until done
const res = await client.actions.wait(submit.id, 10_000);
console.log(res.state, res.output);

// Listen to events (browser)
const es = client.events.subscribe({ topics: ['service.*'], replay: 25 });
es.onmessage = (e) => console.log('event', e.data);

// Resume from a known event id (browser falls back to ?after=, Node sends Last-Event-ID header)
const es2 = client.events.subscribe({ lastEventId: '12345' });

// Snapshot latest observations (admin token required)
const observations = await client.state.observations({ limit: 50, kindPrefix: 'service.' });
console.log('service observations', observations.items?.length ?? 0);

// Live observations feed (applies JSON Patch deltas under the hood)
const obsSub = client.state.watchObservations({
  limit: 200,
  kindPrefix: 'service.',
  onUpdate: (snapshot) => {
    console.log('live version', snapshot?.version);
  },
});

// Later, tear down
obsSub.close();

// Filter action history (admin token required)
const recentActions = await client.state.actions({ state: 'completed', kindPrefix: 'chat.' });
console.log('completed chat actions', recentActions.items?.length ?? 0);
```

Node tips:
- No polyfill required. The client streams SSE via `fetch` in Node and parses it internally.
- Ensure `fetch`/`ReadableStream` are available (Node 18+ includes them by default).
- Admin endpoints and `/events` require an admin token unless you run with `ARW_DEBUG=1` locally. Pass it as the second arg to `ArwClient` or via `client.setAdminToken()`.
- Both the browser `EventSource` path and the Node fallback automatically reconnect when the stream drops. Tune the behaviour with `autoReconnect`, `reconnectInitialDelayMs`, `reconnectMaxDelayMs`, `reconnectJitterMs`, or `inactivityTimeoutMs` (Node fallback) and observe lifecycle changes via `onStateChange` to drive status badges or accessibility announcements.

Patches shortcut:
- `client.events.subscribePatches(lastIdOrOptions?)` subscribes to `state.read.model.patch` with a small replay for UI warm‑up (accepts either a `lastEventId` string or the richer `EventsOptions`).
- `client.events.subscribeReadModel(id, opts)` keeps a local snapshot updated (applies JSON Patch deltas, exposes `getSnapshot()`/`lastEventId()` accessors, optional `loadInitial()` hydrates the starting snapshot).

```ts
const sub = client.events.subscribeReadModel('projects', {
  loadInitial: async () => {
    const headers: Record<string, string> = {};
    if (process.env.ARW_ADMIN_TOKEN) headers['X-ARW-Admin'] = process.env.ARW_ADMIN_TOKEN;
    const res = await fetch(`${client.base}/state/projects`, { headers });
    if (!res.ok) throw new Error(`snapshot failed: ${res.status}`);
    return res.json();
  },
  onUpdate: (snapshot) => {
    console.log('projects items', snapshot?.items?.length ?? 0);
  },
  onStateChange: ({ state, attempt, delayMs }) => {
    if (state === 'retrying') {
      console.log(`retrying read-model stream in ${Math.round(delayMs ?? 0)}ms (attempt ${attempt})`);
    }
  },
  inactivityTimeoutMs: 60_000,
});

// Later
sub.close();
```

Node streaming helper:

```ts
const controller = new AbortController();
setTimeout(() => controller.abort(), 10_000);

const stream = client.events.stream({
  topics: ['service.'],
  replay: 5,
  signal: controller.signal,
  inactivityTimeoutMs: 60_000,
  onStateChange: ({ state, attempt, delayMs }) => {
    if (state === 'retrying') {
      console.warn(`events retry #${attempt} in ${Math.round(delayMs ?? 0)}ms`);
    }
  },
});

for await (const evt of stream) {
  console.log(`[${evt.lastEventId ?? 'live'}] ${evt.type ?? 'message'}`, evt.data);
}
```

See `clients/typescript/examples/reliable_stream.ts` for a fuller Node example that emits structured lifecycle logs, honors idle timeouts, and optionally appends every record to a JSONL file via `--out` for later analysis. The CLI adds rotation/retention/compression knobs when you combine it with `--out-format`, `--out-max-bytes`, `--out-keep`, and `--out-compress`.

Economy ledger snapshot + SSE patches:

```ts
import { ArwClient } from '@arw/client';

const client = new ArwClient(process.env.BASE!, process.env.ARW_ADMIN_TOKEN);

// Fetch snapshot (supports limit/offset)
const snap = await client.state.economyLedger({ limit: 25 });
console.log('version', snap.version, 'entries', snap.entries.length);

// Subscribe to read-model patches (id = "economy_ledger")
const sub = client.state.watchEconomyLedger({
  loadInitial: async () => snap,
  onUpdate: (next) => console.log('updated version', next?.version),
});

// later
sub.close();
```

See also: `clients/typescript/examples/economy_ledger.ts`.

Smoke helper (manual):

```bash
BASE=http://127.0.0.1:8091 ARW_ADMIN_TOKEN=... \
ts-node clients/typescript/examples/smoke_watch_economy_ledger.ts --timeout 10000
```

Daily Brief (snapshot + live):

```ts
import { ArwClient } from '@arw/client';

const client = new ArwClient(process.env.BASE!, process.env.ARW_ADMIN_TOKEN);

// Fetch the latest brief
const brief = await client.state.dailyBrief();
console.log('brief summary', brief.summary);

// Watch publish events and emit summaries
const sub = client.state.watchDailyBrief({
  loadInitial: async () => brief,
  onUpdate: (next) => console.log('brief updated', next?.generated_at, next?.summary),
});

// later
sub.close();
```

Example runner: `clients/typescript/examples/daily_brief.ts`.

Task shortcuts (repo root):

- Just
  - `just ts-readmodel-watch id=projects timeout=15000`
  - `just ts-readmodel-watch id=actions snapshot=/state/actions?state=completed json=true`
  - `just ts-events args="--prefix service.,state.read.model.patch --replay 25 --store .arw/last-event-id"`
  - `just ts-events-patches structured=true`
  - `just ts-economy-smoke base=http://127.0.0.1:8091 timeout=10000`
  - `just ts-daily-brief-watch base=http://127.0.0.1:8091 timeout=8000`

- Mise
  - `mise run ts:readmodel:watch ID=projects TIMEOUT=15000`
  - `mise run ts:events ARGS="--prefix service. --replay 10"`
  - `mise run ts:events:patches BASE=http://127.0.0.1:8091 REPLAY=25 STORE=.arw/last-event-id`
  - `mise run ts:economy:smoke BASE=http://127.0.0.1:8091 TIMEOUT=10000`
  - `mise run ts:daily:brief BASE=http://127.0.0.1:8091 TIMEOUT=8000`

Read-model watcher (generic):

```bash
# Direct (ts-node)
BASE=http://127.0.0.1:8091 ARW_ADMIN_TOKEN=... \
ts-node clients/typescript/examples/readmodel_watch.ts \
  --id projects \
  --timeout 15000 \
  --require-version \
  --require-key items \
  --json

# Just task
just ts-readmodel-watch id=projects timeout=15000 json=true
just ts-readmodel-watch id=actions snapshot=/state/actions?state=completed json=true

# Mise task (env)
mise run ts:readmodel:watch ID=projects TIMEOUT=15000 JSON=true REQUIRE_VERSION=1 REQUIRE_KEY=items
```

Flags/env:
- `--id <id>` (required): read-model id to watch.
- `--snapshot /state/...` (optional): custom snapshot route if it differs from `/state/<id>`.
- `--timeout <ms>`: auto-close after timeout (0 = run until manually closed).
- `--json`: print full snapshots (default prints version only).
- `--require-version` / `REQUIRE_VERSION=1`: assert initial snapshot has a numeric `version`.
- `--require-key <key>` / `REQUIRE_KEY=...`: assert initial snapshot includes the top-level key.
- `--require-update` (script only): fail if no patch is observed within the timeout.

CLI:
- Install or use via NPX after publishing: `arw-events --prefix service.,state.read.model.patch --replay 25 --store .arw/last-event-id` (uses `BASE` and `ARW_ADMIN_TOKEN` env vars).
- Flags like `--no-reconnect`, `--delay`, `--max-delay`, `--jitter`, `--idle`, `--structured`, `--out`, `--out-format`, `--out-max-bytes`, `--out-keep`, and `--out-compress` tweak reconnect/idle policy, output format, and persistence (idle/structured/out/* only affect the Node fallback). Structured mode mirrors the `reliable_stream.ts` JSONL schema so operators can parse lifecycle logs without extra scripting, and the `--out` family appends, rotates, prunes, and optionally gzips JSONL logs for replay/backfill workflows.
