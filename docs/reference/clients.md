---
title: Clients
---

# Clients
<a href="https://www.npmjs.com/package/@arw/client"><img alt="npm" src="https://img.shields.io/npm/v/%40arw%2Fclient?label=%40arw%2Fclient"></a>
Updated: 2025-09-29

This repo includes a minimal TypeScript client for `arw-server`. It focuses on the unified API: `/actions`, `/events`, `/state/*`, and `/about`.

## TypeScript

Location: `clients/typescript/`

Install (after publish):
- `npm i @arw/client`

Use cases:
- Submit and poll actions (fire‑and‑wait or fire‑and‑forget)
- Subscribe to SSE events (browser EventSource or Node stream fallback)
- Query service status (`/healthz`, `/about`)

Example:

```ts
import { ArwClient } from '@arw/client';

const client = new ArwClient('http://127.0.0.1:8091', process.env.ARW_ADMIN_TOKEN);
const { id } = await client.actions.submit({ kind: 'demo.echo', input: { msg: 'hello' } });
const result = await client.actions.wait(id, 10000);
console.log(result.state, result.output);
```

Publishing:
- The `TS Client Publish` workflow publishes the package when a tag matching `ts-client-v*` is pushed and `NPM_TOKEN` is provided as a repository secret.

Notes:
- SSE requires admin access unless running locally with `ARW_DEBUG=1` (see Security). The Node fallback sends `X-ARW-Admin` automatically when a token is configured.
- Prefer resuming with `Last-Event-ID`; the client uses the header in Node and `?after=` in browsers.

Examples:
- Node SSE patches (projects): `clients/typescript/examples/projects_patches.ts`

CLI
- After publishing the package, a small CLI is available:
  - Install globally: `npm i -g @arw/client`
  - Tail events with resume and filters:
    - `BASE=http://127.0.0.1:8091 ARW_ADMIN_TOKEN=$ARW_ADMIN_TOKEN arw-events --prefix service.,state.read.model.patch --replay 25`
  - Stores `Last-Event-ID` between runs when `--store <file>` is provided (defaults to `.arw/last-event-id`).
  - For a curl+jq alternative, see `scripts/sse_tail.sh` (honors `SSE_TAIL_TIMEOUT_SECS` / `SMOKE_TIMEOUT_SECS`; set to `0` to keep the stream open indefinitely).
