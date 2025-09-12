const base = (port) => ARW.base(port);
async function loadStats() {
  const out = document.getElementById('out');
  out.innerHTML = '';
  const port = ARW.getPortFromInput('port') || 8090;
  const url = ARW.base(port) + '/introspect/stats';
  document.getElementById('stat').textContent = 'Loading...';
  try {
    const resp = await fetch(url, { headers: { 'Accept': 'application/json' } });
    if (!resp.ok) throw new Error('HTTP ' + resp.status);
    const j = await resp.json();
    const wrap = document.createElement('div');
    wrap.innerHTML = `<h3>Counters</h3><div class="kv">
      <div class="k">Routes</div><div>${(j.routes||[]).length}</div>
      <div class="k">Events kinds</div><div>${Object.keys((j.events&&j.events.kinds)||{}).length}</div>
      <div class="k">Bus published</div><div>${(j.bus&&j.bus.published)||0}</div>
      <div class="k">Bus delivered</div><div>${(j.bus&&j.bus.delivered)||0}</div>
    </div>`;
    const pre = document.createElement('pre');
    pre.textContent = JSON.stringify(j, null, 2);
    out.appendChild(wrap);
    out.appendChild(pre);
    document.getElementById('stat').textContent = 'OK';
  } catch (e) {
    document.getElementById('stat').textContent = 'Error';
    const pre = document.createElement('pre');
    pre.textContent = String(e);
    out.appendChild(pre);
  }
}
document.addEventListener('DOMContentLoaded', () => {
  document.getElementById('btn-refresh').addEventListener('click', loadStats);
  (async () => { await ARW.applyPortFromPrefs('port'); loadStats(); setInterval(loadStats, 5000) })();
});

