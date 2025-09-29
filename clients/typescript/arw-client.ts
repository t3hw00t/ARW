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
  // CSV prefixes for server-side filtering
  topics?: string[];
  // Resume from a known event id (prefer Last-Event-ID header; falls back to ?after=)
  lastEventId?: string;
  // Request the last N events if not resuming
  replay?: number;
}

export interface EventEnvelope<T = Json> {
  id?: string;
  event?: string;
  data?: T;
}

export type EventHandler = (e: { data: any; lastEventId?: string; type?: string }) => void;
export type ErrorHandler = (e: Event) => void;

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
    // Browser EventSource (no auth headers), Node fallback uses fetch streaming with headers
    subscribe: (opts?: EventsOptions): EventSource | { close: () => void; onmessage: EventHandler | null; onerror: ErrorHandler | null } => {
      const p = new URL(`${this.base}/events`);
      if (opts?.topics?.length) p.searchParams.set('prefix', opts.topics.join(','));
      if (opts?.replay && !opts.lastEventId) p.searchParams.set('replay', String(opts.replay));
      const url = p.toString();

      // If EventSource exists (browser/Deno), prefer it. Cannot set headers here.
      if (typeof (globalThis as any).EventSource !== 'undefined') {
        if (opts?.lastEventId) {
          // Use query param fallback for resume when headers are unavailable
          p.searchParams.set('after', opts.lastEventId);
        }
        // @ts-ignore
        return new (globalThis as any).EventSource(p.toString(), { withCredentials: false });
      }

      // Node fallback: stream and parse SSE manually; can send admin header
      let controller = new AbortController();
      const headers: Record<string, string> = {};
      if (this.adminToken) headers['X-ARW-Admin'] = this.adminToken;
      if (opts?.lastEventId) headers['Last-Event-ID'] = opts.lastEventId;
      const out = {
        onmessage: null as EventHandler | null,
        onerror: null as ErrorHandler | null,
        close: () => {
          try { controller.abort(); } catch {}
        },
      };
      (async () => {
        try {
          const r = await fetch(url, { headers, signal: controller.signal as any });
          if (!r.ok || !r.body) throw new Error(`SSE failed: ${r.status}`);
          const reader = (r.body as any).getReader();
          let buf = '';
          let ev: { id?: string; event?: string; data?: string } = {};
          const flush = () => {
            if (ev.data == null) return;
            const payload = ev.data.endsWith('\n') ? ev.data.slice(0, -1) : ev.data;
            const msg = {
              data: payload,
              // @ts-ignore
              lastEventId: ev.id || '',
              type: ev.event || 'message',
            };
            out.onmessage?.(msg);
            ev = {};
          };
          while (true) {
            const { done, value } = await reader.read();
            if (done) break;
            buf += new TextDecoder().decode(value, { stream: true });
            let idx: number;
            while ((idx = buf.indexOf('\n')) >= 0) {
              const line = buf.slice(0, idx);
              buf = buf.slice(idx + 1);
              if (line === '') { flush(); continue; }
              if (line.startsWith(':')) continue; // comment
              const colon = line.indexOf(':');
              const field = colon === -1 ? line : line.slice(0, colon);
              let val = colon === -1 ? '' : line.slice(colon + 1);
              if (val.startsWith(' ')) val = val.slice(1);
              switch (field) {
                case 'id': ev.id = val; break;
                case 'event': ev.event = val; break;
                case 'data': ev.data = (ev.data || '') + val + '\n'; break;
              }
            }
          }
        } catch (e) {
          try { out.onerror?.(e as any); } catch {}
        }
      })();
      return out;
    },

    // Convenience: subscribe to read-model patch stream with resume
    subscribePatches: (lastEventId?: string) => {
      return this.events.subscribe({ topics: ['state.read.model.patch'], lastEventId, replay: 50 });
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
