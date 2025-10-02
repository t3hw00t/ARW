/*
  Stream events via AsyncIterator (Node only)
  - Replays the last 5 events matching the prefix
  - Uses AbortController to stop after 10 seconds

  Usage:
    BASE=http://127.0.0.1:8091 ARW_ADMIN_TOKEN=... ts-node clients/typescript/examples/events_stream.ts
*/

import { ArwClient, StreamEvent } from '../arw-client';

async function main() {
  const base = process.env.BASE || 'http://127.0.0.1:8091';
  const token = process.env.ARW_ADMIN_TOKEN;
  const client = new ArwClient(base, token);

  const controller = new AbortController();
  setTimeout(() => controller.abort(), 10_000);

  try {
    const stream = client.events.stream({
      topics: ['service.'],
      replay: 5,
      signal: controller.signal,
      inactivityTimeoutMs: 60_000,
      onStateChange: ({ state, attempt, delayMs }) => {
        if (state === 'open') {
          console.error('stream connected');
        } else if (state === 'retrying') {
          console.error(`retrying in ${Math.round(delayMs ?? 0)}ms (attempt ${attempt})`);
        } else if (state === 'closed') {
          console.error('stream closed');
        }
      },
    });

    for await (const evt of stream) {
      logEvent(evt);
    }
  } catch (err) {
    if ((err as any)?.name === 'AbortError') {
      console.log('stream aborted');
    } else {
      console.error('stream error', err);
    }
  }
}

function logEvent(evt: StreamEvent) {
  const id = evt.lastEventId ?? 'live';
  const typ = evt.type ?? 'message';
  console.log(`[${id}] ${typ}`, evt.data);
}

main().catch((e) => {
  console.error(e);
  process.exit(1);
});
