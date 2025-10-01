// Lightweight helpers shared by launcher pages
// Capture optional base override from query string (`?base=http://host:port`)
(() => {
  try {
    const current = new URL(window.location.href);
    const raw = current.searchParams.get('base');
    if (!raw) return;
    const cleaned = (() => {
      const str = String(raw).trim();
      if (!str) return '';
      const strip = (val) => val.replace(/\/+$/, '');
      try {
        return strip(new URL(str).origin || str);
      } catch {
        if (!/^[a-zA-Z][a-zA-Z0-9+\-.]*:\/\//.test(str)) {
          try { return strip(new URL(`http://${str}`).origin || str); }
          catch { return strip(str); }
        }
        return strip(str);
      }
    })();
    if (cleaned) {
      window.__ARW_BASE_OVERRIDE = cleaned;
    }
  } catch {}
})();

window.ARW = {
  _prefsCache: new Map(),
  _prefsTimers: new Map(),
  _ocrCache: new Map(),
  util: {
    pageId(){
      try{
        const d = document.body?.dataset?.page; if (d) return String(d);
        const p = (window.location.pathname||'').split('/').pop() || 'index.html';
        return p.replace(/\.html?$/i,'') || 'index';
      }catch{ return 'index' }
    }
  },
  validateProjectName(name) {
    const raw = String(name ?? '').trim();
    if (!raw) return { ok: false, error: 'Project name cannot be empty' };
    if (raw.length > 120) return { ok: false, error: 'Project name must be 120 characters or fewer' };
    if (raw.startsWith('.')) return { ok: false, error: 'Project name cannot start with a dot' };
    const valid = /^[A-Za-z0-9 _.-]+$/.test(raw);
    if (!valid) return { ok: false, error: 'Project name may only contain letters, numbers, spaces, ., -, _' };
    return { ok: true, value: raw };
  },
  validateProjectRelPath(rel) {
    const raw = String(rel ?? '').trim();
    if (!raw) return { ok: false, error: 'Destination path cannot be empty' };
    if (/^[\\/]/.test(raw)) return { ok: false, error: 'Destination must be relative (no leading / or \\)' };
    if (/^[A-Za-z]:/.test(raw)) return { ok: false, error: 'Destination must not include a drive prefix' };
    if (/^\\\\/.test(raw)) return { ok: false, error: 'Destination must not include a UNC prefix' };
    const parts = raw.split(/[\\/]+/).filter(Boolean);
    if (!parts.length) return { ok: false, error: 'Destination path cannot be empty' };
    if (parts.some(seg => seg === '.' || seg === '..')) {
      return { ok: false, error: 'Destination must not contain . or .. segments' };
    }
    return { ok: true, value: parts.join('/') };
  },
  invoke(cmd, args) {
    return window.__TAURI__.invoke(cmd, args)
  },
  // Clipboard helper
  async copy(text){ try{ await navigator.clipboard.writeText(text); this.toast('Copied'); }catch{} },
  templates: {
    async save(ns, tpl){
      try{
        const key = 'ui:'+ns;
        const cur = await ARW.getPrefs(key);
        const next = { ...(cur&&typeof cur==='object'?cur:{}), template: tpl };
        await ARW.setPrefs(key, next);
        ARW.toast('Layout saved');
      }catch(e){ ARW.toast('Save failed'); }
    },
    async load(ns){
      try{
        const key = 'ui:'+ns; const v = await ARW.getPrefs(key); return v?.template || null;
      }catch{ return null }
    }
  },
  connections: {
    _norm(b){
      try{
        const normalized = ARW.normalizeBase(b);
        return normalized || '';
      }catch{ return ''; }
    },
    async tokenFor(base){
      try{
        const prefs = await ARW.getPrefs('launcher') || {};
        const norm = this._norm(base);
        const list = Array.isArray(prefs.connections) ? prefs.connections : [];
        const hit = list.find(c => this._norm(c.base) === norm);
        const connToken = typeof hit?.token === 'string' ? hit.token.trim() : '';
        if (connToken) return connToken;
        const fallback = typeof prefs.adminToken === 'string' ? prefs.adminToken.trim() : '';
        return fallback || null;
      }catch{ return null }
    }
  },
  http: {
    _norm(base){
      try{
        const norm = ARW.normalizeBase(base);
        if (norm) return norm;
      }catch{}
      try{ return String(base||'').replace(/\/+$/,''); }catch{ return ''; }
    },
    async _headers(base, extra){
      const headers = Object.assign({}, extra || {});
      let token = null;
      try {
        if (base) token = await ARW.connections.tokenFor(base);
      } catch {}
      if (token) {
        const hasAuth = Object.keys(headers).some(k => k.toLowerCase() === 'authorization');
        if (!headers['X-ARW-Admin'] && !headers['x-arw-admin']) headers['X-ARW-Admin'] = token;
        if (!hasAuth) headers['Authorization'] = `Bearer ${token}`;
      }
      return headers;
    },
    async fetch(baseOrUrl, pathOrInit, maybeInit){
      let url = baseOrUrl;
      let init = {};
      let tokenBase = null;
      if (typeof pathOrInit === 'string') {
        tokenBase = baseOrUrl;
        url = this._norm(baseOrUrl) + (pathOrInit.startsWith('/') ? pathOrInit : '/' + pathOrInit);
        init = maybeInit || {};
      } else {
        init = pathOrInit || {};
        tokenBase = (()=>{
          try { return new URL(baseOrUrl).origin; } catch { return baseOrUrl; }
        })();
      }
      const opts = Object.assign({}, init);
      opts.headers = await this._headers(tokenBase, init.headers);
      return fetch(url, opts);
    },
    async json(baseOrUrl, pathOrInit, maybeInit){
      const resp = await this.fetch(baseOrUrl, pathOrInit, maybeInit);
      if (!resp.ok) throw new Error('HTTP '+resp.status);
      return resp.json();
    },
    async text(baseOrUrl, pathOrInit, maybeInit){
      const resp = await this.fetch(baseOrUrl, pathOrInit, maybeInit);
      if (!resp.ok) throw new Error('HTTP '+resp.status);
      return resp.text();
    }
  },
  toast(msg) {
    if (!this._toastWrap) {
      const wrap = document.createElement('div');
      wrap.className = 'toast-wrap';
      document.body.appendChild(wrap);
      this._toastWrap = wrap;
    }
    const d = document.createElement('div');
    d.className = 'toast'; d.textContent = msg;
    this._toastWrap.appendChild(d);
    setTimeout(()=>{ try{ this._toastWrap.removeChild(d); }catch(e){} }, 2500);
  },
  async getPrefs(ns = 'launcher') {
    try{
      if (this._prefsCache.has(ns)) {
        const v = this._prefsCache.get(ns);
        // return a shallow clone to avoid surprise mutation
        return v && typeof v === 'object' ? { ...v } : v;
      }
      const v = await this.invoke('get_prefs', { namespace: ns });
      if (v && typeof v === 'object') this._prefsCache.set(ns, { ...v }); else this._prefsCache.set(ns, v);
      return v;
    }catch{ return {} }
  },
  async saveToProjectPrompt(path){
    try{
      const projInput = prompt('Project name'); if (!projInput) return null;
      const projCheck = this.validateProjectName(projInput);
      if (!projCheck.ok){ this.toast(projCheck.error); return null; }
      const proj = projCheck.value;
      const baseName = (path||'').split(/[\\/]/).pop() || 'capture.png';
      let destInput = prompt('Destination path inside project', 'images/'+baseName);
      if (destInput == null) return null;
      destInput = String(destInput).trim();
      if (!destInput) destInput = 'images/'+baseName;
      const destCheck = this.validateProjectRelPath(destInput);
      if (!destCheck.ok){ this.toast(destCheck.error); return null; }
      const dest = destCheck.value;
      const out = await this.invoke('projects_import', { proj, dest, src_path: path, mode: 'copy', port: this.getPortFromInput('port') });
      this.toast('Saved to '+proj+': '+dest);
      return { proj, dest };
    }catch(e){ console.error(e); this.toast('Import failed'); return null; }
  },
  _bestAltForPath(path, fallback){
    const record = path ? this._ocrCache.get(path) : null;
    if (record && typeof record.text === 'string'){
      const firstLine = record.text.split(/\r?\n/).find(line => line.trim());
      if (firstLine){
        const trimmed = firstLine.trim();
        if (trimmed.length > 120) return trimmed.slice(0, 117) + '…';
        return trimmed;
      }
    }
    if (fallback && fallback.trim()) return fallback;
    return 'screenshot';
  },
  _updateAltForPath(path){
    if (!path) return;
    const alt = this._bestAltForPath(path, path.split(/[\\/]/).pop() || 'screenshot');
    try{
      const selector = `[data-screenshot-path="${window.CSS?.escape ? CSS.escape(path) : path.replace(/"/g,'\\"')}"]`;
      document.querySelectorAll(selector).forEach(img => { if (img instanceof HTMLImageElement){ img.alt = alt; img.dataset.alt = alt; } });
    }catch{}
  },
  _storeOcrResult(path, payload){
    if (!path) return;
    const record = {
      text: typeof payload?.text === 'string' ? payload.text : '',
      lang: payload?.lang || 'eng',
      generated_at: payload?.generated_at || new Date().toISOString(),
      cached: !!payload?.cached,
    };
    this._ocrCache.set(path, record);
    if (this._ocrCache.size > 200){
      try{
        const firstKey = this._ocrCache.keys().next?.().value;
        if (typeof firstKey === 'string') this._ocrCache.delete(firstKey);
      }catch{}
    }
    this._updateAltForPath(path);
  },
  copyMarkdown(path, alt){
    try{
      const altText = this._bestAltForPath(path, alt);
      const safeAlt = String(altText || '').replace(/[\[\]]/g, ' ');
      const md = `![${safeAlt}](${path})`;
      navigator.clipboard.writeText(md);
      this.toast('Markdown copied');
    }catch{ this.toast('Copy failed'); }
  },
  async appendMarkdownToNotes(proj, relPath){
    try{
      const get = await this.invoke('projects_file_get', { proj, path: 'NOTES.md', port: this.getPortFromInput('port') });
      const prev = get && get.sha256 || null;
      const existing = (get && get.content) || '';
      const line = `\n![screenshot](${relPath})\n`;
      const content = existing + line;
      await this.invoke('projects_file_set', { proj, path: 'NOTES.md', content, prev_sha256: prev, port: this.getPortFromInput('port') });
      this.toast('Appended to NOTES.md');
    }catch(e){ console.error(e); this.toast('Append failed'); }
  },
  async maybeAppendToNotes(proj, relPath){
    try{ const prefs = await this.getPrefs('launcher') || {}; if (prefs.appendToNotes){ await this.appendMarkdownToNotes(proj, relPath); } }catch{}
  },
  async setPrefs(ns, value) {
    // Update cache immediately
    try{
      if (value && typeof value === 'object') this._prefsCache.set(ns, { ...value }); else this._prefsCache.set(ns, value);
    }catch{}
    // Debounce disk write (250ms per-namespace)
    const key = ns || 'launcher';
    if (this._prefsTimers.has(key)) clearTimeout(this._prefsTimers.get(key));
    const timer = setTimeout(async () => {
      try{ const val = this._prefsCache.get(key) || {}; await this.invoke('set_prefs', { namespace: key, value: val }); }catch{}
      finally{ this._prefsTimers.delete(key); }
    }, 250);
    this._prefsTimers.set(key, timer);
    return Promise.resolve();
  },
  normalizeBase(base) {
    const raw = (base ?? '').toString().trim();
    if (!raw) return '';
    const strip = (val) => val.replace(/\/+$/, '');
    const parse = (input) => {
      const url = new URL(input);
      if (!url || url.origin === 'null') return strip(input.toLowerCase());
      return strip(url.origin.toLowerCase());
    };
    const attempts = [raw, `http://${raw}`];
    for (const attempt of attempts) {
      try {
        return parse(attempt);
      } catch {}
    }
    const lowered = strip(raw.toLowerCase());
    if (!lowered) return '';
    return /^[a-zA-Z][a-zA-Z0-9+\-.]*:\/\//.test(lowered) ? lowered : `http://${lowered}`;
  },
  baseMeta(port) {
    const override = this.baseOverride();
    if (override) {
      const info = {
        base: override,
        origin: override,
        override: true,
        protocol: null,
        host: override,
        port: null,
      };
      const parseUrl = (value) => {
        if (typeof URL === 'function') {
          try { return new URL(value); }
          catch {}
        }
        return null;
      };
      let url = parseUrl(override);
      if (!url && !override.endsWith('/')) {
        url = parseUrl(`${override}/`);
      }
      if (url) {
        info.origin = url.origin || info.origin;
        info.protocol = url.protocol ? url.protocol.replace(/:$/, '') : info.protocol;
        info.host = url.host || info.host;
        if (url.port) {
          const parsedPort = Number(url.port);
          info.port = Number.isFinite(parsedPort) ? parsedPort : null;
        } else if (url.protocol === 'https:') {
          info.port = 443;
        } else if (url.protocol === 'http:') {
          info.port = 80;
        }
      } else {
        const match = override.match(/^(https?):\/\/([^\/#?]+)/i);
        if (match) {
          info.protocol = match[1].toLowerCase();
          info.host = match[2].toLowerCase();
          info.origin = `${info.protocol}://${info.host}`;
          const portMatch = info.host.match(/:(\d+)$/);
          if (portMatch) {
            const parsedPort = Number(portMatch[1]);
            if (Number.isFinite(parsedPort)) info.port = parsedPort;
          } else if (info.protocol === 'https') {
            info.port = 443;
          } else if (info.protocol === 'http') {
            info.port = 80;
          }
        }
      }
      if (!info.origin) info.origin = info.base;
      return info;
    }
    const resolved = Number.isFinite(port) && port > 0 ? Number(port) : 8091;
    const baseUrl = `http://127.0.0.1:${resolved}`;
    return {
      base: baseUrl,
      origin: baseUrl,
      override: false,
      protocol: 'http',
      host: `127.0.0.1:${resolved}`,
      port: resolved,
    };
  },
  baseOverride() {
    try {
      const override = typeof window.__ARW_BASE_OVERRIDE === 'string'
        ? window.__ARW_BASE_OVERRIDE.trim()
        : '';
      if (override) return this.normalizeBase(override);
    } catch {
    }
    try {
      const stored = typeof localStorage !== 'undefined'
        ? (localStorage.getItem(this._BASE_OVERRIDE_KEY) || '').trim()
        : '';
      if (stored) return this.normalizeBase(stored);
    } catch {}
    return '';
  },
  baseOverridePort() {
    const override = this.baseOverride();
    if (!override) return null;
    const parsed = (() => {
      try { return new URL(override); }
      catch {
        if (!/^[a-zA-Z][a-zA-Z0-9+\-.]*:\/\//.test(override)) {
          try { return new URL(`http://${override}`); }
          catch { return null; }
        }
        return null;
      }
    })();
    if (!parsed) return null;
    if (parsed.port) {
      const asNum = Number(parsed.port);
      return Number.isFinite(asNum) ? asNum : null;
    }
    if (parsed.protocol === 'https:') return 443;
    if (parsed.protocol === 'http:') return 80;
    return null;
  },
  applyBaseMeta({ portInputId, badgeId, label = 'Base' } = {}) {
    const portInput = portInputId ? document.getElementById(portInputId) : null;
    const currentPort = portInput ? parseInt(portInput.value, 10) : null;
    const meta = this.baseMeta(currentPort);
    if (portInput) {
      if (meta.override) {
        if (meta.port != null) portInput.value = String(meta.port);
        portInput.disabled = true;
        portInput.setAttribute('aria-disabled', 'true');
        portInput.title = 'Port pinned by saved connection base';
      } else {
        portInput.disabled = false;
        portInput.removeAttribute('aria-disabled');
        portInput.removeAttribute('title');
      }
    }
    if (badgeId) {
      const badge = document.getElementById(badgeId);
      if (badge) {
        const text = `${label}: ${meta.origin || meta.base}`;
        badge.textContent = text;
        badge.setAttribute('data-override', meta.override ? 'true' : 'false');
        badge.setAttribute('title', text);
      }
    }
    return meta;
  },
  base(port) {
    const override = this.baseOverride();
    if (override) return override;
    const p = Number.isFinite(port) && port > 0 ? port : 8091
    return `http://127.0.0.1:${p}`
  },
  toolPort() {
    const meta = this.baseMeta(this.getPortFromInput('port'));
    return meta.port || 8091;
  },
  _BASE_OVERRIDE_KEY: 'arw:base:override',
  setBaseOverride(base) {
    const normalized = this.normalizeBase(base || '');
    if (!normalized) {
      this.clearBaseOverride();
      return '';
    }
    try { localStorage.setItem(this._BASE_OVERRIDE_KEY, normalized); } catch {}
    try { window.__ARW_BASE_OVERRIDE = normalized; } catch {}
    this._emitBaseOverride(normalized);
    return normalized;
  },
  clearBaseOverride() {
    try { localStorage.removeItem(this._BASE_OVERRIDE_KEY); } catch {}
    try { delete window.__ARW_BASE_OVERRIDE; } catch {}
    this._emitBaseOverride('');
    return '';
  },
  _emitBaseOverride(base) {
    try { window.dispatchEvent(new CustomEvent('arw:base-override-changed', { detail: { base } })); } catch {}
  },
  // Theme override (Auto/Light/Dark) — OS-first when 'auto'
  theme: {
    KEY: 'arw:theme',
    // light/dark neutrals (align to tokens)
    L: { surface:'#ffffff', surfaceMuted:'#fafaf9', ink:'#111827', line:'#e5e7eb' },
    D: { surface:'#0f1115', surfaceMuted:'#0b0d11', ink:'#e5e7eb', line:'#1f232a' },
    apply(val){
      try{
        const root = document.documentElement;
        const body = document.body;
        body?.classList.remove('theme-light','theme-dark');
        // Clear inline overrides first
        const clear = ()=>{
          root.style.removeProperty('--surface');
          root.style.removeProperty('--surface-muted');
          root.style.removeProperty('--color-ink');
          root.style.removeProperty('--color-line');
        };
        if (val === 'light'){
          const v = this.L; body?.classList.add('theme-light');
          root.style.setProperty('--surface', v.surface);
          root.style.setProperty('--surface-muted', v.surfaceMuted);
          root.style.setProperty('--color-ink', v.ink);
          root.style.setProperty('--color-line', v.line);
        } else if (val === 'dark'){
          const v = this.D; body?.classList.add('theme-dark');
          root.style.setProperty('--surface', v.surface);
          root.style.setProperty('--surface-muted', v.surfaceMuted);
          root.style.setProperty('--color-ink', v.ink);
          root.style.setProperty('--color-line', v.line);
        } else { // auto
          clear();
        }
      }catch{}
    },
    set(val){ try{ localStorage.setItem(this.KEY, val); }catch{} this.apply(val); try{ ARW.ui?.badges?.update(); }catch{} ARW.toast('Theme: '+(val||'auto')); },
    init(){ let v='auto'; try{ v = localStorage.getItem(this.KEY)||'auto'; }catch{} this.apply(v); }
  },
  density: {
    KEY: 'arw:density',
    _k(){ return this.KEY + ':' + ARW.util.pageId(); },
    apply(val){ try{ document.body.classList.toggle('compact', val === 'compact'); }catch{} },
    set(val){ try{ localStorage.setItem(this._k(), val); }catch{} this.apply(val); try{ ARW.ui?.badges?.update(); }catch{} ARW.toast('Density: '+(val==='compact'?'compact':'normal')); },
    toggle(){ let v=this.get(); this.set(v==='compact'?'normal':'compact'); },
    get(){ let v='normal'; try{ v = localStorage.getItem(this._k()) || localStorage.getItem(this.KEY) || 'normal'; }catch{} return v; },
    init(){ this.apply(this.get()); }
  },
  layout: {
    KEY: 'arw:focus',
    _k(){ return this.KEY + ':' + ARW.util.pageId(); },
    apply(on){ try{ const root = document.querySelector('.layout'); if (!root) return; root.classList.toggle('full', !!on); }catch{} },
    set(on){ try{ localStorage.setItem(this._k(), on ? '1' : '0'); }catch{} this.apply(!!on); },
    toggle(){ const cur = this.get(); this.set(!cur); ARW.toast('Focus: '+(!cur ? 'on' : 'off')); },
    get(){ let v='0'; try{ v = localStorage.getItem(this._k()) || '0'; }catch{} return v==='1'; },
    init(){ this.apply(this.get()); }
  },
  getPortFromInput(id) {
    const v = parseInt(document.getElementById(id)?.value, 10)
    return Number.isFinite(v) && v > 0 ? v : null
  },
  async applyPortFromPrefs(id, ns = 'launcher') {
    const v = await this.getPrefs(ns)
    if (v && v.port && document.getElementById(id)) document.getElementById(id).value = v.port
  },
  quantReplace(url, q) {
    try {
      if (!url || !/\.gguf$/i.test(url)) return url
      // Replace existing quant token like Q4_K_M, Q5_K_S, Q8_0 etc., else insert before .gguf
      const m = url.match(/(.*?)(Q\d[^/]*?)?(\.gguf)$/i)
      if (!m) return url
      const prefix = m[1]
      const has = !!m[2]
      const ext = m[3]
      if (has) return prefix + q + ext
      // insert with hyphen if the filename part doesn't already end with '-'
      return url.replace(/\.gguf$/i, (prefix.endsWith('-') ? '' : '-') + q + '.gguf')
    } catch { return url }
  },
  // Lightweight SSE store with prefix filters and replay support
  sse: {
    _es: null,
    _subs: new Map(),
    _nextId: 1,
    _lastId: null,
    _connected: false,
    _status: 'idle',
    _last: null,
    _lastRaw: null,
    _lastKind: null,
    _base: null,
    _opts: null,
    _mode: 'eventsource',
    _retryMs: 500,
    _retryTimer: null,
    _closing: false,
    _abortController: null,
    _updateStatus(status, extra){
      this._status = status;
      try{ if (document && document.body) document.body.setAttribute('data-sse-status', status); }catch{}
      const payload = { status, ...(extra||{}) };
      this._emit('*status*', payload);
    },
    _url(baseUrl, opts, afterId){
      const params = new URLSearchParams();
      if (afterId) params.set('after', String(afterId));
      if (!afterId && opts?.replay) params.set('replay', String(opts.replay));
      if (opts?.prefix && Array.isArray(opts.prefix)) {
        for (const p of opts.prefix) params.append('prefix', p);
      } else if (typeof opts?.prefix === 'string' && opts.prefix) {
        params.append('prefix', opts.prefix);
      }
      return baseUrl.replace(/\/$/, '') + '/events' + (params.toString() ? ('?' + params.toString()) : '');
    },
    _clearTimer(){ if (this._retryTimer){ clearTimeout(this._retryTimer); this._retryTimer=null; } },
    _teardownEventSource(){ if (this._es){ try { this._closing = true; this._es.close(); } catch {} this._es = null; this._closing = false; } },
    _teardownFetch(){ if (this._abortController){ try { this._closing = true; this._abortController.abort(); } catch {} } this._abortController = null; this._closing = false; },
    connect(baseUrl, opts = {}, resumeLast = false) {
      this._connectAsync(baseUrl, opts, resumeLast).catch((err)=>{ console.error('SSE connect failed', err); });
    },
    async _connectAsync(baseUrl, opts = {}, resumeLast = false) {
      const prevBase = this._base;
      const baseChanged = typeof prevBase === 'string' && prevBase !== baseUrl;
      this._base = baseUrl;
      this._opts = { ...(opts || {}) };
      if (baseChanged) {
        this._lastId = null;
      }
      this._clearTimer();
      this._teardownEventSource();
      this._teardownFetch();
      const useAfter = resumeLast && !baseChanged && this._lastId;
      const url = this._url(baseUrl, this._opts, useAfter ? this._lastId : null);
      let token = typeof opts.token === 'function' ? null : opts.token;
      if (token === undefined) {
        try { token = await ARW.connections.tokenFor(baseUrl); }
        catch { token = null; }
      }
      if (typeof opts.token === 'function') {
        try { token = await opts.token(); } catch { token = null; }
      }
      if (token) {
        this._mode = 'fetch';
        await this._connectFetch(url, token);
      } else {
        this._mode = 'eventsource';
        this._connectEventSource(url);
      }
    },
    _connectEventSource(url) {
      this._updateStatus('connecting');
      const es = new EventSource(url, { withCredentials: false });
      es.onopen = () => {
        this._connected = true;
        this._retryMs = 500;
        this._emit('*open*', {});
        this._updateStatus('open');
      };
      es.onerror = () => {
        this._connected = false;
        const ms = Math.min(this._retryMs, 5000);
        const closing = this._closing;
        this._emit('*error*', {});
        this._updateStatus(closing ? 'closed' : 'error', closing ? {} : { retryIn: ms });
        if (!closing) {
          this._scheduleReconnect(ms);
        }
      };
      es.onmessage = (ev) => {
        this._lastId = ev.lastEventId || this._lastId;
        let data = null;
        try { data = JSON.parse(ev.data); } catch { data = { raw: ev.data }; }
        const kind = data?.kind || 'unknown';
        this._last = data;
        this._lastRaw = ev.data;
        this._lastKind = kind;
        this._emit(kind, data);
      };
      this._es = es;
      this._wireOnlineReconnect();
    },
    async _connectFetch(url, token){
      this._updateStatus('connecting');
      const controller = new AbortController();
      this._abortController = controller;
      const headers = { 'Accept': 'text/event-stream', 'X-ARW-Admin': token };
      let response = null;
      try {
        response = await fetch(url, { headers, signal: controller.signal, credentials: 'omit' });
      } catch (err) {
        if (controller.signal.aborted) {
          this._updateStatus('closed');
          return;
        }
        this._handleFetchError(err);
        return;
      }
      if (!response || !response.ok || !response.body) {
        this._handleFetchError(new Error('SSE fetch failed'));
        return;
      }
      this._connected = true;
      this._retryMs = 500;
      this._emit('*open*', {});
      this._updateStatus('open');
      const reader = response.body.getReader();
      const decoder = new TextDecoder('utf-8');
      let buffer = '';
      const readLoop = async () => {
        try {
          while (true) {
            const { done, value } = await reader.read();
            if (done) break;
            buffer += decoder.decode(value, { stream: true });
            buffer = this._processBuffer(buffer);
          }
          // drain remainder
          if (buffer) {
            this._processBuffer(buffer + '\n\n');
          }
        } catch (err) {
          if (!controller.signal.aborted) {
            this._handleFetchError(err);
            return;
          }
        }
        if (!controller.signal.aborted) {
          this._handleFetchError(new Error('SSE stream ended'));
        } else {
          this._updateStatus('closed');
        }
      };
      readLoop();
      this._wireOnlineReconnect();
    },
    _processBuffer(buffer){
      let remaining = buffer;
      let idx = remaining.indexOf('\n\n');
      while (idx >= 0) {
        const chunk = remaining.slice(0, idx);
        remaining = remaining.slice(idx + 2);
        this._handleSseChunk(chunk);
        idx = remaining.indexOf('\n\n');
      }
      return remaining;
    },
    _handleSseChunk(chunk){
      const lines = chunk.split('\n');
      let dataLines = [];
      let eventName = null;
      let lastId = null;
      for (const rawLine of lines) {
        const line = rawLine.trimEnd();
        if (!line || line.startsWith(':')) continue;
        if (line.startsWith('data:')) {
          dataLines.push(line.slice(5).trimStart());
        } else if (line.startsWith('event:')) {
          eventName = line.slice(6).trimStart();
        } else if (line.startsWith('id:')) {
          lastId = line.slice(3).trimStart();
        }
      }
      if (lastId) {
        this._lastId = lastId;
      }
      const payloadRaw = dataLines.join('\n');
      if (!payloadRaw) return;
      let data = null;
      try { data = JSON.parse(payloadRaw); }
      catch { data = { raw: payloadRaw }; }
      const kind = eventName || data?.kind || 'unknown';
      this._last = data;
      this._lastRaw = payloadRaw;
      this._lastKind = kind;
      this._emit(kind, data);
    },
    _handleFetchError(err){
      console.warn('SSE fetch error', err?.message || err);
      this._connected = false;
      const ms = Math.min(this._retryMs, 5000);
      const closing = this._closing;
      this._emit('*error*', { error: err });
      this._updateStatus(closing ? 'closed' : 'error', closing ? {} : { retryIn: ms });
      this._abortController = null;
      if (!closing) {
        this._scheduleReconnect(ms);
        this._retryMs = Math.min(ms * 2, 5000);
      }
    },
    _scheduleReconnect(ms){
      this._clearTimer();
      this._retryTimer = setTimeout(() => { try { this.reconnect(); } catch {} }, ms);
    },
    _wireOnlineReconnect(){
      try {
        window.removeEventListener('online', this._onlineOnce);
      } catch {}
      this._onlineOnce = () => { try { this.reconnect(); } catch {} };
      try { window.addEventListener('online', this._onlineOnce, { once: true }); } catch {}
    },
    reconnect(){ if (this._base) this.connect(this._base, this._opts || {}, true); },
    close(){
      this._clearTimer();
      this._teardownEventSource();
      this._teardownFetch();
      this._closing=false;
      this._connected = false;
      this._updateStatus('closed');
    },
    indicator(target, opts = {}){
      const node = typeof target === 'string' ? document.getElementById(target) : target;
      if (!node) return { dispose(){} };
      const self = this;
      try{ if (!node.dataset.indicator) node.dataset.indicator = 'sse'; }catch{}
      const labels = Object.assign({ open:'on', connecting:'connecting', idle:'off', error:'retrying', closed:'off' }, opts.labels || {});
      const prefix = opts.prefix === undefined ? (node.dataset.ssePrefix ?? 'SSE') : opts.prefix;
      const renderOpt = typeof opts.render === 'function' ? opts.render : null;
      const render = (status, info) => {
        try{ node.dataset.state = status; }catch{}
        if (renderOpt) { renderOpt(node, status, info, { labels, prefix }); return; }
        const label = labels[status] ?? labels.default ?? status;
        if (prefix) node.textContent = `${prefix}: ${label}`;
        else node.textContent = label;
      };
      const subId = this.subscribe('*status*', ({ env }) => render(env?.status || 'idle', env));
      render(this.status(), { status: this.status() });
      return { dispose(){ self.unsubscribe(subId); } };
    },
    status(){
      try{ if (document && document.body) document.body.setAttribute('data-sse-status', this._status); }catch{}
      return this._status;
    },
    last(){ return { kind: this._lastKind, data: this._last, raw: this._lastRaw }; },
    subscribe(filter, cb) {
      const id = this._nextId++;
      this._subs.set(id, { filter, cb });
      return id;
    },
    unsubscribe(id) { this._subs.delete(id); },
    _emit(kind, env) {
      for (const { filter, cb } of this._subs.values()) {
        try {
          if (filter === '*' || (typeof filter === 'string' && kind.startsWith(filter)) || (typeof filter === 'function' && filter(kind, env))) {
            cb({ kind, env });
          }
        } catch {}
      }
    }
  },
  // SLO preference helper (p95 threshold)
  async slo(){ try{ const p = await this.getPrefs('launcher')||{}; return Number(p.sloP95)||150; }catch{ return 150 } },
  async setSlo(v){ try{ const p = await this.getPrefs('launcher')||{}; p.sloP95 = Number(v)||150; await this.setPrefs('launcher', p); this.toast('SLO set to '+p.sloP95+' ms'); }catch(e){ console.error(e); } },
  // Minimal sidecar mount helper (lanes placeholder + basic wiring)
  sidecar: {
    mount(el, lanes = ["timeline","metrics","models"], opts = {}) {
      const node = (typeof el === 'string') ? document.getElementById(el) : el;
      if (!node) return { dispose(){} };
      node.classList.add('arw-sidecar');
      node.innerHTML = '';
      const sections = [];
      for (const name of lanes) {
        const sec = document.createElement('section');
        sec.dataset.lane = name;
        const h = document.createElement('h3');
        h.textContent = name;
        h.addEventListener('click', ()=> sec.classList.toggle('collapsed'));
        const body = document.createElement('div');
        body.className = 'lane-body';
        sec.append(h, body);
        node.appendChild(sec);
        sections.push([name, body]);
      }
      const bodyFor = (lane) => sections.find(([n]) => n === lane)?.[1] || null;
      let approvalsSub = null;
      if (lanes.includes('approvals')) {
        const approvalsState = {
          error: null,
          detail: null,
          loading: false,
          reviewer: null,
          reviewerLoaded: false,
          filter: '',
          filterMode: 'text',
          filterCaret: null,
          staleThresholdMs: 60 * 60 * 1000,
          lanePrefsLoaded: false,
          shortcutHandler: null,
          shortcutMap: {},
          sortMode: 'newest',
        };
        const fmtRelative = (iso) => {
          if (!iso) return '';
          const dt = new Date(iso);
          if (Number.isNaN(dt.getTime())) return '';
          const diffMs = Date.now() - dt.getTime();
          const absSec = Math.round(Math.abs(diffMs) / 1000);
          const units = [
            { limit: 60, div: 1, label: 's' },
            { limit: 3600, div: 60, label: 'm' },
            { limit: 86400, div: 3600, label: 'h' },
            { limit: 2592000, div: 86400, label: 'd' },
            { limit: 31536000, div: 2592000, label: 'mo' },
          ];
          for (const unit of units) {
            if (absSec < unit.limit) {
              const value = Math.max(1, Math.floor(absSec / unit.div));
              return diffMs >= 0 ? `${value}${unit.label} ago` : `in ${value}${unit.label}`;
            }
          }
          const years = Math.max(1, Math.floor(absSec / 31536000));
          return diffMs >= 0 ? `${years}y ago` : `in ${years}y`;
        };
        const formatJson = (value, maxLen = 2000) => {
          try {
            let text = JSON.stringify(value ?? {}, null, 2);
            if (text === '{}' || text === '[]') {
              text = JSON.stringify(value);
            }
            if (typeof text !== 'string') {
              text = String(value ?? '');
            }
            if (text.length > maxLen) {
              return `${text.slice(0, maxLen - 1)}…`;
            }
            return text;
          } catch {
            const str = typeof value === 'string' ? value : String(value ?? '');
            return str.length > maxLen ? `${str.slice(0, maxLen - 1)}…` : str;
          }
        };
        const setReviewerPref = async (name) => {
          try {
            const prefs = (await ARW.getPrefs('launcher')) || {};
            if (name) {
              prefs.approvalsReviewer = name;
            } else {
              delete prefs.approvalsReviewer;
            }
            await ARW.setPrefs('launcher', prefs);
          } catch {}
        };
        const setFilterPref = async (value) => {
          try {
            const prefs = (await ARW.getPrefs('launcher')) || {};
            if (value) {
              prefs.approvalsFilter = value;
            } else {
              delete prefs.approvalsFilter;
            }
            await ARW.setPrefs('launcher', prefs);
          } catch {}
        };
        const setStalePref = async (ms) => {
          try {
            const prefs = (await ARW.getPrefs('launcher')) || {};
            prefs.approvalsStaleMs = ms;
            await ARW.setPrefs('launcher', prefs);
          } catch {}
        };
        const setFilterModePref = async (mode) => {
          try {
            const prefs = (await ARW.getPrefs('launcher')) || {};
            prefs.approvalsFilterMode = mode;
            await ARW.setPrefs('launcher', prefs);
          } catch {}
        };
        const setSortPref = async (mode) => {
          try {
            const prefs = (await ARW.getPrefs('launcher')) || {};
            prefs.approvalsSortMode = mode;
            await ARW.setPrefs('launcher', prefs);
          } catch {}
        };
        const promptReviewer = async () => {
          const current = approvalsState.reviewer || '';
          const input = window.prompt('Reviewer (stored for audit trail):', current);
          if (input === null) {
            return approvalsState.reviewer;
          }
          const trimmed = input.trim();
          if (!trimmed) {
            approvalsState.reviewer = null;
            await setReviewerPref(null);
            return null;
          }
          approvalsState.reviewer = trimmed;
          await setReviewerPref(trimmed);
          return trimmed;
        };
        const ensureReviewer = async () => {
          if (approvalsState.reviewer) {
            return approvalsState.reviewer;
          }
          return await promptReviewer();
        };
        const parseIso = (maybeIso) => {
          if (!maybeIso) return null;
          const ts = Date.parse(maybeIso);
          return Number.isFinite(ts) ? ts : null;
        };
        const ageMs = (item) => {
          const ts = parseIso(item?.created) ?? parseIso(item?.updated);
          if (ts == null) return null;
          return Date.now() - ts;
        };
        const formatAge = (ms) => {
          if (!Number.isFinite(ms) || ms < 0) return '';
          const min = Math.round(ms / 60000);
          if (min < 1) return '<1m';
          if (min < 60) return `${min}m`;
          const hr = Math.floor(min / 60);
          const rem = min % 60;
          if (hr < 24) {
            return rem ? `${hr}h ${rem}m` : `${hr}h`;
          }
          const days = Math.floor(hr / 24);
          const hRem = hr % 24;
          return hRem ? `${days}d ${hRem}h` : `${days}d`;
        };
        const makePill = (label, value, { mono = false } = {}) => {
          if (value === null || value === undefined || value === '') return null;
          const pill = document.createElement('span');
          pill.className = 'pill';
          const tag = document.createElement('span');
          tag.className = 'tag';
          tag.textContent = label;
          const val = document.createElement('span');
          if (mono) val.classList.add('mono');
          val.textContent = String(value);
          pill.append(tag, val);
          return pill;
        };
        const makeJsonBlock = (label, value) => {
          const wrap = document.createElement('div');
          wrap.className = 'approval-evidence-block';
          const head = document.createElement('div');
          head.className = 'approval-evidence-head';
          const title = document.createElement('span');
          title.className = 'approval-evidence-title';
          title.textContent = label;
          head.appendChild(title);
          const copyBtn = document.createElement('button');
          copyBtn.type = 'button';
          copyBtn.className = 'ghost btn-small';
          copyBtn.textContent = 'Copy';
          copyBtn.addEventListener('click', (ev) => {
            ev.preventDefault();
            ev.stopPropagation();
            try {
              ARW.copy(JSON.stringify(value ?? {}, null, 2));
            } catch {
              ARW.toast('Copy failed');
            }
          });
          head.appendChild(copyBtn);
          wrap.appendChild(head);
          const pre = document.createElement('pre');
          pre.className = 'approval-evidence-json mono';
          pre.textContent = formatJson(value);
          wrap.appendChild(pre);
          return wrap;
        };
        const appendReviewerRow = (parent) => {
          const wrap = document.createElement('div');
          wrap.className = 'approval-reviewer';
          const label = document.createElement('span');
          label.className = 'dim';
          label.textContent = approvalsState.reviewer
            ? `Reviewer: ${approvalsState.reviewer}`
            : 'Reviewer not set';
          const btn = document.createElement('button');
          btn.type = 'button';
          btn.className = 'ghost btn-small';
          btn.textContent = approvalsState.reviewer ? 'Change reviewer' : 'Set reviewer';
          btn.addEventListener('click', async (ev) => {
            ev.preventDefault();
            ev.stopPropagation();
            const prev = approvalsState.reviewer;
            const updated = await promptReviewer();
            if (updated === prev) {
              return;
            }
            if (updated) {
              ARW.toast(`Reviewer set to ${updated}`);
            } else {
              ARW.toast('Reviewer cleared');
            }
            renderApprovals();
          });
          wrap.append(label, btn);
          parent.appendChild(wrap);
        };
        const createApprovalCard = (item = {}, autoOpen = false) => {
          const card = document.createElement('article');
          card.className = 'approval-card';
          const itemAge = ageMs(item);
          if (Number.isFinite(itemAge) && itemAge >= approvalsState.staleThresholdMs) {
            card.classList.add('stale');
          }
          const head = document.createElement('div');
          head.className = 'approval-head';
          const kindPill = makePill('Kind', item.action_kind || 'unknown', { mono: true });
          if (kindPill) head.appendChild(kindPill);
          const projPill = makePill('Project', item.project);
          if (projPill) head.appendChild(projPill);
          const reqPill = makePill('By', item.requested_by);
          if (reqPill) head.appendChild(reqPill);
          if (Number.isFinite(itemAge)) {
            const agePill = makePill('Age', formatAge(itemAge), { mono: true });
            if (agePill) head.appendChild(agePill);
          }
          if (head.childElementCount) card.appendChild(head);
          const meta = document.createElement('div');
          meta.className = 'approval-meta';
          if (item.created) {
            const timeEl = document.createElement('time');
            timeEl.dateTime = item.created;
            timeEl.title = new Date(item.created).toLocaleString();
            timeEl.textContent = fmtRelative(item.created) || item.created;
            meta.appendChild(timeEl);
          }
          if (item.status && item.status !== 'pending') {
            const statusSpan = document.createElement('span');
            statusSpan.textContent = item.status;
            meta.appendChild(statusSpan);
          }
          if (item.project && !projPill) {
            const projectSpan = document.createElement('span');
            projectSpan.textContent = item.project;
            meta.appendChild(projectSpan);
          }
          if (meta.childElementCount) card.appendChild(meta);
          const details = document.createElement('details');
          details.className = 'approval-details';
          if (autoOpen) details.open = true;
          const summary = document.createElement('summary');
          summary.textContent = 'Review details';
          details.appendChild(summary);
          const body = document.createElement('div');
          body.className = 'approval-evidence';
          body.appendChild(makeJsonBlock('Action input', item.action_input ?? {}));
          const hasEvidence =
            item.evidence &&
            ((typeof item.evidence === 'object' && Object.keys(item.evidence).length > 0) ||
              typeof item.evidence === 'string');
          if (hasEvidence) {
            body.appendChild(makeJsonBlock('Evidence', item.evidence));
          } else {
            const none = document.createElement('div');
            none.className = 'dim';
            none.textContent = 'No evidence provided';
            body.appendChild(none);
          }
          details.appendChild(body);
          card.appendChild(details);

          const addDecisionButtons = () => {
            if (!opts.base || !item.id) return;
            const actionsRow = document.createElement('div');
            actionsRow.className = 'row approval-actions';

            const runDecision = async (verb, payload = {}) => {
              approvalsState.loading = true;
              renderApprovals();
              const bodyPayload = { ...payload };
              if (approvalsState.reviewer && !bodyPayload.decided_by) {
                bodyPayload.decided_by = approvalsState.reviewer;
              }
              try {
                const path = `/staging/actions/${encodeURIComponent(item.id)}/${verb}`;
                const hasBody = Object.keys(bodyPayload).length > 0;
                const fetchOpts = { method: 'POST' };
                if (hasBody) {
                  fetchOpts.headers = { 'Content-Type': 'application/json' };
                  fetchOpts.body = JSON.stringify(bodyPayload);
                }
                const resp = await ARW.http.fetch(opts.base, path, fetchOpts);
                if (!resp.ok) {
                  throw new Error(`HTTP ${resp.status}`);
                }
                const toastMsg = verb === 'approve' ? 'Action approved' : 'Action denied';
                ARW.toast(toastMsg);
              } catch (err) {
                console.error('decision failed', err);
                ARW.toast('Decision failed');
              } finally {
                approvalsState.loading = false;
              }
              await primeApprovals();
            };

            const approveBtn = document.createElement('button');
            approveBtn.type = 'button';
            approveBtn.className = 'primary btn-small';
            approveBtn.textContent = 'Approve';
            approveBtn.addEventListener('click', async (ev) => {
              ev.preventDefault();
              ev.stopPropagation();
              const confirmMsg = `Approve ${item.action_kind || 'action'}${item.project ? ` in ${item.project}` : ''}?`;
              if (!window.confirm(confirmMsg)) return;
              const reviewer = approvalsState.reviewer || await ensureReviewer();
              if (!reviewer) {
                ARW.toast('Reviewer required');
                return;
              }
              await runDecision('approve', { decided_by: reviewer });
            });

            const denyBtn = document.createElement('button');
            denyBtn.type = 'button';
            denyBtn.className = 'ghost btn-small';
            denyBtn.textContent = 'Deny';
            denyBtn.addEventListener('click', async (ev) => {
              ev.preventDefault();
              ev.stopPropagation();
              const reason = window.prompt('Enter a reason (optional):');
              if (reason === null) return;
              const reviewer = approvalsState.reviewer || await ensureReviewer();
              if (!reviewer) {
                ARW.toast('Reviewer required');
                return;
              }
              const trimmedReason = reason.trim();
              const payload = { decided_by: reviewer };
              if (trimmedReason) payload.reason = trimmedReason;
              await runDecision('deny', payload);
            });

            actionsRow.append(approveBtn, denyBtn);
            card.appendChild(actionsRow);
          };

          addDecisionButtons();
          return card;
        };
        const renderApprovals = (restoreFilterFocus = false) => {
          const el = bodyFor('approvals');
          if (!el) return;
          el.innerHTML = '';
          if (approvalsState.error) {
            const msg = document.createElement('div');
            msg.className = 'dim';
            msg.textContent = approvalsState.error;
            if (approvalsState.detail) msg.title = approvalsState.detail;
            el.appendChild(msg);
            return;
          }
          const model = ARW.read.get('staging_actions');
          if (approvalsState.loading && !model) {
            const msg = document.createElement('div');
            msg.className = 'dim';
            msg.textContent = 'Loading approvals…';
            el.appendChild(msg);
            return;
          }
          if (!model) {
            const msg = document.createElement('div');
            msg.className = 'dim';
            msg.textContent = 'Waiting for approvals data';
            el.appendChild(msg);
            return;
          }
          const pending = Array.isArray(model.pending) ? model.pending : [];
          const recent = Array.isArray(model.recent) ? model.recent : [];
          const filterMode = approvalsState.filterMode || 'text';
          const filterNeedle = filterMode === 'text' ? (approvalsState.filter || '').trim().toLowerCase() : '';
          const matchesFilter = (item) => {
            if (filterMode === 'stale') {
              const age = ageMs(item);
              return Number.isFinite(age) && age >= approvalsState.staleThresholdMs;
            }
            if (!filterNeedle) return true;
            const haystackParts = [
              item?.action_kind,
              item?.project,
              item?.requested_by,
              item?.id,
            ];
            try {
              if (item?.action_input) {
                haystackParts.push(JSON.stringify(item.action_input));
              }
            } catch {}
            return haystackParts
              .filter(Boolean)
              .some((part) =>
                String(part)
                  .toLowerCase()
                  .includes(filterNeedle),
              );
          };
          const applyFilterChip = (mode, value, caret) => {
            if (mode === 'stale') {
              if (approvalsState.filterMode === 'stale') return;
              approvalsState.filterMode = 'stale';
              approvalsState.filter = '';
              approvalsState.filterCaret = null;
              setFilterModePref('stale');
              setFilterPref('');
              window.requestAnimationFrame(() => renderApprovals(true));
              return;
            }
            const next = value || '';
            const caretPos = caret ?? next.length;
            if (
              approvalsState.filterMode === 'text' &&
              approvalsState.filter === next &&
              approvalsState.filterCaret === caretPos
            ) {
              return;
            }
            approvalsState.filterMode = 'text';
            approvalsState.filter = next;
            approvalsState.filterCaret = caretPos;
            setFilterModePref('text');
            setFilterPref(next.trim());
            window.requestAnimationFrame(() => renderApprovals(true));
          };

          const filtered =
            filterMode === 'text' && filterNeedle
              ? pending.filter(matchesFilter)
              : filterMode === 'stale'
              ? pending.filter(matchesFilter)
              : pending;
          const sorted = filtered.slice();
          if (sortMode === 'oldest') {
            sorted.sort((a, b) => {
              const ageA = ageMs(a) ?? -Infinity;
              const ageB = ageMs(b) ?? -Infinity;
              return ageB - ageA;
            });
          } else if (sortMode === 'project') {
            sorted.sort((a, b) => {
              const projA = (a?.project || 'unassigned').toLowerCase();
              const projB = (b?.project || 'unassigned').toLowerCase();
              if (projA !== projB) return projA.localeCompare(projB);
              const ageA = ageMs(a) ?? -Infinity;
              const ageB = ageMs(b) ?? -Infinity;
              return ageB - ageA;
            });
          }
          const summary = document.createElement('div');
          summary.className = 'approval-summary';
          const count = document.createElement('strong');
          if (!pending.length) {
            count.textContent = 'No approvals waiting';
          } else if (filterMode === 'stale') {
            count.textContent = `${sorted.length}/${pending.length} stale (≥ ${formatAge(
              approvalsState.staleThresholdMs,
            )})`;
          } else if (filterNeedle) {
            count.textContent = `${sorted.length}/${pending.length} pending`;
          } else {
            count.textContent = `${pending.length} pending`;
          }
          summary.appendChild(count);
          if (model.generated) {
            const timeEl = document.createElement('time');
            timeEl.dateTime = model.generated;
            timeEl.title = new Date(model.generated).toLocaleString();
            timeEl.textContent = fmtRelative(model.generated) || model.generated;
            summary.appendChild(timeEl);
          }
          let oldestTs = null;
          if (pending.length) {
            oldestTs = pending.reduce((acc, item) => {
              const ts = item?.created || item?.updated || null;
              if (!ts) return acc;
              return !acc || new Date(ts).getTime() < new Date(acc).getTime() ? ts : acc;
            }, oldestTs);
            if (oldestTs) {
              const span = document.createElement('span');
              span.className = 'dim';
              span.textContent = `oldest ${fmtRelative(oldestTs) || oldestTs}`;
              summary.appendChild(span);
            }
          }
          el.appendChild(summary);
          const filterRow = document.createElement('div');
          filterRow.className = 'approval-filter row';
          const filterLabel = document.createElement('span');
          filterLabel.className = 'dim';
          filterLabel.textContent = 'Filter';
          const filterInput = document.createElement('input');
          filterInput.type = 'search';
          filterInput.placeholder = 'project, action, reviewer…';
          filterInput.dataset.approvalsFilter = '1';
          filterInput.value = filterMode === 'text' ? approvalsState.filter || '' : '';
          filterInput.addEventListener('input', (ev) => {
            const caret = ev.target.selectionStart ?? ev.target.value.length;
            applyFilterChip('text', ev.target.value, caret);
          });
          filterRow.append(filterLabel, filterInput);
          el.appendChild(filterRow);
          const staleRow = document.createElement('div');
          staleRow.className = 'approval-stale row';
          const staleLabel = document.createElement('span');
          staleLabel.className = 'dim';
          staleLabel.textContent = 'Highlight ≥';
          const staleSelect = document.createElement('select');
          const staleOptions = [
            { label: '15m', value: 15 * 60 * 1000 },
            { label: '30m', value: 30 * 60 * 1000 },
            { label: '1h', value: 60 * 60 * 1000 },
            { label: '4h', value: 4 * 60 * 60 * 1000 },
            { label: '1d', value: 24 * 60 * 60 * 1000 },
          ];
          staleOptions.forEach((opt) => {
            const option = document.createElement('option');
            option.value = String(opt.value);
            option.textContent = opt.label;
            if (opt.value === approvalsState.staleThresholdMs) option.selected = true;
            staleSelect.appendChild(option);
          });
          staleSelect.addEventListener('change', (ev) => {
            const next = parseInt(ev.target.value, 10);
            if (!Number.isFinite(next) || next <= 0) return;
            approvalsState.staleThresholdMs = next;
            window.requestAnimationFrame(() => renderApprovals());
            (async () => setStalePref(next))();
          });
          staleRow.append(staleLabel, staleSelect);
          el.appendChild(staleRow);
          const sortRow = document.createElement('div');
          sortRow.className = 'approval-sort row';
          const sortLabel = document.createElement('span');
          sortLabel.className = 'dim';
          sortLabel.textContent = 'Sort';
          const sortSelect = document.createElement('select');
          const sortOptions = [
            { label: 'Newest first', value: 'newest' },
            { label: 'Oldest first', value: 'oldest' },
            { label: 'Project', value: 'project' },
          ];
          sortOptions.forEach((opt) => {
            const option = document.createElement('option');
            option.value = opt.value;
            option.textContent = opt.label;
            if (opt.value === sortMode) option.selected = true;
            sortSelect.appendChild(option);
          });
          sortSelect.addEventListener('change', (ev) => {
            const next = String(ev.target.value || 'newest');
            if (next === approvalsState.sortMode) return;
            approvalsState.sortMode = next;
            setSortPref(next);
            window.requestAnimationFrame(() => renderApprovals(true));
          });
          sortRow.append(sortLabel, sortSelect);
          el.appendChild(sortRow);
          const chips = [];
          chips.push({ label: 'Clear', value: '', mode: 'text' });
          chips.push({ label: 'Stale only', value: '', mode: 'stale' });
          if (approvalsState.reviewer) {
            chips.push({
              label: `Mine (${approvalsState.reviewer})`,
              value: approvalsState.reviewer,
              mode: 'text',
            });
          }
          const projectSeen = new Set();
          for (const item of pending) {
            const proj = (item?.project || '').trim();
            if (!proj || projectSeen.has(proj)) continue;
            projectSeen.add(proj);
            chips.push({ label: `Project: ${proj}`, value: proj, mode: 'text' });
            if (projectSeen.size >= 3) break;
          }
          const shortcutKeys = ['1', '2', '3', '4', '5'];
          let shortcutIndex = 0;
          chips.forEach((chip) => {
            if (shortcutIndex < shortcutKeys.length) {
              chip.shortcut = shortcutKeys[shortcutIndex++];
            }
          });
          approvalsState.shortcutMap = {};
          const quickWrap = document.createElement('div');
          quickWrap.className = 'approval-filter-chips row';
          const makeChip = (chip) => {
            const { label, value, mode, shortcut } = chip;
            const btn = document.createElement('button');
            btn.type = 'button';
            btn.className = 'ghost btn-small';
            const isActive =
              mode === 'stale'
                ? filterMode === 'stale'
                : filterMode === 'text' && (approvalsState.filter || '') === (value || '');
            if (isActive) {
              btn.classList.add('active');
            }
            btn.dataset.mode = mode;
            btn.textContent = label;
            btn.addEventListener('click', (ev) => {
              ev.preventDefault();
              if (mode === 'stale') {
                applyFilterChip('stale', '');
              } else {
                applyFilterChip('text', value || '');
              }
            });
            if (shortcut) {
              btn.dataset.shortcut = shortcut;
              btn.title = `${label} (Alt+${shortcut})`;
              approvalsState.shortcutMap[shortcut] = chip;
            } else {
              btn.title = label;
            }
            return btn;
          };
          chips.forEach((chip) => quickWrap.appendChild(makeChip(chip)));
          if (quickWrap.childElementCount) {
            el.appendChild(quickWrap);
          }
          if (!approvalsState.shortcutHandler) {
            approvalsState.shortcutHandler = (ev) => {
              if (!ev.altKey || ev.ctrlKey || ev.metaKey || ev.shiftKey) return;
              const key = (ev.key || '').toLowerCase();
              if (!key) return;
              const chip = approvalsState.shortcutMap?.[key];
              if (!chip) return;
              const node = bodyFor('approvals');
              if (!node || !node.isConnected) return;
              const tag = (ev.target?.tagName || '').toLowerCase();
              if (['input', 'textarea', 'select'].includes(tag)) return;
              ev.preventDefault();
              if (chip.mode === 'stale') {
                applyFilterChip('stale', '');
              } else {
                applyFilterChip('text', chip.value || '');
              }
            };
            window.addEventListener('keydown', approvalsState.shortcutHandler);
          }
          appendReviewerRow(el);
          if (!pending.length) {
            const empty = document.createElement('div');
            empty.className = 'dim';
            empty.textContent = 'Queue is clear.';
            el.appendChild(empty);
            approvalsState.filterCaret = null;
            return;
          }
          if (!sorted.length) {
            const empty = document.createElement('div');
            empty.className = 'dim';
            empty.textContent = 'No approvals match filter.';
            el.appendChild(empty);
          } else {
            const maxItems = 8;
            const frag = document.createDocumentFragment();
            sorted.slice(0, maxItems).forEach((item) => {
              frag.appendChild(createApprovalCard(item, sorted.length <= 2));
            });
            el.appendChild(frag);
            if (sorted.length > maxItems) {
              const more = document.createElement('div');
              more.className = 'dim';
              more.textContent = `+${sorted.length - maxItems} more pending`;
              el.appendChild(more);
            }
          }
          const projectMap = new Map();
          const staleProjectMap = new Map();
          let staleTotal = 0;
          sorted.forEach((item) => {
            const proj = (item?.project || 'unassigned').trim() || 'unassigned';
            projectMap.set(proj, (projectMap.get(proj) || 0) + 1);
            const age = ageMs(item);
            if (Number.isFinite(age) && age >= approvalsState.staleThresholdMs) {
              staleTotal += 1;
              staleProjectMap.set(proj, (staleProjectMap.get(proj) || 0) + 1);
            }
          });
          if (staleTotal > 0) {
            const badge = document.createElement('span');
            badge.className = 'badge warn';
            badge.textContent = `≥${formatAge(approvalsState.staleThresholdMs)}: ${staleTotal}`;
            summary.appendChild(badge);
          }
          if (projectMap.size) {
            const stats = Array.from(projectMap.entries())
              .map(([proj, count]) => ({ proj, count }))
              .sort((a, b) => b.count - a.count || a.proj.localeCompare(b.proj))
              .slice(0, 5);
            const statsWrap = document.createElement('div');
            statsWrap.className = 'approval-project-stats';
            const headingRow = document.createElement('div');
            headingRow.className = 'approval-project-stats-header row';
            const heading = document.createElement('div');
            heading.className = 'dim';
            heading.textContent = 'Projects waiting';
            const copyBtn = document.createElement('button');
            copyBtn.type = 'button';
            copyBtn.className = 'ghost btn-small';
            copyBtn.textContent = 'Copy summary';
            copyBtn.addEventListener('click', (ev) => {
              ev.preventDefault();
              ev.stopPropagation();
              const lines = [];
              lines.push(
                `Approvals pending: ${sorted.length}${
                  filterMode === 'text' && filterNeedle ? ` (filtered from ${pending.length})` : ''
                }`,
              );
              lines.push(`Sort mode: ${sortMode}`);
              if (filterMode === 'stale') {
                lines.push(
                  `Mode: stale (≥ ${formatAge(approvalsState.staleThresholdMs)})`,
                );
              }
              if (oldestTs) {
                const rel = fmtRelative(oldestTs) || oldestTs;
                lines.push(`Oldest pending: ${rel}`);
              }
              if (approvalsState.reviewer) {
                lines.push(`Reviewer: ${approvalsState.reviewer}`);
              }
              if (staleTotal > 0) {
                lines.push(`Stale (≥ ${formatAge(approvalsState.staleThresholdMs)}): ${staleTotal}`);
              }
              const projectSummary = stats
                .map(({ proj, count }) => {
                  const staleCount = staleProjectMap.get(proj) || 0;
                  return staleCount
                    ? `${proj}: ${count} (${staleCount} stale)`
                    : `${proj}: ${count}`;
                })
                .join(', ');
              if (projectSummary) {
                lines.push(`Projects: ${projectSummary}`);
              }
              if (projectMap.size > stats.length) {
                lines.push(`(+${projectMap.size - stats.length} more projects)`);
              }
              const text = lines.join('\n');
              try {
                ARW.copy(text);
                ARW.toast('Summary copied');
              } catch (err) {
                console.error('copy summary failed', err);
                ARW.toast('Copy failed');
              }
            });
            headingRow.append(heading, copyBtn);
            statsWrap.appendChild(headingRow);
            const list = document.createElement('ul');
            stats.forEach(({ proj, count }) => {
              const li = document.createElement('li');
              const staleCount = staleProjectMap.get(proj) || 0;
              li.innerHTML = `<span class="mono">${proj}</span> <span class="badge">${count}</span>${
                staleCount ? ` <span class="badge warn">${staleCount} stale</span>` : ''
              }`;
              list.appendChild(li);
            });
            if (projectMap.size > stats.length) {
              const remaining = projectMap.size - stats.length;
              const li = document.createElement('li');
              li.className = 'dim';
              li.textContent = `+${remaining} more project${remaining === 1 ? '' : 's'}`;
              list.appendChild(li);
            }
            statsWrap.appendChild(list);
            el.appendChild(statsWrap);
          }
          if (recent.length) {
            const details = document.createElement('details');
            details.className = 'approval-recent';
            const sum = document.createElement('summary');
            sum.textContent = 'Recent decisions';
            details.appendChild(sum);
            const list = document.createElement('ul');
            recent.slice(0, 5).forEach((item) => {
              const li = document.createElement('li');
              const label = `${item.decision || item.status || 'updated'} · ${item.action_kind || ''}`.trim();
              const span = document.createElement('span');
              span.textContent = label;
              li.appendChild(span);
              const ts = item.updated || item.decided_at || item.created;
              if (ts) {
                li.appendChild(document.createTextNode(' — '));
                const timeEl = document.createElement('time');
                timeEl.dateTime = ts;
                timeEl.title = new Date(ts).toLocaleString();
                timeEl.textContent = fmtRelative(ts) || ts;
                li.appendChild(timeEl);
              }
              list.appendChild(li);
            });
            details.appendChild(list);
            el.appendChild(details);
          }
          if (restoreFilterFocus) {
            window.requestAnimationFrame(() => {
              const field = bodyFor('approvals')?.querySelector('[data-approvals-filter]');
              if (field instanceof HTMLInputElement) {
                field.focus();
                const caret = approvalsState.filterCaret ?? field.value.length;
                try {
                  field.setSelectionRange(caret, caret);
                } catch {}
              }
            });
          } else {
            approvalsState.filterCaret = null;
          }
        };
        const loadLanePrefs = async () => {
          if (approvalsState.lanePrefsLoaded) return;
          approvalsState.lanePrefsLoaded = true;
          try {
            const prefs = await ARW.getPrefs('launcher');
            if (prefs && typeof prefs.approvalsFilter === 'string') {
              approvalsState.filter = prefs.approvalsFilter;
            }
            if (prefs && typeof prefs.approvalsFilterMode === 'string') {
              approvalsState.filterMode = prefs.approvalsFilterMode === 'stale' ? 'stale' : 'text';
            }
            if (prefs && Number.isFinite(prefs.approvalsStaleMs)) {
              approvalsState.staleThresholdMs = prefs.approvalsStaleMs;
            }
            if (prefs && typeof prefs.approvalsSortMode === 'string') {
              approvalsState.sortMode = ['newest', 'oldest', 'project'].includes(
                prefs.approvalsSortMode,
              )
                ? prefs.approvalsSortMode
                : 'newest';
            }
          } catch {}
        };
        const loadReviewerPref = async () => {
          if (approvalsState.reviewerLoaded) return;
          approvalsState.reviewerLoaded = true;
          try {
            const prefs = await ARW.getPrefs('launcher');
            const saved =
              prefs && typeof prefs.approvalsReviewer === 'string'
                ? prefs.approvalsReviewer.trim()
                : '';
            if (saved) {
              approvalsState.reviewer = saved;
              renderApprovals();
            }
          } catch {}
        };
        const primeApprovals = async () => {
          if (!opts.base) return;
          approvalsState.loading = true;
          renderApprovals();
          try {
            const pendingSnap = await ARW.http.json(opts.base, '/state/staging/actions?status=pending&limit=50');
            let recentSnap = null;
            try {
              recentSnap = await ARW.http.json(opts.base, '/state/staging/actions?limit=30');
            } catch (err) {
              console.warn('approvals recent fetch failed', err);
            }
            const current = ARW.read.get('staging_actions') || {};
            const next = { ...current };
            next.generated = new Date().toISOString();
            next.pending = Array.isArray(pendingSnap?.items) ? pendingSnap.items : [];
            if (recentSnap && Array.isArray(recentSnap.items)) {
              next.recent = recentSnap.items;
            }
            approvalsState.error = null;
            approvalsState.detail = null;
            approvalsState.loading = false;
            ARW.read._store.set('staging_actions', next);
            ARW.read._emit('staging_actions');
          } catch (err) {
            const msg = err?.message || String(err);
            approvalsState.loading = false;
            approvalsState.detail = msg;
            approvalsState.error = /HTTP\s+401/.test(msg)
              ? 'Authorize to view approvals queue'
              : 'Approvals queue unavailable';
            renderApprovals();
          }
        };
        Promise.all([loadLanePrefs(), loadReviewerPref()]).then(() => renderApprovals());
        approvalsSub = ARW.read.subscribe('staging_actions', () => renderApprovals());
        if (!approvalsState.lanePrefsLoaded) {
          renderApprovals();
        }
        if (opts.base) {
          primeApprovals();
        }
      }
      // Micro-batched updaters to reduce DOM thrash
      let tlQ = []; let tlTimer = null;
      const rTimeline = (env) => { if (!env) return; tlQ.push(env); if (tlTimer) return; tlTimer = setTimeout(()=>{
        try{ const el = sections.find(([n])=>n==='timeline')?.[1]; if (!el) return; const frag=document.createDocumentFragment(); const take = tlQ.splice(0, tlQ.length); for (const e of take){ const d=document.createElement('div'); d.className='evt mono'; d.textContent = `${e.kind}: ${safeJson(e.env?.payload)}`.slice(0, 800); frag.prepend ? frag.prepend(d) : frag.appendChild(d); } el.prepend(frag); while (el.childElementCount>100) el.removeChild(el.lastChild); }finally{ tlTimer=null; }
      }, 50); };
      let mdQ = []; let mdTimer = null;
      const rModels = (env) => { if (!(env && (env.kind.startsWith('models.') || env.kind==='state.read.model.patch'))) return; mdQ.push(env); if (mdTimer) return; mdTimer = setTimeout(()=>{
        try{ const el = sections.find(([n])=>n==='models')?.[1]; if (!el) return; const frag=document.createDocumentFragment(); const take = mdQ.splice(0, mdQ.length); for (const e of take){ const d=document.createElement('div'); d.className='evt mono'; d.textContent = `${e.kind}: ${safeJson(e.env?.payload)}`.slice(0, 800); frag.prepend ? frag.prepend(d) : frag.appendChild(d); } el.prepend(frag); while (el.childElementCount>60) el.removeChild(el.lastChild); }finally{ mdTimer=null; }
      }, 50); };
      // Policy lane: poll /state/policy (read-only) if base provided
      let policyTimer = null;
      const rPolicy = async () => {
        const el = sections.find(([n])=>n==='policy')?.[1]; if (!el || !opts.base) return;
        try {
          const j = await ARW.http.json(opts.base, '/state/policy');
          const leases = j?.leases || j?.data?.leases || [];
          el.innerHTML = '';
          if (!Array.isArray(leases) || leases.length===0) { el.innerHTML = '<div class="dim">No active leases</div>'; return; }
          for (const l of leases) {
            const p = document.createElement('div'); p.className='pill';
            const scope = (l.scope||l.key||'').toString();
            const ttl = (l.ttl_ms||l.ttl||'');
            const who = (l.principal||'');
            p.innerHTML = `<span class="tag">${scope}</span><span class="dim">${ttl} ms</span><span class="dim">${who}</span>`;
            el.appendChild(p);
          }
        } catch {}
      };
      if (opts.base) {
        rPolicy(); policyTimer = setInterval(rPolicy, 5000);
      }
      // Context lane: fetch top claims (world.select)
      const rContext = async () => {
        const el = sections.find(([n])=>n==='context')?.[1]; if (!el || !opts.base) return;
        try {
          const j = await ARW.http.json(opts.base, '/state/world/select?k=8');
          const items = j?.items || j?.data?.items || [];
          el.innerHTML = '';
          const ul = document.createElement('ul'); ul.style.paddingLeft='16px'; ul.style.margin='0';
          for (const it of items) {
            const li = document.createElement('li');
            const name = it?.props?.name || it?.id || '';
            li.textContent = `${name}`.slice(0,160);
            ul.appendChild(li);
          }
          el.appendChild(ul);
          if (items.length===0) el.innerHTML = '<div class="dim">No beliefs</div>';
        } catch {}
      };
      if (opts.base) rContext();
      // client-side trend store for p95 sparkline
      ARW.metricsTrend = ARW.metricsTrend || { _m: new Map(), push(route,p){ const a=this._m.get(route)||[]; a.push(Number(p)||0); if(a.length>32)a.shift(); this._m.set(route,a); }, get(route){ return this._m.get(route)||[] } };
      function sparkline(vals){ const v=(vals||[]).slice(-32); if(!v.length) return ''; const w=90,h=18,max=Math.max(1,...v); const pts=v.map((x,i)=>{const xx=Math.round(i*(w-2)/Math.max(1,v.length-1))+1; const yy=h-1-Math.round((x/max)*(h-2)); return `${xx},${yy}`;}).join(' '); return `<svg class="spark" viewBox="0 0 ${w} ${h}" xmlns="http://www.w3.org/2000/svg"><polyline fill="none" stroke="#1bb3a3" stroke-width="1.5" points="${pts}"/></svg>`; }
      const rMetrics = async () => {
        const el = sections.find(([n])=>n==='metrics')?.[1]; if (!el) return;
        const model = ARW.read.get('route_stats') || {};
        const by = model.by_path || {};
        const rows = Object.entries(by)
          .map(([p, s]) => ({ p, hits: s.hits||0, p95: s.p95_ms||0, ewma: s.ewma_ms||0, max: s.max_ms||0 }))
          .sort((a,b)=> b.hits - a.hits)
          .slice(0, 6);
        el.innerHTML = '';
        const tbl = document.createElement('table');
        const slo = await ARW.slo();
        const thead = document.createElement('thead'); thead.innerHTML = `<tr><th>route</th><th>hits</th><th>p95 ≤ ${slo}</th><th>ewma</th><th>max</th><th></th></tr>`;
        tbl.appendChild(thead);
        const tb = document.createElement('tbody');
        for (const r of rows) {
          const tr = document.createElement('tr');
          const p95c = r.p95 <= slo ? 'ok' : '';
          ARW.metricsTrend.push(r.p, r.p95);
          const sp = sparkline(ARW.metricsTrend.get(r.p));
          tr.innerHTML = `<td class="mono">${r.p}</td><td>${r.hits}</td><td class="${p95c}">${r.p95}</td><td>${r.ewma.toFixed ? r.ewma.toFixed(1) : r.ewma}</td><td>${r.max}</td><td>${sp}</td>`;
          tb.appendChild(tr);
        }
        tbl.appendChild(tb);
        el.appendChild(tbl);

        const snappy = ARW.read.get('snappy') || null;
        const snappyBox = document.createElement('div');
        snappyBox.style.marginTop = '12px';
        snappyBox.className = 'snappy-detail';
        if (snappy && snappy.observed) {
          const breach = !!(snappy.breach && snappy.breach.full_result);
          if (breach) {
            snappyBox.style.borderLeft = '4px solid var(--color-warn, #d97706)';
            snappyBox.style.paddingLeft = '8px';
          }
          const budget = snappy?.budgets?.full_result_p95_ms;
          const header = document.createElement('div');
          header.className = 'dim';
          header.textContent = `Snappy budget ≤ ${budget ?? '–'} ms — observed max: ${snappy.observed.max_p95_ms ?? '–'} ms (${snappy.observed.max_path || 'n/a'})`;
          snappyBox.appendChild(header);
          const routes = Object.entries(snappy.observed.routes || {})
            .map(([path, stats]) => ({
              path,
              p95: Number(stats?.p95_ms ?? 0),
              hits: Number(stats?.hits ?? 0),
            }))
            .sort((a, b) => b.p95 - a.p95)
            .slice(0, 4);
          if (routes.length) {
            const tblRoutes = document.createElement('table');
            tblRoutes.innerHTML = '<thead><tr><th>path</th><th>p95</th><th>hits</th></tr></thead>';
            const body = document.createElement('tbody');
            routes.forEach((r) => {
              const tr = document.createElement('tr');
              tr.innerHTML = `<td class="mono">${r.path}</td><td>${r.p95}</td><td>${r.hits}</td>`;
              body.appendChild(tr);
            });
            tblRoutes.appendChild(body);
            snappyBox.appendChild(tblRoutes);
          } else {
            const empty = document.createElement('div');
            empty.className = 'dim';
            empty.textContent = 'Snappy: no protected routes observed yet';
            snappyBox.appendChild(empty);
          }
        } else {
          const wait = document.createElement('div');
          wait.className = 'dim';
          wait.textContent = 'Snappy detail: waiting for data';
          snappyBox.appendChild(wait);
        }
        el.appendChild(snappyBox);
      };
      function safeJson(v){ try { return JSON.stringify(v); } catch { return String(v) } }
      const idAll = ARW.sse.subscribe('*', rTimeline);
      const idModels = ARW.sse.subscribe((k)=> k.startsWith('models.'), rModels);
      const idMetrics = ARW.read.subscribe('route_stats', rMetrics);
      const idSnappy = ARW.read.subscribe('snappy', rMetrics);
      // Activity lane: listen for screenshots.captured and render thumbnails
      const rActivity = ({ env }) => {
        const el = sections.find(([n])=>n==='activity')?.[1]; if (!el) return;
        const p = env?.payload || env;
        const kind = env?.kind || '';
        if (!kind.startsWith('screenshots.')) return;
        if (kind === 'screenshots.ocr.completed') {
          const src = p?.source_path || p?.sourcePath || p?.path;
          ARW._storeOcrResult(src, p);
          return;
        }
        if (kind !== 'screenshots.captured') return;
        const box = document.createElement('div'); box.className='evt';
        const ts = env?.time || new Date().toISOString();
        const img = document.createElement('img');
        img.dataset.screenshotPath = p?.path||'';
        img.alt = ARW._bestAltForPath(p?.path, p?.path||'');
        img.style.maxWidth='100%'; img.style.maxHeight='120px';
        if (p?.preview_b64 && /^data:image\//.test(p.preview_b64)) { img.src = p.preview_b64; }
        else { img.src = ''; img.style.display='none'; }
        const cap = document.createElement('div'); cap.className='dim mono'; cap.textContent = `${ts} ${p?.path||''}`;
      const actions = document.createElement('div'); actions.className='row';
      const openBtn = document.createElement('button'); openBtn.className='ghost'; openBtn.textContent='Open'; openBtn.addEventListener('click', async ()=>{ try{ if (p?.path) await ARW.invoke('open_path', { path: p.path }); }catch(e){ console.error(e); } });
      const copyBtn = document.createElement('button'); copyBtn.className='ghost'; copyBtn.textContent='Copy path'; copyBtn.addEventListener('click', ()=>{ if (p?.path) ARW.copy(String(p.path)); });
        const mdBtn = document.createElement('button'); mdBtn.className='ghost'; mdBtn.textContent='Copy MD'; mdBtn.addEventListener('click', ()=>{ if (p?.path) ARW.copyMarkdown(p.path); });
        const annBtn = document.createElement('button'); annBtn.className='ghost'; annBtn.textContent='Annotate'; annBtn.addEventListener('click', async ()=>{ try{ if (p?.preview_b64){ const rects = await ARW.annot.start(p.preview_b64); const res = await ARW.invoke('run_tool_admin', { id: 'ui.screenshot.annotate_burn', input: { path: p.path, annotate: rects, downscale:640 }, port: ARW.toolPort() }); if (res && res.preview_b64){ img.src = res.preview_b64; cap.textContent = `${ts} ${res.path||''}`; } } else { ARW.toast('No preview for annotate'); } }catch(e){ console.error(e); }});
        const saveBtn = document.createElement('button'); saveBtn.className='ghost'; saveBtn.textContent='Save to project'; saveBtn.addEventListener('click', async ()=>{ if (p?.path){ const res = await ARW.saveToProjectPrompt(p.path); if (res) await ARW.maybeAppendToNotes(res.proj, res.dest); } });
        actions.appendChild(openBtn); actions.appendChild(copyBtn); actions.appendChild(mdBtn); actions.appendChild(annBtn); actions.appendChild(saveBtn);
        box.appendChild(img); box.appendChild(cap); box.appendChild(actions);
        el.prepend(box);
        if (p?.path) ARW._updateAltForPath(p.path);
        while (el.childElementCount>6) el.removeChild(el.lastChild);
      };
      const idActivity = ARW.sse.subscribe((k)=> k.startsWith('screenshots.'), rActivity);
      // initial render for metrics if any
      rMetrics();
      return {
        dispose() {
          ARW.sse.unsubscribe(idAll);
          ARW.sse.unsubscribe(idModels);
          ARW.read.unsubscribe(idMetrics);
          ARW.read.unsubscribe(idSnappy);
          if (approvalsSub) ARW.read.unsubscribe(approvalsSub);
          ARW.sse.unsubscribe(idActivity);
          if (policyTimer) clearInterval(policyTimer);
          if (approvalsState.shortcutHandler) {
            window.removeEventListener('keydown', approvalsState.shortcutHandler);
            approvalsState.shortcutHandler = null;
          }
          approvalsState.shortcutMap = {};
          node.innerHTML = '';
        },
      };
    }
  }
}

// Read‑model store: maintain local snapshots via RFC6902 patches from SSE
// Payload shape from SSE: { id, patch: [ {op, path, value?} ... ] }
window.ARW.read = {
  _store: new Map(),
  _subs: new Map(),
  _next: 1,
  get(id){ return this._store.get(id); },
  subscribe(id, cb){ const k = this._next++; this._subs.set(k, { id, cb }); return k; },
  unsubscribe(k){ this._subs.delete(k); },
  _emit(id){ for (const {id: iid, cb} of this._subs.values()) if (iid===id) { try{ cb(this._store.get(id)); }catch{} } },
  _applyPointer(obj, path){
    // returns [parent, key] for a JSON pointer path, creating objects/arrays as needed for add
    if (path === '/' || path === '') return [ { '': obj }, '' ];
    const parts = path.split('/').slice(1).map(p=> p.replace(/~1/g,'/').replace(/~0/g,'~'));
    let parent = null, key = null, cur = obj;
    for (let i=0;i<parts.length;i++){
      key = parts[i];
      if (i === parts.length - 1) { parent = cur; break; }
      if (Array.isArray(cur)) {
        const idx = key === '-' ? cur.length : parseInt(key, 10);
        if (!Number.isFinite(idx)) return [null, null];
        if (!cur[idx]) cur[idx] = {};
        cur = cur[idx];
      } else if (cur && typeof cur === 'object') {
        if (!(key in cur)) cur[key] = {};
        cur = cur[key];
      } else {
        return [null, null];
      }
    }
    return [parent, key];
  },
  _applyOp(target, op){
    const { op: kind, path } = op;
    if (!path) return;
    if (kind === 'test') return; // ignored for now
    if (kind === 'copy' || kind === 'move') {
      // basic move/copy support
      const from = op.from;
      const [fp, fk] = this._applyPointer(target, from);
      if (!fp) return;
      let val;
      if (Array.isArray(fp)) val = fp[Number(fk)]; else val = fp[fk];
      if (kind === 'move') {
        if (Array.isArray(fp)) fp.splice(Number(fk),1); else delete fp[fk];
      }
      const [tp, tk] = this._applyPointer(target, path);
      if (!tp) return;
      if (Array.isArray(tp)) {
        const idx = tk === '-' ? tp.length : parseInt(tk,10);
        tp.splice(idx, 0, val);
      } else { tp[tk] = val; }
      return;
    }
    const [p, k] = this._applyPointer(target, path);
    if (!p) return;
    if (kind === 'add') {
      if (Array.isArray(p)) {
        const idx = k === '-' ? p.length : parseInt(k,10);
        p.splice(idx, 0, op.value);
      } else { p[k] = op.value; }
    } else if (kind === 'replace') {
      if (Array.isArray(p)) p[parseInt(k,10)] = op.value; else p[k] = op.value;
    } else if (kind === 'remove') {
      if (Array.isArray(p)) p.splice(parseInt(k,10),1); else delete p[k];
    }
  }
};

// Attach SSE patch listener
window.ARW.sse.subscribe('state.read.model.patch', ({ env }) => {
  try {
    const id = env?.id || env?.payload?.id;
    const patch = env?.patch || env?.payload?.patch;
    if (!id || !Array.isArray(patch)) return;
    const cur = ARW.read._store.get(id) || {};
    for (const op of patch) ARW.read._applyOp(cur, op);
    ARW.read._store.set(id, cur);
    ARW.read._emit(id);
  } catch {}
});

// Command Palette (Ctrl/Cmd-K)
  window.ARW.palette = {
  _wrap: null,
  _input: null,
  _list: null,
  _items: [],
  _actions: [],
  _active: -1,
  mount(opts={}){
    if (this._wrap) return; // singleton
    const wrap = document.createElement('div'); wrap.className='palette-wrap';
    const pal = document.createElement('div'); pal.className='palette'; pal.setAttribute('role','dialog'); pal.setAttribute('aria-modal','true'); pal.setAttribute('aria-label','Command palette'); wrap.appendChild(pal);
    const header = document.createElement('header');
    const inp = document.createElement('input'); inp.placeholder = 'Search commands…'; inp.setAttribute('aria-label','Search commands'); header.appendChild(inp);
    pal.appendChild(header);
    const ul = document.createElement('ul'); ul.setAttribute('role','listbox'); pal.appendChild(ul);
    document.body.appendChild(wrap);
    this._wrap = wrap; this._input = inp; this._list = ul;
    const base = opts.base;
    this._actions = [
      { id:'open:hub', label:'Open Project Hub', hint:'window', run:()=> ARW.invoke('open_hub_window') },
      { id:'open:chat', label:'Open Chat', hint:'window', run:()=> ARW.invoke('open_chat_window') },
      { id:'open:training', label:'Open Training Park', hint:'window', run:()=> ARW.invoke('open_training_window') },
      { id:'open:debug', label:'Open Debug (Window)', hint:'window', run:()=> ARW.invoke('open_debug_window', { port: ARW.getPortFromInput('port') }) },
      { id:'open:events', label:'Open Events Window', hint:'window', run:()=> ARW.invoke('open_events_window') },
      { id:'open:docs', label:'Open Docs Website', hint:'web', run:()=> ARW.invoke('open_url', { url: 'https://t3hw00t.github.io/ARW/' }) },
      { id:'models:refresh', label:'Refresh Models', hint:'action', run:()=> ARW.invoke('models_refresh', { port: ARW.getPortFromInput('port') }) },
          { id:'sse:replay', label:'Replay SSE (50)', hint:'sse', run:()=> {
              const meta = ARW.baseMeta(ARW.getPortFromInput('port'));
              ARW.sse.connect(meta.base, { replay: 50 });
            }
          },
      { id:'layout:focus', label:'Toggle Focus Mode', hint:'layout', run:()=> ARW.layout.toggle() },
      { id:'layout:density', label:'Toggle Compact Density', hint:'layout', run:()=> ARW.density.toggle() },
      { id:'copy:last', label:'Copy last event JSON', hint:'sse', run:()=> ARW.copy(JSON.stringify(ARW.sse._last||{}, null, 2)) },
      { id:'toggle:auto-ocr', label:'Toggle Auto OCR', hint:'pref', run: async ()=>{
          try{
            const prefs = await ARW.getPrefs('launcher') || {}; prefs.autoOcr = !prefs.autoOcr; await ARW.setPrefs('launcher', prefs);
            ARW.toast('Auto OCR: ' + (prefs.autoOcr? 'on':'off'));
            const el = document.getElementById('autoOcr'); if (el) el.checked = !!prefs.autoOcr;
          }catch(e){ console.error(e); }
        }
      },
      { id:'shot:capture', label:'Capture screen (preview)', hint:'screenshot', run: async ()=>{
          try{
            const port = ARW.toolPort();
            const out = await ARW.invoke('run_tool_admin', { id: 'ui.screenshot.capture', input: { scope:'screen', format:'png', downscale:640 }, port });
            ARW.toast(out && out.path ? ('Saved: ' + out.path) : 'Capture requested');
          }catch(e){ console.error(e); ARW.toast('Capture failed'); }
        }
      },
      { id:'shot:capture-window', label:'Capture this window (preview)', hint:'screenshot', run: async ()=>{
          try{
            const bounds = await ARW.invoke('active_window_bounds', { label: null });
            const x = bounds?.x ?? 0, y = bounds?.y ?? 0, w = bounds?.w ?? 0, h = bounds?.h ?? 0;
            const scope = `region:${x},${y},${w},${h}`;
            const port = ARW.toolPort();
            const out = await ARW.invoke('run_tool_admin', { id: 'ui.screenshot.capture', input: { scope, format:'png', downscale:640 }, port });
            ARW.toast(out && out.path ? ('Saved: ' + out.path) : 'Capture requested');
          }catch(e){ console.error(e); ARW.toast('Capture failed'); }
        }
      },
      { id:'shot:capture-region', label:'Capture region (drag)', hint:'screenshot', run: async ()=>{ await ARW.region.captureAndSave(); } },
      { id:'gallery:open', label:'Open Screenshots Gallery', hint:'screenshot', run: ()=> ARW.gallery.show() },
      { id:'prefs:set-editor', label:'Set preferred editor…', hint:'pref', run: async ()=>{
          try{
            const cur = ((await ARW.getPrefs('launcher'))||{}).editorCmd || '';
            const next = prompt('Editor command (use {path} placeholder)', cur || 'code --goto {path}');
            if (next != null){ const p = (await ARW.getPrefs('launcher'))||{}; p.editorCmd = String(next).trim(); await ARW.setPrefs('launcher', p); ARW.toast('Editor set'); }
          }catch(e){ console.error(e); ARW.toast('Failed to save'); }
        }
      },
      { id:'theme:auto', label:'Theme: Auto (OS)', hint:'theme', run:()=> ARW.theme.set('auto') },
      { id:'theme:light', label:'Theme: Light', hint:'theme', run:()=> ARW.theme.set('light') },
      { id:'theme:dark', label:'Theme: Dark', hint:'theme', run:()=> ARW.theme.set('dark') },
      { id:'ui:reset', label:'Reset UI (Theme/Density/Focus)', hint:'layout', run:()=>{
          try{
            // Theme → auto
            localStorage.removeItem(ARW.theme.KEY); ARW.theme.apply('auto');
            // Density → normal (clear page-specific key)
            localStorage.removeItem(ARW.density._k()); ARW.density.apply('normal');
            // Focus → off
            localStorage.removeItem(ARW.layout._k()); ARW.layout.apply(false);
            ARW.ui?.badges?.update(); ARW.toast('UI reset');
          }catch(e){ ARW.toast('Reset failed'); }
        }
      },
    ];
    const render = (q='')=>{
      ul.innerHTML=''; this._items = [];
      const qq = q.toLowerCase();
      for (const a of this._actions) {
        if (!qq || a.label.toLowerCase().includes(qq) || a.id.includes(qq)) {
          const li = document.createElement('li'); li.dataset.id = a.id; li.setAttribute('role','option'); li.setAttribute('aria-selected','false');
          li.innerHTML = `<span>${a.label}</span><span class="hint">${a.hint}</span>`;
          li.addEventListener('click', ()=>{ this.hide(); try{ a.run(); }catch{} });
          ul.appendChild(li); this._items.push(li);
        }
      }
      this._active = this._items.length ? 0 : -1; this._highlight();
    };
    inp.addEventListener('input', ()=> render(inp.value));
    inp.addEventListener('keydown', (e)=>{
      if (e.key==='ArrowDown'){ this._move(1); e.preventDefault(); }
      else if (e.key==='ArrowUp'){ this._move(-1); e.preventDefault(); }
      else if (e.key==='Enter'){ if (this._active>=0) { const id = this._items[this._active].dataset.id; const act = this._actions.find(a=>a.id===id); this.hide(); try{ act?.run(); }catch{} } }
      else if (e.key==='Escape'){ this.hide(); }
    });
    wrap.addEventListener('click', (e)=>{ if (e.target===wrap) this.hide(); });
    window.addEventListener('keydown', (e)=>{
      const mod = e.metaKey || e.ctrlKey;
      if (mod && e.key.toLowerCase()==='k'){ this.toggle(); e.preventDefault(); }
    });
    render('');
  },
  _move(dir){ if (!this._items.length) return; this._active = (this._active + dir + this._items.length) % this._items.length; this._highlight(); },
  _highlight(){ this._items.forEach((el,i)=> { const on = i===this._active; el.classList.toggle('active', on); el.setAttribute('aria-selected', on? 'true':'false'); }); },
  show(){ if (!this._wrap) return; this._wrap.style.display='grid'; this._input.value=''; this._input.focus({ preventScroll: true }); },
  hide(){ if (!this._wrap) return; this._wrap.style.display='none'; },
  toggle(){ if (!this._wrap) return; const shown = this._wrap.style.display==='grid'; if (shown) this.hide(); else this.show(); }
};

// Screenshots gallery
window.ARW.gallery = {
  _wrap: null,
  _items: [],
  add(ev){
    try{
      const p = ev?.env?.payload || ev?.env || ev;
      const time = ev?.env?.time || new Date().toISOString();
      if (!p || !p.path) return;
      // Deduplicate by path (keep most recent)
      const idx = this._items.findIndex(it => it.path === p.path);
      if (idx >= 0) this._items.splice(idx, 1);
      this._items.unshift({ time, path: p.path, preview_b64: p.preview_b64 || null });
      if (this._items.length > 60) this._items.pop();
    }catch{}
  },
  mount(){
    if (this._wrap) return; const w=document.createElement('div'); w.className='gallery-wrap'; const g=document.createElement('div'); g.className='gallery'; g.setAttribute('role','dialog'); g.setAttribute('aria-modal','true'); g.setAttribute('aria-label','Screenshots gallery');
    const h=document.createElement('header'); const title=document.createElement('strong'); title.id='galleryTitle'; title.textContent='Screenshots'; g.setAttribute('aria-labelledby','galleryTitle'); const close=document.createElement('button'); close.className='ghost'; close.textContent='Close'; close.addEventListener('click', ()=> this.hide()); h.appendChild(title); h.appendChild(close);
    const m=document.createElement('main'); const grid=document.createElement('div'); grid.className='grid-thumbs'; m.appendChild(grid);
    g.appendChild(h); g.appendChild(m); w.appendChild(g); document.body.appendChild(w); this._wrap=w;
    // click-out close
    w.addEventListener('click', (e)=>{ if (e.target===w) this.hide(); });
  },
  render(){ if (!this._wrap) this.mount(); const grid=this._wrap.querySelector('.grid-thumbs'); if (!grid) return; grid.innerHTML='';
    for (const it of this._items){ const d=document.createElement('div'); d.className='thumb'; const img=document.createElement('img'); if (it.preview_b64) img.src=it.preview_b64; img.dataset.screenshotPath = it.path; img.alt=ARW._bestAltForPath(it.path, it.path); const meta=document.createElement('div'); meta.className='dim mono'; meta.textContent = `${it.time} ${it.path}`; const row=document.createElement('div'); row.className='row'; const open=document.createElement('button'); open.className='ghost'; open.textContent='Open'; open.addEventListener('click', async ()=>{ try{ await ARW.invoke('open_path', { path: it.path }); }catch(e){ console.error(e); } }); const copy=document.createElement('button'); copy.className='ghost'; copy.textContent='Copy path'; copy.addEventListener('click', ()=> ARW.copy(it.path)); const md=document.createElement('button'); md.className='ghost'; md.textContent='Copy MD'; md.addEventListener('click', ()=> ARW.copyMarkdown(it.path)); const save=document.createElement('button'); save.className='ghost'; save.textContent='Save to project'; save.addEventListener('click', async ()=>{ await ARW.saveToProjectPrompt(it.path); }); const ann=document.createElement('button'); ann.className='ghost'; ann.textContent='Annotate'; ann.addEventListener('click', async ()=>{ try{ if (it.preview_b64){ const rects = await ARW.annot.start(it.preview_b64); const res = await ARW.invoke('run_tool_admin', { id: 'ui.screenshot.annotate_burn', input: { path: it.path, annotate: rects, downscale:640 }, port: ARW.toolPort() }); if (res && res.preview_b64){ img.src = res.preview_b64; meta.textContent = `${it.time} ${res.path||''}`; it.path = res.path||it.path; it.preview_b64 = res.preview_b64||it.preview_b64; img.dataset.screenshotPath = it.path; ARW._updateAltForPath(it.path); } } else { ARW.toast('No preview for annotate'); } }catch(e){ console.error(e); } }); row.appendChild(open); row.appendChild(copy); row.appendChild(md); row.appendChild(save); row.appendChild(ann); d.appendChild(img); d.appendChild(meta); d.appendChild(row); grid.appendChild(d); ARW._updateAltForPath(it.path); }
  },
  show(){ if (!this._wrap) this.mount(); this.render(); this._wrap.style.display='grid'; try{ const btn=this._wrap.querySelector('header button'); btn?.focus({ preventScroll:true }); }catch{} },
  hide(){ if (this._wrap) this._wrap.style.display='none'; }
};

// Subscribe gallery to screenshots events
ARW.sse.subscribe((k)=> k.startsWith('screenshots.'), (ev)=> ARW.gallery.add(ev));

// UI badges (Theme/Density)
window.ARW.ui = window.ARW.ui || {};
window.ARW.ui.badges = {
  _el: null,
  mount(){
    if (this._el) return; const el = document.getElementById('statusBadges'); if (!el) return; this._el = el; this.update();
  },
  update(){ if (!this._el) return; const curTheme = (localStorage.getItem(ARW.theme.KEY)||'auto'); const curDen = (localStorage.getItem(ARW.density.KEY)||'normal');
    this._el.innerHTML = '';
    // Theme badge with select
    const b1 = document.createElement('span'); b1.className='badge'; b1.title = 'Theme override (device-wide). Auto follows OS setting.';
    const s1 = document.createElement('select');
    for (const [v,l] of [['auto','Auto (OS)'],['light','Light'],['dark','Dark']]){ const o=document.createElement('option'); o.value=v; o.textContent=l; if (v===curTheme) o.selected=true; s1.appendChild(o); }
    s1.addEventListener('change', ()=> ARW.theme.set(s1.value));
    const t1 = document.createElement('span'); t1.textContent='Theme:'; t1.style.marginRight='6px'; b1.appendChild(t1); b1.appendChild(s1);
    // Density badge with select (per page)
    const b2 = document.createElement('span'); b2.className='badge'; b2.title = 'Density (per page). Compact reduces spacing and radii.';
    const s2 = document.createElement('select');
    const curD = ARW.density.get();
    for (const [v,l] of [['normal','Normal'],['compact','Compact']]){ const o=document.createElement('option'); o.value=v; o.textContent=l; if (v===curD) o.selected=true; s2.appendChild(o); }
    s2.addEventListener('change', ()=> ARW.density.set(s2.value));
    const t2 = document.createElement('span'); t2.textContent='Density:'; t2.style.marginRight='6px'; b2.appendChild(t2); b2.appendChild(s2);
    this._el.appendChild(b1); this._el.appendChild(b2);
  }
};

// Apply theme/density on load and mount badges
document.addEventListener('DOMContentLoaded', ()=>{ try{ ARW.theme.init(); }catch{} try{ ARW.density.init(); }catch{} try{ ARW.layout.init(); }catch{} try{ ARW.ui.badges.mount(); }catch{} });
// Universal ESC closes overlays (palette/gallery/shortcuts/annot)
window.addEventListener('keydown', (e)=>{
  if (e.key !== 'Escape') return;
  let closed = false;
  try{ if (ARW.palette && ARW.palette._wrap && ARW.palette._wrap.style.display==='grid'){ ARW.palette.hide(); closed = true; } }catch{}
  try{ if (ARW.gallery && ARW.gallery._wrap && ARW.gallery._wrap.style.display && ARW.gallery._wrap.style.display!=='none'){ ARW.gallery.hide(); closed = true; } }catch{}
  try{ if (ARW.shortcuts && ARW.shortcuts._wrap && ARW.shortcuts._wrap.style.display && ARW.shortcuts._wrap.style.display!=='none'){ ARW.shortcuts.hide(); closed = true; } }catch{}
  try{ if (ARW.annot && ARW.annot._wrap && ARW.annot._wrap.style.display && ARW.annot._wrap.style.display!=='none'){ ARW.annot.hide(); closed = true; } }catch{}
  if (closed){ try{ e.preventDefault(); e.stopPropagation(); }catch{} }
});

  // Region capture (drag overlay)
  window.ARW.region = {
  _wrap: null,
  _rect: null,
  _onUp: null,
  mount(){
    if (this._wrap) return;
    const w = document.createElement('div'); w.className='region-wrap';
    const dim = document.createElement('div'); dim.className='region-dim'; w.appendChild(dim);
    const hint = document.createElement('div'); hint.className='region-hint'; hint.textContent='Drag to capture region — Esc to cancel'; w.appendChild(hint);
    const rect = document.createElement('div'); rect.className='region-rect'; w.appendChild(rect);
    document.body.appendChild(w); this._wrap = w; this._rect = rect;
  },
  start(){
    this.mount();
    this._wrap.style.display='block';
    let sx=0, sy=0, ex=0, ey=0; let active=false;
    const rect = this._rect; rect.style.display='none';
    const px = (n)=> Math.floor(n);
    const onMouseDown = (e)=>{ active=true; sx=e.clientX; sy=e.clientY; rect.style.display='block'; update(e); };
    const onMouseMove = (e)=>{ if (!active) return; update(e); };
    const onMouseUp = (e)=>{ if (!active) return; active=false; cleanup(); const r=this._calc(sx,sy,e.clientX,e.clientY); if (r.w>2 && r.h>2) { this._resolve(r); } else { this._reject('empty'); } };
    const onKey = (e)=>{ if (e.key==='Escape'){ cleanup(); this._reject('cancel'); } };
    const update = (e)=>{ ex=e.clientX; ey=e.clientY; const r=this._calc(sx,sy,ex,ey); rect.style.left=r.x+'px'; rect.style.top=r.y+'px'; rect.style.width=r.w+'px'; rect.style.height=r.h+'px'; };
    const cleanup = ()=>{ window.removeEventListener('mousedown', onMouseDown, true); window.removeEventListener('mousemove', onMouseMove, true); window.removeEventListener('mouseup', onMouseUp, true); window.removeEventListener('keydown', onKey, true); this._wrap.style.display='none'; };
    return new Promise((resolve,reject)=>{ this._resolve=resolve; this._reject=reject; window.addEventListener('mousedown', onMouseDown, true); window.addEventListener('mousemove', onMouseMove, true); window.addEventListener('mouseup', onMouseUp, true); window.addEventListener('keydown', onKey, true); });
  },
  _calc(sx,sy,ex,ey){ const x=Math.min(sx,ex), y=Math.min(sy,ey); const w=Math.abs(ex-sx), h=Math.abs(ey-sy); return { x, y, w, h } },
  async captureAndSave(){
    try{
      const win = await ARW.invoke('active_window_bounds', { label: null });
      const r = await this.start();
      const dpr = window.devicePixelRatio || 1;
      // Convert to physical pixels and absolute screen coords
      const X = Math.round((win.x||0) + r.x * dpr);
      const Y = Math.round((win.y||0) + r.y * dpr);
      const W = Math.max(1, Math.round(r.w * dpr));
      const H = Math.max(1, Math.round(r.h * dpr));
      const scope = `region:${X},${Y},${W},${H}`;
      const port = ARW.toolPort();
      const out = await ARW.invoke('run_tool_admin', { id: 'ui.screenshot.capture', input: { scope, format:'png', downscale:640 }, port });
      ARW.toast(out && out.path ? ('Saved: ' + out.path) : 'Capture requested');
      return out;
    }catch(e){ ARW.toast('Region capture canceled'); return null; }
  }
};

// Annotation overlay (draw rectangles on an image)
window.ARW.annot = {
  _wrap: null,
  _panel: null,
  _img: null,
  _rects: [],
  _active: null,
  mount(){ if (this._wrap) return; const w=document.createElement('div'); w.className='annot-wrap'; const dim=document.createElement('div'); dim.className='annot-dim'; const panel=document.createElement('div'); panel.className='annot-panel'; const head=document.createElement('header'); head.innerHTML='<strong>Annotate</strong>'; const main=document.createElement('div'); main.className='annot-canvas'; const img=document.createElement('img'); main.appendChild(img); const foot=document.createElement('footer'); const cancel=document.createElement('button'); cancel.className='ghost'; cancel.textContent='Cancel'; cancel.addEventListener('click', ()=> this.hide()); const apply=document.createElement('button'); apply.className='primary'; apply.textContent='Apply'; apply.addEventListener('click', ()=> this._apply()); foot.appendChild(cancel); foot.appendChild(apply); panel.appendChild(head); panel.appendChild(main); panel.appendChild(foot); w.appendChild(dim); w.appendChild(panel); document.body.appendChild(w); this._wrap=w; this._panel=panel; this._img=img; },
  show(src){ this.mount(); this._wrap.style.display='block'; this._img.src = src; this._rects=[]; this._bind(); },
  hide(){ if (this._wrap) this._wrap.style.display='none'; this._unbind(); },
  _bind(){ const canvas=this._panel.querySelector('.annot-canvas'); let sx=0, sy=0; const onDown=(e)=>{ const r=canvas.getBoundingClientRect(); sx=e.clientX - r.left; sy=e.clientY - r.top; const div=document.createElement('div'); div.className='ann-rect'; canvas.appendChild(div); this._active={ div, sx, sy }; }; const onMove=(e)=>{ if (!this._active) return; const r=canvas.getBoundingClientRect(); const ex=e.clientX - r.left; const ey=e.clientY - r.top; const x=Math.min(this._active.sx, ex), y=Math.min(this._active.sy, ey); const w=Math.abs(ex - this._active.sx), h=Math.abs(ey - this._active.sy); Object.assign(this._active.div.style, { left:x+'px', top:y+'px', width:w+'px', height:h+'px' }); }; const onUp=(e)=>{ if (!this._active) return; const rect=this._active.div.getBoundingClientRect(); const cref=canvas.getBoundingClientRect(); const x=Math.max(0, rect.left - cref.left), y=Math.max(0, rect.top - cref.top), w=rect.width, h=rect.height; this._rects.push({ x, y, w, h, blur:true }); this._active=null; }; this._onDown=onDown; this._onMove=onMove; this._onUp=onUp; canvas.addEventListener('mousedown', onDown); window.addEventListener('mousemove', onMove); window.addEventListener('mouseup', onUp); },
  _unbind(){ const canvas=this._panel?.querySelector('.annot-canvas'); if (!canvas) return; if (this._onDown) canvas.removeEventListener('mousedown', this._onDown); if (this._onMove) window.removeEventListener('mousemove', this._onMove); if (this._onUp) window.removeEventListener('mouseup', this._onUp); this._onDown=this._onMove=this._onUp=null; const rects = Array.from(canvas.querySelectorAll('.ann-rect')); rects.forEach(d=> d.remove()); },
  _apply(){ try{ if (!this._img) return; const imgEl=this._img; const natW=imgEl.naturalWidth||1, natH=imgEl.naturalHeight||1; const disp = imgEl.getBoundingClientRect(); const scaleX = natW / disp.width, scaleY = natH / disp.height; const canvas=this._panel.querySelector('.annot-canvas'); const cref=canvas.getBoundingClientRect(); const rects=Array.from(canvas.querySelectorAll('.ann-rect')).map(el=>{ const r=el.getBoundingClientRect(); const x=Math.max(0, r.left - cref.left) * scaleX; const y=Math.max(0, r.top - cref.top) * scaleY; const w=r.width * scaleX; const h=r.height * scaleY; return { x: Math.round(x), y: Math.round(y), w: Math.round(w), h: Math.round(h), blur:true }; }); this._resolve(rects); this.hide(); }catch(e){ this._reject(e); this.hide(); } },
  start(src){ this.show(src); return new Promise((resolve,reject)=>{ this._resolve=resolve; this._reject=reject; }); }
};

// Keyboard Shortcuts overlay (global)
window.ARW.shortcuts = {
  _wrap: null, _panel: null, _list: null,
  _mkRow(k,d){ const tr=document.createElement('tr'); tr.innerHTML=`<td class="mono">${k}</td><td>${d}</td>`; return tr; },
  _content(page){
    const base = [ ['Ctrl/Cmd+K','Command palette'], ['?','Shortcuts help'] ];
    const map = {
      hub: [['Arrows','Navigate files tree'], ['Enter','Open folder / preview file'], ['Left/Right','Collapse/Expand or focus parent/child'], ['/', 'Focus file filter'], ['n','Focus new project'], ['b','Back to previous folder']],
      events: [['p','Pause (checkbox)'], ['c','Clear log']],
      logs: [['r','Refresh'], ['w','Toggle wrap'], ['a','Toggle auto']],
      models: [['R','Refresh'], ['L','Load'], ['S','Save'], ['J','Refresh jobs'], ['A','Toggle jobs auto']],
      chat: [['Enter','Send message'], ['C','Capture (buttons)']],
      training: [['A','Run A/B']],
      index: [['S','Save prefs'], ['T','Start service'], ['X','Stop service'], ['H','Check health'], ['O','Open Debug UI']]
    };
    return base.concat(map[page]||[]);
  },
  mount(){ if (this._wrap) return; const w=document.createElement('div'); w.className='gallery-wrap'; const p=document.createElement('div'); p.className='gallery'; p.setAttribute('role','dialog'); p.setAttribute('aria-modal','true'); p.setAttribute('aria-label','Keyboard shortcuts'); const h=document.createElement('header'); const t=document.createElement('strong'); t.textContent='Keyboard Shortcuts'; const x=document.createElement('button'); x.className='ghost'; x.textContent='Close'; x.addEventListener('click', ()=> this.hide()); h.appendChild(t); h.appendChild(x); const m=document.createElement('main'); const tbl=document.createElement('table'); tbl.className='cmp-table'; const tb=document.createElement('tbody'); tbl.appendChild(tb); m.appendChild(tbl); p.appendChild(h); p.appendChild(m); w.appendChild(p); document.body.appendChild(w); this._wrap=w; this._panel=p; this._list=tb; },
  mount(){ if (this._wrap) return; const w=document.createElement('div'); w.className='gallery-wrap'; const p=document.createElement('div'); p.className='gallery'; p.setAttribute('role','dialog'); p.setAttribute('aria-modal','true'); p.setAttribute('aria-label','Keyboard shortcuts'); const h=document.createElement('header'); const t=document.createElement('strong'); t.textContent='Keyboard Shortcuts'; const x=document.createElement('button'); x.className='ghost'; x.textContent='Close'; x.addEventListener('click', ()=> this.hide()); h.appendChild(t); h.appendChild(x); const m=document.createElement('main'); const tbl=document.createElement('table'); tbl.className='cmp-table'; const tb=document.createElement('tbody'); tbl.appendChild(tb); m.appendChild(tbl); p.appendChild(h); p.appendChild(m); w.appendChild(p); document.body.appendChild(w); this._wrap=w; this._panel=p; this._list=tb; w.addEventListener('click', (e)=>{ if (e.target===w) this.hide(); }); },
  show(){ this.mount(); const tb=this._list; if (!tb) return; tb.innerHTML=''; const page = ARW.util.pageId(); const rows=this._content(page); rows.forEach(([k,d])=> tb.appendChild(this._mkRow(k,d))); this._wrap.style.display='grid'; try{ this._panel.querySelector('header button')?.focus({ preventScroll:true }); }catch{} },
  hide(){ if (this._wrap) this._wrap.style.display='none'; },
  toggle(){ if (!this._wrap || this._wrap.style.display==='none') this.show(); else this.hide(); }
};

// Global shortcuts help wiring
document.addEventListener('DOMContentLoaded', ()=>{
  try{ const b=document.getElementById('btn-shortcuts'); if (b) b.addEventListener('click', ()=> ARW.shortcuts.show()); }catch{}
  try{ const h=document.getElementById('btn-help'); if (h) h.addEventListener('click', ()=> ARW.invoke('open_url', { url: 'https://t3hw00t.github.io/ARW/guide/shortcuts/' })); }catch{}
});
window.addEventListener('keydown', (e)=>{
  const tag=(e.target && e.target.tagName || '').toLowerCase();
  if (tag==='input' || tag==='textarea' || tag==='select') return;
  if (e.ctrlKey || e.metaKey || e.altKey) return;
  if (e.key==='?' || (e.shiftKey && e.key==='/')){ e.preventDefault(); ARW.shortcuts.toggle(); }
});
