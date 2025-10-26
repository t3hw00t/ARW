import { ArwClient, DailyBriefSnapshot } from '../arw-client';

// usage:
//   BASE=http://127.0.0.1:8091 ARW_ADMIN_TOKEN=... \
//   ts-node clients/typescript/examples/daily_brief.ts --timeout 8000

function parseArgs(argv: string[]) {
  const out: Record<string, string | number | boolean> = {};
  for (let i = 0; i < argv.length; i++) {
    const a = argv[i];
    if (a === '--timeout' && argv[i + 1]) { out.timeout = Number(argv[++i]); continue; }
    if (a === '--json') { out.json = true; continue; }
  }
  return out;
}

async function main() {
  const { timeout = 0, json = false } = parseArgs(process.argv.slice(2)) as any;
  const base = process.env.BASE || 'http://127.0.0.1:8091';
  const token = process.env.ARW_ADMIN_TOKEN;
  const client = new ArwClient(base, token);

  const snap: DailyBriefSnapshot = await client.state.dailyBrief();
  if (json) {
    console.log(JSON.stringify(snap, null, 2));
  } else {
    console.log(`[daily-brief] generated_at=${snap.generated_at} summary=${snap.summary}`);
  }

  const sub = client.state.watchDailyBrief({
    loadInitial: async () => snap,
    onUpdate: (next) => {
      if (!next) return;
      if (json) {
        console.log(JSON.stringify({ event: 'brief.daily.published', generated_at: next.generated_at, summary: next.summary }));
      } else {
        console.log(`[daily-brief] event summary=${next.summary}`);
      }
    },
    inactivityTimeoutMs: 60_000,
  });

  if (timeout && timeout > 0) {
    setTimeout(() => { try { sub.close(); } catch {} }, Number(timeout));
  }
}

main().catch((err) => {
  console.error('[daily-brief] error', err);
  process.exitCode = 1;
});

