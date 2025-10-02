/*
  Minimal Node example:
  - Fetch initial projects snapshot
  - Subscribe to state.read.model.patch via subscribeReadModel helper
  - Keep a local snapshot updated automatically

  Usage:
    BASE=http://127.0.0.1:8091 ARW_ADMIN_TOKEN=... ts-node clients/typescript/examples/projects_patches.ts
    (or compile with tsc and run with node)
*/

import { ArwClient } from '../arw-client';

async function main() {
  const base = process.env.BASE || 'http://127.0.0.1:8091';
  const token = process.env.ARW_ADMIN_TOKEN;
  const client = new ArwClient(base, token);

  let projects: any;
  const subscription = client.events.subscribeReadModel('projects', {
    loadInitial: async () => {
      const headers: Record<string, string> = {};
      if (token) headers['X-ARW-Admin'] = token;
      const resp = await fetch(`${base}/state/projects`, { headers });
      if (!resp.ok) throw new Error(`snapshot failed: ${resp.status}`);
      const json = await resp.json();
      console.log('initial projects snapshot received');
      projects = json;
      return json;
    },
    onUpdate: (state) => {
      projects = state;
      const items = Array.isArray(state?.items) ? state.items.length : 0;
      const last = subscription.lastEventId() ?? 'n/a';
      console.log(`[patch] projects updated — items=${items}, lastEventId=${last}`);
    },
    onStateChange: ({ state, attempt, delayMs }) => {
      if (state === 'open') {
        console.error('read-model stream connected');
      } else if (state === 'retrying') {
        console.error(`read-model retry in ${Math.round(delayMs ?? 0)}ms (attempt ${attempt})`);
      } else if (state === 'closed') {
        console.error('read-model stream closed');
      }
    },
    inactivityTimeoutMs: 60_000,
  });

  process.on('SIGINT', () => {
    console.log('\nclosing subscription');
    subscription.close();
    process.exit(0);
  });
}

main().catch((e) => {
  console.error(e);
  process.exit(1);
});
