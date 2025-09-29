export type Json = any;

export interface ActionSubmit {
  kind: string;
  input?: Json;
}

export interface ActionState {
  id: string;
  state: 'pending' | 'running' | 'completed' | 'failed';
  output?: Json;
  error?: Json;
}

export interface EventsOptions {
  topics?: string[];
  lastEventId?: string;
}

export class ArwClient {
  constructor(public base: string, public adminToken?: string) {}

  private headers(extra?: Record<string, string>): Record<string, string> {
    const h: Record<string, string> = { ...(extra || {}) };
    if (this.adminToken) h['X-ARW-Admin'] = this.adminToken;
    return h;
  }

  async health(): Promise<string> {
    const r = await fetch(`${this.base}/healthz`);
    return r.text();
  }

  async about(): Promise<Json> {
    const r = await fetch(`${this.base}/about`);
    return r.json();
  }

  setAdminToken(token?: string) {
    this.adminToken = token;
  }

  actions = {
    submit: async (req: ActionSubmit): Promise<{ id: string }> => {
      const r = await fetch(`${this.base}/actions`, {
        method: 'POST',
        headers: this.headers({ 'content-type': 'application/json' }),
        body: JSON.stringify(req),
      });
      if (!r.ok) throw new Error(`submit failed: ${r.status}`);
      return r.json();
    },

    get: async (id: string): Promise<ActionState> => {
      const r = await fetch(`${this.base}/actions/${encodeURIComponent(id)}`);
      if (!r.ok) throw new Error(`get failed: ${r.status}`);
      return r.json();
    },

    wait: async (id: string, timeoutMs = 30_000, intervalMs = 250): Promise<ActionState> => {
      const start = Date.now();
      // eslint-disable-next-line no-constant-condition
      while (true) {
        const st = await this.actions.get(id);
        if (st.state === 'completed' || st.state === 'failed') return st;
        if (Date.now() - start > timeoutMs) throw new Error('timeout');
        await new Promise((r) => setTimeout(r, intervalMs));
      }
    },
  };

  events = {
    subscribe: (opts?: EventsOptions): EventSource => {
      const p = new URL(`${this.base}/events`);
      if (opts?.topics?.length) p.searchParams.set('prefix', opts.topics.join(','));
      if (opts?.lastEventId) p.searchParams.set('after', opts.lastEventId);
      const es = new EventSource(p.toString(), { withCredentials: false });
      return es;
    },
  };

  leases = {
    create: async (capability: string, ttl_secs = 60): Promise<Json> => {
      const r = await fetch(`${this.base}/leases`, {
        method: 'POST',
        headers: this.headers({ 'content-type': 'application/json' }),
        body: JSON.stringify({ capability, ttl_secs }),
      });
      if (!r.ok) throw new Error(`lease create failed: ${r.status}`);
      return r.json();
    },
    state: async (): Promise<Json> => {
      const r = await fetch(`${this.base}/state/leases`, { headers: this.headers() });
      if (!r.ok) throw new Error(`lease state failed: ${r.status}`);
      return r.json();
    },
  };
}
