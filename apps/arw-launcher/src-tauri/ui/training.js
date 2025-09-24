document.addEventListener('DOMContentLoaded', async () => {
  await ARW.applyPortFromPrefs('port');
  const port = ARW.getPortFromInput('port') || 8091;
  const base = ARW.base(port);
  const sc = ARW.sidecar.mount('sidecar', ['timeline','context','policy','metrics','models'], { base });
  ARW.sse.indicator('sseStat', { prefix: 'SSE' });
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
    const p = ARW.getPortFromInput('port') || 8091;
    await ARW.setPrefs('launcher', { ...(await ARW.getPrefs('launcher')), port: p });
    ARW.sse.connect(ARW.base(p), { replay: 5 });
  });
  ARW.palette.mount({ base });
});
