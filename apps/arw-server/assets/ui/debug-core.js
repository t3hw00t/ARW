// Debug UI core logic extracted from inline scripts (CSP-safe)
// Important: depends on showToast() and helpers from debug.js

const base = location.origin;
document.getElementById('port').textContent = location.host;
let lastCurl = null;
let __adminOk = false;
let __adminHeader = '';
function mkcurl(method, url, body){
  const parts = ["curl -sS", "-X", method, `'${url}'`];
  if (body != null){ parts.push("-H", `'Content-Type: application/json'`); parts.push("-d", `'${JSON.stringify(body)}'`); }
  if (__adminHeader) { parts.push("-H", `'X-ARW-Admin: ${__adminHeader}'`); }
  return parts.join(' ');
}
function setLastCurl(s){
  lastCurl = s; const b = document.getElementById('copyCurlBtn'); if (s){ b.style.display='inline-block'; } else { b.style.display='none'; }
}
document.getElementById('copyCurlBtn').addEventListener('click', async ()=>{
  if(!lastCurl) return; try{ await navigator.clipboard.writeText(lastCurl); const b = document.getElementById('copyCurlBtn'); const old = b.textContent; b.textContent='Copied'; setTimeout(()=> b.textContent=old, 900);}catch{}
});
async function req(method, path, body, outId, rtId){
  let p = path;
  // Auto-prefix admin for sensitive paths unless explicitly public
  if (!(p.startsWith('/admin') || p.startsWith('/metrics') || p.startsWith('/version') || p.startsWith('/about') || p.startsWith('/spec') || p.startsWith('/state') || p.startsWith('/events'))) {
    p = '/admin' + p;
  }
  const url = base + p; const init = { method, headers: {}, body: undefined };
  if (body != null){ init.headers['Content-Type'] = 'application/json'; init.body = JSON.stringify(body); }
  if (__adminHeader) { init.headers['X-ARW-Admin'] = __adminHeader; }
  setLastCurl(mkcurl(method, url, body));
  const t0 = performance.now();
  try{
    const resp = await fetch(url, init);
    const txt = await resp.text();
    const dt = Math.round(performance.now() - t0);
    if(rtId){ const el = document.getElementById(rtId); if (el) el.textContent = dt + ' ms'; }
    const out = document.getElementById(outId);
    try{ out.textContent = JSON.stringify(JSON.parse(txt), null, 2); }catch{ out.textContent = txt; }
    if(!resp.ok){
      if (resp.status === 403) { showToast('403 Forbidden — admin endpoints locked. Set ARW_DEBUG=1 or use X-ARW-Admin'); setAdmin(false); }
      else if (resp.status === 429) { showToast('429 Too Many Requests — admin rate limit'); }
      else { showToast('Request failed: ' + resp.status + ' ' + (resp.statusText||'')); }
    }
    return txt;
  }catch(e){
    if(rtId){ const el = document.getElementById(rtId); if (el) el.textContent = 'error'; }
    showToast('Network error');
    throw e;
  }
}
async function hit(path){ await req('GET', path, null, 'out', 'rt-main'); }
async function post(path, body){ await req('POST', path, body, 'out', 'rt-main'); }
async function refreshMemory(){
  const txt = await req('GET', '/state/memory/recent?limit=200', null, 'memOut', 'rt-mem'); return txt;
}
async function quickApply(){
  let kind = document.getElementById('memKind').value;
  let bodyTxt = document.getElementById('memBody').value;
  let value; try{ value = JSON.parse(bodyTxt); }catch(e){ showToast('Invalid JSON'); return; }
  await req('POST', '/admin/memory/apply', { lane: kind, value }, 'out', 'rt-main');
  await refreshMemory();
}

// Tools
async function runTool(){
  const id = document.getElementById('toolId').value;
  let bodyTxt = document.getElementById('toolBody').value;
  let input; try{ input = JSON.parse(bodyTxt); }catch(e){ showToast('Invalid JSON'); return; }
  const tb=document.getElementById('toolsBadge'); if(tb){ tb.className='badge warn'; tb.innerHTML='<span class=\"dot\"></span> Tools: active'; }
  await req('POST', '/tools/run', { id, input }, 'toolOut', 'rt-tool');
  if(tb){ tb.className='badge ok'; tb.innerHTML='<span class=\"dot\"></span> Tools: done'; setTimeout(()=>{ tb.className='badge warn'; tb.innerHTML='<span class=\"dot\"></span> Tools: idle'; }, 1200); }
}

// Orchestration actions
async function orProbe(){ await req('GET', '/probe', null, 'out', 'rt-orch'); }
async function orRefreshModels(){ await req('POST', '/models/refresh', null, 'out', 'rt-orch'); }
async function orShutdown(){ await req('GET', '/shutdown', null, 'out', 'rt-orch'); }
async function orProfileApply(){ const name = document.getElementById('profileSel').value; await req('POST','/governor/profile',{name},'out','rt-orch'); }
async function orProfileGet(){ await req('GET','/governor/profile',null,'out','rt-orch'); }
async function orHintsApply(){ const h={};
  const c=document.getElementById('hintConc').value; const b=document.getElementById('hintBuf').value; const t=document.getElementById('hintTimeout').value;
  const rk=document.getElementById('hintRetrK').value; const dv=document.getElementById('hintDiv').value; const lam=document.getElementById('hintLambda').value; const cg=document.getElementById('hintCompr').value; const vk=document.getElementById('hintVoteK').value;
  const cb=document.getElementById('hintCtxBudget').value; const pi=document.getElementById('hintCtxPer').value; const fmt=document.getElementById('hintCtxFmt').value; const prov=document.getElementById('hintProv').checked;
  const hdr=document.getElementById('hintCtxHeader').value; const ftr=document.getElementById('hintCtxFooter').value; const tpl=document.getElementById('hintCtxTpl').value; const jn=document.getElementById('hintJoiner').value;
  if(c) h.max_concurrency=parseInt(c,10); if(b) h.event_buffer=parseInt(b,10); if(t) h.http_timeout_secs=parseInt(t,10);
  if(rk) h.retrieval_k=parseInt(rk,10); if(dv) h.retrieval_div=parseFloat(dv); if(lam) h.mmr_lambda=parseFloat(lam); if(cg) h.compression_aggr=parseFloat(cg); if(vk) h.vote_k=parseInt(vk,10);
  if(cb) h.context_budget_tokens=parseInt(cb,10); if(pi) h.context_item_budget_tokens=parseInt(pi,10);
  if(fmt) h.context_format=fmt; if(prov) h.include_provenance=!!prov; if(hdr) h.context_header=hdr; if(ftr) h.context_footer=ftr; if(tpl) h.context_item_template=tpl; if(jn) h.joiner=jn;
  if(Object.keys(h).length===0) return; await req('POST','/governor/hints',h,'out','rt-orch'); }
async function orHintsGet(){ await req('GET','/governor/hints',null,'out','rt-orch'); }

// Models UI (subset)
async function modelsList(){ try{
  const r = await fetch(base + '/admin/models', { headers: { 'X-ARW-Admin': __adminHeader || '' } });
  if(!r.ok) return;
  const list = await r.json();
  window.__modelsRM = window.__modelsRM || { items: [], default: null };
  window.__modelsRM.items = Array.isArray(list) ? list : (Array.isArray(list?.data)? list.data : []);
  modelsRender();
}catch{} }
async function modelsAdd(){ const id=document.getElementById('mId').value.trim(); if(!id) return; const provider=(document.getElementById('mProv').value||'local').trim(); await req('POST','/models/add',{id,provider},'modelsOut','rt-orch'); await modelsList(); }
async function modelsDelete(){ const id=document.getElementById('mId').value.trim(); if(!id) return; await req('POST','/models/delete',{id},'modelsOut','rt-orch'); await modelsList(); }
async function modelsDefaultGet(){ try{ const r=await fetch(base + '/admin/models/default', { headers: { 'X-ARW-Admin': __adminHeader || '' } }); const j=await r.json(); const d = (j?.data?.default) || j?.default || null; window.__modelsRM = window.__modelsRM || { items: [], default: null }; window.__modelsRM.default = d; modelsRender(); }catch{} }
async function modelsDefaultSet(){ try{ const id=document.getElementById('mId').value.trim(); if(!id) return; await fetch(base + '/admin/models/default', { method:'POST', headers: { 'Content-Type':'application/json','X-ARW-Admin': __adminHeader || '' }, body: JSON.stringify({id}) }); window.__modelsRM = window.__modelsRM || { items: [], default: null }; window.__modelsRM.default = id; modelsRender(); }catch{} }
async function modelsDownload(){
  const id=(document.getElementById('mId')?.value||'').trim();
  const provider=(document.getElementById('mProv')?.value||'local').trim();
  const url=(document.getElementById('mUrl')?.value||'').trim();
  const sha256=(document.getElementById('mSha')?.value||'').trim();
  if(!id){ showToast('model id required'); return; }
  if(!url){ showToast('url required'); return; }
  if(!/^https?:\/\//i.test(url)){ showToast('invalid url (http/https)'); return; }
  if(!(sha256 && /^[0-9a-fA-F]{64}$/.test(sha256))){ showToast('sha256 (64 hex) required'); return; }
  await req('POST','/models/download',{ id, url, provider, sha256 },'modelsOut','rt-orch');
}
async function modelsCancel(){ const id=document.getElementById('mId').value.trim(); if(!id) return; await req('POST','/models/download/cancel',{id},'modelsOut','rt-orch'); }
async function modelsCancelId(id){ if(!id) return; await req('POST','/models/download/cancel',{id},'out','rt-orch'); }

// Download manager helpers (subset)
const downloads = Object.create(null);
function nowMs(){ return (typeof performance!=='undefined' && performance.now) ? performance.now() : Date.now(); }
function msHuman(ms){ if(!ms && ms!==0) return '–'; const s=Math.max(0, Math.round(ms/1000)); const m=Math.floor(s/60); const ss=(s%60).toString().padStart(2,'0'); return (m>0? m+':':'0:')+ss; }

// Shortcuts (subset)
const Shortcuts = { enabled: true, binds: [], normalize(e){ const parts=[]; if(e.ctrlKey||e.metaKey) parts.push('Ctrl'); if(e.altKey) parts.push('Alt'); if(e.shiftKey && !['?',':'].includes(e.key)) parts.push('Shift'); let k=e.key; if(k.length===1){k=k.toUpperCase();} return parts.concat([k]).join('+'); }, inEditable(e){ const t=e.target; if(!t) return false; const el=t.closest?t.closest('input,textarea,select,[contenteditable="true"]'):null; return !!el; }, register(seq,desc,fn,opts){ this.binds.push({ seq, desc, fn, when:(opts&&opts.when)||(()=>true), inInputs:!!(opts&&opts.captureInInputs)}); }, setEnabled(on){ this.enabled=!!on; try{ localStorage.setItem('shortcuts.enabled', String(this.enabled)); }catch{} const cb=document.getElementById('shortcutsToggle'); if(cb) cb.checked=this.enabled; }, list(){ return this.binds.map(b=>({ seq:b.seq, desc:b.desc })).filter((v,i,a)=> a.findIndex(x=> x.seq===v.seq && x.desc===v.desc)===i); }, ensureHelp(){ if(document.getElementById('scut')) return; const wrap=document.createElement('div'); wrap.className='palette'; wrap.id='scut'; wrap.innerHTML = `<div class="pal-box"><div style="padding:10px 12px;display:flex;align-items:center;gap:8px"><b>Shortcuts</b><span class="key" style="margin-left:auto">enable</span><input type="checkbox" id="shortcutsToggle2"></div><div id="scutList" class="pal-list"></div></div>`; document.body.appendChild(wrap); document.getElementById('shortcutsToggle2').addEventListener('change', (e)=> this.setEnabled(e.target.checked)); }, showHelp(){ this.ensureHelp(); const el=document.getElementById('scut'); const list=document.getElementById('scutList'); const rows=this.list().map(x=> `<div class="pal-item"><span>${x.desc}</span><span class="hint">${x.seq}</span></div>`).join(''); list.innerHTML = rows || '<div class="pal-item"><span>No shortcuts</span></div>'; el.style.display='flex'; document.getElementById('shortcutsToggle2').checked=this.enabled; }, hideHelp(){ const el=document.getElementById('scut'); if(el) el.style.display='none'; }, hookOnce(){ if(this._hooked) return; this._hooked=true; document.addEventListener('keydown', (e)=>{ if(!this.enabled) return; const seq=this.normalize(e); const inEdit=this.inEditable(e); for(const b of this.binds){ if(b.seq===seq && b.when() && (b.inInputs || !inEdit)){ e.preventDefault(); try{ b.fn(e);}catch{} return; } } if(e.key==='Escape'){ const pal=document.getElementById('pal'); const sc=document.getElementById('scut'); if(pal && pal.style.display!=='none'){ if(window.palette&&palette.close) palette.close(); e.preventDefault(); return; } if(sc && sc.style.display!=='none'){ this.hideHelp(); e.preventDefault(); return; } } }); } };
document.addEventListener('DOMContentLoaded', ()=>{
  try{ Shortcuts.setEnabled((localStorage.getItem('shortcuts.enabled')||'true')==='true'); }catch{}
  Shortcuts.hookOnce();
  const cb=document.getElementById('shortcutsToggle'); if(cb){ cb.checked = Shortcuts.enabled; cb.addEventListener('change', ()=> Shortcuts.setEnabled(cb.checked)); }
  document.getElementById('openShortcuts')?.addEventListener('click', ()=> Shortcuts.showHelp());
});
