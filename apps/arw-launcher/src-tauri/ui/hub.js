document.addEventListener('DOMContentLoaded', async () => {
  await ARW.applyPortFromPrefs('port');
  const port = ARW.getPortFromInput('port') || 8090;
  const base = ARW.base(port);
  const sc = ARW.sidecar.mount('sidecar', ['timeline','context','policy','metrics','models','activity'], { base });
  ARW.sse.subscribe('*open*', ()=> document.getElementById('sseStat').textContent = 'SSE: on');
  ARW.sse.subscribe('*error*', ()=> document.getElementById('sseStat').textContent = 'SSE: off');
  ARW.sse.connect(base, { replay: 25 });
  // ---------- Runs: episodes list + snapshot ----------
  let runsCache = [];
  let runSnapshot = null;
  const elRunsTbl = document.getElementById('runsTbl');
  const elRunsStat = document.getElementById('runsStat');
  const elRunFilter = document.getElementById('runFilter');
  const elRunErrOnly = document.getElementById('runErrOnly');
  const elRunSnap = document.getElementById('runSnap');
  const elRunSnapMeta = document.getElementById('runSnapMeta');
  function setRunsStat(txt){ if (elRunsStat){ elRunsStat.textContent = txt||''; if (txt) setTimeout(()=>{ if (elRunsStat.textContent===txt) elRunsStat.textContent=''; }, 1200); } }
  function ms(n){ return Number.isFinite(n) ? n : 0 }
  function renderRuns(){
    if (!elRunsTbl) return;
    const q = (elRunFilter?.value||'').toLowerCase();
    const errOnly = !!(elRunErrOnly && elRunErrOnly.checked);
    const rows = runsCache.filter(r => {
      if (errOnly && (r.errors|0) === 0) return false;
      if (!q) return true;
      return String(r.id||'').toLowerCase().includes(q);
    });
    elRunsTbl.innerHTML='';
    for (const r of rows){
      const tr = document.createElement('tr');
      const id = document.createElement('td'); id.className='mono'; id.textContent = r.id||'';
      const count = document.createElement('td'); count.textContent = r.count||0;
      const dur = document.createElement('td'); dur.textContent = ms(r.duration_ms||0);
      const err = document.createElement('td'); err.textContent = r.errors||0; if ((r.errors|0)>0) err.className='bad';
      const act = document.createElement('td');
      const view = document.createElement('button'); view.className='ghost'; view.textContent='View'; view.title='View snapshot'; view.addEventListener('click', ()=> viewRun(r.id));
      act.appendChild(view);
      tr.appendChild(id); tr.appendChild(count); tr.appendChild(dur); tr.appendChild(err); tr.appendChild(act);
      elRunsTbl.appendChild(tr);
    }
  }
  let runsAbort = null;
  async function loadRuns(){
    try{
      if (runsAbort) { try{ runsAbort.abort(); }catch{} }
      runsAbort = new AbortController();
      const r = await fetch(base + '/state/episodes', { signal: runsAbort.signal });
      const j = await r.json();
      runsCache = Array.isArray(j?.items) ? j.items : (Array.isArray(j) ? j : []);
      renderRuns();
    }catch(e){ console.error(e); }
  }
  let snapAbort = null;
  async function viewRun(id){
    try{
      if (snapAbort) { try{ snapAbort.abort(); }catch{} }
      snapAbort = new AbortController();
      const r = await fetch(base + '/state/episode/' + encodeURIComponent(id) + '/snapshot', { signal: snapAbort.signal });
      const j = await r.json();
      runSnapshot = j;
      if (elRunSnap) elRunSnap.textContent = JSON.stringify(j, null, 2);
      if (elRunSnapMeta) elRunSnapMeta.textContent = 'episode: ' + id;
    }catch(e){ console.error(e); runSnapshot=null; if (elRunSnap) elRunSnap.textContent=''; }
  }
  document.getElementById('btnRunsRefresh')?.addEventListener('click', loadRuns);
  // Do not persist filters; just render on change
  elRunFilter?.addEventListener('input', ()=>{ renderRuns(); });
  elRunErrOnly?.addEventListener('change', ()=>{ renderRuns(); });
  document.getElementById('btnRunCopy')?.addEventListener('click', ()=>{ if (runSnapshot) ARW.copy(JSON.stringify(runSnapshot, null, 2)); });
  document.getElementById('btnRunPinA')?.addEventListener('click', ()=>{ if (runSnapshot){ const ta=document.getElementById('cmpA'); if (ta) ta.value = JSON.stringify(runSnapshot, null, 2); } });
  document.getElementById('btnRunPinB')?.addEventListener('click', ()=>{ if (runSnapshot){ const tb=document.getElementById('cmpB'); if (tb) tb.value = JSON.stringify(runSnapshot, null, 2); } });
  await loadRuns();
  // Throttle SSE-driven refresh on episode-related activity
  let _lastRunsAt = 0;
  const runsTick = ()=>{ const now=Date.now(); if (now - _lastRunsAt > 1200){ _lastRunsAt = now; loadRuns(); } };
  ARW.sse.subscribe((k, e) => {
    try{
      const p = e?.env?.payload || {};
      return !!p.corr_id || /^intents\.|^actions\.|^feedback\./.test(k||'');
    }catch{ return false }
  }, runsTick);
  // ---------- Projects: list/create/tree/notes ----------
  let curProj = null;
  // Simple file metadata cache to avoid repeated GETs (5s TTL)
  const fileCache = new Map(); // rel -> { data, t }
  const fileTTL = 5000;
  // Nested tree cache and expansion state
  const treeCache = new Map(); // path -> items
  let expanded = new Set(); // rel paths that are expanded (persisted)
  let searchExpanded = new Set(); // ephemeral expansions from filter
  const elProjSel = document.getElementById('projSel');
  const elProjName = document.getElementById('projName');
  const elProjTree = document.getElementById('projTree');
  const elProjStat = document.getElementById('projStat');
  const elNotes = document.getElementById('notesArea');
  const elCurProj = document.getElementById('curProj');
  const elNotesAutosave = document.getElementById('notesAutosave');
  const elProjPrefsBadge = document.getElementById('projPrefsBadge');
  const elFileFilter = document.getElementById('fileFilter');
  let currentPath = '';
  const pathStack = [];
  function setStat(txt){ if (elProjStat) { elProjStat.textContent = txt||''; if (txt) setTimeout(()=>{ if (elProjStat.textContent===txt) elProjStat.textContent=''; }, 1500); } }
  async function setProj(name){
    curProj = name||null;
    if (elCurProj) elCurProj.textContent = curProj||'–';
    try { const hub = await ARW.getPrefs('ui:hub')||{}; hub.lastProject = curProj; await ARW.setPrefs('ui:hub', hub); } catch {}
    try { projPrefs = await ARW.getPrefs('ui:proj:'+curProj) || {}; } catch { projPrefs = {}; }
    try { const arr = Array.isArray(projPrefs.expanded)? projPrefs.expanded : []; expanded = new Set(arr.map(String)); } catch { expanded = new Set(); }
    const as = (projPrefs && projPrefs.notesAutoSave !== false);
    if (elNotesAutosave) elNotesAutosave.checked = as;
    const hasEditor = !!(projPrefs && projPrefs.editorCmd);
    if (elProjPrefsBadge) elProjPrefsBadge.style.display = hasEditor? 'inline-flex':'none';
    // restore last folder if present
    if (curProj){
      loadNotes();
      try{ const lp = projPrefs && projPrefs.lastPath ? String(projPrefs.lastPath) : ''; currentPath=''; pathStack.length=0; loadTree(lp); }
      catch{ loadTree(''); }
    }
  }
  async function listProjs(){
    try{
      const r = await fetch(base + '/projects/list');
      const j = await r.json().catch(()=>({items:[]}));
      if (elProjSel){
        elProjSel.innerHTML='';
        (j.items||[]).forEach(n=>{ const o=document.createElement('option'); o.value=n; o.textContent=n; elProjSel.appendChild(o); });
        if (!curProj){
          try{ const hub=await ARW.getPrefs('ui:hub')||{}; const lp=hub.lastProject; if (lp && (j.items||[]).includes(lp)){ await setProj(lp); elProjSel.value = lp; } else if (j.items && j.items[0]) { await setProj(j.items[0]); elProjSel.value = j.items[0]; } }
          catch{ if (j.items && j.items[0]) { await setProj(j.items[0]); elProjSel.value = j.items[0]; } }
        }
      }
    }catch(e){ console.error(e); }
  }
  async function createProj(){
    const n = (elProjName?.value||'').trim(); if (!n) return;
    try{
      const resp = await fetch(base + '/projects/create', { method:'POST', headers:{'Content-Type':'application/json'}, body: JSON.stringify({ name:n }) });
      if (!resp.ok) throw new Error('HTTP '+resp.status);
      await listProjs(); setProj(n); ARW.toast('Project created');
    }catch(e){ console.error(e); ARW.toast('Create failed'); }
  }
  async function loadNotes(){ if (!curProj||!elNotes) return; try{ const r = await fetch(base + '/projects/notes?proj='+encodeURIComponent(curProj)); const t = await r.text(); elNotes.value = t; }catch(e){ console.error(e); elNotes.value=''; } }
  async function saveNotes(quiet=false){ if (!curProj||!elNotes) return; try{ const t = elNotes.value||''; await fetch(base + '/projects/notes?proj='+encodeURIComponent(curProj), { method:'POST', headers:{'Content-Type':'text/plain'}, body: t + '\n' }); const ns=document.getElementById('notesStat'); if (ns){ ns.textContent='Saved'; setTimeout(()=>{ if (ns.textContent==='Saved') ns.textContent=''; }, 1200); } if (!quiet) { /* optional toast removed for quieter UX */ } }catch(e){ console.error(e); const ns=document.getElementById('notesStat'); if (ns){ ns.textContent='Error'; setTimeout(()=>{ if (ns.textContent==='Error') ns.textContent=''; }, 1500); } }
  async function loadTree(rel){
    if (!curProj||!elProjTree) return;
    const next = String(rel||'');
    if (next !== currentPath && currentPath !== '') { pathStack.push(currentPath); }
    currentPath = next;
    try{ const ns='ui:proj:'+curProj; const p=await ARW.getPrefs(ns)||{}; p.lastPath = currentPath; await ARW.setPrefs(ns, p); }catch{}
    renderCrumbs(currentPath);
    try{
      const r = await fetch(base + '/projects/tree?proj='+encodeURIComponent(curProj)+'&path='+encodeURIComponent(currentPath));
      const j = await r.json().catch(()=>({items:[]}));
      window.__curItems = (j.items||[]);
      treeCache.set(String(currentPath||''), window.__curItems);
      await expandOnSearch((elFileFilter?.value||'').trim());
      renderTree(treeCache.get(String(currentPath||''))||[]);
    }catch(e){ console.error(e); elProjTree.textContent = 'Error'; }
  }
  async function ensureChildren(path){
    const key = String(path||'');
    if (!treeCache.has(key)){
      try{ const r=await fetch(base + '/projects/tree?proj='+encodeURIComponent(curProj)+'&path='+encodeURIComponent(key)); const j=await r.json().catch(()=>({items:[]})); treeCache.set(key, (j.items||[])); }
      catch{ treeCache.set(key, []); }
    }
    return treeCache.get(key)||[];
  }
  function prefixesOf(rel){
    const parts = String(rel||'').split('/').filter(Boolean);
    const out = [];
    for (let i=1;i<parts.length;i++){ out.push(parts.slice(0,i).join('/')); }
    return out;
  }
  async function expandOnSearch(q){
    searchExpanded = new Set();
    const needle = String(q||'').trim().toLowerCase();
    if (!needle){ return; }
    const MAX_NODES = 1000;
    let scanned = 0;
    const queue = [ String(currentPath||'') ];
    const seen = new Set(queue);
    while (queue.length && scanned < MAX_NODES){
      const dir = queue.shift();
      const items = await ensureChildren(dir);
      scanned += items.length;
      for (const it of items){
        const name = String(it.name||'').toLowerCase();
        const rel = String(it.rel||'');
        if (name.includes(needle)){
          prefixesOf(rel).forEach(p => searchExpanded.add(p));
        }
        if (it.dir && !seen.has(rel)) { seen.add(rel); queue.push(rel); }
      }
    }
  }
  function renderCrumbs(path){
    const bar = document.getElementById('crumbs'); if (!bar) return; bar.innerHTML='';
    const parts = (String(path||'').split('/').filter(Boolean));
    const rootBtn = document.createElement('button'); rootBtn.className='ghost'; rootBtn.textContent='root'; rootBtn.addEventListener('click', ()=> loadTree('')); bar.appendChild(rootBtn);
    let acc = '';
    for (const seg of parts){ const sep=document.createElement('span'); sep.className='dim'; sep.textContent=' / '; bar.appendChild(sep); acc = acc ? (acc + '/' + seg) : seg; const btn=document.createElement('button'); btn.className='ghost'; btn.textContent=seg; btn.addEventListener('click', ()=> loadTree(acc)); bar.appendChild(btn); }
  }
  async function renderInlineChildren(parentRel, host, depth){
    try{
      let items = treeCache.get(parentRel);
      if (!Array.isArray(items)){
        const r = await fetch(base + '/projects/tree?proj='+encodeURIComponent(curProj)+'&path='+encodeURIComponent(parentRel||''));
        const j = await r.json().catch(()=>({items:[]}));
        items = (j.items||[]);
        treeCache.set(parentRel, items);
      }
      // Clear and render
      host.innerHTML = '';
      const q = (elFileFilter?.value||'').toLowerCase().trim();
      for (const it of items){
        if (q){
          const match = String(it.name||'').toLowerCase().includes(q);
          const isDir = !!it.dir;
          const isExpandedBySearch = searchExpanded.has(String(it.rel||''));
          if (!match && !(isDir && isExpandedBySearch)) continue;
        }
        const row = document.createElement('div'); row.className='row'; row.style.justifyContent='space-between'; row.style.borderBottom='1px dashed #eee'; row.style.padding='4px 2px'; row.style.paddingLeft = (depth*12)+'px';
        row.setAttribute('data-row','1'); row.setAttribute('data-rel', String(it.rel||'')); row.setAttribute('data-dir', it.dir? '1':'0'); row.tabIndex = -1;
        row.setAttribute('role','treeitem'); row.setAttribute('aria-level', String((depth||0)+1));
        const nameWrap = document.createElement('div'); nameWrap.style.display='flex'; nameWrap.style.alignItems='center'; nameWrap.style.gap='6px';
        if (it.dir){ const btn=document.createElement('button'); btn.className='ghost'; btn.style.width='22px'; btn.style.padding='2px 4px'; const open=expanded.has(it.rel||'') || searchExpanded.has(String(it.rel||'')); btn.textContent = open?'▾':'▸'; btn.setAttribute('aria-label', (open? 'Collapse ':'Expand ') + (String(it.name||''))); row.setAttribute('aria-expanded', open? 'true':'false'); btn.addEventListener('click', async (e)=>{ e.stopPropagation(); if (expanded.has(it.rel||'')) expanded.delete(it.rel||''); else expanded.add(it.rel||''); try{ const ns='ui:proj:'+curProj; const p=await ARW.getPrefs(ns)||{}; p.expanded = Array.from(expanded); await ARW.setPrefs(ns, p);}catch{} await renderInlineChildren(parentRel, host, depth); }); nameWrap.appendChild(btn); } else { row.removeAttribute('aria-expanded'); }
        else { const sp=document.createElement('span'); sp.style.display='inline-block'; sp.style.width='22px'; nameWrap.appendChild(sp); }
        const name=document.createElement('div'); name.style.cursor='pointer';
        const icon = (it.dir?'📁 ':'📄 ');
        const label = String(it.name||'');
        const filterText = (elFileFilter?.value||'').trim();
        if (filterText){
          const esc = (s)=> s.replace(/&/g,'&amp;').replace(/</g,'&lt;');
          const re = new RegExp(filterText.replace(/[-/\\^$*+?.()|[\]{}]/g,'\\$&'), 'ig');
          const marked = esc(label).replace(re, (m)=> '<span class="hl-match">'+esc(m)+'</span>');
          name.innerHTML = icon + marked;
        } else {
          name.textContent = icon + label;
        }
        name.addEventListener('click', ()=>{ if (it.dir) loadTree(it.rel||''); else filePreview(it.rel||''); }); nameWrap.appendChild(name);
        const actions=document.createElement('div'); actions.className='row';
        if (!it.dir){
          const copyBtn=document.createElement('button'); copyBtn.className='ghost'; copyBtn.textContent='Copy'; copyBtn.title='Copy file contents to clipboard'; copyBtn.addEventListener('click', async ()=>{ try{ const rel=it.rel||''; const cached=fileCache.get(rel); const fresh=cached&&(Date.now()-cached.t<fileTTL); let j=fresh?cached.data:await (await fetch(base + '/projects/file?proj='+encodeURIComponent(curProj)+'&path='+encodeURIComponent(rel))).json(); if(!fresh) fileCache.set(rel,{data:j,t:Date.now()}); ARW.copy(String(j.content||'')); }catch(e){ console.error(e); ARW.toast('Copy failed'); } }); actions.appendChild(copyBtn);
          const openBtn=document.createElement('button'); openBtn.className='ghost'; openBtn.textContent='Open'; openBtn.title='Open with system default'; openBtn.addEventListener('click', async ()=>{ try{ const rel=it.rel||''; const cached=fileCache.get(rel); const fresh=cached&&(Date.now()-cached.t<fileTTL); let j=fresh?cached.data:await (await fetch(base + '/projects/file?proj='+encodeURIComponent(curProj)+'&path='+encodeURIComponent(rel))).json(); if(!fresh) fileCache.set(rel,{data:j,t:Date.now()}); if(j&&j.abs_path){ await ARW.invoke('open_path',{path:j.abs_path}); } else { ARW.toast('Path unavailable'); } }catch(e){ console.error(e); ARW.toast('Open failed'); } }); actions.appendChild(openBtn);
          const editOpen=document.createElement('button'); editOpen.className='ghost'; editOpen.textContent='Open in Editor'; editOpen.title='Open in configured editor'; editOpen.addEventListener('click', async ()=>{ try{ const rel=it.rel||''; const cached=fileCache.get(rel); const fresh=cached&&(Date.now()-cached.t<fileTTL); let j=fresh?cached.data:await (await fetch(base + '/projects/file?proj='+encodeURIComponent(curProj)+'&path='+encodeURIComponent(rel))).json(); if(!fresh) fileCache.set(rel,{data:j,t:Date.now()}); if(j&&j.abs_path){ const eff=(projPrefs&&projPrefs.editorCmd)||((await ARW.getPrefs('launcher'))||{}).editorCmd||null; await ARW.invoke('open_in_editor',{path:j.abs_path, editor_cmd: eff}); } else { ARW.toast('Path unavailable'); } }catch(e){ console.error(e); ARW.toast('Open in Editor failed'); } }); actions.appendChild(editOpen);
        }
        const left=document.createElement('div'); left.style.display='flex'; left.style.alignItems='center'; left.style.gap='6px'; left.appendChild(nameWrap); row.appendChild(left); row.appendChild(actions); host.appendChild(row);
        if (it.dir && (expanded.has(it.rel||'') || searchExpanded.has(String(it.rel||'')))){
          const childHost=document.createElement('div'); childHost.setAttribute('role','group'); host.appendChild(childHost);
          await renderInlineChildren(it.rel||'', childHost, depth+1);
        }
      }
    }catch(e){ console.error(e); }
  }
  // Keyboard navigation for the tree
  function collectRows(){ return Array.from(elProjTree.querySelectorAll('[data-row="1"]')); }
  function parentOf(rel){ const s=String(rel||''); const i=s.lastIndexOf('/'); return i>=0? s.slice(0,i): ''; }
  function focusRowByIndex(idx){ const rows=collectRows(); if(!rows.length) return; idx=Math.max(0,Math.min(idx,rows.length-1)); rows.forEach(r=> { r.tabIndex=-1; r.setAttribute('aria-selected','false'); }); const row=rows[idx]; row.tabIndex=0; row.setAttribute('aria-selected','true'); row.focus({ preventScroll:true }); }
  function focusRowByRel(rel){ const rows=collectRows(); const i=rows.findIndex(r=> (r.getAttribute('data-rel')||'')===String(rel||'')); if (i>=0) focusRowByIndex(i); }
  elProjTree.addEventListener('keydown', async (e)=>{
    const rows = collectRows(); if (!rows.length) return;
    const active = document.activeElement; const curIdx = rows.findIndex(r=> r===active || r.contains(active));
    const cur = curIdx>=0? rows[curIdx]: rows[0]; const rel=cur.getAttribute('data-rel')||''; const isDir = cur.getAttribute('data-dir')==='1';
    if (e.key==='ArrowDown'){ e.preventDefault(); focusRowByIndex((curIdx>=0?curIdx:0)+1); }
    else if (e.key==='ArrowUp'){ e.preventDefault(); focusRowByIndex((curIdx>=0?curIdx:0)-1); }
    else if (e.key==='Home'){ e.preventDefault(); focusRowByIndex(0); }
    else if (e.key==='End'){ e.preventDefault(); focusRowByIndex(rows.length-1); }
    else if (e.key==='ArrowRight'){
      if (!isDir) return; e.preventDefault();
      const open = expanded.has(rel) || searchExpanded.has(rel);
      if (!open){ expanded.add(rel); try{ const ns='ui:proj:'+curProj; const p=await ARW.getPrefs(ns)||{}; p.expanded = Array.from(expanded); await ARW.setPrefs(ns, p);}catch{} await renderInlineChildren(parentOf(rel), cur.parentElement, (cur.style.paddingLeft? (parseInt(cur.style.paddingLeft)/12):0)); focusRowByRel(rel); }
      else { const kids = await ensureChildren(rel); if (kids && kids.length){ focusRowByRel(kids[0].rel||''); } }
    }
    else if (e.key==='ArrowLeft'){
      e.preventDefault();
      if (isDir && expanded.has(rel)){ expanded.delete(rel); try{ const ns='ui:proj:'+curProj; const p=await ARW.getPrefs(ns)||{}; p.expanded = Array.from(expanded); await ARW.setPrefs(ns, p);}catch{} await renderInlineChildren(parentOf(rel), cur.parentElement, (cur.style.paddingLeft? (parseInt(cur.style.paddingLeft)/12):0)); focusRowByRel(rel); }
      else { focusRowByRel(parentOf(rel)); }
    }
    else if (e.key==='Enter'){
      e.preventDefault(); if (isDir) loadTree(rel); else filePreview(rel);
    }
  });
  // Global shortcuts (avoid when typing in inputs)
  window.addEventListener('keydown', (e)=>{
    const tag = (e.target && e.target.tagName || '').toLowerCase();
    const typing = tag==='input' || tag==='textarea' || tag==='select' || e.metaKey || e.ctrlKey || e.altKey;
    if (typing) return;
    if (e.key === '/') { const ff = document.getElementById('fileFilter'); if (ff) { e.preventDefault(); ff.focus({ preventScroll:true }); } }
    else if (e.key.toLowerCase() === 'n') { const np = document.getElementById('projName'); if (np) { e.preventDefault(); np.focus({ preventScroll:true }); } }
    else if (e.key.toLowerCase() === 'b') { const bb = document.getElementById('btnTreeBack'); if (bb) { e.preventDefault(); bb.click(); } }
  });
  function renderTree(items){
    elProjTree.innerHTML='';
    const wrap = document.createElement('div');
    // Top-level rows
    const topHost = document.createElement('div'); wrap.appendChild(topHost);
    // Render top-level and any expanded children beneath
    (async ()=>{ await renderInlineChildren(String(currentPath||''), topHost, 0); })();
    elProjTree.appendChild(wrap);
    const prev = document.createElement('div'); prev.id='treePrev'; elProjTree.appendChild(prev);
    elProjTree.innerHTML='';
    const wrap = document.createElement('div');
    const q = (elFileFilter?.value||'').toLowerCase().trim();
    (items||[]).filter(it=>{ if (!q) return true; const n=(it.name||'').toLowerCase(); return n.includes(q); }).forEach(it=>{
      const row = document.createElement('div'); row.className='row'; row.style.justifyContent='space-between'; row.style.borderBottom='1px dashed #eee'; row.style.padding='4px 2px';
      const name = document.createElement('div'); name.style.cursor='pointer'; name.textContent = (it.dir?'📁 ':'📄 ') + (it.name||'');
      name.addEventListener('click', ()=>{ if (it.dir) loadTree(it.rel||''); else filePreview(it.rel||''); });
      const actions = document.createElement('div'); actions.className='row';
      if (!it.dir){
        const copyBtn=document.createElement('button'); copyBtn.className='ghost'; copyBtn.textContent='Copy'; copyBtn.addEventListener('click', async ()=>{
          try{
            const rel = it.rel||''; const cached = fileCache.get(rel); const fresh = cached && (Date.now()-cached.t < fileTTL);
            let j = fresh ? cached.data : await (await fetch(base + '/projects/file?proj='+encodeURIComponent(curProj)+'&path='+encodeURIComponent(rel))).json();
            if (!fresh) fileCache.set(rel, { data: j, t: Date.now() });
            ARW.copy(String(j.content||''));
          }catch(e){ console.error(e); ARW.toast('Copy failed'); }
        }); actions.appendChild(copyBtn);
        const openBtn=document.createElement('button'); openBtn.className='ghost'; openBtn.textContent='Open'; openBtn.addEventListener('click', async ()=>{
          try{
            const rel = it.rel||''; const cached = fileCache.get(rel); const fresh = cached && (Date.now()-cached.t < fileTTL);
            let j = fresh ? cached.data : await (await fetch(base + '/projects/file?proj='+encodeURIComponent(curProj)+'&path='+encodeURIComponent(rel))).json();
            if (!fresh) fileCache.set(rel, { data: j, t: Date.now() });
            if (j && j.abs_path) { await ARW.invoke('open_path', { path: j.abs_path }); } else { ARW.toast('Path unavailable'); }
          }catch(e){ console.error(e); ARW.toast('Open failed'); }
        }); actions.appendChild(openBtn);
        const editOpen=document.createElement('button'); editOpen.className='ghost'; editOpen.textContent='Open in Editor'; editOpen.addEventListener('click', async ()=>{
          try{
            const rel = it.rel||''; const cached = fileCache.get(rel); const fresh = cached && (Date.now()-cached.t < fileTTL);
            let j = fresh ? cached.data : await (await fetch(base + '/projects/file?proj='+encodeURIComponent(curProj)+'&path='+encodeURIComponent(rel))).json();
            if (!fresh) fileCache.set(rel, { data: j, t: Date.now() });
            if (j && j.abs_path) { const eff = (projPrefs&&projPrefs.editorCmd) || ((await ARW.getPrefs('launcher'))||{}).editorCmd || null; await ARW.invoke('open_in_editor', { path: j.abs_path, editor_cmd: eff }); } else { ARW.toast('Path unavailable'); }
          }catch(e){ console.error(e); ARW.toast('Open in Editor failed'); }
        }); actions.appendChild(editOpen);
      }
      row.appendChild(name); row.appendChild(actions); wrap.appendChild(row);
    });
    elProjTree.appendChild(wrap);
    const prev = document.createElement('div'); prev.id='treePrev'; elProjTree.appendChild(prev);
    // Drag & drop upload onto tree area
    elProjTree.addEventListener('dragover', (e)=>{ e.preventDefault(); elProjTree.style.outline='2px dashed #b87333'; });
    elProjTree.addEventListener('dragleave', ()=>{ elProjTree.style.outline=''; });
    elProjTree.addEventListener('drop', async (e)=>{
      e.preventDefault(); elProjTree.style.outline='';
      const list = e.dataTransfer?.files || [];
      if (!curProj || !list.length) return;
      for (const f of list){
        try{
          let dest = currentPath ? (currentPath.replace(/\/$/, '') + '/' + f.name) : f.name;
          let exists = false;
          try{ const r = await fetch(base + '/projects/file?proj='+encodeURIComponent(curProj)+'&path='+encodeURIComponent(dest)); exists = r.ok; }catch{}
          if (exists){ const ow = confirm('Overwrite '+dest+'?'); if (!ow){ const m=f.name.match(/^(.*?)(\.[^.]*)?$/); const baseN=m?m[1]:f.name; const ext=m?m[2]||'':''; dest = (currentPath? currentPath+'/' : '') + baseN + ' (copy)' + ext; } }
          if (f.size > 10*1024*1024){ alert('File too large (max 10 MiB)'); continue; }
          let body = {};
          if ((f.type||'').startsWith('text/') || f.size < 256*1024){ const t = await f.text(); body = { content: t, prev_sha256: null }; }
          else {
            const ab = await f.arrayBuffer();
            const b64 = (function(u8){ let bin=''; const CHUNK=0x8000; for(let i=0;i<u8.length;i+=CHUNK){ bin += String.fromCharCode.apply(null, u8.subarray(i,i+CHUNK)); } return btoa(bin); })(new Uint8Array(ab));
            body = { content_b64: b64, prev_sha256: null };
          }
          const resp = await fetch(base + '/projects/file?proj='+encodeURIComponent(curProj)+'&path='+encodeURIComponent(dest), { method:'POST', headers:{'Content-Type':'application/json'}, body: JSON.stringify(body) });
          if (!resp.ok){ console.warn('Upload failed', f.name, resp.status); continue; }
        }catch(err){ console.error('Upload error', err); }
      }
      await loadTree(currentPath);
    });
  }
  elFileFilter?.addEventListener('input', ()=> renderTree(treeCache.get(String(currentPath||''))||[]));
  const btnBack = document.getElementById('btnTreeBack'); if (btnBack) btnBack.addEventListener('click', ()=>{ const prev = pathStack.pop(); loadTree(prev||''); });
  async function filePreview(rel){
    try{
      const r=await fetch(base + '/projects/file?proj='+encodeURIComponent(curProj)+'&path='+encodeURIComponent(rel||''));
      const j=await r.json();
      try{ fileCache.set(rel||'', { data: j, t: Date.now() }); }catch{}
      const prev=document.getElementById('treePrev'); if (!prev) return;
      prev.innerHTML='';
      // Header line
      const cap=document.createElement('div'); cap.className='dim mono'; cap.textContent = String(rel||''); prev.appendChild(cap);
      // Action row
      const row=document.createElement('div'); row.className='row';
      const editBtn=document.createElement('button'); editBtn.className='ghost'; editBtn.textContent='Edit'; editBtn.title='Edit this file inline';
      const saveBtn=document.createElement('button'); saveBtn.className='primary'; saveBtn.textContent='Save'; saveBtn.title='Save changes'; saveBtn.style.display='none';
      const revertBtn=document.createElement('button'); revertBtn.className='ghost'; revertBtn.textContent='Revert'; revertBtn.title='Revert to last loaded content'; revertBtn.style.display='none';
      const openEditor=document.createElement('button'); openEditor.className='ghost'; openEditor.textContent='Open in Editor';
      row.appendChild(editBtn); row.appendChild(saveBtn); row.appendChild(revertBtn); row.appendChild(openEditor); prev.appendChild(row);
      // Preview and editor
      const pre=document.createElement('pre'); pre.className='mono'; pre.style.maxHeight='140px'; pre.style.overflow='auto'; pre.textContent = String(j.content||'');
      const ta=document.createElement('textarea'); ta.style.width='100%'; ta.style.minHeight='140px'; ta.style.display='none'; ta.value = String(j.content||'');
      prev.appendChild(pre); prev.appendChild(ta);
      // State for sha
      let sha = j.sha256 || null;
      const pathRel = rel;
      function toggleEditing(on){ pre.style.display = on? 'none':'block'; ta.style.display = on? 'block':'none'; editBtn.style.display = on? 'none':'inline-block'; saveBtn.style.display = on? 'inline-block':'none'; revertBtn.style.display = on? 'inline-block':'none'; }
      editBtn.addEventListener('click', ()=> toggleEditing(true));
      openEditor.addEventListener('click', async ()=>{ try{ const rel = pathRel||''; const cached = fileCache.get(rel); const fresh = cached && (Date.now()-cached.t < fileTTL); let jx = fresh ? cached.data : await (await fetch(base + '/projects/file?proj='+encodeURIComponent(curProj)+'&path='+encodeURIComponent(rel))).json(); if (!fresh) fileCache.set(rel, { data: jx, t: Date.now() }); if (jx && jx.abs_path) { const eff = (projPrefs&&projPrefs.editorCmd) || ((await ARW.getPrefs('launcher'))||{}).editorCmd || null; await ARW.invoke('open_in_editor', { path: jx.abs_path, editor_cmd: eff }); } else { ARW.toast('Path unavailable'); } }catch(e){ console.error(e); ARW.toast('Open in Editor failed'); } });
      revertBtn.addEventListener('click', ()=>{ ta.value = String(j.content||''); toggleEditing(false); });
      saveBtn.addEventListener('click', async ()=>{
        try{
          const body = { content: ta.value||'', prev_sha256: sha };
          const resp = await fetch(base + '/projects/file?proj='+encodeURIComponent(curProj)+'&path='+encodeURIComponent(pathRel||''), { method:'POST', headers:{'Content-Type':'application/json'}, body: JSON.stringify(body) });
          if (!resp.ok){
            if (resp.status === 409){
              // Conflict: fetch latest and present a simple merge panel
              try{
                const r3=await fetch(base + '/projects/file?proj='+encodeURIComponent(curProj)+'&path='+encodeURIComponent(pathRel||''));
                const j3=await r3.json();
                const merge = document.createElement('div'); merge.className='card'; merge.style.marginTop='6px';
                const h=document.createElement('div'); h.className='row'; h.innerHTML = '<strong>Merge needed</strong><span class="dim">Server changed since you loaded this file</span>';
                const help=document.createElement('p'); help.className='sr-only'; help.id='mergeHelp'; help.textContent='Conflict detected. You can replace the editor with the server version, or save your version by overwriting the server. Use Diff to inspect line differences.';
                const controls=document.createElement('div'); controls.className='row';
                const useServer=document.createElement('button'); useServer.className='ghost'; useServer.textContent='Replace with server'; useServer.title='Load server version into editor'; useServer.setAttribute('aria-describedby','mergeHelp');
                const saveMine=document.createElement('button'); saveMine.className='primary'; saveMine.textContent='Save my version'; saveMine.title='Overwrite server version with your changes'; saveMine.setAttribute('aria-describedby','mergeHelp');
                const copyServer=document.createElement('button'); copyServer.className='ghost'; copyServer.textContent='Copy server'; copyServer.title='Copy server content to clipboard'; copyServer.setAttribute('aria-describedby','mergeHelp');
                const showDiff=document.createElement('button'); showDiff.className='ghost'; showDiff.textContent='Diff'; showDiff.title='Show differences between your version and server'; showDiff.setAttribute('aria-describedby','mergeHelp');
                controls.appendChild(useServer); controls.appendChild(saveMine); controls.appendChild(copyServer); controls.appendChild(showDiff);
                const grid=document.createElement('div'); grid.className='grid cols-2';
                const left=document.createElement('div'); const lt=document.createElement('textarea'); lt.style.width='100%'; lt.style.minHeight='140px'; lt.value = ta.value||''; left.appendChild(lt);
                const right=document.createElement('div'); const rp=document.createElement('pre'); rp.className='mono'; rp.style.maxHeight='140px'; rp.style.overflow='auto'; rp.style.whiteSpace='pre'; rp.textContent = String(j3.content||''); right.appendChild(rp);
                grid.appendChild(left); grid.appendChild(right);
                merge.appendChild(h); merge.appendChild(help); merge.appendChild(controls); merge.appendChild(grid);
                const diffOut=document.createElement('div'); diffOut.className='cmp-code'; diffOut.style.marginTop='6px'; merge.appendChild(diffOut);
                prev.appendChild(merge);
                // Wire actions
                copyServer.addEventListener('click', ()=> ARW.copy(String(j3.content||'')));
                useServer.addEventListener('click', ()=>{ ta.value = String(j3.content||''); lt.value = String(j3.content||''); merge.remove(); ARW.toast('Server version loaded'); });
                saveMine.addEventListener('click', async ()=>{
                  try{
                    const body2 = { content: lt.value||'', prev_sha256: j3.sha256||null };
                    const resp2 = await fetch(base + '/projects/file?proj='+encodeURIComponent(curProj)+'&path='+encodeURIComponent(pathRel||''), { method:'POST', headers:{'Content-Type':'application/json'}, body: JSON.stringify(body2) });
                    if (!resp2.ok){ ARW.toast('Save failed'); return; }
                    const r4=await fetch(base + '/projects/file?proj='+encodeURIComponent(curProj)+'&path='+encodeURIComponent(pathRel||''));
                    const j4=await r4.json(); sha=j4.sha256||null; j.content=j4.content||''; pre.textContent=String(j.content||''); ta.value=j4.content||''; toggleEditing(false); merge.remove(); ARW.toast('Saved');
                  }catch(e){ console.error(e); ARW.toast('Save failed'); }
                });
                // Scroll sync
                let syncing = false;
                const sync = (src, dst)=>{
                  if (syncing) return; syncing = true;
                  try{
                    const ratio = src.scrollTop / Math.max(1, (src.scrollHeight - src.clientHeight));
                    dst.scrollTop = ratio * (dst.scrollHeight - dst.clientHeight);
                  }finally{ syncing = false; }
                };
                lt.addEventListener('scroll', ()=> sync(lt, rp));
                rp.addEventListener('scroll', ()=> sync(rp, lt));

                showDiff.addEventListener('click', ()=>{
                  try{
                    // Use existing diff/highlight helpers
                    const a = lt.value || '';
                    const b = String(j3.content||'');
                    diffOut.innerHTML='';
                    const wrap = document.createDocumentFragment();
                    const frag = (function(){
                      const aa = String(a).split(/\r?\n/);
                      const bb = String(b).split(/\r?\n/);
                      const n = Math.max(aa.length, bb.length);
                      const f = document.createDocumentFragment();
                      const hl = (s)=>{
                        const esc = (t)=> t.replace(/&/g,'&amp;').replace(/</g,'&lt;');
                        let txt = s; try{ txt = JSON.stringify(JSON.parse(s), null, 2) }catch{}
                        return esc(txt)
                          .replace(/\b(true|false)\b/g, '<span class="hl-bool">$1<\/span>')
                          .replace(/\b(null)\b/g, '<span class="hl-null">$1<\/span>')
                          .replace(/\"([^\"]+)\"\s*\:/g, '<span class="hl-key">"$1"<\/span>:')
                          .replace(/:\s*\"([^\"]*)\"/g, ': <span class="hl-str">"$1"<\/span>')
                          .replace(/:\s*(-?\d+(?:\.\d+)?)/g, ': <span class="hl-num">$1<\/span>');
                      };
                      for(let i=0;i<n;i++){
                        const la = aa[i] ?? '';
                        const lb = bb[i] ?? '';
                        if(la === lb){ const div = document.createElement('div'); div.className='cmp-line'; div.textContent=la; f.appendChild(div); }
                        else { const di = document.createElement('div'); di.className='cmp-line del'; di.innerHTML = hl(la); f.appendChild(di); const da = document.createElement('div'); da.className='cmp-line add'; da.innerHTML = hl(lb); f.appendChild(da); }
                      }
                      return f;
                    })();
                    diffOut.appendChild(frag);
                  }catch(e){ console.error(e); }
                });
              }catch(e){ console.error(e); ARW.toast('Conflict: reload file'); }
              return;
            }
            ARW.toast('Save failed'); return;
          }
          // reload to get new sha/content
          const r2=await fetch(base + '/projects/file?proj='+encodeURIComponent(curProj)+'&path='+encodeURIComponent(pathRel||''));
          const j2=await r2.json();
          sha = j2.sha256||null; j.content = j2.content||''; pre.textContent = String(j.content||''); toggleEditing(false); ARW.toast('Saved');
        }catch(e){ console.error(e); ARW.toast('Save failed'); }
      });
    }catch(e){ console.error(e); }
  }
  // Wire events
  if (elProjSel) elProjSel.addEventListener('change', ()=> setProj(elProjSel.value||''));
  const btnCreate = document.getElementById('btnCreateProj'); if (btnCreate) btnCreate.addEventListener('click', createProj);
  const btnRefresh = document.getElementById('btnRefreshProj'); if (btnRefresh) btnRefresh.addEventListener('click', ()=>{ listProjs(); if (curProj) loadTree(''); });
  const btnSaveNotes = document.getElementById('btnSaveNotes'); if (btnSaveNotes) btnSaveNotes.addEventListener('click', saveNotes);
  const btnReloadNotes = document.getElementById('btnReloadNotes'); if (btnReloadNotes) btnReloadNotes.addEventListener('click', loadNotes);
  if (elFileFilter){ elFileFilter.addEventListener('input', async ()=>{ try{ await expandOnSearch((elFileFilter.value||'').trim()); }catch{} renderTree(treeCache.get(String(currentPath||''))||[]); }); }
  // Debounced autosave for notes
  let notesTimer = null;
  if (elNotes){ elNotes.addEventListener('input', async ()=>{ try{ const on = elNotesAutosave ? !!elNotesAutosave.checked : true; if (!on) return; if (notesTimer) clearTimeout(notesTimer); notesTimer = setTimeout(()=> saveNotes(true), 1200); }catch{} }); }
  if (elNotesAutosave){ elNotesAutosave.addEventListener('change', async ()=>{ try{ if (!curProj) return; const ns='ui:proj:'+curProj; const p=await ARW.getPrefs(ns)||{}; p.notesAutoSave = !!elNotesAutosave.checked; await ARW.setPrefs(ns, p); }catch{} }); }
  // Project prefs (per-project overrides like editor)
  const btnProjPrefs = document.getElementById('btnProjPrefs');
  if (btnProjPrefs) btnProjPrefs.addEventListener('click', async ()=>{
    if (!curProj) return;
    try{ const ns='ui:proj:'+curProj; const cur=await ARW.getPrefs(ns)||{}; const prev=cur.editorCmd||''; const next = prompt('Project editor command (use {path} placeholder)', prev||''); if (next != null){ cur.editorCmd = String(next).trim(); await ARW.setPrefs(ns, cur); projPrefs = cur; if (elProjPrefsBadge) elProjPrefsBadge.style.display = (cur.editorCmd? 'inline-flex':'none'); ARW.toast('Project prefs saved'); } }
    catch(e){ console.error(e); ARW.toast('Save failed'); }
  });
  await listProjs();
  // Quick state probe (models count)
  try {
    const r = await fetch(base + '/state/models');
    const j = await r.json();
    const sec = document.querySelector('#agents');
    if (sec) {
      const b = document.createElement('div');
      b.className = 'badge';
      b.innerHTML = `<span class="dot"></span> models: ${(Array.isArray(j)?j.length:(j?.data?.length||0))}`;
      sec.querySelector('h3')?.appendChild(b);
    }
  } catch {}

  // Compare: JSON-highlighting line diff
  function highlightJSON(s){
    const esc = (t)=> t.replace(/&/g,'&amp;').replace(/</g,'&lt;');
    let txt = s; try{ txt = JSON.stringify(JSON.parse(s), null, 2) }catch{}
    return esc(txt)
      .replace(/\b(true|false)\b/g, '<span class="hl-bool">$1<\/span>')
      .replace(/\b(null)\b/g, '<span class="hl-null">$1<\/span>')
      .replace(/\"([^\"]+)\"\s*\:/g, '<span class="hl-key">"$1"<\/span>:')
      .replace(/:\s*\"([^\"]*)\"/g, ': <span class="hl-str">"$1"<\/span>')
      .replace(/:\s*(-?\d+(?:\.\d+)?)/g, ': <span class="hl-num">$1<\/span>');
  }
  function diffLines(a, b){
    const aa = String(a||'').split(/\r?\n/);
    const bb = String(b||'').split(/\r?\n/);
    const n = Math.max(aa.length, bb.length);
    const frag = document.createDocumentFragment();
    for(let i=0;i<n;i++){
      const la = aa[i] ?? '';
      const lb = bb[i] ?? '';
      if(la === lb){ const div = document.createElement('div'); div.className='cmp-line'; div.textContent=la; frag.appendChild(div); }
      else {
        const di = document.createElement('div'); di.className='cmp-line del'; di.innerHTML = highlightJSON(la); frag.appendChild(di);
        const da = document.createElement('div'); da.className='cmp-line add'; da.innerHTML = highlightJSON(lb); frag.appendChild(da);
      }
    }
    return frag;
  }
  document.getElementById('btn-diff').addEventListener('click', ()=>{
    const a = document.getElementById('cmpA').value;
    const b = document.getElementById('cmpB').value;
    const onlyChanges = document.getElementById('txtOnlyChanges').checked;
    const wrap = document.getElementById('txtWrap').checked;
    const out = document.getElementById('cmpOut'); out.innerHTML='';
    out.style.whiteSpace = wrap ? 'pre-wrap' : 'pre';
    const frag = diffLines(a,b);
    if (onlyChanges){
      [...frag.childNodes].forEach(node=>{ if (node.classList && !/add|del/.test(node.className)) node.remove(); });
    }
    out.appendChild(frag);
  });
  document.getElementById('btn-copy-diff').addEventListener('click', ()=>{
    const txt = document.getElementById('cmpOut').innerText || '';
    ARW.copy(txt);
  });
  function wireDrop(id){
    const el = document.getElementById(id);
    el.addEventListener('dragover', (e)=>{ e.preventDefault(); el.style.outline='2px dashed #b87333' });
    el.addEventListener('dragleave', ()=> el.style.outline='');
    el.addEventListener('drop', async (e)=>{
      e.preventDefault(); el.style.outline='';
      const f = e.dataTransfer?.files?.[0]; if(!f) return;
      try{
        const text = await f.text(); el.value = text;
      }catch{}
    });
  }
  wireDrop('cmpA'); wireDrop('cmpB');
  // Image compare
  function loadImg(input, imgEl){ input.addEventListener('change', ()=>{ const f = input.files?.[0]; if(!f) return; const url = URL.createObjectURL(f); imgEl.src = url; }); }
  loadImg(document.getElementById('imgA'), document.getElementById('imgOver'));
  loadImg(document.getElementById('imgB'), document.getElementById('imgUnder'));
  document.getElementById('imgSlider').addEventListener('input', (e)=>{
    const v = parseInt(e.target.value,10); const clip = 100 - v;
    document.getElementById('imgOver').style.clipPath = `inset(0 ${clip}% 0 0)`;
  });
  // Tabs
  const tabText = document.getElementById('tab-text'); const tabImg = document.getElementById('tab-image');
  const tabCsv = document.getElementById('tab-csv');
  function showTab(which){
    const map = { text:'cmpText', image:'cmpImage', csv:'cmpCSV' };
    const pText = document.getElementById('cmpText'); const pImg = document.getElementById('cmpImage'); const pCsv = document.getElementById('cmpCSV');
    // visibility
    pText.style.display = which==='text' ? 'block':'none'; pText.toggleAttribute('hidden', which!=='text');
    pImg.style.display = which==='image' ? 'block':'none'; pImg.toggleAttribute('hidden', which!=='image');
    pCsv.style.display = which==='csv' ? 'block':'none'; pCsv.toggleAttribute('hidden', which!=='csv');
    // tabs active + aria-selected/tabindex
    const setTab = (el, on)=>{ el.classList.toggle('active', on); el.setAttribute('aria-selected', on? 'true':'false'); el.tabIndex = on? 0 : -1; };
    setTab(tabText, which==='text'); setTab(tabImg, which==='image'); setTab(tabCsv, which==='csv');
  }
  tabText.addEventListener('click', ()=> showTab('text'));
  tabImg.addEventListener('click', ()=> showTab('image'));
  tabCsv.addEventListener('click', ()=> showTab('csv'));
  // Tabs keyboard navigation (roving tabindex)
  const tabList = document.querySelector('.cmp-tabs[role="tablist"]');
  tabList?.addEventListener('keydown', (e)=>{
    const tabs = [tabText, tabImg, tabCsv];
    const cur = document.activeElement;
    let idx = tabs.indexOf(cur);
    if (e.key==='ArrowRight'){ e.preventDefault(); idx = (idx+1+tabs.length)%tabs.length; tabs[idx].focus({ preventScroll:true }); tabs[idx].click(); }
    else if (e.key==='ArrowLeft'){ e.preventDefault(); idx = (idx-1+tabs.length)%tabs.length; tabs[idx].focus({ preventScroll:true }); tabs[idx].click(); }
    else if (e.key==='Home'){ e.preventDefault(); tabs[0].focus({ preventScroll:true }); tabs[0].click(); }
    else if (e.key==='End'){ e.preventDefault(); tabs[tabs.length-1].focus({ preventScroll:true }); tabs[tabs.length-1].click(); }
  });

  // CSV/Table compare
  function detectDelim(s){ const tc=(s.match(/\t/g)||[]).length; const cc=(s.match(/,/g)||[]).length; return tc>cc?'\t':',' }
  function parseCSV(text, hasHeader){
    const delim = detectDelim(text);
    const rows = []; let row = []; let cur=''; let q=false; for(let i=0;i<text.length;i++){
      const ch = text[i];
      if (q){ if (ch==='"'){ if (text[i+1]==='"'){ cur+='"'; i++; } else { q=false; } } else { cur+=ch; } continue; }
      if (ch==='"'){ q=true; continue; }
      if (ch===delim){ row.push(cur); cur=''; continue; }
      if (ch==='\n'){ row.push(cur); rows.push(row); row=[]; cur=''; continue; }
      if (ch==='\r'){ continue; }
      cur += ch;
    }
    if (cur.length>0 || row.length>0) { row.push(cur); rows.push(row); }
    if (!rows.length) return { headers: [], rows: [] };
    let headers = [];
    if (hasHeader){ headers = rows.shift(); }
    else { const maxLen = Math.max(...rows.map(r=>r.length)); headers = Array.from({length:maxLen}, (_,i)=> 'col'+(i+1)); }
    const out = rows.map(r=>{ const o={}; headers.forEach((h,idx)=> o[h]=r[idx]??''); return o; });
    return { headers, rows: out };
  }
  function diffTables(aText, bText, keyColsStr, hasHeader){
    const A = parseCSV(aText, hasHeader); const B = parseCSV(bText, hasHeader);
    const headers = Array.from(new Set([...(A.headers||[]), ...(B.headers||[])]));
    const keys = (keyColsStr||'').split(',').map(s=>s.trim()).filter(Boolean);
    const ks = keys.length? keys : [headers[0]].filter(Boolean);
    const kfun = (o)=> ks.map(k=> (o[k]??'')).join('||');
    const mapA = new Map(A.rows.map(r=> [kfun(r), r]));
    const mapB = new Map(B.rows.map(r=> [kfun(r), r]));
    const added=[], removed=[], changed=[];
    // added/changed
    for (const [k, rb] of mapB.entries()){
      const ra = mapA.get(k);
      if (!ra) { added.push({k, r: rb}); continue; }
      let diff=false; for (const h of headers){ if ((ra[h]||'') !== (rb[h]||'')) { diff=true; break; } }
      if (diff) changed.push({k, a: ra, b: rb});
    }
    for (const [k, ra] of mapA.entries()) if (!mapB.has(k)) removed.push({k, r: ra});
    return { headers, ks, added, removed, changed };
  }
  function renderTableDiff(res){
    const out = document.getElementById('csvOut'); const sum = document.getElementById('csvSummary'); out.innerHTML=''; sum.textContent='';
    window.__lastCsvDiff = res;
    sum.textContent = `+${res.added.length} −${res.removed.length} Δ${res.changed.length}`;
    const tbl = document.createElement('table'); tbl.className='cmp-table';
    const thead = document.createElement('thead'); thead.innerHTML = '<tr>' + ['key', ...res.headers].map(h=>`<th>${h}</th>`).join('') + '</tr>'; tbl.appendChild(thead);
    const tb = document.createElement('tbody');
    const addRows = res.added.map(x=>({ type:'add', key:x.k, row:x.r }));
    const delRows = res.removed.map(x=>({ type:'del', key:x.k, row:x.r }));
    const chRows = res.changed.map(x=>({ type:'chg', key:x.k, a:x.a, b:x.b }));
    const all = [...addRows, ...delRows, ...chRows].sort((x,y)=> String(x.key).localeCompare(String(y.key)));
    for (const e of all){
      const tr = document.createElement('tr'); tr.className = 'cmp-row ' + (e.type==='add'?'add': e.type==='del'?'del':'');
      const keytd = document.createElement('td'); keytd.textContent = e.key; keytd.className='mono'; tr.appendChild(keytd);
      for (const h of res.headers){
        const td = document.createElement('td');
        if (e.type==='chg'){
          const av = e.a[h]||''; const bv = e.b[h]||'';
          if (av !== bv){ td.className='cmp-cell diff'; td.title = `was: ${av}`; td.textContent = bv; }
          else { td.textContent = bv; }
        } else {
          td.textContent = e.row[h]||'';
        }
        tr.appendChild(td);
      }
      tb.appendChild(tr);
    }
    tbl.appendChild(tb); out.appendChild(tbl);
  }
  document.getElementById('btn-csv-diff').addEventListener('click', ()=>{
    const a = document.getElementById('csvA').value; const b = document.getElementById('csvB').value;
    const key = document.getElementById('csvKey').value; const hasHeader = document.getElementById('csvHeader').checked;
    const res = diffTables(a,b,key,hasHeader);
    if (document.getElementById('csvOnlyChanges').checked){
      res.added = res.added; res.removed = res.removed; res.changed = res.changed; // no-op; already only changed
    }
    renderTableDiff(res);
  });
  function downloadCsv(filename, rows){
    const csv = rows.map(r => r.map(v => /[",\n]/.test(String(v)) ? '"'+String(v).replace(/"/g,'""')+'"' : v).join(',')).join('\n');
    const blob = new Blob([csv], { type: 'text/csv;charset=utf-8;' });
    const link = document.createElement('a'); link.href = URL.createObjectURL(blob); link.download = filename; document.body.appendChild(link); link.click(); document.body.removeChild(link);
  }
  document.getElementById('btn-csv-export').addEventListener('click', ()=>{
    const d = window.__lastCsvDiff; if (!d) { ARW.toast('No diff to export'); return; }
    const twoRow = !!document.getElementById('csvTwoRow')?.checked;
    let rows = [['type','key', ...d.headers]];
    d.added.forEach(x=> rows.push(['add', x.k, ...d.headers.map(h=> x.r[h]||'')]));
    d.removed.forEach(x=> rows.push(['del', x.k, ...d.headers.map(h=> x.r[h]||'')]));
    if (twoRow) {
      d.changed.forEach(x=> {
        rows.push(['chg-before', x.k, ...d.headers.map(h=> x.a[h]||'')]);
        rows.push(['chg-after',  x.k, ...d.headers.map(h=> x.b[h]||'')]);
      });
    } else {
      // Wide format: after values only
      d.changed.forEach(x=> rows.push(['chg', x.k, ...d.headers.map(h=> x.b[h]||'')]));
    }
    downloadCsv('table_diff.csv', rows);
  });
  document.getElementById('btn-csv-copy').addEventListener('click', ()=>{
    const sum = document.getElementById('csvSummary').textContent.trim(); if (sum) ARW.copy(sum);
  });
  document.getElementById('port').addEventListener('change', async ()=>{
    const p = ARW.getPortFromInput('port') || 8090;
    await ARW.setPrefs('launcher', { ...(await ARW.getPrefs('launcher')), port: p });
    ARW.sse.connect(ARW.base(p), { replay: 10 });
  });
  document.getElementById('btn-save').addEventListener('click', async ()=>{
    const layout = {
      lanes: ['timeline','context','policy','metrics','models','activity'],
      grid: 'cols-2',
      focused: document.querySelector('.layout').classList.contains('full')
    };
    await ARW.templates.save('hub', layout);
    const perProj = !!document.getElementById('tplPerProj')?.checked;
    if (perProj && curProj) {
      try { const ns = 'ui:proj:'+curProj; const cur = await ARW.getPrefs(ns)||{}; cur.template = layout; await ARW.setPrefs(ns, cur); } catch {}
    }
  })
  // Apply layout (grid/focused/lanes) from per-project or global template
  function applyTemplate(tpl){
    if (!tpl || typeof tpl !== 'object') return;
    try{
      // Focused mode
      const root = document.querySelector('.layout');
      if (tpl.focused){ root.classList.add('full'); } else { root.classList.remove('full'); }
      // Grid columns
      const main = document.getElementById('main');
      if (main && typeof tpl.grid === 'string'){
        main.classList.remove('cols-1','cols-2','cols-3');
        main.classList.add(tpl.grid);
      }
      // Lanes: re-mount sidecar if lanes differ
      if (Array.isArray(tpl.lanes) && tpl.lanes.length){ try{ sc.dispose?.(); }catch{} ARW.sidecar.mount('sidecar', tpl.lanes, { base }); }
      ARW.toast('Layout applied');
    }catch(e){ console.error(e); ARW.toast('Apply failed'); }
  }
  document.getElementById('btn-apply').addEventListener('click', async ()=>{
    try{
      let tpl = null;
      try { if (curProj) { const p = await ARW.getPrefs('ui:proj:'+curProj)||{}; tpl = p.template || null; } } catch {}
      if (!tpl) { tpl = await ARW.templates.load('hub'); }
      applyTemplate(tpl);
    }catch(e){ console.error(e); ARW.toast('Apply failed'); }
  });
  // Apply saved template (focused toggle)
  try{
    // Per-project template overrides the global hub template
    let tpl = null;
    try { if (curProj) { const p = await ARW.getPrefs('ui:proj:'+curProj)||{}; tpl = p.template || null; } } catch {}
    if (!tpl) { tpl = await ARW.templates.load('hub'); }
    if (tpl) applyTemplate(tpl);
  }catch{}
  // ---------- Agents: role and state ----------
  async function loadAgentState(){
    try{ const r=await fetch(base + '/hierarchy/state'); const t=await r.text(); const el=document.getElementById('agentState'); if (el) el.textContent = t; }catch(e){ console.error(e); }
  }
  async function applyRole(){
    try{ const role = (document.getElementById('roleSel')?.value||'edge'); await fetch(base + '/hierarchy/role', { method:'POST', headers:{'Content-Type':'application/json'}, body: JSON.stringify({ role }) }); await loadAgentState(); ARW.toast('Role applied'); }catch(e){ console.error(e); ARW.toast('Apply failed'); }
  }
  document.getElementById('btnRoleApply')?.addEventListener('click', applyRole);
  document.getElementById('btnRoleRefresh')?.addEventListener('click', loadAgentState);
  loadAgentState();
  // Focused layout toggle
  document.getElementById('btn-focus').addEventListener('click', ()=>{
    const root = document.querySelector('.layout');
    root.classList.toggle('full');
  });
  document.getElementById('btn-gallery')?.addEventListener('click', ()=> ARW.gallery.show());
  // Gallery badge: show recent screenshots count
  function updateGalleryBadge(){ try{ const b=document.getElementById('galleryBadge'); if (!b) return; const n = (ARW.gallery && Array.isArray(ARW.gallery._items)) ? ARW.gallery._items.length : 0; b.innerHTML = '<span class="dot"></span> ' + n; b.className = 'badge ' + (n>0 ? 'ok' : ''); }catch{} }
  updateGalleryBadge();
  ARW.sse.subscribe((k)=> k.startsWith('screenshots.'), ()=> updateGalleryBadge());
  // Command palette
  ARW.palette.mount({ base });
});
