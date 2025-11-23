/**
 * Usage:
 *   BASE=http://127.0.0.1:8091 ARW_ADMIN_TOKEN=... \
 *   ts-node clients/typescript/examples/managed_stream.ts --topic service. --replay 10 --max-queue 500 --duration 15000
 */
import { ArwClient } from '../arw-client';

type Args = {
  base: string;
  token?: string;
  topics: string[];
  replay?: number;
  maxQueue?: number;
  durationMs?: number;
};

function parseArgs(): Args {
  const argv = process.argv.slice(2);
  const out: Args = {
    base: process.env.BASE ?? 'http://127.0.0.1:8091',
    token: process.env.ARW_ADMIN_TOKEN,
    topics: ['service.'],
    replay: 10,
    maxQueue: 500,
    durationMs: 15_000,
  };
  for (let i = 0; i < argv.length; i++) {
    const a = argv[i];
    const next = argv[i + 1];
    switch (a) {
      case '--base':
        out.base = next;
        i++;
        break;
      case '--token':
        out.token = next;
        i++;
        break;
      case '--topic':
      case '--topics':
        out.topics = (next ?? '').split(',').filter(Boolean);
        i++;
        break;
      case '--replay':
        out.replay = Number(next);
        i++;
        break;
      case '--max-queue':
        out.maxQueue = Number(next);
        i++;
        break;
      case '--duration':
      case '--duration-ms':
        out.durationMs = Number(next);
        i++;
        break;
    }
  }
  return out;
}

async function main() {
  const args = parseArgs();
  const client = new ArwClient(args.base, args.token);

  const stream = client.events.managedStream({
    topics: args.topics,
    replay: args.replay,
    maxQueue: args.maxQueue,
    logDrops: true,
    autoReconnect: true,
    onStateChange: (s) => {
      console.log('[state]', s.state, 'attempt', s.attempt, 'delay', s.delayMs ?? 0);
    },
  });

  const endTimer =
    args.durationMs && args.durationMs > 0
      ? setTimeout(() => {
          console.log('closing after duration', args.durationMs);
          stream.close();
        }, args.durationMs)
      : null;

  try {
    for await (const evt of stream) {
      console.log(`[${evt.lastEventId ?? 'live'}] ${evt.type ?? 'message'} ${evt.raw ?? ''}`);
    }
  } catch (err) {
    console.error('stream error', err);
  } finally {
    if (endTimer) clearTimeout(endTimer);
    console.log('final stats', stream.stats());
  }
}

main().catch((err) => {
  console.error(err);
  process.exitCode = 1;
});
