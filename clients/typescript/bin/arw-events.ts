#!/usr/bin/env node
/*
  arw-events: Tail ARW events with prefix filters and resume.

  Environment:
    BASE               Server base URL (default http://127.0.0.1:8091)
    ARW_ADMIN_TOKEN    Admin token (required unless ARW_DEBUG=1 locally)

  Usage:
    arw-events --prefix service.,state.read.model.patch --replay 25 --store .arw/last-event-id
*/
import { ArwClient } from '../arw-client.js';
import { readFileSync, writeFileSync, mkdirSync } from 'node:fs';
import { dirname } from 'node:path';

function parseArgs(argv: string[]): Record<string, string> {
  const out: Record<string, string> = {};
  for (let i = 0; i < argv.length; i++) {
    const a = argv[i];
    if (a === '--prefix' || a === '-p') out.prefix = argv[++i] || '';
    else if (a === '--replay' || a === '-r') out.replay = argv[++i] || '';
    else if (a === '--store' || a === '-s') out.store = argv[++i] || '';
    else if (a === '--after' || a === '-a') out.after = argv[++i] || '';
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
  let lastId = args.after || loadLastId(store);

  const client = new ArwClient(base, token);
  const controller = new AbortController();
  process.on('SIGINT', () => {
    controller.abort();
  });

  try {
    for await (const evt of client.events.stream({ topics: prefixes, lastEventId: lastId, replay, signal: controller.signal })) {
      lastId = evt.lastEventId || lastId;
      const payload = evt.data;
      if (payload && typeof payload === 'object') {
        const kind = (payload as any).kind;
        const out = {
          id: lastId,
          kind,
          payload: (payload as any).payload ?? payload,
        };
        console.log(JSON.stringify(out));
      } else {
        console.log(JSON.stringify({ id: lastId, type: evt.type ?? 'message', data: payload }));
      }
      saveLastId(store, lastId);
    }
  } catch (err) {
    if ((err as any)?.name === 'AbortError') {
      return;
    }
    console.error('events error', err);
    process.exitCode = 1;
  }
}

main().catch((e) => { console.error(e); process.exit(1); });
