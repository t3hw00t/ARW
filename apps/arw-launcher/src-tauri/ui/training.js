document.addEventListener('DOMContentLoaded', async () => {
  await ARW.applyPortFromPrefs('port');
  const port = ARW.getPortFromInput('port') || 8090;
  const base = ARW.base(port);
  const sc = ARW.sidecar.mount('sidecar', ['timeline','context','policy','metrics','models'], { base });
  ARW.sse.subscribe('*open*', ()=> document.getElementById('sseStat').textContent = 'SSE: on');
  ARW.sse.subscribe('*error*', ()=> document.getElementById('sseStat').textContent = 'SSE: off');
  ARW.sse.connect(base, { replay: 10, prefix: ['state.', 'models.'] });
  document.getElementById('abRun').addEventListener('click', ()=>{
    ARW.toast('A/B run started (stub)');
    setTimeout(()=>{
      const r = document.createElement('div');
      r.textContent = 'Result ' + new Date().toISOString();
      document.getElementById('results').prepend(r);
    }, 200);
  });
  document.getElementById('port').addEventListener('change', async ()=>{
    const p = ARW.getPortFromInput('port') || 8090;
    await ARW.setPrefs('launcher', { ...(await ARW.getPrefs('launcher')), port: p });
    ARW.sse.connect(ARW.base(p), { replay: 5 });
  });
  ARW.palette.mount({ base });
});
