/**
  Smoke: connect to economy ledger SSE and validate patch flow.

  Usage (dev server running):
    BASE=http://127.0.0.1:8091 ARW_ADMIN_TOKEN=... \
    ts-node clients/typescript/examples/smoke_watch_economy_ledger.ts --timeout 10000

  Exits 0 when it successfully connects and hydrates the initial snapshot,
  even if no patches arrive during the window.
*/
import { ArwClient } from '../arw-client';

function parseArgs(argv: string[]): Record<string, string | boolean> {
  const out: Record<string, string | boolean> = {};
  for (let i = 0; i < argv.length; i++) {
    const a = argv[i];
    if (a === '--timeout' || a === '-t') out.timeout = argv[++i] || '';
    else if (a === '--trigger' || a === '-x') out.trigger = true;
    else if (a === '--require-update' || a === '-r') out.require = true;
  }
  return out;
}

async function main() {
  const args = parseArgs(process.argv.slice(2));
  const base = process.env.BASE || 'http://127.0.0.1:8091';
  const token = process.env.ARW_ADMIN_TOKEN;
  const client = new ArwClient(base, token);
  const timeoutMs = Math.max(1000, Number((args.timeout as string) || '8000'));

  const snap = await client.state.economyLedger({ limit: 10 });
  console.error('[smoke] snapshot version', snap.version, 'entries', snap.entries.length);

  let updated = false;
  const sub = client.state.watchEconomyLedger({
    loadInitial: async () => snap,
    onUpdate: (next) => {
      updated = true;
      console.error('[smoke] patch version', next?.version);
    },
    onStateChange: ({ state, attempt, delayMs }) => {
      if (state === 'retrying') {
        console.error('[smoke] retrying', attempt, 'in', Math.round(delayMs ?? 0), 'ms');
      }
    },
  });

  // Optionally trigger an action to nudge contributions and the ledger
  if (args.trigger) {
    try {
      await client.actions.submit({ kind: 'ci.demo', input: { ts: Date.now() } });
      console.error('[smoke] submitted ci.demo action');
    } catch (e) {
      console.error('[smoke] action submit failed', e);
    }
  }

  await new Promise((resolve) => setTimeout(resolve, timeoutMs));
  sub.close();
  if (args.require && !updated) {
    console.error('[smoke] require-update set but no patch observed');
    process.exit(2);
  }
  if (args.require) {
    try {
      const latest = await client.state.economyLedger({ limit: 50 });
      const totals = Array.isArray((latest as any).totals) ? (latest as any).totals : [];
      const sum = totals.reduce((acc: number, t: any) => acc + (Number(t.pending) || 0) + (Number(t.settled) || 0), 0);
      if (!(sum > 0)) {
        console.error('[smoke] expected ledger totals to be > 0 after trigger');
        process.exit(3);
      }
    } catch (e) {
      console.error('[smoke] failed to validate totals', e);
      process.exit(3);
    }
  }
  console.error('[smoke] done; sawUpdate=', updated);
}

main().catch((e) => {
  console.error('[smoke] error', e);
  process.exit(1);
});
