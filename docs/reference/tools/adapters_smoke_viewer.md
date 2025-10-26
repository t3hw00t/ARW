---
title: Adapters Smoke Viewer
---

# Adapters Smoke Viewer
Updated: 2025-10-26
Type: Tool

Use this page to visualize an adapters-smoke JSON report. You can either:

- Auto-load a bundled report at build time from `docs/static/adapters-smoke.json` (if present), or
- Select a JSON file produced by the smoke harness (`ADAPTER_SMOKE_OUT=...`), or
- Paste the JSON directly.

Controls

- Select JSON: <input type="file" id="file" accept="application/json" />
- Or paste JSON below and click Parse:

<textarea id="paste" style="width:100%;min-height:120px" placeholder="Paste the JSON array here..."></textarea>
<br />
<button id="parse" class="md-button">Parse</button>
<button id="downloadCsv" class="md-button" disabled>Download CSV</button>
<div id="summary"></div>
<div id="table"></div>

<script>
(function(){
  const byId = (id) => document.getElementById(id);
  const file = byId('file');
  const paste = byId('paste');
  const parseBtn = byId('parse');
  const tableDiv = byId('table');
  const summaryDiv = byId('summary');
  const dlBtn = byId('downloadCsv');
  let current = [];

  function cnt(v){ return Array.isArray(v) ? v.length : 0; }
  function toCsv(rows){
    const cols = ['path','id','version','ok','errors','warnings','advisories','validate_ms','health_probe_ms'];
    const esc = (s)=>`"${String(s??'').replaceAll('"','""')}"`;
    const lines = [cols.join(',')];
    for (const r of rows){
      lines.push(cols.map(c=>esc(r[c])).join(','));
    }
    return lines.join('\n');
  }
  function render(rows){
    current = rows || [];
    const total = current.length;
    const oks = current.filter(r=>r.ok).length;
    const errs = current.reduce((a,r)=>a+cnt(r.errors),0);
    const warns = current.reduce((a,r)=>a+cnt(r.warnings),0);
    const advs = current.reduce((a,r)=>a+cnt(r.advisories),0);
    summaryDiv.innerHTML = `<p><strong>Summary:</strong> files=${total} ok=${oks} errors=${errs} warnings=${warns} advisories=${advs}</p>`;
    const hdr = ['manifest','ok','errors','warnings','advisories','validate_ms','health_ms'];
    const rowsHtml = current.map(r=>`<tr>
      <td style="word-break:break-all">${r.path||''}</td>
      <td>${r.ok?'yes':'no'}</td>
      <td style="text-align:right">${cnt(r.errors)}</td>
      <td style="text-align:right">${cnt(r.warnings)}</td>
      <td style="text-align:right">${cnt(r.advisories)}</td>
      <td style="text-align:right">${r.validate_ms||0}</td>
      <td style="text-align:right">${r.health_probe_ms||0}</td>
    </tr>`).join('');
    tableDiv.innerHTML = `<table><thead><tr>${hdr.map(h=>`<th>${h}</th>`).join('')}</tr></thead><tbody>${rowsHtml}</tbody></table>`;
    dlBtn.disabled = total === 0;
  }
  function parseText(text){
    try { const data = JSON.parse(text); if (Array.isArray(data)) render(data); }
    catch (e){ alert('Invalid JSON'); }
  }
  file.addEventListener('change', async (e)=>{
    const f = e.target.files && e.target.files[0]; if (!f) return;
    const text = await f.text(); parseText(text);
  });
  parseBtn.addEventListener('click', ()=> parseText(paste.value || '[]'));
  dlBtn.addEventListener('click', ()=>{
    const csv = toCsv(current);
    const blob = new Blob([csv], {type:'text/csv'});
    const a = document.createElement('a');
    a.href = URL.createObjectURL(blob);
    a.download = 'adapters-smoke.csv';
    a.click();
  });
  // Attempt to auto-load docs/static/adapters-smoke.json if present
  fetch('adapters-smoke.json', {cache:'no-store'}).then(r=>{ if (r.ok) return r.json(); throw 0; }).then(render).catch(()=>{});
})();
</script>

How to produce a JSON report

- One-shot: `just adapters-smoke-oneshot out=docs/static/adapters-smoke.json` (or `mise run adapters:smoke:oneshot OUT=docs/static/adapters-smoke.json`)
- Manual: set `ADAPTER_SMOKE_OUT=...` when running `scripts/adapter_smoke.sh`.

