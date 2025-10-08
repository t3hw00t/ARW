const invoke = (cmd, args) => ARW.invoke(cmd, args);
let lastJson = null;
let routeStatsSubId = null;
let probeMetricsSubId = null;
let sseIndicatorHandle = null;
const MAX_TAIL_LINES = 400;
const tailEntries = [];
let tailListener = null;
let tailLogPath = null;
const updateBaseMeta = () => ARW.applyBaseMeta({ portInputId: 'port', badgeId: 'baseBadge', label: 'Base' });
function bytesHuman(n){ if(!n && n!==0) return '–'; const kb=1024, mb=kb*1024, gb=mb*1024, tb=gb*1024; if(n>=tb) return (n/tb).toFixed(2)+' TiB'; if(n>=gb) return (n/gb).toFixed(2)+' GiB'; if(n>=mb) return (n/mb).toFixed(1)+' MiB'; if(n>=kb) return (n/kb).toFixed(1)+' KiB'; return n+' B'; }
function setCpuBadge(p){ try{ const el=document.getElementById('cpuBadge'); if(!el) return; const v = Number(p)||0; el.textContent = 'CPU: ' + v.toFixed(1) + '%'; el.className = 'badge ' + (v>=90? 'bad' : v>=75? 'warn':''); }catch{} }
function setMemBadge(used,total){ try{ const el=document.getElementById('memBadge'); if(!el) return; const pct = total>0? (100*used/total):0; el.textContent = 'Mem: ' + pct.toFixed(1) + '% ('+bytesHuman(used)+'/'+bytesHuman(total)+')'; el.className = 'badge ' + (pct>=90? 'bad' : pct>=75? 'warn':''); }catch{} }
function setGpuBadge(used,total){ try{ const el=document.getElementById('gpuBadge'); if(!el) return; const pct = total>0? (100*used/total):0; el.textContent = 'GPU: ' + pct.toFixed(1) + '%'; el.className = 'badge ' + (pct>=95? 'bad' : pct>=80? 'warn':''); }catch{} }
function tableRoutes(routes){
  const by = routes?.by_path || {};
  const filt = (document.getElementById('routeFilter')?.value||'').toLowerCase();
  const rows = Object.entries(by)
    .map(([p,s])=>({ p, hits:s.hits||0, p95:s.p95_ms||0, ewma:s.ewma_ms||0, max:s.max_ms||0 }))
    .filter(r=> !filt || r.p.toLowerCase().includes(filt))
    .sort((a,b)=> b.hits - a.hits)
    .slice(0, 12);
  const t = document.createElement('table'); t.className='cmp-table';
  const sloEl = document.getElementById('slo'); const slo = sloEl? (parseInt(sloEl.value,10)||150) : 150;
  t.innerHTML = `<thead><tr><th>route</th><th>hits</th><th>p95 ≤ ${slo}</th><th>ewma</th><th>max</th></tr></thead>`;
  const tb = document.createElement('tbody');
  for (const r of rows){
    const tr = document.createElement('tr'); const cls = r.p95 <= slo ? 'ok':'';
    tr.innerHTML = `<td class="mono">${r.p}</td><td>${r.hits}</td><td class="${cls}">${r.p95}</td><td>${r.ewma.toFixed? r.ewma.toFixed(1): r.ewma}</td><td>${r.max}</td>`;
    tb.appendChild(tr);
  }
  t.appendChild(tb); return t;
}
function tableKinds(ev){
  const kinds = ev?.kinds || {};
  const rows = Object.entries(kinds).sort((a,b)=> b[1]-a[1]).slice(0,12);
  const t = document.createElement('table'); t.className='cmp-table';
  t.innerHTML = '<thead><tr><th>kind</th><th>count</th></tr></thead>';
  const tb = document.createElement('tbody');
  for (const [k,c] of rows){ const tr=document.createElement('tr'); tr.innerHTML = `<td class="mono">${k}</td><td>${c}</td>`; tb.appendChild(tr); }
  t.appendChild(tb); return t;
}
function ensureSseIndicator() {
  const wrap = document.getElementById('statusBadges');
  if (!wrap) return;
  if (sseIndicatorHandle) return;
  let badge = document.getElementById('logsSseBadge');
  if (!badge) {
    badge = document.createElement('span');
    badge.id = 'logsSseBadge';
    badge.className = 'badge';
    wrap.appendChild(badge);
  }
  sseIndicatorHandle = ARW.sse.indicator(badge, { prefix: 'SSE' });
}

function connectSse({ replay = 25, resume = false } = {}) {
  ensureSseIndicator();
  const meta = updateBaseMeta();
  ARW.sse.connect(meta.base, { replay, prefix: ['state.read.model.patch', 'probe.metrics'] }, resume);
}

function normalizeTailEntry(raw) {
  const line = typeof raw?.line === 'string' ? raw.line : '';
  const stream =
    typeof raw?.stream === 'string' && raw.stream ? raw.stream : 'stdout';
  const ts =
    typeof raw?.timestamp === 'number' && Number.isFinite(raw.timestamp)
      ? raw.timestamp
      : Date.now() / 1000;
  return { line, stream, timestamp: ts };
}

function followTailEnabled() {
  const el = document.getElementById('tailFollow');
  return !el || !!el.checked;
}

function setLogPath(path) {
  const resolved =
    typeof path === 'string' && path.trim().length > 0 ? path.trim() : null;
  tailLogPath = resolved;
  const location = document.getElementById('tailLocation');
  if (location) {
    location.textContent = resolved
      ? `Log file: ${resolved}`
      : 'Log file not available yet';
  }
  const openBtn = document.getElementById('btn-tail-open');
  if (openBtn) openBtn.disabled = !resolved;
}

function ensureTailPlaceholder() {
  const wrap = document.getElementById('logTail');
  if (!wrap) return;
  if (!tailEntries.length) {
    wrap.dataset.empty = 'true';
    wrap.innerHTML = '<div class="empty">No output yet.</div>';
  } else {
    wrap.dataset.empty = 'false';
    wrap.innerHTML = '';
  }
}

function formatTailTime(timestamp) {
  const date = Number.isFinite(timestamp)
    ? new Date(timestamp * 1000)
    : new Date();
  return date.toLocaleTimeString([], {
    hour: '2-digit',
    minute: '2-digit',
    second: '2-digit',
  });
}

function createTailLine(entry) {
  const row = document.createElement('div');
  row.className = 'tail-line';
  const time = document.createElement('span');
  time.className = 'time';
  time.textContent = formatTailTime(entry.timestamp);
  const stream = document.createElement('span');
  const slug = entry.stream || 'stdout';
  stream.className = `stream ${slug}`;
  stream.textContent = slug.toUpperCase();
  const text = document.createElement('span');
  text.className = 'text';
  text.textContent = entry.line || '';
  row.appendChild(time);
  row.appendChild(stream);
  row.appendChild(text);
  return row;
}

function renderTailFromHistory() {
  const wrap = document.getElementById('logTail');
  if (!wrap) return;
  ensureTailPlaceholder();
  if (!tailEntries.length) return;
  const frag = document.createDocumentFragment();
  tailEntries.forEach((entry) => {
    frag.appendChild(createTailLine(entry));
  });
  wrap.appendChild(frag);
  if (followTailEnabled()) {
    wrap.scrollTop = wrap.scrollHeight;
  }
}

function appendTailEntry(entry) {
  const wrap = document.getElementById('logTail');
  if (!wrap) return;
  if (wrap.dataset.empty === 'true') {
    wrap.innerHTML = '';
    wrap.dataset.empty = 'false';
  }
  wrap.appendChild(createTailLine(entry));
  while (wrap.childElementCount > MAX_TAIL_LINES) {
    wrap.removeChild(wrap.firstChild);
  }
  if (followTailEnabled()) {
    wrap.scrollTop = wrap.scrollHeight;
  }
}

function pushTailEntry(entry) {
  tailEntries.push(entry);
  if (tailEntries.length > MAX_TAIL_LINES) {
    tailEntries.shift();
  }
  appendTailEntry(entry);
}

async function preloadTailHistory() {
  try {
    const history = await invoke('launcher_recent_service_logs', {
      limit: MAX_TAIL_LINES,
    });
    tailEntries.length = 0;
    if (Array.isArray(history)) {
      history.forEach((raw) => {
        tailEntries.push(normalizeTailEntry(raw));
      });
    }
    renderTailFromHistory();
  } catch (err) {
    console.warn('tail history load failed', err);
    tailEntries.length = 0;
    ensureTailPlaceholder();
  }
}

async function subscribeToTail() {
  if (!window.__TAURI__ || !window.__TAURI__.event) return;
  tailListener = await window.__TAURI__.event.listen(
    'launcher://service-log',
    ({ payload }) => {
      if (!payload) return;
      if (payload.path) {
        setLogPath(payload.path);
      }
      const entry = normalizeTailEntry(payload);
      pushTailEntry(entry);
    },
  );
}

function disposeTailListener() {
  if (typeof tailListener === 'function') {
    tailListener();
    tailListener = null;
  }
}

async function fetchLogPath({ toastOnError = false } = {}) {
  try {
    const path = await invoke('launcher_service_log_path');
    setLogPath(path);
    return tailLogPath;
  } catch (err) {
    console.error(err);
    setLogPath(null);
    if (toastOnError) {
      ARW.toast('Unable to resolve log path');
    }
    return null;
  }
}

function autoEnabled(){
  const el = document.getElementById('auto');
  return !el || !!el.checked;
}

function applyRouteStatsModel(model){
  lastJson = model || {};
  if (autoEnabled()) render(lastJson);
}

function handleProbeMetrics({ env }){
  try {
    const payload = env?.payload || env || {};
    const data = payload?.data || payload;
    const cpu = data?.cpu?.avg || 0;
    setCpuBadge(cpu);
    const mu = data?.memory?.used || 0;
    const mt = data?.memory?.total || 0;
    setMemBadge(mu, mt);
    const gpus = Array.isArray(data?.gpus) ? data.gpus : [];
    let total = 0, used = 0;
    for (const gpu of gpus) {
      total += Number(gpu?.mem_total || 0);
      used += Number(gpu?.mem_used || 0);
    }
    setGpuBadge(used, total);
  } catch {}
}

async function fetchRouteStatsSnapshot({ renderNow = false } = {}) {
  const out = document.getElementById('out');
  if (renderNow && out) out.innerHTML = '';
  const statEl = document.getElementById('stat');
  if (statEl) statEl.textContent = 'Loading…';
  const meta = updateBaseMeta();
  const baseUrl = meta.base;
  try {
    const snapshot = await ARW.metrics.routeStats({ base: baseUrl });
    lastJson = snapshot;
    if (!autoEnabled() || renderNow) render(snapshot);
    if (statEl) statEl.textContent = 'OK';
  } catch (e) {
    if (statEl) statEl.textContent = 'Error';
    if (renderNow && out) {
      const pre = document.createElement('pre');
      pre.textContent = String(e);
      out.appendChild(pre);
    }
  }
}
function render(j){
  const out = document.getElementById('out'); out.innerHTML='';
  const focus = document.getElementById('focus').checked;
  const kv = document.createElement('div');
  kv.className = 'kv';
  const routesCount = Object.keys((j.routes&&j.routes.by_path)||{}).length;
  const kindsCount = Object.keys((j.events&&j.events.kinds)||{}).length;
  kv.innerHTML = `
    <div class="k">Routes</div><div>${routesCount}</div>
    <div class="k">Event kinds</div><div>${kindsCount}</div>
    <div class="k">Bus published</div><div>${(j.bus&&j.bus.published)||0}</div>
    <div class="k">Bus delivered</div><div>${(j.bus&&j.bus.delivered)||0}</div>
  `;
  out.appendChild(kv);
  const cols = document.createElement('div'); cols.className='grid cols-2';
  const col1 = document.createElement('div'); const h1=document.createElement('h3'); h1.textContent='Top routes';
  const tools=document.createElement('div'); tools.className='row'; const btnCsv=document.createElement('button'); btnCsv.className='ghost'; btnCsv.textContent='Export CSV'; btnCsv.addEventListener('click', ()=> exportRoutesCsv(j.routes)); tools.appendChild(btnCsv);
  col1.appendChild(h1); col1.appendChild(tools); col1.appendChild(tableRoutes(j.routes));
  const col2 = document.createElement('div'); const h2=document.createElement('h3'); h2.textContent='Top event kinds'; const tools2=document.createElement('div'); tools2.className='row'; const btnCsv2=document.createElement('button'); btnCsv2.className='ghost'; btnCsv2.textContent='Export CSV'; btnCsv2.addEventListener('click', ()=> exportKindsCsv(j.events)); tools2.appendChild(btnCsv2);
  col2.appendChild(h2); col2.appendChild(tools2); col2.appendChild(tableKinds(j.events));
  cols.appendChild(col1); cols.appendChild(col2); out.appendChild(cols);
  if (!focus){
    const pre = document.createElement('pre'); pre.style.whiteSpace = document.getElementById('wrap').checked ? 'pre-wrap' : 'pre';
    pre.textContent = JSON.stringify(j, null, 2);
    out.appendChild(pre);
  }
}

function downloadCsv(filename, rows){
  const csv = rows.map(r => r.map(v => /[",\n]/.test(String(v)) ? '"'+String(v).replace(/"/g,'""')+'"' : v).join(',')).join('\n');
  const blob = new Blob([csv], { type: 'text/csv;charset=utf-8;' });
  const link = document.createElement('a'); link.href = URL.createObjectURL(blob); link.download = filename; document.body.appendChild(link); link.click(); document.body.removeChild(link);
}
function exportRoutesCsv(routes){ const by = routes?.by_path || {}; const rows = [['route','hits','p95','ewma','max']]; Object.entries(by).forEach(([p,s])=> rows.push([p, s.hits||0, s.p95_ms||0, s.ewma_ms||0, s.max_ms||0])); downloadCsv('routes.csv', rows); }
function exportKindsCsv(ev){ const kinds = ev?.kinds || {}; const rows = [['kind','count']]; Object.entries(kinds).forEach(([k,c])=> rows.push([k,c])); downloadCsv('event_kinds.csv', rows); }
document.addEventListener('DOMContentLoaded', () => {
  updateBaseMeta();
  ensureTailPlaceholder();
  document.getElementById('btn-refresh').addEventListener('click', () => fetchRouteStatsSnapshot({ renderNow: true }));
  document.getElementById('wrap').addEventListener('change', ()=> render(lastJson||{}));
  document.getElementById('auto').addEventListener('change', ()=>{ if (autoEnabled() && lastJson) render(lastJson); });
  document.getElementById('focus').addEventListener('change', ()=> render(lastJson||{}));
  document.getElementById('routeFilter').addEventListener('input', ()=> render(lastJson||{}));
  document.getElementById('btn-copy').addEventListener('click', ()=>{ if (lastJson) ARW.copy(JSON.stringify(lastJson, null, 2)); });
  const tailClear = document.getElementById('btn-tail-clear');
  if (tailClear) {
    tailClear.addEventListener('click', () => {
      tailEntries.length = 0;
      ensureTailPlaceholder();
    });
  }
  const tailCopy = document.getElementById('btn-tail-copy');
  if (tailCopy) {
    tailCopy.addEventListener('click', async () => {
      if (!tailEntries.length) {
        ARW.toast('No log lines to copy');
        return;
      }
      const slice = tailEntries.slice(-100).map((entry) => entry.line || '');
      try {
        await navigator.clipboard.writeText(slice.join('\n'));
        ARW.toast('Copied');
      } catch (err) {
        console.error(err);
        ARW.toast('Copy failed');
      }
    });
  }
  const tailOpen = document.getElementById('btn-tail-open');
  if (tailOpen) {
    tailOpen.addEventListener('click', async () => {
      const path = await fetchLogPath({ toastOnError: true });
      if (!path) return;
      try {
        await invoke('open_path', { path });
      } catch (err) {
        console.error(err);
        ARW.toast('Unable to open log file');
      }
    });
  }
  const tailFollow = document.getElementById('tailFollow');
  if (tailFollow) {
    tailFollow.addEventListener('change', () => {
      if (tailFollow.checked) {
        const wrap = document.getElementById('logTail');
        if (wrap) wrap.scrollTop = wrap.scrollHeight;
      }
    });
  }
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
    connectSse({ replay: 25, resume: false });
    await fetchRouteStatsSnapshot({ renderNow: true });
  };
  const portInput = document.getElementById('port');
  if (portInput) portInput.addEventListener('change', () => {
    rebindBase().catch(() => {});
  });
  (async () => {
    await fetchLogPath();
    await preloadTailHistory();
    await subscribeToTail();
    await ARW.applyPortFromPrefs('port');
    updateBaseMeta();
    connectSse({ replay: 25, resume: false });
    if (routeStatsSubId) ARW.read.unsubscribe(routeStatsSubId);
    routeStatsSubId = ARW.read.subscribe('route_stats', applyRouteStatsModel);
    if (probeMetricsSubId) ARW.sse.unsubscribe(probeMetricsSubId);
    probeMetricsSubId = ARW.sse.subscribe('probe.metrics', handleProbeMetrics);
    await fetchRouteStatsSnapshot({ renderNow: true });
  })();
  window.addEventListener('arw:base-override-changed', () => {
    rebindBase().catch(() => {});
    fetchLogPath().catch(() => {});
  });
});

// Keyboard shortcuts (ignore when typing)
window.addEventListener('keydown', (e)=>{
  const tag=(e.target && e.target.tagName || '').toLowerCase();
  if (tag==='input' || tag==='textarea' || tag==='select') return;
  if (e.ctrlKey || e.metaKey || e.altKey) return;
  const btn = (id)=> document.getElementById(id);
  if (e.key.toLowerCase()==='r'){ e.preventDefault(); btn('btn-refresh')?.click(); }
  else if (e.key.toLowerCase()==='w'){ e.preventDefault(); const el=document.getElementById('wrap'); if (el){ el.checked=!el.checked; render(lastJson||{}); } }
  else if (e.key.toLowerCase()==='a'){ e.preventDefault(); const el=document.getElementById('auto'); if (el){ el.checked=!el.checked; if (el.checked) fetchRouteStatsSnapshot({ renderNow: true }); } }
});

window.addEventListener('beforeunload', () => {
  disposeTailListener();
});
