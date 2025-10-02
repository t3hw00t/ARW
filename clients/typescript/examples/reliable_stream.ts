/*
  Example: resilient event follower with idle detection and structured logging.

  - Streams events with a custom idle timeout (default 45 seconds).
  - Emits lifecycle/status messages in a compact JSONL format.
  - Demonstrates how to supply a fallback handler when reconnect attempts are exhausted.

  Usage:
    BASE=http://127.0.0.1:8091 ARW_ADMIN_TOKEN=... \
    ts-node clients/typescript/examples/reliable_stream.ts --prefix service. \
      --idle 45000 --out logs/events.jsonl
*/

import { ArwClient, StreamEvent } from '../arw-client';
import { createWriteStream, mkdirSync, WriteStream } from 'node:fs';
import { dirname } from 'node:path';

interface CliArgs {
  prefix?: string;
  idle?: number;
  replay?: number;
  out?: string;
}

function parseArgs(argv: string[]): CliArgs {
  const out: CliArgs = {};
  for (let i = 0; i < argv.length; i++) {
    const arg = argv[i];
    if (arg === '--prefix' || arg === '-p') {
      out.prefix = argv[++i];
    } else if (arg === '--idle' || arg === '-i') {
      out.idle = Number(argv[++i]);
    } else if (arg === '--replay' || arg === '-r') {
      out.replay = Number(argv[++i]);
    } else if (arg === '--out' || arg === '-o') {
      out.out = argv[++i];
    }
  }
  return out;
}

let writer: WriteStream | undefined;

function emit(record: unknown) {
  const line = JSON.stringify(record);
  console.log(line);
  if (writer) {
    writer.write(line + '\n');
  }
}

function closeWriter() {
  if (writer && !writer.closed) {
    writer.end();
  }
}

async function main() {
  const args = parseArgs(process.argv.slice(2));
  const base = process.env.BASE || 'http://127.0.0.1:8091';
  const token = process.env.ARW_ADMIN_TOKEN;
  const client = new ArwClient(base, token);

  const idleMs = Number.isFinite(args.idle) && (args.idle ?? 0) > 0 ? args.idle! : 45_000;
  const replay = Number.isFinite(args.replay) && (args.replay ?? 0) >= 0 ? args.replay! : 25;
  const topics = args.prefix ? args.prefix.split(',').map((s) => s.trim()).filter(Boolean) : ['service.'];

  const controller = new AbortController();

  const outPath = args.out?.trim();
  if (outPath) {
    mkdirSync(dirname(outPath), { recursive: true });
    writer = createWriteStream(outPath, { flags: 'a' });
  }

  process.on('SIGINT', () => {
    controller.abort();
    closeWriter();
  });
  process.on('exit', closeWriter);

  try {
    emit({ msg: 'stream.start', base, topics, idleMs, replay, outPath: outPath ?? null });
    const stream = client.events.stream({
      topics,
      replay,
      signal: controller.signal,
      inactivityTimeoutMs: idleMs,
      onStateChange: (state) => {
        emit({ msg: 'stream.state', state });
      },
    });
    for await (const evt of stream) {
      logEvent(evt);
    }
  } catch (err) {
    if ((err as any)?.name === 'AbortError') {
      emit({ msg: 'stream.aborted' });
      closeWriter();
      return;
    }
    emit({ msg: 'stream.failed', error: serializeError(err) });
    process.exitCode = 1;
    closeWriter();
  }
}

function logEvent(evt: StreamEvent) {
  emit({
    msg: 'stream.event',
    id: evt.lastEventId ?? null,
    type: evt.type ?? 'message',
    data: evt.data,
  });
}

function serializeError(err: unknown) {
  if (err instanceof Error) {
    return { name: err.name, message: err.message, stack: err.stack };
  }
  return err;
}

main().catch((err) => {
  emit({ msg: 'stream.crashed', error: serializeError(err) });
  closeWriter();
  process.exit(1);
});
