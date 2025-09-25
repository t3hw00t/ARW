// Lightweight helpers shared by launcher pages
// Capture optional base override from query string (`?base=http://host:port`)
(() => { try { const u = new URL(window.location.href); const b = u.searchParams.get('base'); if (b) { window.__ARW_BASE_OVERRIDE = String(b).replace(/\/?$/, ''); } } catch {} })();

window.ARW = {
  _prefsCache: new Map(),
  _prefsTimers: new Map(),
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
    _norm(b){ try{ return String(b||'').trim().replace(/\/$/,''); }catch{ return '' } },
    async tokenFor(base){
      try{
        const prefs = await ARW.getPrefs('launcher') || {};
        const norm = this._norm(base);
        const list = Array.isArray(prefs.connections) ? prefs.connections : [];
        const hit = list.find(c => this._norm(c.base) === norm);
        return (hit && hit.token) || prefs.adminToken || null;
      }catch{ return null }
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
  copyMarkdown(path, alt){
    try{
      const md = `![${(alt||'')}](${path})`;
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
  base(port) {
    try { if (window.__ARW_BASE_OVERRIDE && typeof window.__ARW_BASE_OVERRIDE === 'string') return window.__ARW_BASE_OVERRIDE; } catch {}
    const p = Number.isFinite(port) && port > 0 ? port : 8091
    return `http://127.0.0.1:${p}`
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
    _retryMs: 500,
    _retryTimer: null,
    _closing: false,
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
    connect(baseUrl, opts = {}, resumeLast = false) {
      const prevBase = this._base;
      const baseChanged = typeof prevBase === 'string' && prevBase !== baseUrl;
      this._base = baseUrl;
      this._opts = { ...(opts || {}) };
      if (baseChanged) {
        this._lastId = null;
      }
      this._clearTimer();
      if (this._es) { try { this._closing = true; this._es.close(); } catch {} this._es = null; this._closing = false; }
      const useAfter = resumeLast && !baseChanged && this._lastId;
      const url = this._url(baseUrl, this._opts, useAfter ? this._lastId : null);
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
        // EventSource auto-reconnects, but in some environments it can stall.
        // Kick a fresh connection with modest backoff unless we intentionally closed.
        if (!closing) {
          this._clearTimer();
          this._retryTimer = setTimeout(() => { try { this.reconnect(); } catch {} }, ms);
          this._retryMs = Math.min(ms * 2, 5000);
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
      // Reconnect on network re-gain
      try {
        window.removeEventListener('online', this._onlineOnce);
      } catch {}
      this._onlineOnce = () => { try { this.reconnect(); } catch {} };
      try { window.addEventListener('online', this._onlineOnce, { once: true }); } catch {}
    },
    reconnect(){ if (this._base) this.connect(this._base, this._opts || {}, true); },
    close(){
      this._clearTimer();
      if (this._es){ try { this._closing = true; this._es.close(); } catch {} this._es=null; }
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
          const r = await fetch(opts.base.replace(/\/$/,'') + '/state/policy');
          const j = await r.json();
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
          const r = await fetch(opts.base.replace(/\/$/,'') + '/state/world/select?k=8');
          const j = await r.json(); const items = j?.items || j?.data?.items || [];
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
        const box = document.createElement('div'); box.className='evt';
        const ts = env?.time || new Date().toISOString();
        const img = document.createElement('img'); img.alt = p?.path||''; img.style.maxWidth='100%'; img.style.maxHeight='120px';
        if (p?.preview_b64 && /^data:image\//.test(p.preview_b64)) { img.src = p.preview_b64; }
        else { img.src = ''; img.style.display='none'; }
        const cap = document.createElement('div'); cap.className='dim mono'; cap.textContent = `${ts} ${p?.path||''}`;
      const actions = document.createElement('div'); actions.className='row';
      const openBtn = document.createElement('button'); openBtn.className='ghost'; openBtn.textContent='Open'; openBtn.addEventListener('click', async ()=>{ try{ if (p?.path) await ARW.invoke('open_path', { path: p.path }); }catch(e){ console.error(e); } });
      const copyBtn = document.createElement('button'); copyBtn.className='ghost'; copyBtn.textContent='Copy path'; copyBtn.addEventListener('click', ()=>{ if (p?.path) ARW.copy(String(p.path)); });
        const mdBtn = document.createElement('button'); mdBtn.className='ghost'; mdBtn.textContent='Copy MD'; mdBtn.addEventListener('click', ()=>{ if (p?.path) ARW.copyMarkdown(p.path, 'screenshot'); });
        const annBtn = document.createElement('button'); annBtn.className='ghost'; annBtn.textContent='Annotate'; annBtn.addEventListener('click', async ()=>{ try{ if (p?.preview_b64){ const rects = await ARW.annot.start(p.preview_b64); const res = await ARW.invoke('run_tool_admin', { id: 'ui.screenshot.annotate_burn', input: { path: p.path, annotate: rects, downscale:640 }, port: ARW.getPortFromInput('port') }); if (res && res.preview_b64){ img.src = res.preview_b64; cap.textContent = `${ts} ${res.path||''}`; } } else { ARW.toast('No preview for annotate'); } }catch(e){ console.error(e); }});
        const saveBtn = document.createElement('button'); saveBtn.className='ghost'; saveBtn.textContent='Save to project'; saveBtn.addEventListener('click', async ()=>{ if (p?.path){ const res = await ARW.saveToProjectPrompt(p.path); if (res) await ARW.maybeAppendToNotes(res.proj, res.dest); } });
        actions.appendChild(openBtn); actions.appendChild(copyBtn); actions.appendChild(mdBtn); actions.appendChild(annBtn); actions.appendChild(saveBtn);
        box.appendChild(img); box.appendChild(cap); box.appendChild(actions);
        el.prepend(box);
        while (el.childElementCount>6) el.removeChild(el.lastChild);
      };
      const idActivity = ARW.sse.subscribe((k)=> k.startsWith('screenshots.'), rActivity);
      // initial render for metrics if any
      rMetrics();
      return { dispose(){ ARW.sse.unsubscribe(idAll); ARW.sse.unsubscribe(idModels); ARW.read.unsubscribe(idMetrics); ARW.read.unsubscribe(idSnappy); ARW.sse.unsubscribe(idActivity); if(policyTimer) clearInterval(policyTimer); node.innerHTML=''; } }
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
      { id:'sse:replay', label:'Replay SSE (50)', hint:'sse', run:()=> ARW.sse.connect((base||ARW.base(ARW.getPortFromInput('port'))), { replay:50 }) },
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
            const p = ARW.getPortFromInput('port');
            const out = await ARW.invoke('run_tool_admin', { id: 'ui.screenshot.capture', input: { scope:'screen', format:'png', downscale:640 }, port: p });
            ARW.toast(out && out.path ? ('Saved: ' + out.path) : 'Capture requested');
          }catch(e){ console.error(e); ARW.toast('Capture failed'); }
        }
      },
      { id:'shot:capture-window', label:'Capture this window (preview)', hint:'screenshot', run: async ()=>{
          try{
            const bounds = await ARW.invoke('active_window_bounds', { label: null });
            const x = bounds?.x ?? 0, y = bounds?.y ?? 0, w = bounds?.w ?? 0, h = bounds?.h ?? 0;
            const scope = `region:${x},${y},${w},${h}`;
            const p = ARW.getPortFromInput('port');
            const out = await ARW.invoke('run_tool_admin', { id: 'ui.screenshot.capture', input: { scope, format:'png', downscale:640 }, port: p });
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
    for (const it of this._items){ const d=document.createElement('div'); d.className='thumb'; const img=document.createElement('img'); if (it.preview_b64) img.src=it.preview_b64; img.alt=it.path; const meta=document.createElement('div'); meta.className='dim mono'; meta.textContent = `${it.time} ${it.path}`; const row=document.createElement('div'); row.className='row'; const open=document.createElement('button'); open.className='ghost'; open.textContent='Open'; open.addEventListener('click', async ()=>{ try{ await ARW.invoke('open_path', { path: it.path }); }catch(e){ console.error(e); } }); const copy=document.createElement('button'); copy.className='ghost'; copy.textContent='Copy path'; copy.addEventListener('click', ()=> ARW.copy(it.path)); const md=document.createElement('button'); md.className='ghost'; md.textContent='Copy MD'; md.addEventListener('click', ()=> ARW.copyMarkdown(it.path, 'screenshot')); const save=document.createElement('button'); save.className='ghost'; save.textContent='Save to project'; save.addEventListener('click', async ()=>{ await ARW.saveToProjectPrompt(it.path); }); const ann=document.createElement('button'); ann.className='ghost'; ann.textContent='Annotate'; ann.addEventListener('click', async ()=>{ try{ if (it.preview_b64){ const rects = await ARW.annot.start(it.preview_b64); const res = await ARW.invoke('run_tool_admin', { id: 'ui.screenshot.annotate_burn', input: { path: it.path, annotate: rects, downscale:640 }, port: ARW.getPortFromInput('port') }); if (res && res.preview_b64){ img.src = res.preview_b64; meta.textContent = `${it.time} ${res.path||''}`; it.path = res.path||it.path; it.preview_b64 = res.preview_b64||it.preview_b64; } } else { ARW.toast('No preview for annotate'); } }catch(e){ console.error(e); } }); row.appendChild(open); row.appendChild(copy); row.appendChild(md); row.appendChild(save); row.appendChild(ann); d.appendChild(img); d.appendChild(meta); d.appendChild(row); grid.appendChild(d); }
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
      const p = ARW.getPortFromInput('port');
      const out = await ARW.invoke('run_tool_admin', { id: 'ui.screenshot.capture', input: { scope, format:'png', downscale:640 }, port: p });
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
