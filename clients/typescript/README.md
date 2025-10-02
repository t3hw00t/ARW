# ARW TypeScript Client (Minimal)

This is a lightweight, hand‑written client for `arw-server` focusing on the core API surfaces:

- `POST /actions` → submit an action
- `GET /actions/:id` → poll action state
- `GET /events` (SSE) → subscribe to events (browser and Node supported)
- `GET /about` and `GET /healthz`

It does not rely on a generator and has no runtime dependencies beyond the standard browser/Node `fetch` and `EventSource` APIs.

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

CLI:
- Install or use via NPX after publishing: `arw-events --prefix service.,state.read.model.patch --replay 25 --store .arw/last-event-id` (uses `BASE` and `ARW_ADMIN_TOKEN` env vars).
- Flags like `--no-reconnect`, `--delay`, `--max-delay`, `--jitter`, `--idle`, `--structured`, `--out`, `--out-format`, `--out-max-bytes`, `--out-keep`, and `--out-compress` tweak reconnect/idle policy, output format, and persistence (idle/structured/out/* only affect the Node fallback). Structured mode mirrors the `reliable_stream.ts` JSONL schema so operators can parse lifecycle logs without extra scripting, and the `--out` family appends, rotates, prunes, and optionally gzips JSONL logs for replay/backfill workflows.
