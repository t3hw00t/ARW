const port = () => ARW.getPortFromInput('port');
const currentRemoteBase = () => {
  try {
    const override = ARW.baseOverride();
    return override ? override : null;
  } catch {
    return null;
  }
};
const updateBaseMeta = () => ARW.applyBaseMeta({ portInputId: 'port', badgeId: 'baseBadge', label: 'Base' });

let modelsSseSub = null;
let modelsSseIndicator = null;
let currentEgressSettings = null;
let currentEgressScopes = [];
let scopeCapabilityIndex = new Map();

function ensureSseIndicator() {
  const wrap = document.getElementById('statusBadges');
  if (!wrap) return;
  if (modelsSseIndicator) return;
  let badge = document.getElementById('modelsSseBadge');
  if (!badge) {
    badge = document.createElement('span');
    badge.id = 'modelsSseBadge';
    badge.className = 'badge';
    wrap.appendChild(badge);
  }
  modelsSseIndicator = ARW.sse.indicator(badge, { prefix: 'SSE' });
}

function connectModelsSse({ replay = 0, resume = true } = {}) {
  ensureSseIndicator();
  const opts = { prefix: 'models.' };
  if (replay > 0) opts.replay = replay;
  const meta = updateBaseMeta();
  ARW.sse.connect(meta.base, opts, resume);
}

async function ivk(cmd, args){
  const remoteBase = currentRemoteBase();
  if (!remoteBase) return ARW.invoke(cmd, args);
  const tok = await ARW.connections.tokenFor(remoteBase);
  const get = (p)=> ARW.invoke('admin_get_json_base', { base: remoteBase, path: p, token: tok });
  const post = (p, body)=> ARW.invoke('admin_post_json_base', { base: remoteBase, path: p, body: body||{}, token: tok });
  switch(cmd){
    case 'models_summary':{
      const env = await get('admin/models/summary');
      return (env && env.data) ? env.data : env;
    }
    case 'models_list': return get('admin/models');
    case 'models_concurrency_get': return get('admin/models/concurrency');
    case 'models_concurrency_set':{
      const body = { max: args?.max, block: args?.block };
      const env = await post('admin/models/concurrency', body);
      return (env && env.data) ? env.data : env;
    }
    case 'models_jobs': return get('admin/models/jobs');
    case 'models_refresh': return post('admin/models/refresh', null);
    case 'models_save': return post('admin/models/save', null);
    case 'models_load': return post('admin/models/load', null);
    case 'models_add': return post('admin/models/add', { id: args?.id, provider: args?.provider||null });
    case 'models_delete': return post('admin/models/delete', { id: args?.id });
    case 'models_default_get':{
      const v = await get('admin/models/default');
      return (v && typeof v.default === 'string') ? v.default : '';
    }
    case 'models_default_set': return post('admin/models/default', { id: args?.id });
    case 'models_download':{
      const id = String(args?.id||'').trim();
      const url = String(args?.url||'').trim();
      const sha = String(args?.sha256||'').trim().toLowerCase();
      if (!id || !url) throw new Error('id/url required');
      if (!(url.startsWith('http://') || url.startsWith('https://'))) throw new Error('invalid url');
      if (!/^[0-9a-f]{64}$/.test(sha)) throw new Error('invalid sha256');
      return post('admin/models/download', { id, url, provider: args?.provider||null, sha256: sha });
    }
    case 'models_download_cancel': return post('admin/models/download/cancel', { id: args?.id });
    case 'state_models_hashes':{
      const limit = args?.limit ?? 100, offset = args?.offset ?? 0;
      const prov = args?.provider ? `&provider=${encodeURIComponent(args.provider)}` : '';
      const sort = args?.sort ? `&sort=${encodeURIComponent(args.sort)}` : '';
      const order= args?.order ? `&order=${encodeURIComponent(args.order)}` : '';
      const path = `state/models_hashes?limit=${limit}&offset=${offset}${prov}${sort}${order}`;
      // public endpoint
      return ARW.invoke('admin_get_json_base', { base: remoteBase, path, token: null });
    }
    default:
      // Fallback to local behavior
      return ARW.invoke(cmd, args);
  }
}

function svgIcon(kind, tone){
  const cls = `ico ${tone||'info'}`;
  switch(kind){
    case 'download':
      return `<svg class="${cls}" viewBox="0 0 24 24" fill="none" xmlns="http://www.w3.org/2000/svg"><path d="M12 3v10m0 0l4-4m-4 4L8 9M4 17h16" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"/></svg>`;
    case 'check':
      return `<svg class="${cls}" viewBox="0 0 24 24" fill="none" xmlns="http://www.w3.org/2000/svg"><path d="M20 6l-11 11-5-5" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"/></svg>`;
    case 'refresh':
      return `<svg class="${cls}" viewBox="0 0 24 24" fill="none" xmlns="http://www.w3.org/2000/svg"><path d="M20 12a8 8 0 10-2.34 5.66M20 12V6m0 6h-6" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"/></svg>`;
    case 'timer':
      return `<svg class="${cls}" viewBox="0 0 24 24" fill="none" xmlns="http://www.w3.org/2000/svg"><path d="M12 8v5l3 3M9 3h6M12 21a8 8 0 100-16 8 8 0 000 16z" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"/></svg>`;
    case 'warn':
      return `<svg class="${cls}" viewBox="0 0 24 24" fill="none" xmlns="http://www.w3.org/2000/svg"><path d="M12 9v4m0 4h.01M10.29 3.86l-8 14A2 2 0 004 21h16a2 2 0 001.71-3.14l-8-14a2 2 0 00-3.42 0z" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"/></svg>`;
    case 'stop':
      return `<svg class="${cls}" viewBox="0 0 24 24" fill="none" xmlns="http://www.w3.org/2000/svg"><rect x="6" y="6" width="12" height="12" rx="2" stroke="currentColor" stroke-width="2"/></svg>`;
    case 'hdd':
      return `<svg class="${cls}" viewBox="0 0 24 24" fill="none" xmlns="http://www.w3.org/2000/svg"><rect x="3" y="7" width="18" height="10" rx="2" stroke="currentColor" stroke-width="2"/><path d="M7 11h.01M11 11h.01M15 11h2" stroke="currentColor" stroke-width="2" stroke-linecap="round"/></svg>`;
    case 'cloud':
      return `<svg class="${cls}" viewBox="0 0 24 24" fill="none" xmlns="http://www.w3.org/2000/svg"><path d="M17.5 19a4.5 4.5 0 100-9 5.5 5.5 0 10-10.8 1.5A3.5 3.5 0 007 19h10.5z" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"/></svg>`;
    case 'hash':
      return `<svg class="${cls}" viewBox="0 0 24 24" fill="none" xmlns="http://www.w3.org/2000/svg"><path d="M5 9h14M5 15h14M9 5L7 19M17 5l-2 14" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"/></svg>`;
    case 'lock':
      return `<svg class="${cls}" viewBox="0 0 24 24" fill="none" xmlns="http://www.w3.org/2000/svg"><rect x="5" y="11" width="14" height="9" rx="2" stroke="currentColor" stroke-width="2"/><path d="M9 11V8a3 3 0 116 0v3" stroke="currentColor" stroke-width="2" stroke-linecap="round"/></svg>`;
    default:
      return `<svg class="${cls}" viewBox="0 0 24 24" fill="none" xmlns="http://www.w3.org/2000/svg"><circle cx="12" cy="12" r="4" stroke="currentColor" stroke-width="2"/></svg>`;
  }
}

function iconsFor(status, code){
  if (status === 'complete') return svgIcon('check','ok');
  if (status === 'resumed') return svgIcon('refresh','accent');
  if (status === 'started' || status === 'downloading') return svgIcon('download','accent');
  if (status === 'degraded') return svgIcon('timer','warn');
  if (status === 'canceled') return svgIcon('stop','info');
  switch(String(code||'')){
    case 'admission-denied':
      return svgIcon('lock','bad') + svgIcon('timer','warn');
    case 'hard-exhausted':
      return svgIcon('timer','bad');
    case 'size-limit':
    case 'size-limit-stream':
      return svgIcon('stop','bad');
    case 'disk-insufficient':
    case 'disk-insufficient-stream':
      return svgIcon('hdd','bad') + svgIcon('warn','warn');
    case 'checksum-mismatch':
      return svgIcon('hash','bad') + svgIcon('stop','bad');
    case 'request-failed':
      return svgIcon('cloud','bad') + svgIcon('stop','bad');
    case 'resume-http-status':
    case 'downstream-http-status':
      return svgIcon('cloud','warn') + svgIcon('warn','warn');
    case 'resync':
      return svgIcon('refresh','warn');
    default:
      return '';
  }
}

function escapeHtml(value){
  return String(value || '')
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
    .replace(/"/g, '&quot;')
    .replace(/'/g, '&#39;');
}

function escapeAttr(value){
  return String(value || '')
    .replace(/&/g, '&amp;')
    .replace(/"/g, '&quot;')
    .replace(/'/g, '&#39;');
}

function renderModelsCell(models){
  const list = Array.isArray(models) ? models.filter(Boolean) : [];
  if (!list.length){
    return '<span class="dim">—</span>';
  }
  const pills = list
    .map(m => `<button type="button" class="pill" data-copy-model="${escapeAttr(m)}">${escapeHtml(m)}</button>`)
    .join(' ');
  const encoded = escapeAttr(JSON.stringify(list));
  return `
    <div class="models-cell" data-models="${encoded}">
      ${pills}
      <button type="button" class="ghost models-copy" data-copy-models title="Copy all model ids">Copy</button>
    </div>
  `;
}

function statusLabel(status){
  const map = {
    available: 'Available',
    queued: 'Queued',
    downloading: 'Downloading',
    resumed: 'Resumed',
    'cancel-requested': 'Cancel requested',
    canceled: 'Canceled',
    coalesced: 'Coalesced',
    degraded: 'Degraded',
    error: 'Error',
    complete: 'Completed',
  };
  return map[status] || (status ? status.replace(/_/g, ' ') : '');
}

function statusTone(status){
  switch(status){
    case 'available':
    case 'complete':
      return 'ok';
    case 'resumed':
    case 'downloading':
    case 'coalesced':
      return 'accent';
    case 'degraded':
    case 'cancel-requested':
    case 'queued':
      return 'warn';
    case 'error':
      return 'bad';
    default:
      return 'neutral';
  }
}

function renderStatusBadge(model){
  const status = model && model.status ? String(model.status) : '';
  if (!status){
    return '<span class="dim">—</span>';
  }
  const label = statusLabel(status);
  const tone = statusTone(status);
  const badgeClass = tone && tone !== 'neutral' ? `status-badge ${tone}` : 'status-badge';
  const icon = iconsFor(status, model && model.error_code);
  const titleParts = [label];
  if (model && model.error_code) titleParts.push(`Code: ${model.error_code}`);
  if (model && model.url && status === 'downloading') titleParts.push(`Source: ${model.url}`);
  const title = escapeHtml(titleParts.join(' — '));
  const iconHtml = icon ? `${icon}` : '<span class="status-dot" aria-hidden="true"></span>';
  return `<span class="${badgeClass}" role="status" aria-label="${title}" title="${title}">${iconHtml}<span>${escapeHtml(label)}</span></span>`;
}

async function fetchAdminJson(path){
  const clean = String(path || '').replace(/^\/+/, '');
  try{
    const remoteBase = currentRemoteBase();
    if (remoteBase){
      const token = await ARW.connections.tokenFor(remoteBase);
      return await ARW.invoke('admin_get_json_base', { base: remoteBase, path: clean, token });
    }
  }catch(e){ console.error(e); }
  const headers = {};
  try{
    const tok = document.getElementById('admintok')?.value?.trim();
    if (tok) headers['X-ARW-Admin'] = tok;
  }catch{}
  const meta = updateBaseMeta();
  const baseUrl = meta.base;
  const resp = await ARW.http.fetch(baseUrl, `/${clean}`, { headers });
  if (!resp.ok) throw new Error(`HTTP ${resp.status}`);
  return await resp.json();
}

async function postAdminJson(path, body){
  const clean = String(path || '').replace(/^\/+/, '');
  try{
    const remoteBase = currentRemoteBase();
    if (remoteBase){
      const token = await ARW.connections.tokenFor(remoteBase);
      return await ARW.invoke('admin_post_json_base', {
        base: remoteBase,
        path: clean,
        body: body || {},
        token,
      });
    }
  }catch(e){ console.error(e); throw e; }
  const headers = { 'content-type': 'application/json' };
  try{
    const tok = document.getElementById('admintok')?.value?.trim();
    if (tok){
      headers['X-ARW-Admin'] = tok;
      headers['Authorization'] = `Bearer ${tok}`;
    }
  }catch{}
  const meta = updateBaseMeta();
  const baseUrl = meta.base;
  const resp = await ARW.http.fetch(baseUrl, `/${clean}`, {
    method: 'POST',
    headers,
    body: JSON.stringify(body || {}),
  });
  if (!resp.ok){
    const text = await resp.text().catch(()=> '');
    throw new Error(`HTTP ${resp.status}${text ? `: ${text}` : ''}`);
  }
  return await resp.json();
}

function shortSha(value){
  const s = String(value||'');
  if (s.length <= 12) return s || '—';
  return `${s.slice(0,6)}…${s.slice(-4)}`;
}

function parseListInput(value){
  return String(value || '')
    .split(/[,\n]+/)
    .map((s) => s.trim())
    .filter(Boolean);
}

function parsePortsInput(value){
  const ports = parseListInput(value)
    .map((p) => Number(p))
    .filter((p) => Number.isInteger(p) && p >= 1 && p <= 65535);
  const dedup = Array.from(new Set(ports));
  return dedup;
}

function normalizeProtocols(list){
  const allowed = new Set();
  list.forEach((item) => {
    const lower = String(item).trim().toLowerCase();
    if (!lower) return;
    if (!['http', 'https', 'tcp'].includes(lower)) {
      throw new Error(`Unsupported protocol '${lower}' (use http, https, or tcp)`);
    }
    allowed.add(lower);
  });
  return Array.from(allowed);
}

function scopeLabel(scope){
  const id = typeof scope.id === 'string' ? scope.id.trim() : '';
  const desc = typeof scope.description === 'string' ? scope.description.trim() : '';
  return id || desc || '(unnamed)';
}

function normalizeHosts(list){
  const out = [];
  list.forEach((raw) => {
    if (!raw) return;
    let value = String(raw).trim();
    if (!value) return;
    const wildcard = value.startsWith('*.');
    let body = wildcard ? value.slice(2) : value;
    body = body.replace(/\.+$/, '').toLowerCase();
    if (!body) return;
    const normalized = wildcard ? `*.${body}` : body;
    if (!out.includes(normalized)) {
      out.push(normalized);
    }
  });
  return out;
}

async function loadPrefs() {
  await ARW.applyPortFromPrefs('port');
  updateBaseMeta();
  const v = await ARW.getPrefs('launcher');
  if (v && v.adminToken) document.getElementById('admintok').value = v.adminToken;
  try{
    const auto = document.getElementById('jobs-auto'); if (auto){ auto.checked = !!(v && v.jobsAuto); }
    const act = document.getElementById('models-active-only'); if (act){ act.checked = !!(v && v.modelsActiveOnly); }
    const hp = document.getElementById('hash-prov'); if (hp) hp.value = (v && v.hashProvider) || '';
    const hs = document.getElementById('hash-sort'); if (hs) hs.value = (v && v.hashSort) || 'bytes';
    const ho = document.getElementById('hash-order'); if (ho) ho.value = (v && v.hashOrder) || 'desc';
    const hl = document.getElementById('hash-limit'); if (hl) hl.value = (v && v.hashLimit) || 50;
    const cb = document.getElementById('concblock'); if (cb) cb.checked = (v && typeof v.concBlock==='boolean') ? !!v.concBlock : cb.checked;
    // Start auto jobs if enabled
    if (auto && auto.checked){ setJobsAuto(true); }
  }catch{}
}
async function savePrefs() {
  const v = await ARW.getPrefs('launcher') || {};
  v.port = port();
  v.adminToken = document.getElementById('admintok').value || '';
  await ARW.setPrefs('launcher', v);
  document.getElementById('stat').textContent = 'Saved prefs';
  updateBaseMeta();
  connectModelsSse({ replay: 10, resume: false });
  startModelsSse();
}

async function refresh() {
  document.getElementById('stat').textContent = 'Loading...';
  const sum = await ivk('models_summary', { port: port() });
  const def = (sum && sum.default) || '';
  const remoteBase = currentRemoteBase();
  document.getElementById('def').textContent = `Default: ${def || '(none)'}`;
  // Concurrency + metrics line
  try{
    const c = sum.concurrency || {};
    window.__lastConcurrency = c;
    const mm = sum.metrics || {};
    const rate = (typeof mm.ewma_mbps === 'number') ? `${mm.ewma_mbps.toFixed(1)} MB/s` : 'n/a';
    const counters = `S:${mm.started||0} Q:${mm.queued||0} A:${mm.admitted||0} R:${mm.resumed||0} C:${mm.completed||0}+${mm.completed_cached||0} E:${mm.errors||0}`;
    const conc = `Conc ${c.available_permits||0}/${c.configured_max||0}${c.hard_cap?'/'+c.hard_cap: ''} (held ${c.held_permits||0})`;
    document.getElementById('stat').textContent = `${conc} · Rate ${rate} · ${counters}`;
    // Reflect configured max into input
    const ci = document.getElementById('concmax'); if (ci && c.configured_max) ci.value = c.configured_max;
    // Show pending shrink, if any (non-blocking path)
    const ps = (typeof c.pending_shrink==='number') ? c.pending_shrink : 0;
    const pendEl = document.getElementById('conc-pending'); if (pendEl) pendEl.textContent = ps>0 ? `Pending shrink: ${ps} (non-blocking)` : '';
    // Timed nudge for pending shrink
    const since = (window.__pendingSince||0);
    if (ps>0){
      if (!since) window.__pendingSince = Date.now();
      const elapsed = Date.now() - (window.__pendingSince||0);
      if (elapsed > 15000 && !window.__pendingNudged){
        window.__pendingNudged = true;
        ARW.toast('Pending shrink detected. Consider "Apply Shrink Now" to finish.');
        const sh = document.getElementById('btn-conc-shrink'); if (sh){ sh.classList.add('blink'); setTimeout(()=> sh.classList.remove('blink'), 4000); }
      }
    } else {
      window.__pendingSince = 0; window.__pendingNudged = false;
    }
  }catch{ document.getElementById('stat').textContent = 'OK'; }
  const tb = document.getElementById('models-body') || document.getElementById('models'); tb.innerHTML='';
  const fragModels = document.createDocumentFragment();
  const onlyActive = !!(document.getElementById('models-active-only') && document.getElementById('models-active-only').checked);
  ((sum && sum.items) || [])
    .filter(m => {
      if (!onlyActive) return true;
      const st = String(m.status||'').toLowerCase();
      return st === 'downloading';
    })
    .forEach(m => {
    const tr = document.createElement('tr');
    const id = m.id || '';
    tr.setAttribute('data-model-id', id);
    tr.innerHTML = `
      <td class="mono">${escapeHtml(id)}</td>
      <td>${escapeHtml(m.provider || '')}</td>
      <td>${renderStatusBadge(m)}</td>
      <td class="mono">${escapeHtml(m.path || '')}</td>
      <td>${(id===def)?'★':''}</td>
      <td></td>
    `;

    const pathCell = tr.children[3];
    if (!remoteBase && m.path){
      const openBtn = document.createElement('button');
      openBtn.textContent = 'Open';
      openBtn.title = 'Open path locally';
      openBtn.setAttribute('data-open', m.path);
      pathCell.appendChild(document.createTextNode(' '));
      pathCell.appendChild(openBtn);
      openBtn.addEventListener('click', async (e)=>{
        e.preventDefault();
        try{ await ivk('open_path', { path: m.path }); }catch(err){ console.error(err); ARW.toast('Unable to open path'); }
      });
    }

    const actionsCell = tr.lastElementChild;
    const actionWrap = document.createElement('div');
    actionWrap.className = 'pill-buttons';
    const defButton = document.createElement('button');
    defButton.textContent = 'Make Default';
    defButton.title = 'Set as default model';
    defButton.addEventListener('click', async ()=>{
      await ivk('models_default_set', { id, port: port() });
      refresh();
    });
    actionWrap.appendChild(defButton);

    const statusLower = String(m.status||'').toLowerCase();
    if (statusLower === 'downloading' || statusLower === 'resumed' || statusLower === 'cancel-requested'){
      const cancelBtn = document.createElement('button');
      cancelBtn.textContent = 'Cancel';
      cancelBtn.title = 'Cancel download';
      cancelBtn.addEventListener('click', async ()=>{
        try{
          await ivk('models_download_cancel', { id, port: port() });
          ARW.toast('Cancel requested');
          refresh();
        }catch(err){ console.error(err); ARW.toast('Cancel failed'); }
      });
      actionWrap.appendChild(cancelBtn);
    }

    actionsCell.appendChild(actionWrap);
    fragModels.appendChild(tr);
  });
  tb.appendChild(fragModels);
  loadEgressScopes().catch(err => console.error(err));
}

async function add() {
  const id = document.getElementById('mid').value.trim();
  const pr = document.getElementById('mpr').value.trim() || null;
  if (!id) return;
  await ivk('models_add', { id, provider: pr, port: port() });
  refresh();
}
async function del() {
  const id = document.getElementById('mid').value.trim();
  if (!id) return;
  await ivk('models_delete', { id, port: port() });
  refresh();
}
async function setdef() {
  const id = document.getElementById('mid').value.trim();
  if (!id) return;
  await ivk('models_default_set', { id, port: port() });
  refresh();
}
async function dl() {
  const id = document.getElementById('did').value.trim();
  let url= document.getElementById('durl').value.trim();
  const q = document.getElementById('dquant').value.trim(); if (q) url = ARW.quantReplace(url, q);
  const sha= (document.getElementById('dsha').value.trim()||null);
  if (!id || !url) return;
  await ivk('models_download', { id, url, provider: null, sha256: sha, port: port() });
  ARW.toast('Download started');
}
async function cancel() {
  const id = document.getElementById('did').value.trim();
  if (!id) return;
  await ivk('models_download_cancel', { id, port: port() });
}

function ensureBar(id){
  const bars = document.getElementById('dlbars');
  let row = bars.querySelector(`[data-row="${id}"]`);
  if(!row){
    row = document.createElement('div');
    row.setAttribute('data-row', id);
    row.innerHTML = `<div class="mono">${id}</div><div class="bar"><i data-bar></i></div><div class="mono dim" data-text></div>`;
    bars.appendChild(row);
  }
  return row;
}
function removeBar(id){ const bars=document.getElementById('dlbars'); const row=bars.querySelector(`[data-row="${id}"]`); if(row) bars.removeChild(row); }
function bytesHuman(n){ if(!n && n!==0) return '–'; const kb=1024, mb=kb*1024, gb=mb*1024, tb=gb*1024; if(n>=tb) return (n/tb).toFixed(2)+' TiB'; if(n>=gb) return (n/gb).toFixed(2)+' GiB'; if(n>=mb) return (n/mb).toFixed(1)+' MiB'; if(n>=kb) return (n/kb).toFixed(1)+' KiB'; return n+' B'; }

let jobsAutoTimer = null;
function setJobsAuto(on){
  if (jobsAutoTimer) { clearInterval(jobsAutoTimer); jobsAutoTimer = null; }
  if (on) { jobsAutoTimer = setInterval(()=>{ jobsRefresh(); }, 3000); }
}

function startModelsSse() {
  if (modelsSseSub) {
    ARW.sse.unsubscribe(modelsSseSub);
    modelsSseSub = null;
  }
  const last = {};
  let lastJobsAt = 0;
  if (!window.__etaMap) window.__etaMap = {};
  modelsSseSub = ARW.sse.subscribe((kind) => kind.startsWith('models.'), ({ kind, env }) => {
    try {
      const pl = env?.payload || {};
      if (kind === 'models.download.progress') {
        document.getElementById('dlprog').textContent = JSON.stringify(pl, null, 2);
        const id = pl.id || '';
        if (id) {
          const row = ensureBar(id);
          const bar = row.querySelector('[data-bar]');
          const txt = row.querySelector('[data-text]');
          const pct = ARW.util.downloadPercent(pl);
          if (bar) bar.style.width = ((pct != null ? pct : 0)) + '%';
          const dled = Number(pl.downloaded || 0);
          const dledTxt = bytesHuman(dled);
          const totalBytes = Number(pl.total);
          const hasTotal = Number.isFinite(totalBytes) && totalBytes > 0;
          const tot = hasTotal ? bytesHuman(totalBytes) : null;
          const now = Date.now();
          let tail = '';
          try {
            const prev = last[id];
            last[id] = { t: now, bytes: dled };
            if (prev && dled >= prev.bytes) {
              const dt = Math.max(1, now - prev.t) / 1000;
              const db = dled - prev.bytes;
              const rate = db / dt;
              const mpers = rate / (1024 * 1024);
              if (mpers > 0.01) {
                tail += ` · speed: ${mpers.toFixed(2)} MiB/s`;
                if (hasTotal && dled < totalBytes) {
                  const rem = totalBytes - dled;
                  const etaSec = Math.max(0, Math.floor(rem / Math.max(1, rate)));
                  const mm = Math.floor(etaSec / 60).toString().padStart(2, '0');
                  const ss = (etaSec % 60).toString().padStart(2, '0');
                  tail += ` · ETA: ${mm}:${ss}`;
                  window.__etaMap[id] = `${mm}:${ss}`;
                }
              }
            }
          } catch {}
          if (pl.budget && (pl.budget.spent_ms != null || pl.budget.remaining_hard_ms != null)) {
            const spent = pl.budget.spent_ms || 0;
            const rem = pl.budget.remaining_hard_ms;
            const spentS = (spent / 1000).toFixed(1);
            const remS = typeof rem === 'number' ? (rem / 1000).toFixed(1) : '∞';
            tail += ` · budget: ${spentS}s/${remS}s`;
          }
          const pctText = Number.isFinite(pct) ? pct.toFixed(1) : '…';
          const baseTxt = hasTotal ? `${dledTxt}/${tot} (${pctText}%)` : (pl.status || '');
          const diskInfo = (() => {
            if (!pl.disk) return '';
            const free = bytesHuman(pl.disk.available);
            const parts = [`Disk: ${free} free`];
            if (pl.disk.reserve != null) {
              parts.push(`reserve ${bytesHuman(pl.disk.reserve)}`);
            }
            const needTxt = pl.disk.need != null ? bytesHuman(pl.disk.need) : null;
            if (needTxt) parts.push(`need ${needTxt}`);
            return ` · ${parts.join(' · ')}`;
          })();
          const iconHtml = iconsFor(pl.status, pl.code);
          if (txt) txt.innerHTML = (iconHtml ? `<span class="icons">${iconHtml}</span>` : '') + baseTxt + tail + diskInfo;
          if (pl.disk) {
            const el = document.getElementById('disk');
            if (el) {
              const parts = [`free ${bytesHuman(pl.disk.available)}`];
              if (pl.disk.reserve != null) {
                parts.push(`reserve ${bytesHuman(pl.disk.reserve)}`);
              }
              const needTxt = pl.disk.need != null ? bytesHuman(pl.disk.need) : null;
              if (needTxt) parts.push(`need ${needTxt}`);
              el.textContent = `Disk: ${parts.join(' · ')}`;
            }
          }
          if (pl.status === 'complete') {
            setTimeout(async () => {
              await refresh();
              const list = await ivk('models_list', { port: port() });
              const found = (list || []).find((mm) => mm.id === id);
              const remote = currentRemoteBase();
              if (!remote && found && found.path) {
                try { await ivk('open_path', { path: found.path }); } catch (e) { console.error(e); }
              }
              removeBar(id);
              try { delete window.__etaMap[id]; } catch {}
            }, 250);
          }
        }
        const nowMs = Date.now();
        if (nowMs - lastJobsAt > 1000) {
          lastJobsAt = nowMs;
          jobsRefresh();
        }
        if (pl.status === 'complete' && pl.id) {
          (async () => {
            await refresh();
            const list2 = await ivk('models_list', { port: port() });
            const found = (list2 || []).find((mm) => mm.id === pl.id);
            const remote = currentRemoteBase();
            if (!remote && found && found.path) {
              try { await ivk('open_path', { path: found.path }); } catch (e) { console.error(e); }
            }
          })();
        }
      }
      if (kind === 'models.changed' || kind === 'models.refreshed') {
        refresh();
        jobsRefresh();
      }
    } catch (e) {
      console.error(e);
    }
  });
}

document.addEventListener('DOMContentLoaded', () => {
  updateBaseMeta();
  document.getElementById('btn-refresh').addEventListener('click', refresh);
  document.getElementById('btn-load').addEventListener('click', async ()=>{ await ivk('models_load', { port: port() }); refresh(); });
  document.getElementById('btn-save').addEventListener('click', async ()=>{ await ivk('models_save', { port: port() }); document.getElementById('stat').textContent='Saved'; });
  document.getElementById('btn-add').addEventListener('click', add);
  document.getElementById('btn-del').addEventListener('click', del);
  document.getElementById('btn-setdef').addEventListener('click', setdef);
  document.getElementById('btn-dl').addEventListener('click', dl);
  document.getElementById('btn-cancel').addEventListener('click', cancel);
  // Add helpful titles on dynamically created elements
  // Catalog queue buttons
  (function(){
    const tb = document.getElementById('catalog'); if (!tb) return;
    const mo = new MutationObserver(()=>{ tb.querySelectorAll('button').forEach(b=>{ if(!b.title) b.title='Queue download'; }); });
    mo.observe(tb, { childList:true, subtree:true });
  })();
  // Models table action buttons (Open/Set default/Delete are created via Rust invocations; patch titles on insert)
  (function(){
    const tb = document.getElementById('models-body') || document.getElementById('models'); if (!tb) return;
    const mo = new MutationObserver(()=>{ tb.querySelectorAll('button').forEach(b=>{ if(!b.title){ const t=(b.textContent||'').trim().toLowerCase(); if (t.includes('default')) b.title='Set as default'; else if (t.includes('delete')) b.title='Delete model'; else if (t.includes('open')) b.title='Open path'; } }); });
    mo.observe(tb, { childList:true, subtree:true });
  })();
  document.getElementById('btn-save-prefs').addEventListener('click', async ()=>{ await savePrefs(); ARW.toast('Preferences saved'); });
  document.getElementById('btn-load-local').addEventListener('click', async ()=>{
    try{
      const resp = await fetch('models_catalog.json');
      const arr = await resp.json();
      renderCatalog(arr);
    }catch(e){ console.error(e); }
  });
  document.getElementById('btn-load-remote').addEventListener('click', async ()=>{
    const u = document.getElementById('caturl').value.trim(); if(!u) return;
    try{
      const resp = await fetch(u, { headers: { 'Accept': 'application/json' }});
      const arr = await resp.json();
      renderCatalog(arr);
    }catch(e){ console.error(e); }
  });
  document.getElementById('btn-copy-sha').addEventListener('click', async ()=>{ try{ await navigator.clipboard.writeText(document.getElementById('dsha').value||''); }catch(e){} });
  document.getElementById('btn-paste-sha').addEventListener('click', async ()=>{ try{ const t=await navigator.clipboard.readText(); if(t) document.getElementById('dsha').value = t.trim(); }catch(e){} });
  document.getElementById('btn-conc-apply').addEventListener('click', async ()=>{
  try{
      const max = parseInt(document.getElementById('concmax').value||'2', 10);
      const block = !!document.getElementById('concblock').checked;
      const res = await ivk('models_concurrency_set', { max, block, port: port() });
      const prev = window.__lastConcurrency || {};
      const snapshot = res || {};
      const target = (typeof snapshot.configured_max === 'number') ? snapshot.configured_max : max;
      const held = (typeof snapshot.held_permits === 'number') ? snapshot.held_permits : 0;
      const avail = (typeof snapshot.available_permits === 'number') ? snapshot.available_permits : null;
      const pending = (typeof snapshot.pending_shrink === 'number') ? snapshot.pending_shrink : 0;
      const changed = (typeof prev.configured_max === 'number') ? prev.configured_max !== target : true;
      const parts = [`→ ${target}`, changed ? '(updated)' : '(unchanged)', `held ${held}`];
      if (avail != null) parts.push(`avail ${avail}`);
      if (pending > 0) parts.push(`pending ${pending}`);
      ARW.toast(`Concurrency ${parts.join(' · ')}`);
      window.__lastConcurrency = snapshot;
      await refresh();
      try{ const p = await ARW.getPrefs('launcher') || {}; p.concBlock = block; await ARW.setPrefs('launcher', p); }catch{}
    }catch(e){ console.error(e); ARW.toast('Failed to set concurrency'); }
  });
  // Blocking shrink (finish pending immediately)
  const shrinkBtn = document.getElementById('btn-conc-shrink');
  if (shrinkBtn) shrinkBtn.addEventListener('click', async ()=>{
    try{
      const max = parseInt(document.getElementById('concmax').value||'2', 10);
      const res = await ivk('models_concurrency_set', { max, block: true, port: port() });
      const snapshot = res || {};
      const target = (typeof snapshot.configured_max === 'number') ? snapshot.configured_max : max;
      const held = (typeof snapshot.held_permits === 'number') ? snapshot.held_permits : 0;
      const pending = (typeof snapshot.pending_shrink === 'number') ? snapshot.pending_shrink : 0;
      ARW.toast(`Concurrency → ${target} (held ${held}, pending ${pending})`);
      window.__lastConcurrency = snapshot;
      await refresh();
    }catch(e){ console.error(e); ARW.toast('Failed to shrink'); }
  });
  document.getElementById('btn-hashes').addEventListener('click', hashesRefresh);
  document.getElementById('btn-jobs').addEventListener('click', jobsRefresh);
  const jfa = document.getElementById('btn-jobs-apply');
  if (jfa) jfa.addEventListener('click', jobsRefresh);
  const jfc = document.getElementById('btn-jobs-clear');
  if (jfc) jfc.addEventListener('click', ()=>{ const jf=document.getElementById('jobs-filter'); if (jf) jf.value=''; jobsRefresh();});
  const prevBtn = document.getElementById('btn-hashes-prev');
  if (prevBtn) prevBtn.addEventListener('click', ()=>{
    const target = parseInt(prevBtn.getAttribute('data-offset')||'', 10);
    if (!Number.isFinite(target)) return;
    window.__hashOffset = Math.max(0, target);
    hashesRefresh();
  });
  const nextBtn = document.getElementById('btn-hashes-next');
  if (nextBtn) nextBtn.addEventListener('click', ()=>{
    const target = parseInt(nextBtn.getAttribute('data-offset')||'', 10);
    if (!Number.isFinite(target)) return;
    window.__hashOffset = Math.max(0, target);
    hashesRefresh();
  });
  const firstBtn = document.getElementById('btn-hashes-first');
  if (firstBtn) firstBtn.addEventListener('click', ()=>{
    window.__hashOffset = 0;
    hashesRefresh();
  });
  const lastBtn = document.getElementById('btn-hashes-last');
  if (lastBtn) lastBtn.addEventListener('click', ()=>{
    const target = parseInt(lastBtn.getAttribute('data-offset')||'', 10);
    if (!Number.isFinite(target)) return;
    window.__hashOffset = Math.max(0, target);
    hashesRefresh();
  });
  const pageInput = document.getElementById('hash-page');
  if (pageInput) pageInput.addEventListener('change', ()=>{
    const stat = document.getElementById('hash-page-stat');
    const lim = parseInt(document.getElementById('hash-limit').value||'50',10)||50;
    const totalPages = stat ? parseInt(stat.getAttribute('data-pages')||'0', 10) : 0;
    const lastOffset = stat ? parseInt(stat.getAttribute('data-last-offset')||'0', 10) : 0;
    const requested = Math.max(1, parseInt(pageInput.value||'1',10)||1);
    const capped = totalPages > 0 ? Math.min(requested, totalPages) : requested;
    pageInput.value = capped;
    const target = Math.max(0, (capped - 1) * lim);
    window.__hashOffset = lastOffset ? Math.min(target, lastOffset) : target;
    hashesRefresh();
  });
  const resetHashes = document.getElementById('btn-hashes-reset');
  if (resetHashes) resetHashes.addEventListener('click', async ()=>{
    try{
      document.getElementById('hash-prov').value = '';
      document.getElementById('hash-sort').value = 'bytes';
      document.getElementById('hash-order').value = 'desc';
      document.getElementById('hash-limit').value = 50;
      const p = await ARW.getPrefs('launcher') || {};
      p.hashProvider = '';
      p.hashSort = 'bytes';
      p.hashOrder = 'desc';
      p.hashLimit = 50;
      await ARW.setPrefs('launcher', p);
    }catch{}
    window.__hashOffset = 0;
    hashesRefresh();
  });
  const rebindBase = async () => {
    const meta = updateBaseMeta();
    const p = ARW.getPortFromInput('port') || meta.port || 8091;
    try {
      const prefs = (await ARW.getPrefs('launcher')) || {};
      if (prefs.port !== p) {
        prefs.port = p;
        await ARW.setPrefs('launcher', prefs);
      }
    } catch {}
    connectModelsSse({ replay: 10, resume: false });
    startModelsSse();
    await Promise.allSettled([
      (async () => { try { await refresh(); } catch (err) { console.error(err); } })(),
      (async () => { try { await hashesRefresh(); } catch (err) { console.error(err); } })(),
      (async () => { try { await jobsRefresh(); } catch (err) { console.error(err); } })(),
    ]);
  };
  // Persist and react to control changes
  const autoEl = document.getElementById('jobs-auto'); if (autoEl) autoEl.addEventListener('change', async (e)=>{
    setJobsAuto(!!e.target.checked);
    try{ const p = await ARW.getPrefs('launcher') || {}; p.jobsAuto = !!e.target.checked; await ARW.setPrefs('launcher', p); }catch{}
  });
  const portInput = document.getElementById('port');
  if (portInput) portInput.addEventListener('change', () => {
    rebindBase().catch((err) => console.error(err));
  });
  const hp = document.getElementById('hash-prov'); if (hp) hp.addEventListener('change', async (e)=>{ try{ const p = await ARW.getPrefs('launcher')||{}; p.hashProvider = e.target.value||''; await ARW.setPrefs('launcher', p);}catch{}; hashesRefresh(); });
  const hs = document.getElementById('hash-sort'); if (hs) hs.addEventListener('change', async (e)=>{ try{ const p = await ARW.getPrefs('launcher')||{}; p.hashSort = e.target.value||'bytes'; await ARW.setPrefs('launcher', p);}catch{}; hashesRefresh(); });
  const ho = document.getElementById('hash-order'); if (ho) ho.addEventListener('change', async (e)=>{ try{ const p = await ARW.getPrefs('launcher')||{}; p.hashOrder = e.target.value||'desc'; await ARW.setPrefs('launcher', p);}catch{}; hashesRefresh(); });
  const hl = document.getElementById('hash-limit'); if (hl) hl.addEventListener('change', async (e)=>{ const n = parseInt(e.target.value||'50',10)||50; e.target.value=n; try{ const p = await ARW.getPrefs('launcher')||{}; p.hashLimit=n; await ARW.setPrefs('launcher', p);}catch{}; hashesRefresh(); });
  const cb = document.getElementById('concblock'); if (cb) cb.addEventListener('change', async (e)=>{ try{ const p = await ARW.getPrefs('launcher')||{}; p.concBlock = !!e.target.checked; await ARW.setPrefs('launcher', p);}catch{}; });
  (async () => {
    await loadPrefs();
    await rebindBase();
  })();
  window.addEventListener('arw:base-override-changed', () => {
    rebindBase().catch((err) => console.error(err));
  });
});

// Keyboard shortcuts (ignore when typing)
window.addEventListener('keydown', (e)=>{
  const tag=(e.target && e.target.tagName || '').toLowerCase();
  if (tag==='input' || tag==='textarea' || tag==='select') return;
  if (e.ctrlKey || e.metaKey || e.altKey) return;
  const click = (id)=> document.getElementById(id)?.click();
  const k = e.key; if (!k) return;
  if (k==='R') { e.preventDefault(); click('btn-refresh'); }
  else if (k==='L') { e.preventDefault(); click('btn-load'); }
  else if (k==='S') { e.preventDefault(); click('btn-save'); }
  else if (k==='J') { e.preventDefault(); click('btn-jobs'); }
  else if (k==='A') { e.preventDefault(); const cb=document.getElementById('jobs-auto'); if (cb){ cb.checked=!cb.checked; } }
});

function renderCatalog(arr){
  const tb = document.getElementById('catalog'); tb.innerHTML='';
  (arr||[]).forEach(item => {
    const tr = document.createElement('tr');
    const id = item.id || '';
    const prov = item.provider || 'local';
    const url = item.url || '';
    const sha = item.sha256 || '';
    const notes = item.notes || '';
    tr.innerHTML = `<td class="mono">${id}</td><td>${prov}</td><td class="mono">${url}</td><td class="mono">${sha}</td><td>${notes}</td><td><button data-id="${id}">Queue</button></td>`;
    tr.querySelector('button').addEventListener('click', async ()=>{
      document.getElementById('did').value = id;
      document.getElementById('durl').value = url;
      document.getElementById('dsha').value = sha;
      await dl();
    });
    tb.appendChild(tr);
  });
}

async function hashesRefresh(){
  try{
    const prov = document.getElementById('hash-prov').value.trim() || null;
    const sort = document.getElementById('hash-sort').value || null;
    const order = document.getElementById('hash-order').value || null;
    const limit = parseInt(document.getElementById('hash-limit').value||'50', 10);
    const offset = (typeof window.__hashOffset==='number') ? Math.max(0, window.__hashOffset|0) : 0;
    const remoteBase = currentRemoteBase();
    const page = await ivk('state_models_hashes', { limit, offset, provider: prov, sort, order, port: port() });
    const tb = document.getElementById('hashes'); tb.innerHTML = '';
    const fragHashes = document.createDocumentFragment();
    (page.items||[]).forEach(it => {
      const tr = document.createElement('tr');
      const providers = Array.isArray(it.providers) ? it.providers.filter(Boolean) : [];
      const provsCell = providers.length
        ? escapeHtml(providers.join(', '))
        : '<span class="dim">—</span>';
      const modelsCell = renderModelsCell(it.models);
      const hasPath = typeof it.path === 'string' && it.path.length > 0;
      const pathSafe = hasPath ? escapeHtml(it.path) : '<span class="dim">—</span>';
      const pathAttr = hasPath ? escapeHtml(it.path) : '';
      const showOpen = hasPath && !remoteBase;
      const pbtn = showOpen ? ` <button data-open="${pathAttr}">Open</button>` : '';
      tr.innerHTML = `
        <td class="mono">${it.sha256||''}</td>
        <td>${bytesHuman(it.bytes||0)}</td>
        <td class="mono">${pathSafe}${pbtn}</td>
        <td>${provsCell}</td>
        <td>${modelsCell}</td>
      `;
      if (showOpen) {
        const op = tr.querySelector('[data-open]');
        if (op) op.addEventListener('click', async (e)=>{ await ivk('open_path', { path: e.target.getAttribute('data-open') }); });
      }
      fragHashes.appendChild(tr);
    });
    tb.appendChild(fragHashes);
    // Update page stat and controls
    const stat = document.getElementById('hash-page-stat');
    const tot = Number.isInteger(page.total) ? page.total : 0;
    const lim = Number.isInteger(page.limit) ? Math.max(1, page.limit) : Math.max(1, limit);
    const cur = Number.isInteger(page.offset) ? Math.max(0, page.offset) : offset;
    const count = Number.isInteger(page.count) ? Math.max(0, page.count) : (page.items||[]).length;
    window.__hashOffset = cur;
    const fallbackPages = Math.max(1, Math.ceil(tot/Math.max(1, lim)));
    const pages = Number.isInteger(page.pages) && page.pages > 0 ? page.pages : fallbackPages;
    const pageNumberRaw = Number.isInteger(page.page) && page.page > 0 ? page.page : (Math.floor(cur/Math.max(1, lim)) + 1);
    const pageNumber = Math.min(Math.max(1, pageNumberRaw), Math.max(1, pages));
    const fallbackPrev = cur > 0 ? Math.max(0, cur - lim) : null;
    const fallbackNext = (cur + lim < tot) ? cur + lim : null;
    const prevOffset = Number.isInteger(page.prev_offset) ? page.prev_offset : fallbackPrev;
    const nextOffset = Number.isInteger(page.next_offset) ? page.next_offset : fallbackNext;
    const fallbackLast = tot > 0 ? Math.floor((tot - 1) / lim) * lim : 0;
    const lastOffset = Number.isInteger(page.last_offset) ? page.last_offset : fallbackLast;
    if (stat){
      stat.textContent = `Showing ${cur}–${cur + count} of ${tot} · Page ${pageNumber}/${Math.max(1, pages)}`;
      stat.setAttribute('data-total', String(tot));
      stat.setAttribute('data-pages', String(Math.max(1, pages)));
      stat.setAttribute('data-last-offset', String(Math.max(0, lastOffset)));
    }
    const prev = document.getElementById('btn-hashes-prev');
    if (prev){
      if (prevOffset === null || prevOffset === undefined || prevOffset === cur){
        prev.disabled = true;
        prev.removeAttribute('data-offset');
      }else{
        prev.disabled = false;
        prev.setAttribute('data-offset', String(Math.max(0, prevOffset)));
      }
    }
    const next = document.getElementById('btn-hashes-next');
    if (next){
      if (nextOffset === null || nextOffset === undefined || nextOffset === cur){
        next.disabled = true;
        next.removeAttribute('data-offset');
      }else{
        next.disabled = false;
        next.setAttribute('data-offset', String(Math.max(0, nextOffset)));
      }
    }
    const first = document.getElementById('btn-hashes-first');
    if (first){
      first.disabled = (cur <= 0);
    }
    const last = document.getElementById('btn-hashes-last');
    if (last){
      if (lastOffset <= cur){
        last.disabled = true;
        last.removeAttribute('data-offset');
      }else{
        last.disabled = false;
        last.setAttribute('data-offset', String(Math.max(0, lastOffset)));
      }
    }
    const pageEl = document.getElementById('hash-page');
    if (pageEl){
      pageEl.value = pageNumber;
      pageEl.setAttribute('max', String(Math.max(1, pages)));
    }
    tb.querySelectorAll('[data-copy-model]').forEach(btn => {
      if (btn.dataset.boundCopy === '1') return;
      btn.dataset.boundCopy = '1';
      btn.addEventListener('click', async (e)=>{
        e.stopPropagation();
        const val = btn.getAttribute('data-copy-model') || '';
        if (!val) return;
        try {
          await ARW.copy(val);
          ARW.toast?.(`Copied ${val}`);
        } catch (err) {
          console.error(err);
        }
      });
    });
    tb.querySelectorAll('[data-copy-models]').forEach(btn => {
      if (btn.dataset.boundCopy === '1') return;
      btn.dataset.boundCopy = '1';
      btn.addEventListener('click', async (e)=>{
        e.stopPropagation();
        const cell = btn.closest('.models-cell');
        if (!cell) return;
        const raw = cell.getAttribute('data-models');
        if (!raw) return;
        try {
          const list = JSON.parse(raw);
          if (!Array.isArray(list) || !list.length) return;
          const joined = list.join('\n');
          await ARW.copy(joined);
          ARW.toast?.('Copied model ids');
        } catch (err) {
          console.error(err);
        }
      });
    });
    // Persist offset
    try{ const p = await ARW.getPrefs('launcher') || {}; p.hashOffset = cur; await ARW.setPrefs('launcher', p);}catch{}
  }catch(e){ console.error(e); }
}

async function jobsRefresh(){
  try{
    const v = await ivk('models_jobs', { port: port() });
    const tb = document.getElementById('jobs-active'); tb.innerHTML='';
    let rows = (v.active||[]);
    const filtEl = document.getElementById('jobs-filter');
    const filt = filtEl ? filtEl.value.trim() : '';
    if (filt) rows = rows.filter(it => (it.model_id||'').includes(filt));
    rows.forEach(it => {
      const tr = document.createElement('tr');
      const eta = (window.__etaMap && window.__etaMap[it.model_id]) ? window.__etaMap[it.model_id] : '';
      const etaHtml = eta ? `<span class=\"dim\">${escapeHtml(eta)}</span>` : '';
      const corr = it.corr_id || '';
      const corrHtml = corr
        ? `<div class=\"pill-buttons\"><button data-copy-corr=\"${escapeHtml(corr)}\">Copy</button><button data-ledger=\"${escapeHtml(corr)}\">Ledger</button></div>`
        : '<span class="dim">—</span>';
      tr.innerHTML = `
        <td class=\"mono\"><a href=\"#\" data-focus=\"${escapeHtml(it.model_id||'')}\">${escapeHtml(it.model_id||'')}</a></td>
        <td class=\"mono\">${escapeHtml(it.job_id||'')}</td>
        <td>${corrHtml}</td>
        <td>${etaHtml}</td>
      `;
      const link = tr.querySelector('[data-focus]');
      if (link) link.addEventListener('click', (e)=>{ e.preventDefault(); focusModel(link.getAttribute('data-focus')); });
      if (corr){
        const copyBtn = tr.querySelector(`[data-copy-corr]`);
        if (copyBtn) copyBtn.addEventListener('click', async ()=>{ await ARW.copy(corr); });
        const ledgerBtn = tr.querySelector(`[data-ledger]`);
        if (ledgerBtn) ledgerBtn.addEventListener('click', async ()=>{ await previewLedger(corr); });
      }
      tb.appendChild(tr);
    });
    if (!rows.length){
      window.__lastLedgerCorr = '';
      clearLedgerPreview();
    }
    const inflight = Array.isArray(v.inflight) ? v.inflight : [];
    const inflText = inflight.length
      ? inflight.map(it => {
          const parts = [shortSha(it.sha256)];
          if (it.primary && it.primary !== it.sha256) parts.push(`primary ${it.primary}`);
          if (Array.isArray(it.followers) && it.followers.length) parts.push(`+${it.followers.length}`);
          else if (typeof it.count === 'number' && it.count > 1) parts.push(`+${Math.max(0, it.count - 1)}`);
          return parts.join(' ');
        }).join(', ')
      : '—';
    document.getElementById('jobs-inflight').textContent = inflText;
    // reflect concurrency snapshot (kept in jobs payload for alignment)
    if (v.concurrency){
      const c = v.concurrency;
      const stat = document.getElementById('jobs-stat');
      if (stat){
        const pending = (typeof c.pending_shrink === 'number' && c.pending_shrink > 0) ? ` · pending ${c.pending_shrink}` : '';
        stat.textContent = `Conc ${c.available_permits||0}/${c.configured_max||0} (held ${c.held_permits||0})${pending}`;
      }
    }
    if (window.__lastLedgerCorr){
      await previewLedger(window.__lastLedgerCorr, { silent: true });
    } else {
      clearLedgerPreview();
    }
  }catch(e){ console.error(e); }
}
function focusModel(id){
  try{
    const rows = Array.from(document.querySelectorAll('#models tr[data-model-id]'));
    const row = rows.find(r => r.getAttribute('data-model-id') === id);
    if (row){
      row.classList.add('hl');
      row.scrollIntoView({ behavior: 'smooth', block: 'center' });
      setTimeout(()=> row.classList.remove('hl'), 1200);
    }
  }catch{}
}

function clearLedgerPreview(){
  const pre = document.getElementById('ledger-preview');
  if (!pre) return;
  if (pre.dataset && pre.dataset.locked === 'true') return;
  if (pre.dataset) pre.dataset.locked = 'false';
  pre.textContent = 'Select a job to preview recent egress entries.';
  delete pre.dataset.count;
  delete pre.dataset.corrId;
}

function normalizeLedgerMeta(meta){
  if (!meta) return null;
  if (typeof meta === 'string'){
    try{
      const parsed = JSON.parse(meta);
      return (parsed && typeof parsed === 'object' && !Array.isArray(parsed)) ? parsed : null;
    }catch(_){
      return null;
    }
  }
  if (typeof meta === 'object' && !Array.isArray(meta)){
    return meta;
  }
  return null;
}

function updateEgressScopes(egress){
  const container = document.getElementById('egress-scopes');
  const summary = document.getElementById('egress-scopes-summary');
  currentEgressSettings = egress || null;
  if (summary) {
    if (!egress) {
      summary.textContent = '';
    } else {
      const bits = [];
      bits.push(`Posture: ${egress.posture || 'unknown'}`);
      bits.push(`Proxy: ${egress.proxy_enable ? 'enabled' : 'disabled'}`);
      bits.push(`DNS guard: ${egress.dns_guard_enable ? 'enabled' : 'disabled'}`);
      bits.push(`Ledger: ${egress.ledger_enable ? 'enabled' : 'disabled'}`);
      bits.push(`Block IP literals: ${egress.block_ip_literals ? 'on' : 'off'}`);
      summary.textContent = bits.join(' · ');
    }
  }
  if (!container) return;
  if (!egress) {
    container.textContent = 'Scopes unavailable.';
    currentEgressScopes = [];
    scopeCapabilityIndex = new Map();
    window.__scopeCapabilityIndex = scopeCapabilityIndex;
    return;
  }

  const scopes = Array.isArray(egress.scopes) ? egress.scopes.map((s) => ({ ...s })) : [];
  currentEgressScopes = scopes.map((s) => JSON.parse(JSON.stringify(s)));
  scopeCapabilityIndex = new Map();
  currentEgressScopes.forEach((scope) => {
    const label = scopeLabel(scope);
    const caps = Array.isArray(scope.lease_capabilities) ? scope.lease_capabilities : [];
    caps.forEach((cap) => {
      if (!scopeCapabilityIndex.has(cap)) {
        scopeCapabilityIndex.set(cap, { label, id: scope.id || '', description: scope.description || '' });
      }
    });
  });
  window.__scopeCapabilityIndex = scopeCapabilityIndex;

  if (!scopes.length) {
    container.textContent = 'No scopes configured.';
    return;
  }

  const table = document.createElement('table');
  table.className = 'scope-table';
  table.innerHTML = `
    <thead>
      <tr>
        <th>Scope</th>
        <th>Hosts / CIDRs</th>
        <th>Ports</th>
        <th>Protocols</th>
        <th>Lease Caps</th>
        <th>Status</th>
        <th>Actions</th>
      </tr>
    </thead>
    <tbody></tbody>
  `;
  const tbody = table.querySelector('tbody');

  scopes.forEach((scope) => {
    const tr = document.createElement('tr');
    const hosts = Array.isArray(scope.hosts) && scope.hosts.length ? scope.hosts.join(', ') : '';
    const cidrs = Array.isArray(scope.cidrs) && scope.cidrs.length ? scope.cidrs.join(', ') : '';
    const ports = Array.isArray(scope.ports) && scope.ports.length ? scope.ports.join(', ') : '—';
    const protocols = Array.isArray(scope.protocols) && scope.protocols.length ? scope.protocols.join(', ') : '—';
    const leaseCaps = Array.isArray(scope.lease_capabilities) && scope.lease_capabilities.length ? scope.lease_capabilities.join(', ') : '—';
    const expires = typeof scope.expires_at === 'string' && scope.expires_at ? scope.expires_at : '';
    const expired = !!scope.expired;
    const statusText = expired ? 'Expired' : 'Active';
    const label = scopeLabel(scope);

    const hostsLines = [];
    if (hosts) hostsLines.push(escapeHtml(hosts));
    if (cidrs) hostsLines.push(`<div class="dim">CIDRs: ${escapeHtml(cidrs)}</div>`);
    if (!hostsLines.length) hostsLines.push('—');

    const description = typeof scope.description === 'string' ? scope.description.trim() : '';

    tr.innerHTML = `
      <td>
        <div class="scope-label mono">${escapeHtml(label)}</div>
        ${description && description !== label ? `<div class="dim">${escapeHtml(description)}</div>` : ''}
        ${expires ? `<div class="scope-expiry dim">Expires ${escapeHtml(expires)}</div>` : ''}
      </td>
      <td>${hostsLines.join('')}</td>
      <td>${escapeHtml(ports)}</td>
      <td>${escapeHtml(protocols)}</td>
      <td>${escapeHtml(leaseCaps)}</td>
      <td><span class="scope-status ${expired ? 'expired' : 'active'}">${statusText}</span></td>
      <td class="scope-actions"></td>
    `;

    const actionsCell = tr.querySelector('.scope-actions');
    const editBtn = document.createElement('button');
    editBtn.textContent = 'Edit';
    editBtn.className = 'pill';
    editBtn.addEventListener('click', () => openScopeForm('edit', JSON.parse(JSON.stringify(scope))));

    const removeBtn = document.createElement('button');
    removeBtn.textContent = 'Remove';
    removeBtn.className = 'pill';
    removeBtn.addEventListener('click', () => removeScope(scope.id || ''));

    actionsCell.appendChild(editBtn);
    actionsCell.appendChild(removeBtn);
    tbody.appendChild(tr);
  });

  container.innerHTML = '';
  container.appendChild(table);
}

function closeScopeForm(){
  const wrap = document.getElementById('scope-form');
  if (!wrap) return;
  wrap.classList.add('hidden');
  wrap.innerHTML = '';
  delete wrap.dataset.mode;
  delete wrap.dataset.scopeId;
}

function setScopeFormError(message){
  const wrap = document.getElementById('scope-form');
  if (!wrap) return;
  const errorEl = wrap.querySelector('.form-error');
  if (errorEl) errorEl.textContent = message || '';
}

function openScopeForm(mode, scope){
  const wrap = document.getElementById('scope-form');
  if (!wrap) return;
  const isEdit = mode === 'edit';
  const data = scope || {};
  const id = isEdit ? (data.id || '') : '';
  const desc = data.description || '';
  const hosts = Array.isArray(data.hosts) ? data.hosts.join(', ') : '';
  const cidrs = Array.isArray(data.cidrs) ? data.cidrs.join(', ') : '';
  const ports = Array.isArray(data.ports) ? data.ports.join(', ') : '';
  const protocols = Array.isArray(data.protocols) ? data.protocols.join(', ') : '';
  const leaseCaps = Array.isArray(data.lease_capabilities) ? data.lease_capabilities.join(', ') : '';
  const expires = typeof data.expires_at === 'string' ? data.expires_at : '';

  wrap.dataset.mode = mode;
  wrap.dataset.scopeId = id;
  const heading = isEdit ? `Edit Scope (${escapeHtml(scopeLabel(data))})` : 'Add Scope';

  wrap.innerHTML = `
    <form>
      <div class="form-title"><strong>${heading}</strong></div>
      <label>Scope ID
        <input name="id" type="text" value="${escapeAttr(id)}" ${isEdit ? 'readonly' : ''} required>
      </label>
      <label>Description
        <input name="description" type="text" value="${escapeAttr(desc)}" placeholder="Optional">
      </label>
      <label>Hosts (comma separated)
        <textarea name="hosts" placeholder="example.com, api.example.com">${escapeAttr(hosts)}</textarea>
      </label>
      <label>CIDRs (comma separated)
        <textarea name="cidrs" placeholder="10.0.0.0/24">${escapeAttr(cidrs)}</textarea>
      </label>
      <label>Ports (comma separated)
        <input name="ports" type="text" value="${escapeAttr(ports)}" placeholder="443, 8443">
      </label>
      <label>Protocols (http, https, tcp)
        <input name="protocols" type="text" value="${escapeAttr(protocols)}" placeholder="https">
      </label>
      <label>Lease capabilities (comma separated)
        <input name="lease_caps" type="text" value="${escapeAttr(leaseCaps)}" placeholder="net:https">
      </label>
      <label>Expires at (RFC3339)
        <input name="expires_at" type="text" value="${escapeAttr(expires)}" placeholder="2025-12-01T00:00:00Z">
      </label>
      <div class="form-error dim" role="alert"></div>
      <div class="actions">
        <button type="button" data-action="cancel" class="pill">Cancel</button>
        <button type="submit" class="pill primary">${isEdit ? 'Save Changes' : 'Create Scope'}</button>
      </div>
    </form>
  `;

  wrap.classList.remove('hidden');
  const form = wrap.querySelector('form');
  form.addEventListener('submit', handleScopeFormSubmit);
  wrap.querySelector('[data-action="cancel"]').addEventListener('click', (event) => {
    event.preventDefault();
    closeScopeForm();
  });
}

async function handleScopeFormSubmit(event){
  event.preventDefault();
  const form = event.currentTarget;
  const wrap = document.getElementById('scope-form');
  if (!wrap) return;
  const mode = wrap.dataset.mode || 'add';
  const scopeId = wrap.dataset.scopeId || '';

  const submitBtn = form.querySelector('button[type="submit"]');
  const originalLabel = submitBtn.textContent;
  submitBtn.disabled = true;
  submitBtn.textContent = 'Saving…';
  setScopeFormError('');

  try {
    const idInput = form.querySelector('input[name="id"]');
    const id = idInput.value.trim();
    if (!id) throw new Error('Scope id is required');

    const hosts = normalizeHosts(parseListInput(form.querySelector('textarea[name="hosts"]').value));
    const cidrs = Array.from(new Set(parseListInput(form.querySelector('textarea[name="cidrs"]').value)));
    if (hosts.length === 0 && cidrs.length === 0) {
      throw new Error('Provide at least one host or CIDR');
    }
    const ports = parsePortsInput(form.querySelector('input[name="ports"]').value);
    const protocols = normalizeProtocols(parseListInput(form.querySelector('input[name="protocols"]').value));
    const leaseCaps = Array.from(new Set(parseListInput(form.querySelector('input[name="lease_caps"]').value)));
    const description = form.querySelector('input[name="description"]').value.trim();
    const expires = form.querySelector('input[name="expires_at"]').value.trim();

    const scopeObj = {
      id,
      hosts,
      cidrs,
    };
    if (description) scopeObj.description = description;
    if (ports.length) scopeObj.ports = ports;
    if (protocols.length) scopeObj.protocols = protocols;
    if (leaseCaps.length) scopeObj.lease_capabilities = leaseCaps;
    if (expires) scopeObj.expires_at = expires;

    const scopes = currentEgressScopes.map((s) => JSON.parse(JSON.stringify(s)));
    if (mode === 'edit') {
      const index = scopes.findIndex((s) => (s.id || '') === scopeId);
      if (index === -1) throw new Error(`Scope '${scopeId}' not found`);
      scopes[index] = scopeObj;
      await saveScopes(scopes, `Scope '${scopeId}' updated.`);
    } else {
      if (scopes.some((s) => (s.id || '') === id)) {
        throw new Error(`Scope '${id}' already exists`);
      }
      scopes.push(scopeObj);
      await saveScopes(scopes, `Scope '${id}' created.`);
    }
    closeScopeForm();
  } catch (err) {
    console.error(err);
    setScopeFormError(err?.message || 'Unable to save scope');
    submitBtn.disabled = false;
    submitBtn.textContent = originalLabel;
    return;
  }
}

async function saveScopes(scopes, successMessage){
  try {
    const updated = await postAdminJson('egress/settings', { scopes });
    const payload = updated?.egress ? updated.egress : updated?.data?.egress;
    if (payload) {
      updateEgressScopes(payload);
      ARW.toast(successMessage || 'Scopes updated.');
    }
  } catch (err) {
    console.error(err);
    ARW.toast(err?.message || 'Failed to update scopes');
    throw err;
  }
}

async function removeScope(id){
  const target = String(id || '').trim();
  if (!target) return;
  const label = scopeLabel(currentEgressScopes.find((s) => (s.id || '') === target) || { id: target });
  const confirmed = window.confirm(`Remove scope '${label}'?`);
  if (!confirmed) return;
  const scopes = currentEgressScopes
    .filter((s) => (s.id || '') !== target)
    .map((s) => JSON.parse(JSON.stringify(s)));
  try {
    await saveScopes(scopes, `Scope '${label}' removed.`);
  } catch (_) {
    // Error already surfaced via toast
  }
}

async function loadEgressScopes() {
  try{
    const data = await fetchAdminJson('state/egress/settings');
    if (data && data.egress) {
      updateEgressScopes(data.egress);
    } else {
      updateEgressScopes(null);
    }
  }catch(err){
    console.error(err);
    const container = document.getElementById('egress-scopes');
    if (container) container.textContent = 'Unable to load scopes.';
  }
}

function describeLedgerEntry(entry){
  if (!entry || typeof entry !== 'object') return '';
  const ts = entry.timestamp || entry.created_at || entry.time || '';
  const decision = entry.decision || entry.allow || entry.result || 'unknown';
  const host = entry.host || entry.url || entry.target || '';
  const code = entry.reason || entry.code || entry.status || '';
  const bytes = typeof entry.bytes === 'number' ? entry.bytes : (typeof entry.total_bytes === 'number' ? entry.total_bytes : null);
  const meta = normalizeLedgerMeta(entry.meta);
  const allowedVia = entry.allowed_via || (meta && typeof meta.allowed_via === 'string' ? meta.allowed_via : null);
  const scopeMeta = meta && typeof meta.policy_scope === 'object' && meta.policy_scope !== null ? meta.policy_scope : null;
  const leaseCaps = Array.isArray((scopeMeta && scopeMeta.lease_capabilities) || meta?.scope_lease_caps)
    ? ((scopeMeta && scopeMeta.lease_capabilities) || meta.scope_lease_caps)
    : null;
  const parts = [];
  parts.push(ts ? `[${ts}]` : '[—]');
  parts.push(String(decision));
  if (code) parts.push(`(${code})`);
  if (host) parts.push(host);
  if (bytes != null) parts.push(`${bytesHuman(bytes)}`);
  if (allowedVia) parts.push(`via:${allowedVia}`);
  if (scopeMeta){
    const idLabel = typeof scopeMeta.id === 'string' ? scopeMeta.id.trim() : '';
    const descLabel = typeof scopeMeta.description === 'string' ? scopeMeta.description.trim() : '';
    const scopeLabel = idLabel || descLabel || 'scope';
    let scopeText = `scope:${scopeLabel}`;
    if (typeof scopeMeta.expires_at === 'string' && scopeMeta.expires_at){
      scopeText += `→${scopeMeta.expires_at}`;
    }
    parts.push(scopeText);
  }
  if (leaseCaps && leaseCaps.length){
    parts.push(`lease:${leaseCaps.join('|')}`);
  }
  return parts.join(' ');
}

async function previewLedger(corrId, opts = {}){
  const pre = document.getElementById('ledger-preview');
  if (!pre) return;
  const corr = corrId || window.__lastLedgerCorr || '';
  window.__lastLedgerCorr = corr || '';
  try{
    pre.dataset.locked = 'true';
    if (!opts.silent) pre.textContent = corr ? `Loading ledger entries for ${corr}…` : 'Loading recent ledger entries…';
    const data = await fetchAdminJson('state/egress?limit=200');
    if (data && data.settings && data.settings.egress){
      updateEgressScopes(data.settings.egress);
    }
    const items = Array.isArray(data.items) ? data.items : [];
    const filtered = corr ? items.filter(it => String(it.corr_id||'') === corr) : items;
    if (!filtered.length){
      pre.textContent = corr ? `No ledger entries for corr_id ${corr}.` : 'No ledger entries available yet.';
      pre.dataset.count = '0';
      pre.dataset.corrId = corr;
      pre.dataset.locked = 'false';
      return;
    }
    const lines = filtered.slice(0, 6).map(describeLedgerEntry).filter(Boolean);
    pre.textContent = lines.join('\n') + (filtered.length > lines.length ? '\n…' : '');
    pre.dataset.count = String(filtered.length);
    pre.dataset.corrId = corr;
  }catch(e){
    console.error(e);
    pre.textContent = 'Ledger preview unavailable.';
  } finally {
    pre.dataset.locked = 'false';
  }
}

// Initialize toggles
document.addEventListener('DOMContentLoaded', ()=>{
  const auto = document.getElementById('jobs-auto');
  if (auto) auto.addEventListener('change', (e)=> setJobsAuto(!!e.target.checked));
  const addScopeBtn = document.getElementById('btn-scope-add');
  if (addScopeBtn) addScopeBtn.addEventListener('click', () => openScopeForm('add'));
  loadEgressScopes().catch(err => console.error(err));
});
