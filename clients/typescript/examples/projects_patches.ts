/*
  Minimal Node example:
  - Fetch initial projects snapshot
  - Subscribe to state.read.model.patch
  - Apply JSON Patch ops to keep a local snapshot updated

  Usage:
    BASE=http://127.0.0.1:8091 ARW_ADMIN_TOKEN=... ts-node clients/typescript/examples/projects_patches.ts
    (or compile with tsc and run with node)
*/

import { ArwClient } from '../arw-client';

type Json = any;

function decodePointerSegment(seg: string): string {
  return seg.replace(/~1/g, '/').replace(/~0/g, '~');
}

function resolveParent(obj: any, pointer: string): { parent: any; key: string | number } {
  if (!pointer.startsWith('/')) throw new Error(`invalid pointer: ${pointer}`);
  const parts = pointer.split('/').slice(1).map(decodePointerSegment);
  if (parts.length === 0) throw new Error('empty pointer');
  let parent: any = obj;
  for (let i = 0; i < parts.length - 1; i++) {
    const k = parts[i];
    const isIndex = /^\d+$/.test(k);
    const key: any = isIndex ? Number(k) : k;
    if (parent[key] == null) {
      // create intermediate container conservatively as object
      parent[key] = {};
    }
    parent = parent[key];
  }
  const last = parts[parts.length - 1];
  const isIndex = /^\d+$/.test(last) || last === '-';
  const key: any = last === '-' ? '-' : isIndex ? Number(last) : last;
  return { parent, key };
}

function applyJsonPatch(doc: any, patch: Array<{ op: string; path: string; value?: Json }>) {
  for (const p of patch) {
    const { op, path } = p;
    const { parent, key } = resolveParent(doc, path);
    if (op === 'add') {
      if (Array.isArray(parent)) {
        if (key === '-') parent.push(p.value);
        else parent.splice(Number(key), 0, p.value);
      } else {
        (parent as any)[key] = p.value;
      }
    } else if (op === 'replace') {
      (parent as any)[key] = p.value;
    } else if (op === 'remove') {
      if (Array.isArray(parent)) parent.splice(Number(key), 1);
      else delete (parent as any)[key];
    } else if (op === 'test') {
      // ignore in client view
    } else {
      // move/copy not supported in this minimal example
    }
  }
}

async function main() {
  const base = process.env.BASE || 'http://127.0.0.1:8091';
  const token = process.env.ARW_ADMIN_TOKEN;
  const client = new ArwClient(base, token);

  const headers: Record<string, string> = {};
  if (token) headers['X-ARW-Admin'] = token;
  const snapResp = await fetch(`${base}/state/projects`, { headers });
  if (!snapResp.ok) throw new Error(`snapshot failed: ${snapResp.status}`);
  let projects = await snapResp.json();
  console.log('initial projects snapshot received');

  let lastId: string | undefined;
  const sub = client.events.subscribePatches(lastId);
  const setHandler = (onmessage: any, onerror: any) => {
    (sub as any).onmessage = onmessage;
    (sub as any).onerror = onerror;
  };
  setHandler(
    (e: any) => {
      lastId = e.lastEventId || lastId;
      try {
        const env = JSON.parse(String(e.data));
        if (env.kind === 'state.read.model.patch' && env.payload?.id === 'projects') {
          const ops = env.payload.patch as any[];
          applyJsonPatch(projects, ops);
          const count = Array.isArray(ops) ? ops.length : 0;
          console.log(`[patch] projects: ${count} ops; lastEventId=${lastId}`);
        }
      } catch {
        // ignore
      }
    },
    (err: any) => {
      console.error('sse error', err);
    },
  );

  // keep process alive
}

main().catch((e) => {
  console.error(e);
  process.exit(1);
});

