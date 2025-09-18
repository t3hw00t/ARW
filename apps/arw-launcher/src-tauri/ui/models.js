const port = () => ARW.getPortFromInput('port');
const __BASE = (()=>{ try{ return window.__ARW_BASE_OVERRIDE ? String(window.__ARW_BASE_OVERRIDE).replace(/\/$/,'') : null; }catch{ return null } })();

async function ivk(cmd, args){
  if (!__BASE) return ARW.invoke(cmd, args);
  const tok = await ARW.connections.tokenFor(__BASE);
  const get = (p)=> ARW.invoke('admin_get_json_base', { base: __BASE, path: p, token: tok });
  const post = (p, body)=> ARW.invoke('admin_post_json_base', { base: __BASE, path: p, body: body||{}, token: tok });
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
      return ARW.invoke('admin_get_json_base', { base: __BASE, path, token: null });
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

async function loadPrefs() {
  await ARW.applyPortFromPrefs('port');
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
}

async function refresh() {
  document.getElementById('stat').textContent = 'Loading...';
  const sum = await ivk('models_summary', { port: port() });
  const def = (sum && sum.default) || '';
  document.getElementById('def').textContent = `Default: ${def || '(none)'}`;
  // Concurrency + metrics line
  try{
    const c = sum.concurrency || {};
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
    const pbtn = (!__BASE && m.path? `<button data-open="${m.path}" title="Open path">Open</button>` : '');
    tr.innerHTML = `<td class="mono">${id}</td><td>${m.provider||''}</td><td>${m.status||''}</td><td class="mono">${m.path||''} ${pbtn}</td><td>${(id===def)?'★':''}</td><td><button data-id="${id}" title="Set as default">Make Default</button></td>`;
    const defbtn = tr.querySelector('button[data-id]');
    if (defbtn) defbtn.addEventListener('click', async (e)=>{
      await ivk('models_default_set', { id: e.target.getAttribute('data-id'), port: port() });
      refresh();
    });
    try {
      if ((String(m.status||'').toLowerCase()) === 'downloading'){
        const tds = tr.querySelectorAll('td');
        const actions = tds[tds.length-1];
        const btn = document.createElement('button');
        btn.textContent = 'Cancel'; btn.title='Cancel download';
        btn.addEventListener('click', async ()=>{ await ivk('models_download_cancel', { id, port: port() }); });
        actions.appendChild(document.createTextNode(' '));
        actions.appendChild(btn);
      }
    } catch {}
    const op = tr.querySelector('[data-open]');
    if (op) op.addEventListener('click', async (e)=>{ if (!__BASE) await ivk('open_path', { path: e.target.getAttribute('data-open') }); });
    fragModels.appendChild(tr);
  });
  tb.appendChild(fragModels);
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

function sse() {
  const p = port() || 8091;
  const es = new EventSource(ARW.base(p) + '/events?prefix=models.');
  const last = {}; // id -> { t: ms, bytes: number }
  let lastJobsAt = 0;
  if (!window.__etaMap) window.__etaMap = {};
  es.onmessage = (ev) => {
    try {
      const j = JSON.parse(ev.data);
      if (j.kind && j.kind.startsWith('models.')) {
        if (j.kind === 'models.download.progress') {
          const pl = j.payload || {};
          document.getElementById('dlprog').textContent = JSON.stringify(pl, null, 2);
          const id = pl.id || '';
          if (id){
            const row=ensureBar(id);
            const bar=row.querySelector('[data-bar]');
            const txt=row.querySelector('[data-text]');
            const pct=pl.progress||0;
            if(bar) bar.style.width=(pct||0)+'%';
            const dled=pl.downloaded||0;
            const dledTxt=bytesHuman(dled);
            const tot=pl.total? bytesHuman(pl.total) : null;
            const now = Date.now();
            let tail = '';
            try{
              const prev = last[id];
              last[id] = { t: now, bytes: dled };
              if (prev && dled >= prev.bytes){
                const dt = Math.max(1, now - prev.t) / 1000;
                const db = dled - prev.bytes;
                const rate = db / dt;
                const mpers = rate / (1024*1024);
                if (mpers > 0.01){
                  tail += ` · speed: ${mpers.toFixed(2)} MiB/s`;
                  if (pl.total && dled < pl.total){
                    const rem = pl.total - dled;
                    const etaSec = Math.max(0, Math.floor(rem / Math.max(1, rate)));
                    const mm = Math.floor(etaSec/60).toString().padStart(2,'0');
                    const ss = (etaSec%60).toString().padStart(2,'0');
                    tail += ` · ETA: ${mm}:${ss}`;
                    window.__etaMap[id] = `${mm}:${ss}`;
                  }
                }
              }
            }catch{}
            if (pl.budget && (pl.budget.spent_ms!=null || pl.budget.remaining_hard_ms!=null)){
              const spent = pl.budget.spent_ms||0;
              const rem = pl.budget.remaining_hard_ms;
              const spentS = (spent/1000).toFixed(1);
              const remS = (typeof rem==='number') ? (rem/1000).toFixed(1) : '∞';
              tail += ` · budget: ${spentS}s/${remS}s`;
            }
            const baseTxt = pl.total? `${dledTxt}/${tot} (${pct}%)` : (pl.status||'');
            const disk = pl.disk? ` · Disk: ${bytesHuman(pl.disk.available)} free / ${bytesHuman(pl.disk.total)} total` : '';
            const iconHtml = iconsFor(pl.status, pl.code);
            if(txt) txt.innerHTML = (iconHtml ? `<span class="icons">${iconHtml}</span>` : '') + baseTxt + tail + disk;
            if(pl.disk){
              const el=document.getElementById('disk');
              if(el) el.textContent = `Disk: ${bytesHuman(pl.disk.available)} free / ${bytesHuman(pl.disk.total)} total`;
            }
            if(pl.status==='complete'){
              setTimeout(async()=>{
                await refresh();
                const list=await ivk('models_list',{port:port()});
                const found=(list||[]).find(mm=>mm.id===id);
                if(!__BASE && found&&found.path){ try{ await ivk('open_path',{path:found.path}); }catch(e){} }
                removeBar(id);
                try{ delete window.__etaMap[id]; }catch{}
              }, 250);
            }
          }
          // Lightly refresh jobs snapshot at most once a second during progress
          const nowMs = Date.now();
          if (nowMs - lastJobsAt > 1000) { lastJobsAt = nowMs; jobsRefresh(); }
          try{
            const pl = j.payload || {};
            if (pl.status === 'complete' && pl.id) {
              (async ()=>{
                await refresh();
                const list2 = await ivk('models_list', { port: port() });
                const found = (list2||[]).find(mm => mm.id === pl.id);
                if (found && found.path) { try { await ivk('open_path', { path: found.path }); } catch(e){} }
              })();
            }
          }catch{}
        }
        if (j.kind === 'models.changed' || j.kind === 'models.refreshed') {
          refresh();
          jobsRefresh();
        }
      }
    } catch {}
  };
}

document.addEventListener('DOMContentLoaded', () => {
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
      const changed = (res && (res.changed===true));
      const newv = (res && res.new!=null) ? res.new : max;
      const heldRel = res && res.held_released || 0;
      const heldAcq = res && res.held_acquired || 0;
      const pend = res && res.pending_shrink || 0;
      const avail = res && res.available_permits;
      const note = `→ ${newv} (${changed?'changed':'noop'}) · released ${heldRel}, acquired ${heldAcq}, pending ${pend}, avail ${avail}`;
      ARW.toast(`Concurrency ${note}`);
      // Update status line with new snapshot
      await refresh();
      // Persist concBlock preference
      try{ const p = await ARW.getPrefs('launcher') || {}; p.concBlock = block; await ARW.setPrefs('launcher', p); }catch{}
    }catch(e){ console.error(e); ARW.toast('Failed to set concurrency'); }
  });
  // Blocking shrink (finish pending immediately)
  const shrinkBtn = document.getElementById('btn-conc-shrink');
  if (shrinkBtn) shrinkBtn.addEventListener('click', async ()=>{
    try{
      const max = parseInt(document.getElementById('concmax').value||'2', 10);
      const res = await ivk('models_concurrency_set', { max, block: true, port: port() });
      const note = `→ ${res.new||max} (blocking); released ${res.held_released||0}, acquired ${res.held_acquired||0}`;
      ARW.toast(`Concurrency ${note}`);
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
  if (prevBtn) prevBtn.addEventListener('click', ()=>{ window.__hashOffset = Math.max(0, (window.__hashOffset|0) - (parseInt(document.getElementById('hash-limit').value||'50',10)||50)); hashesRefresh(); });
  const nextBtn = document.getElementById('btn-hashes-next');
  if (nextBtn) nextBtn.addEventListener('click', ()=>{ const lim = parseInt(document.getElementById('hash-limit').value||'50',10)||50; window.__hashOffset = (window.__hashOffset|0) + lim; hashesRefresh(); });
  const firstBtn = document.getElementById('btn-hashes-first'); if (firstBtn) firstBtn.addEventListener('click', ()=>{ window.__hashOffset = 0; hashesRefresh(); });
  const lastBtn = document.getElementById('btn-hashes-last'); if (lastBtn) lastBtn.addEventListener('click', ()=>{ const lim = parseInt(document.getElementById('hash-limit').value||'50',10)||50; const stat = document.getElementById('hash-page-stat'); const tot = stat? parseInt(stat.getAttribute('data-total')||'0',10) : 0; const pages = Math.max(1, Math.ceil(tot/Math.max(1,lim))); window.__hashOffset = (pages-1)*lim; hashesRefresh(); });
  const pageInput = document.getElementById('hash-page'); if (pageInput) pageInput.addEventListener('change', ()=>{ const lim = parseInt(document.getElementById('hash-limit').value||'50',10)||50; const val = Math.max(1, parseInt(pageInput.value||'1',10)||1); window.__hashOffset = (val-1)*lim; hashesRefresh(); });
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
    hashesRefresh();
  });
  // Persist and react to control changes
  const autoEl = document.getElementById('jobs-auto'); if (autoEl) autoEl.addEventListener('change', async (e)=>{
    setJobsAuto(!!e.target.checked);
    try{ const p = await ARW.getPrefs('launcher') || {}; p.jobsAuto = !!e.target.checked; await ARW.setPrefs('launcher', p); }catch{}
  });
  const hp = document.getElementById('hash-prov'); if (hp) hp.addEventListener('change', async (e)=>{ try{ const p = await ARW.getPrefs('launcher')||{}; p.hashProvider = e.target.value||''; await ARW.setPrefs('launcher', p);}catch{}; hashesRefresh(); });
  const hs = document.getElementById('hash-sort'); if (hs) hs.addEventListener('change', async (e)=>{ try{ const p = await ARW.getPrefs('launcher')||{}; p.hashSort = e.target.value||'bytes'; await ARW.setPrefs('launcher', p);}catch{}; hashesRefresh(); });
  const ho = document.getElementById('hash-order'); if (ho) ho.addEventListener('change', async (e)=>{ try{ const p = await ARW.getPrefs('launcher')||{}; p.hashOrder = e.target.value||'desc'; await ARW.setPrefs('launcher', p);}catch{}; hashesRefresh(); });
  const hl = document.getElementById('hash-limit'); if (hl) hl.addEventListener('change', async (e)=>{ const n = parseInt(e.target.value||'50',10)||50; e.target.value=n; try{ const p = await ARW.getPrefs('launcher')||{}; p.hashLimit=n; await ARW.setPrefs('launcher', p);}catch{}; hashesRefresh(); });
  const cb = document.getElementById('concblock'); if (cb) cb.addEventListener('change', async (e)=>{ try{ const p = await ARW.getPrefs('launcher')||{}; p.concBlock = !!e.target.checked; await ARW.setPrefs('launcher', p);}catch{}; });
  (async ()=>{ await loadPrefs(); await refresh(); await hashesRefresh(); await jobsRefresh(); sse(); })();
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
    const page = await ivk('state_models_hashes', { limit, offset, provider: prov, sort, order, port: port() });
    const tb = document.getElementById('hashes'); tb.innerHTML = '';
    const fragHashes = document.createDocumentFragment();
    (page.items||[]).forEach(it => {
      const tr = document.createElement('tr');
      const provs = (it.providers||[]).join(', ');
      const pbtn = (it.path? ` <button data-open="${it.path}">Open</button>` : '');
      tr.innerHTML = `<td class="mono">${it.sha256||''}</td><td>${bytesHuman(it.bytes||0)}</td><td class="mono">${it.path||''}${pbtn}</td><td>${provs}</td>`;
      const op = tr.querySelector('[data-open]');
      if (op) op.addEventListener('click', async (e)=>{ await ivk('open_path', { path: e.target.getAttribute('data-open') }); });
      fragHashes.appendChild(tr);
    });
    tb.appendChild(fragHashes);
    // Update page stat and prev/next
    const stat = document.getElementById('hash-page-stat');
    const cur = page.offset||0; const tot = page.total||0; const lim = page.limit||limit;
    const curPage = Math.floor(cur/Math.max(1,lim)) + 1;
    const totalPages = Math.max(1, Math.ceil((tot||0)/Math.max(1,lim)));
    if (stat){ stat.textContent = `Showing ${cur}–${cur+(page.count||0)} of ${tot} · Page ${curPage}/${totalPages}`; stat.setAttribute('data-total', String(tot||0)); }
    const prev = document.getElementById('btn-hashes-prev'); const next = document.getElementById('btn-hashes-next');
    if (prev) prev.disabled = (cur<=0);
    if (next) next.disabled = (cur+lim >= tot);
    const first = document.getElementById('btn-hashes-first'); const last = document.getElementById('btn-hashes-last');
    if (first) first.disabled = (cur<=0);
    if (last) last.disabled = (cur+lim >= tot);
    const pageEl = document.getElementById('hash-page'); if (pageEl) pageEl.value = curPage;
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
      const etaHtml = eta ? `<span class=\"dim\">${eta}</span>` : '';
      tr.innerHTML = `<td class=\"mono\"><a href=\"#\" data-focus=\"${it.model_id||''}\">${it.model_id||''}</a></td><td class=\"mono\">${it.job_id||''}</td><td>${etaHtml}</td>`;
      const link = tr.querySelector('[data-focus]');
      if (link) link.addEventListener('click', (e)=>{ e.preventDefault(); focusModel(link.getAttribute('data-focus')); });
      tb.appendChild(tr);
    });
    const infl = (v.inflight_hashes||[]).join(', ');
    document.getElementById('jobs-inflight').textContent = infl || '—';
    // reflect concurrency snapshot (kept in jobs payload for alignment)
    if (v.concurrency){
      const c = v.concurrency;
      const stat = document.getElementById('jobs-stat');
      if (stat){ stat.textContent = `Conc ${c.available_permits||0}/${c.configured_max||0} (held ${c.held_permits||0})`; }
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

// Initialize toggles
document.addEventListener('DOMContentLoaded', ()=>{
  const auto = document.getElementById('jobs-auto');
  if (auto) auto.addEventListener('change', (e)=> setJobsAuto(!!e.target.checked));
});
