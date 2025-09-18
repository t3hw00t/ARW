document.addEventListener('DOMContentLoaded', async () => {
  await ARW.applyPortFromPrefs('port');
  const port = ARW.getPortFromInput('port') || 8091;
  const base = ARW.base(port);
  const sc = ARW.sidecar.mount('sidecar', ['timeline','context','policy','metrics','models'], { base });
  // Load auto OCR pref
  try{ const prefs = await ARW.getPrefs('launcher'); const v = !!(prefs && prefs.autoOcr); const el=document.getElementById('autoOcr'); if (el) el.checked = v; }catch{}
  document.getElementById('autoOcr').addEventListener('change', async (e)=>{
    try{ const prefs = await ARW.getPrefs('launcher') || {}; prefs.autoOcr = !!e.target.checked; await ARW.setPrefs('launcher', prefs); }catch{}
  });
  ARW.sse.subscribe('*open*', ()=> document.getElementById('sseStat').textContent = 'SSE: on');
  ARW.sse.subscribe('*error*', ()=> document.getElementById('sseStat').textContent = 'SSE: off');
  ARW.sse.connect(base, { replay: 10 });

  // Compare helpers
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
  function pin(slot, text){
    const id = slot==='A' ? 'cmpAOut' : 'cmpBOut';
    const el = document.getElementById(id); el.innerHTML = highlightJSON(String(text||''));
  }
  function addMessage(role, text){
    const wrap = document.createElement('div');
    wrap.className = 'msg';
    const meta = document.createElement('div'); meta.className='dim'; meta.textContent = role;
    const body = document.createElement('div'); body.className='mono'; body.textContent = String(text||'');
    const row = document.createElement('div'); row.className='row';
    const pa = document.createElement('button'); pa.className='ghost'; pa.textContent='Pin A'; pa.addEventListener('click', ()=> pin('A', text));
    const pb = document.createElement('button'); pb.className='ghost'; pb.textContent='Pin B'; pb.addEventListener('click', ()=> pin('B', text));
    row.appendChild(pa); row.appendChild(pb);
    wrap.appendChild(meta); wrap.appendChild(body); wrap.appendChild(row);
    document.getElementById('messages').appendChild(wrap);
  }
  document.getElementById('send').addEventListener('click', async ()=>{
    const t = document.getElementById('msg').value.trim();
    if (!t) return; addMessage('you', t); document.getElementById('msg').value='';
    // Placeholder echo; real chat will call service endpoint
    setTimeout(()=> addMessage('assistant', t.split('').reverse().join('')), 120);
  });
  document.getElementById('capture').addEventListener('click', async ()=>{
    try{
      const p = ARW.getPortFromInput('port');
      const out = await ARW.invoke('run_tool_admin', { id: 'ui.screenshot.capture', input: { scope:'screen', format:'png', downscale:640 }, port: p });
      const wrap = document.createElement('div');
      wrap.className = 'msg';
      const meta = document.createElement('div'); meta.className='dim'; meta.textContent = 'capture';
      const body = document.createElement('div');
      if (out && out.preview_b64){ const img=document.createElement('img'); img.src=out.preview_b64; img.alt=out.path||''; img.style.maxWidth='100%'; body.appendChild(img); }
      const cap = document.createElement('div'); cap.className='mono dim'; cap.textContent = out && out.path ? out.path : '';
      const tools = document.createElement('div'); tools.className='row';
      const mdBtn=document.createElement('button'); mdBtn.className='ghost'; mdBtn.textContent='Copy MD'; mdBtn.addEventListener('click', ()=>{ if (out && out.path) ARW.copyMarkdown(out.path, 'screenshot'); });
      const saveBtn=document.createElement('button'); saveBtn.className='ghost'; saveBtn.textContent='Save to project'; saveBtn.addEventListener('click', async ()=>{ if (out && out.path){ const res = await ARW.saveToProjectPrompt(out.path); if (res) await ARW.maybeAppendToNotes(res.proj, res.dest); } });
      const annBtn=document.createElement('button'); annBtn.className='ghost'; annBtn.textContent='Annotate'; annBtn.addEventListener('click', async ()=>{ try{ const rects = await ARW.annot.start(out.preview_b64||''); const res = await ARW.invoke('run_tool_admin', { id: 'ui.screenshot.annotate_burn', input: { path: out.path, annotate: rects, downscale:640 }, port: ARW.getPortFromInput('port') }); if (res && res.preview_b64){ body.innerHTML=''; const img=document.createElement('img'); img.src=res.preview_b64; img.alt=res.path||''; img.style.maxWidth='100%'; body.appendChild(img); cap.textContent = res.path || ''; } }catch(e){ console.error(e); } });
      tools.appendChild(mdBtn); tools.appendChild(saveBtn); tools.appendChild(annBtn);
      wrap.appendChild(meta); wrap.appendChild(body); wrap.appendChild(cap); wrap.appendChild(tools);
      document.getElementById('messages').appendChild(wrap);
      // OCR (optional)
      try{
        if (document.getElementById('autoOcr').checked && out && out.path){
          const o = await ARW.invoke('run_tool_admin', { id: 'ui.screenshot.ocr', input: { path: out.path }, port: ARW.getPortFromInput('port') });
          const txt = (o && o.text) ? String(o.text).trim() : '';
          if (txt){ const pre = document.createElement('pre'); pre.className='mono'; pre.textContent = txt; wrap.appendChild(pre); }
        }
      }catch(e){ console.warn('OCR failed', e); }
    }catch(e){ console.error(e); ARW.toast('Capture failed'); }
  });
  document.getElementById('captureWin').addEventListener('click', async ()=>{
    try{
      const b = await ARW.invoke('active_window_bounds', { label: null });
      const scope = `region:${b?.x??0},${b?.y??0},${b?.w??0},${b?.h??0}`;
      const p = ARW.getPortFromInput('port');
      const out = await ARW.invoke('run_tool_admin', { id: 'ui.screenshot.capture', input: { scope, format:'png', downscale:640 }, port: p });
      const wrap = document.createElement('div');
      wrap.className = 'msg';
      const meta = document.createElement('div'); meta.className='dim'; meta.textContent = 'capture-window';
      const body = document.createElement('div');
      if (out && out.preview_b64){ const img=document.createElement('img'); img.src=out.preview_b64; img.alt=out.path||''; img.style.maxWidth='100%'; body.appendChild(img); }
      const cap = document.createElement('div'); cap.className='mono dim'; cap.textContent = out && out.path ? out.path : '';
      const tools2 = document.createElement('div'); tools2.className='row';
      const mdBtn2=document.createElement('button'); mdBtn2.className='ghost'; mdBtn2.textContent='Copy MD'; mdBtn2.addEventListener('click', ()=>{ if (out && out.path) ARW.copyMarkdown(out.path, 'screenshot'); });
      const saveBtn2=document.createElement('button'); saveBtn2.className='ghost'; saveBtn2.textContent='Save to project'; saveBtn2.addEventListener('click', async ()=>{ if (out && out.path){ const res = await ARW.saveToProjectPrompt(out.path); if (res) await ARW.maybeAppendToNotes(res.proj, res.dest); } });
      const annBtn2=document.createElement('button'); annBtn2.className='ghost'; annBtn2.textContent='Annotate'; annBtn2.addEventListener('click', async ()=>{ try{ const rects = await ARW.annot.start(out.preview_b64||''); const res = await ARW.invoke('run_tool_admin', { id: 'ui.screenshot.annotate_burn', input: { path: out.path, annotate: rects, downscale:640 }, port: ARW.getPortFromInput('port') }); if (res && res.preview_b64){ body.innerHTML=''; const img=document.createElement('img'); img.src=res.preview_b64; img.alt=res.path||''; img.style.maxWidth='100%'; body.appendChild(img); cap.textContent = res.path || ''; } }catch(e){ console.error(e); } });
      tools2.appendChild(mdBtn2); tools2.appendChild(saveBtn2); tools2.appendChild(annBtn2);
      wrap.appendChild(meta); wrap.appendChild(body); wrap.appendChild(cap); wrap.appendChild(tools2);
      document.getElementById('messages').appendChild(wrap);
      // OCR (optional)
      try{
        if (document.getElementById('autoOcr').checked && out && out.path){
          const o = await ARW.invoke('run_tool_admin', { id: 'ui.screenshot.ocr', input: { path: out.path }, port: ARW.getPortFromInput('port') });
          const txt = (o && o.text) ? String(o.text).trim() : '';
          if (txt){ const pre = document.createElement('pre'); pre.className='mono'; pre.textContent = txt; wrap.appendChild(pre); }
        }
      }catch(e){ console.warn('OCR failed', e); }
    }catch(e){ console.error(e); ARW.toast('Capture failed'); }
  });
  document.getElementById('captureRegion').addEventListener('click', async ()=>{
    try{
      const out = await ARW.region.captureAndSave();
      if (!out) return;
      const wrap = document.createElement('div');
      wrap.className = 'msg';
      const meta = document.createElement('div'); meta.className='dim'; meta.textContent = 'capture-region';
      const body = document.createElement('div');
      if (out.preview_b64){ const img=document.createElement('img'); img.src=out.preview_b64; img.alt=out.path||''; img.style.maxWidth='100%'; body.appendChild(img); }
      const cap = document.createElement('div'); cap.className='mono dim'; cap.textContent = out.path || '';
      const tools3 = document.createElement('div'); tools3.className='row';
      const mdBtn3=document.createElement('button'); mdBtn3.className='ghost'; mdBtn3.textContent='Copy MD'; mdBtn3.addEventListener('click', ()=>{ if (out && out.path) ARW.copyMarkdown(out.path, 'screenshot'); });
      const saveBtn3=document.createElement('button'); saveBtn3.className='ghost'; saveBtn3.textContent='Save to project'; saveBtn3.addEventListener('click', async ()=>{ if (out && out.path){ const res = await ARW.saveToProjectPrompt(out.path); if (res) await ARW.maybeAppendToNotes(res.proj, res.dest); } });
      const annBtn3=document.createElement('button'); annBtn3.className='ghost'; annBtn3.textContent='Annotate'; annBtn3.addEventListener('click', async ()=>{ try{ const rects = await ARW.annot.start(out.preview_b64||''); const res = await ARW.invoke('run_tool_admin', { id: 'ui.screenshot.annotate_burn', input: { path: out.path, annotate: rects, downscale:640 }, port: ARW.getPortFromInput('port') }); if (res && res.preview_b64){ body.innerHTML=''; const img=document.createElement('img'); img.src=res.preview_b64; img.alt=res.path||''; img.style.maxWidth='100%'; body.appendChild(img); cap.textContent = res.path || ''; } }catch(e){ console.error(e); } });
      tools3.appendChild(mdBtn3); tools3.appendChild(saveBtn3); tools3.appendChild(annBtn3);
      wrap.appendChild(meta); wrap.appendChild(body); wrap.appendChild(cap); wrap.appendChild(tools3);
      document.getElementById('messages').appendChild(wrap);
      if (document.getElementById('autoOcr').checked && out.path){
        try{ const o = await ARW.invoke('run_tool_admin', { id: 'ui.screenshot.ocr', input: { path: out.path }, port: ARW.getPortFromInput('port') }); const txt=(o&&o.text)?String(o.text).trim():''; if (txt){ const pre=document.createElement('pre'); pre.className='mono'; pre.textContent=txt; wrap.appendChild(pre); } }catch(e){ console.warn('OCR failed', e); }
      }
    }catch(e){ console.error(e); ARW.toast('Capture canceled'); }
  });
  document.getElementById('clearA').addEventListener('click', ()=> document.getElementById('cmpAOut').innerHTML='');
  document.getElementById('clearB').addEventListener('click', ()=> document.getElementById('cmpBOut').innerHTML='');
  document.getElementById('btn-diff').addEventListener('click', ()=>{
    const a = document.getElementById('cmpAOut').innerText;
    const b = document.getElementById('cmpBOut').innerText;
    const only = document.getElementById('txtOnlyChanges').checked;
    const wrap = document.getElementById('txtWrap').checked;
    const out = document.getElementById('cmpOut'); out.innerHTML=''; out.style.whiteSpace = wrap? 'pre-wrap':'pre';
    const frag = diffLines(a,b);
    if (only){ [...frag.childNodes].forEach(node=>{ if (node.classList && !/add|del/.test(node.className)) node.remove(); }); }
    out.appendChild(frag);
  });
  document.getElementById('port').addEventListener('change', async ()=>{
    const p = ARW.getPortFromInput('port') || 8091;
    await ARW.setPrefs('launcher', { ...(await ARW.getPrefs('launcher')), port: p });
    ARW.sse.connect(ARW.base(p), { replay: 5 });
  });
  ARW.palette.mount({ base });
});
