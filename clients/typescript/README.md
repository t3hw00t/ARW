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

Patches shortcut:
- `client.events.subscribePatches(lastId?)` subscribes to `state.read.model.patch` with a small replay for UI warm‑up.

CLI:
- Install or use via NPX after publishing: `arw-events --prefix service.,state.read.model.patch --replay 25` (uses `BASE` and `ARW_ADMIN_TOKEN` env vars).
