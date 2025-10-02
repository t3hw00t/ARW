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

export type EventStreamState = 'connecting' | 'open' | 'retrying' | 'closed';

export interface EventsStateChange {
  state: EventStreamState;
  attempt: number;
  delayMs?: number;
  error?: unknown;
}

export interface EventsOptions {
  // CSV prefixes for server-side filtering
  topics?: string[];
  // Resume from a known event id (prefer Last-Event-ID header; falls back to ?after=)
  lastEventId?: string;
  // Request the last N events if not resuming
  replay?: number;
  // Enable automatic reconnect with exponential backoff (Node fallback only)
  autoReconnect?: boolean;
  // Initial delay before attempting reconnect (ms)
  reconnectInitialDelayMs?: number;
  // Cap for reconnect delay (ms)
  reconnectMaxDelayMs?: number;
  // Jitter added to reconnect delay to avoid thundering herd (ms)
  reconnectJitterMs?: number;
  // Observe connection lifecycle changes (Node fallback emits granular states)
  onStateChange?: (change: EventsStateChange) => void;
  // Force reconnect when no events arrive within this window (Node fallback only)
  inactivityTimeoutMs?: number;
}

export interface EventEnvelope<T = Json> {
  id?: string;
  event?: string;
  data?: T;
}

export type EventHandler = (e: { data: any; lastEventId?: string; type?: string }) => void;
export type ErrorHandler = (e: Event) => void;

export type JsonPatchOp = {
  op: 'add' | 'remove' | 'replace' | 'move' | 'copy' | 'test';
  path: string;
  value?: Json;
  from?: string;
};

export interface SubscribeReadModelOptions {
  lastEventId?: string;
  replay?: number;
  initial?: Json;
  signal?: AbortSignal;
  onUpdate?: (snapshot: Json) => void;
  loadInitial?: () => Promise<Json>;
  autoReconnect?: boolean;
  reconnectInitialDelayMs?: number;
  reconnectMaxDelayMs?: number;
  reconnectJitterMs?: number;
  onStateChange?: (change: EventsStateChange) => void;
  inactivityTimeoutMs?: number;
}

export interface ReadModelSubscription {
  close(): void;
  getSnapshot(): Json | undefined;
  onUpdate(handler: (snapshot: Json) => void): () => void;
  lastEventId(): string | undefined;
}

export interface StreamOptions extends EventsOptions {
  signal?: AbortSignal;
  parseJson?: boolean;
}

export interface StreamEvent<T = Json> {
  data: T;
  raw: string | null;
  lastEventId?: string;
  type?: string;
}

const READ_MODEL_TOPIC = 'state.read.model.patch';

function isPlainObject(value: unknown): value is Record<string, unknown> {
  return Object.prototype.toString.call(value) === '[object Object]';
}

function deepClone<T>(value: T): T {
  if (value === null || typeof value !== 'object') {
    return value;
  }
  const structured = (globalThis as unknown as { structuredClone?: typeof structuredClone })
    .structuredClone;
  if (typeof structured === 'function') {
    try {
      return structured(value);
    } catch {
      // fall through to JSON clone
    }
  }
  return JSON.parse(JSON.stringify(value));
}

function decodePointerSegment(seg: string): string {
  return seg.replace(/~1/g, '/').replace(/~0/g, '~');
}

function parsePointer(pointer: string): string[] {
  if (!pointer) {
    return [];
  }
  if (!pointer.startsWith('/')) {
    throw new Error(`invalid JSON pointer: ${pointer}`);
  }
  return pointer
    .split('/')
    .slice(1)
    .map(decodePointerSegment);
}

function resolveContainer(
  root: Json,
  segments: string[],
  createMissing: boolean,
): { container: any; key: string } | null {
  if (segments.length === 0) {
    return { container: root, key: '' };
  }
  let node = root;
  for (let i = 0; i < segments.length - 1; i++) {
    const seg = segments[i];
    const nextSeg = segments[i + 1];
    const nextIsIndex = nextSeg === '-' || /^\d+$/.test(nextSeg ?? '');
    if (Array.isArray(node)) {
      let idx = seg === '-' ? node.length : Number(seg);
      if (!Number.isFinite(idx)) {
        return null;
      }
      if (idx >= node.length) {
        if (!createMissing) {
          return null;
        }
        while (node.length <= idx) {
          node.push(nextIsIndex ? [] : {});
        }
      }
      if (node[idx] == null || typeof node[idx] !== 'object') {
        if (!createMissing) {
          return null;
        }
        node[idx] = nextIsIndex ? [] : {};
      }
      node = node[idx];
      continue;
    }
    if (isPlainObject(node)) {
      if (!(seg in node) || node[seg] == null || typeof node[seg] !== 'object') {
        if (!createMissing) {
          if (!(seg in node)) {
            return null;
          }
          if (node[seg] == null) {
            return null;
          }
          return null;
        }
        node[seg] = nextIsIndex ? [] : {};
      }
      node = node[seg];
      continue;
    }
    if (!createMissing) {
      return null;
    }
    return null;
  }
  return { container: node, key: segments[segments.length - 1] ?? '' };
}

function getValueAt(root: Json, pointer: string): Json {
  const segments = parsePointer(pointer);
  if (segments.length === 0) {
    return root;
  }
  let node = root;
  for (const seg of segments) {
    if (Array.isArray(node)) {
      let idx = seg === '-' ? node.length - 1 : Number(seg);
      if (!Number.isFinite(idx)) {
        return undefined;
      }
      node = node[idx];
    } else if (isPlainObject(node)) {
      node = (node as Record<string, unknown>)[seg];
    } else {
      return undefined;
    }
  }
  return node;
}

function removeAt(root: Json, pointer: string): void {
  const segments = parsePointer(pointer);
  if (segments.length === 0) {
    if (Array.isArray(root)) {
      root.length = 0;
    } else if (isPlainObject(root)) {
      Object.keys(root).forEach((k) => delete (root as Record<string, unknown>)[k]);
    }
    return;
  }
  const parentInfo = resolveContainer(root, segments, false);
  if (!parentInfo) {
    return;
  }
  const { container, key } = parentInfo;
  if (Array.isArray(container)) {
    const idx = key === '-' ? container.length - 1 : Number(key);
    if (Number.isFinite(idx) && idx >= 0 && idx < container.length) {
      container.splice(idx, 1);
    }
  } else if (isPlainObject(container)) {
    delete container[key];
  }
}

function setAt(
  root: Json,
  pointer: string,
  value: Json,
  mode: 'add' | 'replace',
): void {
  const segments = parsePointer(pointer);
  if (segments.length === 0) {
    return;
  }
  const parentInfo = resolveContainer(root, segments, mode === 'add');
  if (!parentInfo) {
    return;
  }
  const { container, key } = parentInfo;
  if (Array.isArray(container)) {
    if (mode === 'add') {
      if (key === '-') {
        container.push(deepClone(value));
        return;
      }
      let idx = Number(key);
      if (!Number.isFinite(idx)) {
        return;
      }
      if (idx > container.length) {
        idx = container.length;
      }
      container.splice(idx, 0, deepClone(value));
      return;
    }
    const idx = Number(key);
    if (!Number.isFinite(idx) || idx < 0 || idx >= container.length) {
      return;
    }
    container[idx] = deepClone(value);
    return;
  }
  if (isPlainObject(container)) {
    (container as Record<string, unknown>)[key] = deepClone(value);
  }
}

function applyJsonPatchMutable(target: Json, patch: JsonPatchOp[]): Json {
  let root: Json = target;
  let rootChanged = false;
  if (root === undefined || root === null) {
    root = {};
    rootChanged = true;
  }

  const assignRoot = (next: Json) => {
    if (Array.isArray(root) && Array.isArray(next)) {
      root.length = 0;
      next.forEach((item) => root.push(deepClone(item)));
      return;
    }
    if (isPlainObject(root) && isPlainObject(next)) {
      Object.keys(root).forEach((k) => delete (root as Record<string, unknown>)[k]);
      Object.entries(next).forEach(([k, v]) => {
        (root as Record<string, unknown>)[k] = deepClone(v);
      });
      return;
    }
    root = deepClone(next);
    rootChanged = true;
  };

  for (const op of patch) {
    const pointer = op.path ?? '';
    const segments = pointer ? parsePointer(pointer) : [];
    switch (op.op) {
      case 'add':
        if (segments.length === 0) {
          assignRoot(op.value);
        } else {
          setAt(root, pointer, op.value, 'add');
        }
        break;
      case 'replace':
        if (segments.length === 0) {
          assignRoot(op.value);
        } else {
          setAt(root, pointer, op.value, 'replace');
        }
        break;
      case 'remove':
        if (segments.length === 0) {
          assignRoot({});
        } else {
          removeAt(root, pointer);
        }
        break;
      case 'copy':
        if (!op.from) {
          break;
        }
        {
          const value = deepClone(getValueAt(root, op.from));
          if (segments.length === 0) {
            assignRoot(value);
          } else {
            setAt(root, pointer, value, 'add');
          }
        }
        break;
      case 'move':
        if (!op.from) {
          break;
        }
        {
          const current = getValueAt(root, op.from);
          if (current === undefined) {
            break;
          }
          const value = deepClone(current);
          removeAt(root, op.from);
          if (segments.length === 0) {
            assignRoot(value);
          } else {
            setAt(root, pointer, value, 'add');
          }
        }
        break;
      case 'test':
        // no-op client side; real enforcement happens server-side
        break;
      default:
        break;
    }
  }

  return rootChanged ? root : target ?? root;
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
    // Browser EventSource (no auth headers), Node fallback uses fetch streaming with headers
    subscribe: (opts?: EventsOptions): EventSource | { close: () => void; onmessage: EventHandler | null; onerror: ErrorHandler | null } => {
      const p = new URL(`${this.base}/events`);
      if (opts?.topics?.length) p.searchParams.set('prefix', opts.topics.join(','));
      if (opts?.replay && !opts.lastEventId) p.searchParams.set('replay', String(opts.replay));
      const url = p.toString();
      const autoReconnect = opts?.autoReconnect !== false;
      const inactivityBudgetMs = opts?.inactivityTimeoutMs && opts.inactivityTimeoutMs > 0
        ? opts.inactivityTimeoutMs
        : undefined;
      const emitState = (change: EventsStateChange) => {
        try {
          opts?.onStateChange?.(change);
        } catch {
          // ignore observer errors
        }
      };

      // If EventSource exists (browser/Deno), prefer it. Cannot set headers here.
      if (typeof (globalThis as any).EventSource !== 'undefined') {
        emitState({ state: 'connecting', attempt: 0 });
        if (opts?.lastEventId) {
          // Use query param fallback for resume when headers are unavailable
          p.searchParams.set('after', opts.lastEventId);
        }
        const es: EventSource = new (globalThis as any).EventSource(p.toString(), { withCredentials: false });
        let attempt = 0;
        const originalClose = es.close.bind(es);

        const handleOpen = () => {
          attempt = 0;
          emitState({ state: 'open', attempt });
        };

        const handleError = (event: Event) => {
          if (!autoReconnect) {
            cleanup();
            emitState({ state: 'closed', attempt, error: event });
            originalClose();
            return;
          }
          attempt += 1;
          const readyState = (es as any).readyState;
          if (readyState === 2 /* CLOSED */) {
            emitState({ state: 'closed', attempt, error: event });
          } else {
            emitState({ state: 'retrying', attempt, error: event });
          }
        };

        const cleanup = () => {
          es.removeEventListener('open', handleOpen as EventListener);
          es.removeEventListener('error', handleError as EventListener);
        };

        es.addEventListener('open', handleOpen as EventListener);
        es.addEventListener('error', handleError as EventListener);

        (es as EventSource & { close(): void }).close = () => {
          cleanup();
          emitState({ state: 'closed', attempt });
          originalClose();
        };
        return es;
      }

      // Node fallback: stream and parse SSE manually; can send admin header
      let controller: AbortController | null = null;
      let reconnectTimer: ReturnType<typeof setTimeout> | null = null;
      let reconnectAttempt = 0;
      let lastEventId = opts?.lastEventId;
      let serverRetryMs: number | undefined;
      let inactivityTimer: ReturnType<typeof setTimeout> | null = null;
      const nodeInactivityMs = inactivityBudgetMs;
      const initialDelay = Math.max(opts?.reconnectInitialDelayMs ?? 500, 0);
      const maxDelay = Math.max(opts?.reconnectMaxDelayMs ?? 30_000, initialDelay);
      const jitterMs = Math.max(opts?.reconnectJitterMs ?? 250, 0);
      const clearInactivity = () => {
        if (inactivityTimer) {
          clearTimeout(inactivityTimer);
          inactivityTimer = null;
        }
      };
      const armInactivity = () => {
        if (!nodeInactivityMs) {
          return;
        }
        clearInactivity();
        inactivityTimer = setTimeout(() => {
          if (closed) {
            return;
          }
          clearInactivity();
          const timeoutErr = new Error('SSE idle timeout');
          // Surface as regular error before forcing reconnect.
          try {
            out.onerror?.(timeoutErr as any);
          } catch {}
          if (controller) {
            try {
              controller.abort();
            } catch {}
          }
          scheduleReconnect(timeoutErr);
        }, nodeInactivityMs);
      };
      const clearTimer = () => {
        if (reconnectTimer) {
          clearTimeout(reconnectTimer);
          reconnectTimer = null;
        }
      };

      const scheduleReconnect = (error: unknown) => {
        clearInactivity();
        if (!autoReconnect) {
          emitState({ state: 'closed', attempt: reconnectAttempt, error });
          return;
        }
        clearTimer();
        const baseDelay = serverRetryMs ?? Math.min(maxDelay, initialDelay * Math.pow(2, reconnectAttempt));
        const jitter = jitterMs ? Math.random() * jitterMs : 0;
        const delay = baseDelay + jitter;
        emitState({ state: 'retrying', attempt: reconnectAttempt + 1, delayMs: delay, error });
        reconnectTimer = setTimeout(() => {
          reconnectTimer = null;
          if (closed) {
            return;
          }
          reconnectAttempt += 1;
          startStream();
        }, delay);
      };

      let closed = false;

      const headers: Record<string, string> = {};
      if (this.adminToken) headers['X-ARW-Admin'] = this.adminToken;

      const out = {
        onmessage: null as EventHandler | null,
        onerror: null as ErrorHandler | null,
        close: () => {
          closed = true;
          emitState({ state: 'closed', attempt: reconnectAttempt });
          clearTimer();
          clearInactivity();
          if (controller) {
            try { controller.abort(); } catch {}
            controller = null;
          }
        },
      };

      const startStream = async () => {
        if (closed) {
          return;
        }
        clearTimer();
        emitState({ state: 'connecting', attempt: reconnectAttempt });
        controller = new AbortController();
        const requestHeaders: Record<string, string> = { ...headers };
        if (lastEventId) {
          requestHeaders['Last-Event-ID'] = lastEventId;
        }
        try {
          const response = await fetch(url, { headers: requestHeaders, signal: controller.signal as any });
          if (!response.ok || !response.body) {
            throw new Error(`SSE failed: ${response.status}`);
          }
          reconnectAttempt = 0;
          emitState({ state: 'open', attempt: 0 });
          armInactivity();
          const reader = (response.body as any).getReader();
          const decoder = new TextDecoder();
          let buffer = '';
          let event: { id?: string; event?: string; data?: string } = {};
          const flush = () => {
            if (event.data == null) {
              return;
            }
            const payload = event.data.endsWith('\n') ? event.data.slice(0, -1) : event.data;
            if (event.id) {
              lastEventId = event.id;
            }
            const msg = {
              data: payload,
              lastEventId,
              type: event.event || 'message',
            };
            out.onmessage?.(msg);
            event = {};
          };
          while (!closed) {
            const { done, value } = await reader.read();
            if (done) {
              break;
            }
            buffer += decoder.decode(value, { stream: true });
            armInactivity();
            let idx: number;
            while ((idx = buffer.indexOf('\n')) >= 0) {
              let line = buffer.slice(0, idx);
              buffer = buffer.slice(idx + 1);
              if (line.endsWith('\r')) {
                line = line.slice(0, -1);
              }
              if (line === '') {
                flush();
                continue;
              }
              if (line.startsWith(':')) {
                continue;
              }
              const colon = line.indexOf(':');
              const field = colon === -1 ? line : line.slice(0, colon);
              let val = colon === -1 ? '' : line.slice(colon + 1);
              if (val.startsWith(' ')) {
                val = val.slice(1);
              }
              switch (field) {
                case 'id':
                  event.id = val;
                  break;
                case 'event':
                  event.event = val;
                  break;
                case 'data':
                  event.data = (event.data || '') + val + '\n';
                  break;
                case 'retry': {
                  const retryMs = Number(val);
                  if (Number.isFinite(retryMs) && retryMs >= 0) {
                    serverRetryMs = retryMs;
                  }
                  break;
                }
              }
            }
          }
          if (!closed) {
            scheduleReconnect(new Error('SSE stream ended'));
          }
        } catch (err) {
          const name = (err as any)?.name;
          if (closed || name === 'AbortError') {
            return;
          }
          try {
            out.onerror?.(err as any);
          } catch {}
          scheduleReconnect(err);
        } finally {
          controller = null;
        }
      };

      startStream().catch((err) => {
        if (!closed) {
          try {
            out.onerror?.(err as any);
          } catch {}
          scheduleReconnect(err);
        }
      });

      return out;
    },

    // Convenience: subscribe to read-model patch stream with resume
    subscribePatches: (arg?: string | EventsOptions) => {
      const baseOptions: EventsOptions =
        typeof arg === 'string' || typeof arg === 'undefined' ? { lastEventId: arg } : arg;
      const effectiveReplay = baseOptions.lastEventId
        ? baseOptions.replay ?? 0
        : baseOptions.replay ?? 50;
      return this.events.subscribe({
        ...baseOptions,
        topics: [READ_MODEL_TOPIC],
        replay: effectiveReplay,
      });
    },

    subscribeReadModel: (
      id: string,
      options?: SubscribeReadModelOptions,
    ): ReadModelSubscription => {
      const opts = options ?? {};
      let snapshot: Json | undefined =
        opts.initial !== undefined ? deepClone(opts.initial) : undefined;
      let hasSnapshot = opts.initial !== undefined;
      let lastEventId = opts.lastEventId;
      const listeners = new Set<(value: Json) => void>();
      if (opts.onUpdate) {
        listeners.add(opts.onUpdate);
      }

      const notify = () => {
        if (!listeners.size || !hasSnapshot) {
          return;
        }
        const base = snapshot;
        if (base === undefined) {
          return;
        }
        const cloned = deepClone(base);
        for (const fn of listeners) {
          try {
            fn(cloned);
          } catch {
            // ignore listener errors
          }
        }
      };

      const pending: { patch: JsonPatchOp[]; eventId?: string }[] = [];

      const replay = opts.lastEventId ? opts.replay ?? 0 : opts.replay ?? 50;
      const sub = this.events.subscribe({
        topics: [READ_MODEL_TOPIC],
        lastEventId: opts.lastEventId,
        replay,
        autoReconnect: opts.autoReconnect,
        reconnectInitialDelayMs: opts.reconnectInitialDelayMs,
        reconnectMaxDelayMs: opts.reconnectMaxDelayMs,
        reconnectJitterMs: opts.reconnectJitterMs,
        onStateChange: opts.onStateChange,
        inactivityTimeoutMs: opts.inactivityTimeoutMs,
      });

      let closed = false;

      const handleEvent = (payload: string | null, eventId?: string) => {
        if (!payload) {
          return;
        }
        try {
          const env = JSON.parse(String(payload));
          if (!env || env.kind !== READ_MODEL_TOPIC) {
            return;
          }
          const rmPayload = env.payload ?? {};
          if (rmPayload.id !== id) {
            return;
          }
          const patch = rmPayload.patch;
          if (!Array.isArray(patch)) {
            return;
          }
          const normalized = JSON.parse(JSON.stringify(patch)) as JsonPatchOp[];
          if (!hasSnapshot) {
            pending.push({ patch: normalized, eventId });
            if (eventId) {
              lastEventId = eventId;
            }
            return;
          }
          snapshot = applyJsonPatchMutable(snapshot ?? {}, normalized);
          if (eventId) {
            lastEventId = eventId;
          }
          notify();
        } catch {
          // ignore parse errors
        }
      };

      let detach: (() => void) | undefined;

      if (typeof (sub as any).addEventListener === 'function') {
        const patchListener = (evt: MessageEvent) => {
          handleEvent(typeof evt.data === 'string' ? evt.data : String(evt.data ?? ''), evt.lastEventId || undefined);
        };
        (sub as EventSource).addEventListener(READ_MODEL_TOPIC, patchListener as EventListener);
        detach = () => {
          (sub as EventSource).removeEventListener(READ_MODEL_TOPIC, patchListener as EventListener);
          if (typeof (sub as EventSource).close === 'function') {
            (sub as EventSource).close();
          }
        };
      } else {
        const sink = sub as { onmessage: EventHandler | null; close: () => void; onerror?: ErrorHandler | null };
        const prev = sink.onmessage;
        sink.onmessage = (evt) => {
          handleEvent(evt?.data ?? null, evt?.lastEventId);
          prev?.(evt);
        };
        detach = () => {
          sink.onmessage = prev;
          if (typeof sink.close === 'function') {
            sink.close();
          }
        };
      }

      const subscription: ReadModelSubscription = {
        close: () => {
          if (closed) {
            return;
          }
          closed = true;
          detach?.();
          listeners.clear();
        },
        getSnapshot: () => (hasSnapshot ? deepClone(snapshot) : undefined),
        onUpdate: (fn: (value: Json) => void) => {
          listeners.add(fn);
          return () => {
            listeners.delete(fn);
          };
        },
        lastEventId: () => lastEventId,
      };

      if (opts.signal) {
        if (opts.signal.aborted) {
          subscription.close();
        } else {
          opts.signal.addEventListener('abort', () => subscription.close());
        }
      }

      const hydrateFromInitial = (initialValue: Json | undefined) => {
        snapshot = initialValue !== undefined ? deepClone(initialValue) : {};
        hasSnapshot = true;
        if (pending.length) {
          for (const entry of pending.splice(0)) {
            snapshot = applyJsonPatchMutable(snapshot ?? {}, entry.patch);
            if (entry.eventId) {
              lastEventId = entry.eventId;
            }
          }
        }
        notify();
      };

      if (opts.initial !== undefined) {
        hydrateFromInitial(opts.initial);
      } else if (typeof opts.loadInitial === 'function') {
        const loader = opts.loadInitial;
        (async () => {
          try {
            const loaded = await loader();
            hydrateFromInitial(loaded);
          } catch (err) {
            console.warn('subscribeReadModel loadInitial failed', err);
            hydrateFromInitial({});
          }
        })();
      } else {
        hydrateFromInitial({});
      }

      return subscription;
    },

    stream: <T = Json>(options?: StreamOptions): AsyncGenerator<StreamEvent<T>> => {
      if (typeof (globalThis as any).EventSource !== 'undefined') {
        throw new Error('events.stream() is intended for Node environments without EventSource');
      }
      const { signal, parseJson = true, ...eventOpts } = options ?? {};
      const sub = this.events.subscribe(eventOpts as EventsOptions);
      if (typeof (sub as any).addEventListener === 'function') {
        throw new Error('events.stream() cannot operate on EventSource instances; use subscribe() instead.');
      }
      const sink = sub as {
        onmessage: EventHandler | null;
        onerror: ErrorHandler | null;
        close: () => void;
      };
      const prevOnMessage = sink.onmessage;
      const prevOnError = sink.onerror;

      const queue: StreamEvent<T>[] = [];
      const waiters: Array<{
        resolve: (value: IteratorResult<StreamEvent<T>>) => void;
        reject: (err: any) => void;
      }> = [];
      let finished = false;
      let storedError: any = null;
      let abortHandler: (() => void) | null = null;

      const detach = () => {
        sink.onmessage = prevOnMessage;
        sink.onerror = prevOnError ?? null;
        if (signal && abortHandler) {
          signal.removeEventListener('abort', abortHandler);
        }
        abortHandler = null;
      };

      const closeSink = () => {
        if (finished) {
          return;
        }
        finished = true;
        try {
          sink.close();
        } catch {
          // ignore
        }
        detach();
      };

      const toStreamEvent = (evt: any): StreamEvent<T> => {
        const rawValue = evt?.data ?? null;
        const raw = typeof rawValue === 'string' ? rawValue : null;
        let payload: any = rawValue;
        if (parseJson && typeof rawValue === 'string') {
          try {
            payload = JSON.parse(rawValue);
          } catch {
            payload = rawValue;
          }
        }
        return {
          data: payload as T,
          raw,
          lastEventId: evt?.lastEventId,
          type: evt?.type ?? (evt?.event as string | undefined),
        };
      };

      sink.onmessage = (evt) => {
        if (finished) {
          return;
        }
        const event = toStreamEvent(evt);
        if (waiters.length) {
          const { resolve } = waiters.shift()!;
          resolve({ value: event, done: false });
        } else {
          queue.push(event);
        }
      };

      sink.onerror = (err) => {
        if (finished) {
          return;
        }
        storedError = err ?? new Error('SSE stream error');
        closeSink();
        while (waiters.length) {
          const { reject } = waiters.shift()!;
          reject(storedError);
        }
      };

      if (signal) {
        const handler = () => {
          closeSink();
          while (waiters.length) {
            const { resolve } = waiters.shift()!;
            resolve({ value: undefined, done: true });
          }
        };
        abortHandler = handler;
        if (signal.aborted) {
          handler();
        } else {
          signal.addEventListener('abort', handler);
        }
      }

      const iterator: AsyncGenerator<StreamEvent<T>> = {
        async next(): Promise<IteratorResult<StreamEvent<T>>> {
          if (queue.length) {
            const value = queue.shift()!;
            return { value, done: false };
          }
          if (storedError) {
            const err = storedError;
            storedError = null;
            return Promise.reject(err);
          }
          if (finished) {
            return { value: undefined, done: true };
          }
          return new Promise<IteratorResult<StreamEvent<T>>>((resolve, reject) => {
            waiters.push({ resolve, reject });
          });
        },
        async return(): Promise<IteratorResult<StreamEvent<T>>> {
          closeSink();
          while (waiters.length) {
            const { resolve } = waiters.shift()!;
            resolve({ value: undefined, done: true });
          }
          return { value: undefined, done: true };
        },
        async throw(err: any): Promise<IteratorResult<StreamEvent<T>>> {
          closeSink();
          while (waiters.length) {
            const { reject } = waiters.shift()!;
            reject(err);
          }
          return Promise.reject(err);
        },
        [Symbol.asyncIterator](): AsyncGenerator<StreamEvent<T>> {
          return this;
        },
      };

      return iterator;
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
