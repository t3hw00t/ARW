let es = null;
const base = (port) => ARW.base(port);
function add(type, payload) {
  const el = document.createElement('div');
  el.className = 'e';
  const ts = new Date().toISOString();
  el.innerHTML = `<div class="dim">${ts} <b>${type||'message'}</b></div><pre>${payload}</pre>`;
  const log = document.getElementById('log');
  log.prepend(el);
  while (log.childElementCount > 300) log.removeChild(log.lastChild);
}
function sse(replay) {
  const port = ARW.getPortFromInput('port') || 8090;
  let url = ARW.base(port) + '/admin/events';
  const filter = document.getElementById('filter').value.trim();
  const params = new URLSearchParams();
  if (replay) params.set('replay', replay);
  if (filter) params.set('prefix', filter);
  if ([...params.keys()].length) url += '?' + params.toString();
  if (es) { es.close(); es = null; }
  es = new EventSource(url);
  es.onopen = () => document.getElementById('stat').textContent = 'SSE: on';
  es.onerror = () => document.getElementById('stat').textContent = 'SSE: off';
  es.onmessage = ev => add(null, ev.data);
}
document.addEventListener('DOMContentLoaded', () => {
  document.getElementById('btn-replay').addEventListener('click', ()=> sse(50));
  (async () => { await ARW.applyPortFromPrefs('port'); sse(0) })();
});

