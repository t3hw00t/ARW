let es = null;
let lastRaw = '';
const base = (port) => ARW.base(port);
function bytesHuman(n){ if(!n && n!==0) return '–'; const kb=1024, mb=kb*1024, gb=mb*1024, tb=gb*1024; if(n>=tb) return (n/tb).toFixed(2)+' TiB'; if(n>=gb) return (n/gb).toFixed(2)+' GiB'; if(n>=mb) return (n/mb).toFixed(1)+' MiB'; if(n>=kb) return (n/kb).toFixed(1)+' KiB'; return n+' B'; }
function setCpuBadge(p){ try{ const el=document.getElementById('cpuBadge'); if(!el) return; const v = Number(p)||0; el.textContent = 'CPU: ' + v.toFixed(1) + '%'; el.className = 'badge ' + (v>=90? 'bad' : v>=75? 'warn':''); }catch{} }
function setMemBadge(used,total){ try{ const el=document.getElementById('memBadge'); if(!el) return; const pct = total>0? (100*used/total):0; el.textContent = 'Mem: ' + pct.toFixed(1) + '% ('+bytesHuman(used)+'/'+bytesHuman(total)+')'; el.className = 'badge ' + (pct>=90? 'bad' : pct>=75? 'warn':''); }catch{} }
function setGpuBadge(used,total){ try{ const el=document.getElementById('gpuBadge'); if(!el) return; const pct = total>0? (100*used/total):0; el.textContent = 'GPU: ' + pct.toFixed(1) + '%'; el.className = 'badge ' + (pct>=95? 'bad' : pct>=80? 'warn':''); }catch{} }
function add(kind, raw) {
  lastRaw = raw;
  const paused = document.getElementById('pause').checked;
  if (paused) return;
  const el = document.createElement('div');
  el.className = 'e';
  const ts = new Date().toISOString();
  const pretty = document.getElementById('pretty').checked;
  const wrap = document.getElementById('wrap').checked;
  let body = raw;
  // include/exclude filters on raw body
  const inc = (document.getElementById('inc')?.value||'').trim();
  const exc = (document.getElementById('exc')?.value||'').trim();
  const hasAll = inc? inc.split(/\s+/).every(t=> !t || body.includes(t)) : true;
  const hasExc = exc? exc.split(/\s+/).some(t=> t && body.includes(t)) : false;
  if (!hasAll || hasExc) return;
  try { if (pretty) body = JSON.stringify(JSON.parse(raw), null, 2); } catch {}
  el.innerHTML = `<div class="dim">${ts} <b>${kind||'message'}</b></div><pre style="white-space:${wrap?'pre-wrap':'pre'}">${body.replace(/</g,'&lt;')}</pre>`;
  const log = document.getElementById('log');
  log.prepend(el);
  while (log.childElementCount > 300) log.removeChild(log.lastChild);
}
function sse(replay) {
  const port = ARW.getPortFromInput('port') || 8091;
  let url = ARW.base(port) + '/events';
  const filter = document.getElementById('filter').value.trim();
  const params = new URLSearchParams();
  if (replay) params.set('replay', replay);
  if (filter) params.set('prefix', filter);
  if ([...params.keys()].length) url += '?' + params.toString();
  if (es) { es.close(); es = null; }
  es = new EventSource(url);
  es.onopen = () => document.getElementById('stat').textContent = 'on';
  es.onerror = () => document.getElementById('stat').textContent = 'off';
  es.onmessage = ev => {
    let k = 'message';
    try { const j = JSON.parse(ev.data||'{}'); k = j.kind || k; } catch {}
    add(k, ev.data);
  };
  es.addEventListener('probe.metrics', ev => { try{
    const j = JSON.parse(ev.data||'{}');
    const cpu = j?.cpu?.avg || 0; setCpuBadge(cpu);
    const mu = j?.memory?.used || 0; const mt = j?.memory?.total || 0; setMemBadge(mu, mt);
    const g = Array.isArray(j?.gpus)? j.gpus : []; let t=0,u=0; for(const x of g){ t+=Number(x?.mem_total||0); u+=Number(x?.mem_used||0);} setGpuBadge(u,t);
  }catch{}});
}
document.addEventListener('DOMContentLoaded', () => {
  document.getElementById('btn-replay').addEventListener('click', ()=> sse(50));
  document.querySelectorAll('[data-preset]').forEach(b=> b.addEventListener('click', ()=>{
    document.getElementById('filter').value = b.dataset.preset||''; sse(25);
  }));
  document.getElementById('btn-clear').addEventListener('click', ()=>{ const log=document.getElementById('log'); log.innerHTML=''; });
  document.getElementById('btn-copy').addEventListener('click', ()=>{ if (lastRaw) ARW.copy(lastRaw); });
  document.getElementById('pretty').addEventListener('change', ()=>{});
  document.getElementById('wrap').addEventListener('change', ()=>{});
  document.getElementById('pause').addEventListener('change', ()=>{});
  (async () => { await ARW.applyPortFromPrefs('port'); sse(0); try{ const r=await fetch(base(ARW.getPortFromInput('port')||8091) + '/admin/probe/metrics'); const j=await r.json(); const d=j?.data||j; const cpu=d?.cpu?.avg||0; setCpuBadge(cpu); const mu=d?.memory?.used||0; const mt=d?.memory?.total||0; setMemBadge(mu,mt); const g=Array.isArray(d?.gpus)? d.gpus:[]; let t=0,u=0; for(const x of g){ t+=Number(x?.mem_total||0); u+=Number(x?.mem_used||0);} setGpuBadge(u,t);}catch{} })();
});

// Keyboard shortcuts (view-only; ignore when typing)
window.addEventListener('keydown', (e)=>{
  const tag=(e.target && e.target.tagName || '').toLowerCase();
  if (tag==='input' || tag==='textarea' || tag==='select') return;
  if (e.ctrlKey || e.metaKey || e.altKey) return;
  if (e.key.toLowerCase()==='p'){ e.preventDefault(); const cb=document.getElementById('pause'); if (cb){ cb.checked=!cb.checked; } }
  else if (e.key.toLowerCase()==='c'){ e.preventDefault(); const log=document.getElementById('log'); if (log) log.innerHTML=''; }
});
