/**
  Example: economy ledger snapshot + live SSE patches

  Dev (ts-node):
    BASE=http://127.0.0.1:8091 ARW_ADMIN_TOKEN=... \
    ts-node clients/typescript/examples/economy_ledger.ts --limit 25

  Build + run (dist):
    node dist/examples/economy_ledger.js --limit 25
*/
import { ArwClient, type EconomyLedgerSnapshot } from '../arw-client';

function parseArgs(argv: string[]): Record<string, string> {
  const out: Record<string, string> = {};
  for (let i = 0; i < argv.length; i++) {
    const a = argv[i];
    if (a === '--limit' || a === '-l') out.limit = argv[++i] || '';
    else if (a === '--offset' || a === '-o') out.offset = argv[++i] || '';
  }
  return out;
}

async function main() {
  const args = parseArgs(process.argv.slice(2));
  const base = process.env.BASE || 'http://127.0.0.1:8091';
  const token = process.env.ARW_ADMIN_TOKEN;
  const client = new ArwClient(base, token);

  const limit = args.limit ? Number(args.limit) : undefined;
  const offset = args.offset ? Number(args.offset) : undefined;

  // Fetch a snapshot
  const snap: EconomyLedgerSnapshot = await client.state.economyLedger({ limit, offset });
  console.log('[snapshot] version', snap.version);
  console.log('[snapshot] entries', snap.entries.length, 'totals', snap.totals.length);
  if (snap.attention.length) {
    console.log('[snapshot] attention:', snap.attention.join('; '));
  }

  // Subscribe to SSE patches for the economy ledger read-model
  const sub = client.state.watchEconomyLedger({
    loadInitial: async () => snap,
    onUpdate: (next) => {
      const v = (next?.version ?? 0) as number;
      const entries = Array.isArray(next?.entries) ? (next.entries as any[]).length : 0;
      console.log(`[patch] version=${v} entries=${entries}`);
    },
    onStateChange: ({ state, attempt, delayMs }) => {
      if (state === 'retrying') {
        console.warn(`[sse] retrying in ${Math.round(delayMs ?? 0)}ms (attempt ${attempt})`);
      }
    },
  });

  // Run for 30s, then close
  setTimeout(() => sub.close(), 30_000);
}

main().catch((e) => {
  console.error(e);
  process.exit(1);
});

