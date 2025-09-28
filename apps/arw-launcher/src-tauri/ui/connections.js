const invoke = (cmd, args) => ARW.invoke(cmd, args);
let sseIndicatorHandle = null;

function ensureSseIndicator() {
  const wrap = document.getElementById('statusBadges');
  if (!wrap) return;
  if (sseIndicatorHandle) return;
  let badge = document.getElementById('connectionsSseBadge');
  if (!badge) {
    badge = document.createElement('span');
    badge.id = 'connectionsSseBadge';
    badge.className = 'badge';
    wrap.appendChild(badge);
  }
  sseIndicatorHandle = ARW.sse.indicator(badge, { prefix: 'SSE' });
}

function connectSse({ replay = 5, resume = false } = {}) {
  ensureSseIndicator();
  ARW.sse.connect(ARW.base(8091), { replay, prefix: 'probe.metrics' }, resume);
}
function load(){ return ARW.getPrefs('launcher').then(v => (v&&v.connections)||[]) }
async function save(conns){ const v = await ARW.getPrefs('launcher')||{}; v.connections = conns; await ARW.setPrefs('launcher', v) }
async function refresh(){
  const rows = document.getElementById('rows'); rows.innerHTML='';
  const conns = await load();
  conns.forEach(c => {
    const tr = document.createElement('tr');
    tr.innerHTML = `<td>${c.name||''}</td><td>${c.base||''}</td><td data-st class="dim">…</td><td>
      <button data-ev title="Open Events window">Events</button>
      <button data-logs title="Open Logs window">Logs</button>
      <button data-models title="Open Models window">Models</button>
      <button data-open title="Open Debug UI">Open Debug</button>
      <button data-ping title="Ping connection">Ping</button>
      <button data-del title="Delete connection">Delete</button>
    </td>`;
    const statusCell = tr.querySelector('[data-st]');
    if (statusCell) {
      statusCell.setAttribute('role', 'status');
      statusCell.setAttribute('aria-live', 'polite');
    }
    tr.querySelector('[data-ev]').addEventListener('click', async ()=>{ try{ await invoke('open_events_window_base', { base: (c.base||'').replace(/\/$/,''), labelSuffix: (c.name||'') }); }catch(e){} });
    tr.querySelector('[data-logs]').addEventListener('click', async ()=>{ try{ await invoke('open_logs_window_base', { base: (c.base||'').replace(/\/$/,''), labelSuffix: (c.name||'') }); }catch(e){} });
    tr.querySelector('[data-models]').addEventListener('click', async ()=>{ try{ await invoke('open_models_window_base', { base: (c.base||'').replace(/\/$/,''), labelSuffix: (c.name||'') }); }catch(e){} });
    tr.querySelector('[data-open]').addEventListener('click', async ()=>{ try{ await invoke('open_url', { url: (c.base||'').replace(/\/$/,'') + '/admin/debug' }); }catch(e){} });
    tr.querySelector('[data-ping]').addEventListener('click', async ()=>{ await pingRow(tr, c) });
    tr.querySelector('[data-del]').addEventListener('click', async ()=>{ const cc = (await load()).filter(x => x.name !== c.name); await save(cc); refresh(); });
    rows.appendChild(tr);
    // initial ping
    pingRow(tr, c);
  });
}
async function pingRow(tr, c){
  const st = tr.querySelector('[data-st]'); if (st) { st.textContent = '…'; st.className='dim'; }
  try{
    const baseUrl = (c.base||'').trim();
    const r = await ARW.http.fetch(baseUrl, '/healthz', { method: 'GET' });
    if (r.ok) {
      st.innerHTML = '<span class="dot ok"></span> online';
      st.className='ok';
      st.dataset.status = 'online';
    } else if (r.status === 401 || r.status === 403) {
      st.innerHTML = '<span class="dot warn"></span> auth required';
      st.className='warn';
      st.dataset.status = 'auth';
    } else {
      st.innerHTML = `<span class="dot bad"></span> error (${r.status})`;
      st.className='bad';
      st.dataset.status = 'error';
    }
  }catch(e){ if (st) { st.innerHTML = '<span class="dot bad"></span> offline'; st.className='bad'; st.dataset.status = 'offline'; } }
}
document.addEventListener('DOMContentLoaded', () => {
  document.getElementById('btn-add').addEventListener('click', async ()=>{
    const name = document.getElementById('cname').value.trim();
    const base = document.getElementById('curl').value.trim();
    const token = document.getElementById('ctok').value.trim();
    if (!name || !base) return;
    const conns = await load();
    if (!conns.find(x => x.name === name)) conns.push({ name, base, token }); else {
      // update existing
      conns.forEach(x => { if (x.name === name) { x.base = base; x.token = token; } });
    }
    await save(conns); document.getElementById('stat').textContent = 'Saved'; ARW.toast('Connection saved'); refresh();
  });
  document.getElementById('btn-save').addEventListener('click', async ()=>{
    const conns = await load(); await save(conns); document.getElementById('stat').textContent = 'Saved'; ARW.toast('Connections saved');
  });
  (async ()=>{ connectSse({ replay: 5, resume: false }); await refresh(); setInterval(refresh, 10000) })();
});
