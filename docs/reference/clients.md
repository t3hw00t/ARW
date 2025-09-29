---
title: Clients
---

# Clients

This repo includes a minimal TypeScript client for `arw-server`. It focuses on the unified API: `/actions`, `/events`, `/state/*`, and `/about`.

## TypeScript

Location: `clients/typescript/`

Use cases:
- Submit and poll actions (fire‑and‑wait or fire‑and‑forget)
- Subscribe to SSE events
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

