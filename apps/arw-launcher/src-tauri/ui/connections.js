const invoke = (cmd, args) => ARW.invoke(cmd, args);
function load(){ return ARW.getPrefs('launcher').then(v => (v&&v.connections)||[]) }
async function save(conns){ const v = await ARW.getPrefs('launcher')||{}; v.connections = conns; await ARW.setPrefs('launcher', v) }
async function refresh(){
  const rows = document.getElementById('rows'); rows.innerHTML='';
  const conns = await load();
  conns.forEach(c => {
    const tr = document.createElement('tr');
    tr.innerHTML = `<td>${c.name||''}</td><td>${c.base||''}</td><td data-st class="dim">…</td><td>
      <button data-open>Open Debug</button>
      <button data-ping>Ping</button>
      <button data-del>Delete</button>
    </td>`;
    tr.querySelector('[data-open]').addEventListener('click', async ()=>{ try{ await invoke('open_url', { url: (c.base||'').replace(/\/$/,'') + '/debug' }); }catch(e){} });
    tr.querySelector('[data-ping]').addEventListener('click', async ()=>{ await pingRow(tr, c) });
    tr.querySelector('[data-del]').addEventListener('click', async ()=>{ const cc = (await load()).filter(x => x.name !== c.name); await save(cc); refresh(); });
    rows.appendChild(tr);
    // initial ping
    pingRow(tr, c);
  });
}
async function pingRow(tr, c){
  const st = tr.querySelector('[data-st]'); if (st) st.textContent = '…';
  try{
    const url = (c.base||'').replace(/\/$/,'') + '/healthz';
    const r = await fetch(url, { method: 'GET' });
    if (r.ok) { st.innerHTML = '<span class="dot ok"></span> online'; st.className='ok'; }
    else { st.innerHTML = '<span class="dot bad"></span> error'; st.className='bad'; }
  }catch(e){ if (st) { st.innerHTML = '<span class="dot bad"></span> offline'; st.className='bad'; } }
}
document.addEventListener('DOMContentLoaded', () => {
  document.getElementById('btn-add').addEventListener('click', async ()=>{
    const name = document.getElementById('cname').value.trim();
    const base = document.getElementById('curl').value.trim();
    if (!name || !base) return;
    const conns = await load();
    if (!conns.find(x => x.name === name)) conns.push({ name, base });
    await save(conns); document.getElementById('stat').textContent = 'Saved'; ARW.toast('Connection added'); refresh();
  });
  document.getElementById('btn-save').addEventListener('click', async ()=>{
    const conns = await load(); await save(conns); document.getElementById('stat').textContent = 'Saved'; ARW.toast('Connections saved');
  });
  (async ()=>{ await refresh(); setInterval(refresh, 10000) })();
});

