#!/usr/bin/env node
/*
  arw-events: Tail ARW events with prefix filters and resume.

  Environment:
    BASE               Server base URL (default http://127.0.0.1:8091)
    ARW_ADMIN_TOKEN    Admin token (required unless ARW_DEBUG=1 locally)

  Usage:
    arw-events --prefix service.,state.read.model.patch --replay 25 --store .arw/last-event-id \
      [--no-reconnect] [--delay 500] [--max-delay 30000] [--jitter 250] [--idle 60000] [--structured] \
      [--out logs/events.jsonl] [--out-format ndjson|ndjson.gz] \
      [--out-max-bytes 10485760] [--out-keep 5] [--out-compress]

  Flags:
    --no-reconnect   Disable automatic SSE reconnect (exit after first drop)
    --delay          Initial reconnect delay in ms (default 500)
    --max-delay      Max backoff delay in ms (default 30000)
    --jitter         Extra random jitter in ms applied to each reconnect delay (default 250)
    --idle           Force reconnect when no events arrive within this many ms (Node fallback only)
    --structured     Emit lifecycle + event logs as JSON objects (stdout)
    --out            Append logs to the given JSONL file (structured schema); created if missing
    --out-format     Set persisted format (ndjson, ndjson.gz); overrides --out-compress
    --out-max-bytes  Rotate the JSONL file when it reaches this many bytes (new file keeps base path)
    --out-keep       Keep at most this many rotated files (oldest removed)
    --out-compress   Gzip rotated files (alias of --out-format ndjson.gz)
*/
import { ArwClient } from '../arw-client.js';
import { readFileSync, writeFileSync, mkdirSync, appendFileSync, renameSync, statSync, readdirSync, rmSync } from 'node:fs';
import { gzipSync } from 'node:zlib';
import { dirname } from 'node:path';

function parseArgs(argv: string[]): Record<string, string> {
  const out: Record<string, string> = {};
  for (let i = 0; i < argv.length; i++) {
    const a = argv[i];
    if (a === '--prefix' || a === '-p') out.prefix = argv[++i] || '';
    else if (a === '--replay' || a === '-r') out.replay = argv[++i] || '';
    else if (a === '--store' || a === '-s') out.store = argv[++i] || '';
    else if (a === '--after' || a === '-a') out.after = argv[++i] || '';
    else if (a === '--no-reconnect') out.noReconnect = '1';
    else if (a === '--delay') out.delay = argv[++i] || '';
    else if (a === '--max-delay') out.maxDelay = argv[++i] || '';
    else if (a === '--jitter') out.jitter = argv[++i] || '';
    else if (a === '--idle') out.idle = argv[++i] || '';
    else if (a === '--structured') out.structured = '1';
    else if (a === '--out' || a === '-o') out.out = argv[++i] || '';
    else if (a === '--out-format') out.outFormat = argv[++i] || '';
    else if (a === '--out-max-bytes') out.outMaxBytes = argv[++i] || '';
    else if (a === '--out-keep') out.outKeep = argv[++i] || '';
    else if (a === '--out-compress') out.outCompress = '1';
  }
  return out;
}

function loadLastId(path: string): string | undefined {
  try { return readFileSync(path, 'utf8').trim() || undefined; } catch { return undefined; }
}

function saveLastId(path: string, id?: string) {
  if (!id) return;
  try {
    mkdirSync(dirname(path), { recursive: true });
    writeFileSync(path, id + '\n', 'utf8');
  } catch {}
}

async function main() {
  const args = parseArgs(process.argv.slice(2));
  const base = process.env.BASE || 'http://127.0.0.1:8091';
  const token = process.env.ARW_ADMIN_TOKEN;
  const store = args.store || '.arw/last-event-id';
  const prefixes = (args.prefix || '').split(',').map((s) => s.trim()).filter(Boolean);
  const replay = Number(args.replay || '25') || 0;
  let lastId = args.after || loadLastId(store);

  const client = new ArwClient(base, token);
  const controller = new AbortController();

  const structured = Boolean(args.structured);
  const outPath = (args.out || '').trim();
  const outMaxBytes = args.outMaxBytes ? Number(args.outMaxBytes) : undefined;
  const outKeep = args.outKeep ? Number(args.outKeep) : undefined;
  const outFormat = (args.outFormat || '').toLowerCase();
  let outCompress = Boolean(args.outCompress);
  if (outMaxBytes !== undefined && !(outMaxBytes > 0)) {
    throw new Error('--out-max-bytes must be a positive number when provided');
  }
  if (outKeep !== undefined && outKeep < 1) {
    throw new Error('--out-keep must be >= 1 when provided');
  }
  if (outFormat) {
    if (outFormat === 'ndjson' || outFormat === 'jsonl') {
      outCompress = false;
    } else if (outFormat === 'ndjson.gz' || outFormat === 'jsonl.gz') {
      outCompress = true;
    } else {
      throw new Error(`Unsupported --out-format value: ${outFormat}`);
    }
  }

  const ensureDirForOut = () => {
    if (!outPath) return;
    mkdirSync(dirname(outPath), { recursive: true });
  };

  const rotateIfNeeded = () => {
    if (!outPath || outMaxBytes === undefined) {
      return;
    }
    try {
      const size = statSync(outPath).size;
      if (size < outMaxBytes) {
        return;
      }
    } catch {
      return;
    }
    const timestamp = new Date().toISOString().replace(/[:.]/g, '-');
    const rotatedBase = `${outPath}.${timestamp}`;
    let rotatedPath = rotatedBase;
    try {
      renameSync(outPath, rotatedBase);
    } catch {}

    if (outCompress) {
      try {
        const data = readFileSync(rotatedBase);
        rotatedPath = `${rotatedBase}.gz`;
        writeFileSync(rotatedPath, gzipSync(data));
        rmSync(rotatedBase);
      } catch {}
    }

    if (outKeep !== undefined) {
      try {
        const dir = dirname(outPath);
        const base = outPath.split('/').pop() || outPath;
        const prefix = base + '.';
        const files = readdirSync(dir)
          .filter((name) => name.startsWith(prefix))
          .sort();
        if (files.length > outKeep) {
          const remove = files.slice(0, files.length - outKeep);
          for (const file of remove) {
            try { rmSync(`${dir}/${file}`); } catch {}
          }
        }
      } catch {}
    }
  };

  const appendRecord = (record: Record<string, unknown>) => {
    if (!outPath) return;
    ensureDirForOut();
    rotateIfNeeded();
    const line = JSON.stringify(record) + '\n';
    appendFileSync(outPath, line);
  };

  const emitStructured = (record: Record<string, unknown>) => {
    if (structured) {
      console.log(JSON.stringify(record));
    }
    appendRecord(record);
  };

  process.on('SIGINT', () => {
    controller.abort();
  });
  process.on('exit', () => {});

  try {
    if (structured) {
      emitStructured({ msg: 'stream.start', base, prefixes, replay, lastId, outPath: outPath || null });
    } else if (outPath) {
      appendRecord({ msg: 'stream.start', base, prefixes, replay, lastId, outPath: outPath || null });
    }

    const stream = client.events.stream({
      topics: prefixes,
      lastEventId: lastId,
      replay,
      signal: controller.signal,
      autoReconnect: args.noReconnect ? false : undefined,
      reconnectInitialDelayMs: args.delay ? Number(args.delay) : undefined,
      reconnectMaxDelayMs: args.maxDelay ? Number(args.maxDelay) : undefined,
      reconnectJitterMs: args.jitter ? Number(args.jitter) : undefined,
      inactivityTimeoutMs: args.idle ? Number(args.idle) : undefined,
      onStateChange: (change) => {
        if (structured) {
          emitStructured({ msg: 'stream.state', state: change });
          return;
        }
        if (outPath) {
          appendRecord({ msg: 'stream.state', state: change });
        }
        if (change.state === 'retrying') {
          const delay = change.delayMs ?? 0;
          console.error(`retrying SSE in ${Math.round(delay)}ms (attempt ${change.attempt})`);
        } else if (change.state === 'open') {
          console.error('events stream connected');
        } else if (change.state === 'closed') {
          console.error('events stream closed');
        }
      },
    });

    for await (const evt of stream) {
      lastId = evt.lastEventId || lastId;
      const payload = evt.data;
      if (payload && typeof payload === 'object') {
        const kind = (payload as any).kind;
        const out = {
          id: lastId,
          kind,
          payload: (payload as any).payload ?? payload,
        };
        if (structured) {
          emitStructured({ msg: 'stream.event', ...out });
        } else {
          console.log(JSON.stringify(out));
          if (outPath) {
            appendRecord({ msg: 'stream.event', ...out });
          }
        }
      } else {
        const out = { id: lastId, type: evt.type ?? 'message', data: payload };
        if (structured) {
          emitStructured({ msg: 'stream.event', ...out });
        } else {
          console.log(JSON.stringify(out));
          if (outPath) {
            appendRecord({ msg: 'stream.event', ...out });
          }
        }
      }
      saveLastId(store, lastId);
    }

    if (structured) {
      emitStructured({ msg: 'stream.end', lastId });
    } else if (outPath) {
      appendRecord({ msg: 'stream.end', lastId });
    }
  } catch (err) {
    if ((err as any)?.name === 'AbortError') {
      if (structured) {
        emitStructured({ msg: 'stream.aborted' });
      } else if (outPath) {
        appendRecord({ msg: 'stream.aborted' });
      }
      return;
    }
    if (structured) {
      emitStructured({ msg: 'stream.error', error: serializeError(err) });
    } else {
      if (outPath) {
        appendRecord({ msg: 'stream.error', error: serializeError(err) });
      }
      console.error('events error', err);
    }
    process.exitCode = 1;
  }
}

function serializeError(err: unknown) {
  if (err instanceof Error) {
    return { name: err.name, message: err.message, stack: err.stack };
  }
  return err;
}

main().catch((e) => {
  console.error(e);
  process.exit(1);
});
