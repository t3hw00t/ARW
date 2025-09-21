let lastJson = null;
const base = (port) => ARW.base(port);
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
async function loadStats() {
  if (document.getElementById('auto').checked === false && lastJson) {
    render(lastJson); return;
  }
  const out = document.getElementById('out');
  out.innerHTML = '';
  const port = ARW.getPortFromInput('port') || 8091;
  const base = ARW.base(port);
  document.getElementById('stat').textContent = 'Loading…';
  try {
    let j = null;
    const statsPaths = ['state/route_stats', 'admin/introspect/stats'];
    let lastErr = null;
    for (const path of statsPaths) {
      try {
        if (window.__ARW_BASE_OVERRIDE) {
          const tok = await ARW.connections.tokenFor(base);
          j = await ARW.invoke('admin_get_json_base', { base, path, token: tok });
        } else {
          const url = base + '/' + path;
          const resp = await fetch(url, { headers: { 'Accept': 'application/json' } });
          if (!resp.ok) throw new Error('HTTP ' + resp.status);
          j = await resp.json();
        }
        if (path === 'admin/introspect/stats') {
          console.warn('[logs] /admin/introspect/stats is deprecated; prefer /state/route_stats');
        }
        lastErr = null;
        break;
      } catch (err) {
        lastErr = err;
        j = null;
      }
    }
    if (!j) {
      throw lastErr || new Error('Failed to load route stats');
    }
    lastJson = j; render(j);
    document.getElementById('stat').textContent = 'OK';
    // Update system badges
    try{
      const port = ARW.getPortFromInput('port') || 8091;
      const resp = await fetch(base + '/admin/probe/metrics');
      const dj = await resp.json(); const d = dj?.data || dj;
      const cpu = d?.cpu?.avg || 0; setCpuBadge(cpu);
      const mu = d?.memory?.used || 0; const mt = d?.memory?.total || 0; setMemBadge(mu, mt);
      const g = Array.isArray(d?.gpus)? d.gpus : []; let t=0, u=0; for(const x of g){ t+=Number(x?.mem_total||0); u+=Number(x?.mem_used||0);} setGpuBadge(u, t);
    }catch {}
  } catch (e) {
    document.getElementById('stat').textContent = 'Error';
    const pre = document.createElement('pre'); pre.textContent = String(e); out.appendChild(pre);
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
  document.getElementById('btn-refresh').addEventListener('click', loadStats);
  document.getElementById('wrap').addEventListener('change', ()=> render(lastJson||{}));
  document.getElementById('auto').addEventListener('change', ()=>{});
  document.getElementById('focus').addEventListener('change', ()=> render(lastJson||{}));
  document.getElementById('routeFilter').addEventListener('input', ()=> render(lastJson||{}));
  document.getElementById('btn-copy').addEventListener('click', ()=>{ if (lastJson) ARW.copy(JSON.stringify(lastJson, null, 2)); });
  (async () => { await ARW.applyPortFromPrefs('port'); loadStats(); setInterval(loadStats, 5000) })();
});

// Keyboard shortcuts (ignore when typing)
window.addEventListener('keydown', (e)=>{
  const tag=(e.target && e.target.tagName || '').toLowerCase();
  if (tag==='input' || tag==='textarea' || tag==='select') return;
  if (e.ctrlKey || e.metaKey || e.altKey) return;
  const btn = (id)=> document.getElementById(id);
  if (e.key.toLowerCase()==='r'){ e.preventDefault(); btn('btn-refresh')?.click(); }
  else if (e.key.toLowerCase()==='w'){ e.preventDefault(); const el=document.getElementById('wrap'); if (el){ el.checked=!el.checked; render(lastJson||{}); } }
  else if (e.key.toLowerCase()==='a'){ e.preventDefault(); const el=document.getElementById('auto'); if (el){ el.checked=!el.checked; if (el.checked) loadStats(); } }
});
