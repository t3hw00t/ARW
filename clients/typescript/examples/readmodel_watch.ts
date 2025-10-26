/**
  Watch a specific read-model id with automatic snapshot hydration.

  Examples (dev server running):
    BASE=http://127.0.0.1:8091 ARW_ADMIN_TOKEN=... \
    ts-node clients/typescript/examples/readmodel_watch.ts --id projects --timeout 30000

    # Custom snapshot route when id does not map 1:1 to /state/<id>
    BASE=http://127.0.0.1:8091 ARW_ADMIN_TOKEN=... \
    ts-node clients/typescript/examples/readmodel_watch.ts --id actions --snapshot /state/actions?state=completed --timeout 30000 --json
*/
import { ArwClient } from '../arw-client';

function parseArgs(argv: string[]): Record<string, string | boolean> {
  const out: Record<string, string | boolean> = {};
  for (let i = 0; i < argv.length; i++) {
    const a = argv[i];
    if (a === '--id') out.id = argv[++i] || '';
    else if (a === '--snapshot') out.snapshot = argv[++i] || '';
    else if (a === '--timeout' || a === '-t') out.timeout = argv[++i] || '';
    else if (a === '--require-version') out.requireVersion = true;
    else if (a === '--require-key') out.requireKey = argv[++i] || '';
    else if (a === '--require-update') out.requireUpdate = true;
    else if (a === '--json') out.json = true;
  }
  return out;
}

async function main() {
  const args = parseArgs(process.argv.slice(2));
  const id = String(args.id || '').trim();
  if (!id) {
    console.error('usage: readmodel_watch --id <read_model_id> [--snapshot /state/... ] [--timeout ms] [--json]');
    process.exit(2);
  }
  const base = process.env.BASE || 'http://127.0.0.1:8091';
  const token = process.env.ARW_ADMIN_TOKEN;
  const client = new ArwClient(base, token);
  const timeoutMs = Math.max(0, Number(args.timeout || '0'));
  const snapshotRoute = typeof args.snapshot === 'string' && args.snapshot ? args.snapshot : `/state/${id}`;
  const printJson = !!args.json;

  const headers: Record<string, string> = {};
  if (token) headers['X-ARW-Admin'] = token;

  const loadInitial = async () => {
    const url = snapshotRoute.startsWith('http') ? snapshotRoute : `${base}${snapshotRoute}`;
    const r = await fetch(url, { headers });
    if (!r.ok) throw new Error(`snapshot failed: ${r.status}`);
    return r.json();
  };

  // Eagerly hydrate initial snapshot to validate shape when requested
  let initial: any | undefined;
  try {
    initial = await loadInitial();
  } catch (e) {
    console.error(`[${id}] snapshot load failed`, e);
    process.exit(1);
  }
  if (args.requireVersion && (typeof initial?.version !== 'number')) {
    console.error(`[${id}] require-version set but snapshot.version is missing or not a number`);
    process.exit(2);
  }
  if (typeof args.requireKey === 'string' && args.requireKey) {
    const key = String(args.requireKey);
    if (!(key in (initial ?? {}))) {
      console.error(`[${id}] required key '${key}' missing from snapshot`);
      process.exit(2);
    }
  }

  let sawUpdate = false;
  const sub = client.events.subscribeReadModel(id, {
    initial,
    onUpdate: (next) => {
      sawUpdate = true;
      if (printJson) {
        console.log(JSON.stringify(next));
      } else {
        const v = (next?.version ?? 0) as number;
        console.log(`[${id}] version=${v}`);
      }
    },
    onStateChange: ({ state, attempt, delayMs }) => {
      if (state === 'retrying') {
        console.error(`[${id}] retrying in ${Math.round(delayMs ?? 0)}ms (attempt ${attempt})`);
      }
    },
  });

  if (timeoutMs > 0) {
    setTimeout(() => {
      sub.close();
      if (args.requireUpdate && !sawUpdate) {
        console.error(`[${id}] require-update set but no patch observed`);
        process.exit(3);
      }
    }, timeoutMs);
  }
}

main().catch((e) => {
  console.error(e);
  process.exit(1);
});
