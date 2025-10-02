/*
  Simple events tail CLI for Node.

  - Filters by CSV prefixes (e.g., service.,state.)
  - Resumes from stored Last-Event-ID between runs
  - Prints JSON lines {id, kind, payload}

  Usage:
    BASE=http://127.0.0.1:8091 ARW_ADMIN_TOKEN=... \
    node --experimental-specifier-resolution=node dist/examples/events_tail.js --prefix service.,state.read.model.patch

  Dev (ts-node):
    ts-node clients/typescript/examples/events_tail.ts --prefix service.
*/

import { ArwClient } from '../arw-client';
import { readFileSync, writeFileSync, mkdirSync } from 'node:fs';
import { dirname } from 'node:path';

function parseArgs(argv: string[]): Record<string, string> {
  const out: Record<string, string> = {};
  for (let i = 0; i < argv.length; i++) {
    const a = argv[i];
    if (a === '--prefix' || a === '-p') {
      out.prefix = argv[++i] || '';
    } else if (a === '--replay' || a === '-r') {
      out.replay = argv[++i] || '';
    } else if (a === '--store' || a === '-s') {
      out.store = argv[++i] || '';
    }
  }
  return out;
}

function loadLastId(path: string): string | undefined {
  try { return readFileSync(path, 'utf8').trim() || undefined; } catch { return undefined; }
}
function saveLastId(path: string, id?: string) {
  if (!id) return;
  try { mkdirSync(dirname(path), { recursive: true }); writeFileSync(path, id + '\n', 'utf8'); } catch {}
}

async function main() {
  const args = parseArgs(process.argv.slice(2));
  const base = process.env.BASE || 'http://127.0.0.1:8091';
  const token = process.env.ARW_ADMIN_TOKEN;
  const store = args.store || '.arw/last-event-id';
  const prefixes = (args.prefix || '').split(',').map(s => s.trim()).filter(Boolean);
  const replay = Number(args.replay || '25') || 0;

  const client = new ArwClient(base, token);
  let lastId = loadLastId(store);

  const sub = client.events.subscribe({
    topics: prefixes,
    lastEventId: lastId,
    replay,
    inactivityTimeoutMs: 60_000,
    onStateChange: ({ state, attempt, delayMs }) => {
      if (state === 'open') {
        console.error('subscription connected');
      } else if (state === 'retrying') {
        console.error(`subscription retry in ${Math.round(delayMs ?? 0)}ms (attempt ${attempt})`);
      } else if (state === 'closed') {
        console.error('subscription closed');
      }
    },
  });
  (sub as any).onmessage = (e: any) => {
    lastId = e.lastEventId || lastId;
    try {
      const env = JSON.parse(String(e.data));
      console.log(JSON.stringify({ id: lastId, kind: env.kind, payload: env.payload }));
      saveLastId(store, lastId);
    } catch {}
  };
  (sub as any).onerror = (err: any) => {
    console.error('events error', err);
  };
}

main().catch((e) => { console.error(e); process.exit(1); });
