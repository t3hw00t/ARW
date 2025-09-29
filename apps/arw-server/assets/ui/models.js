/* Models page script (no inline handlers) */
(() => {
  const base = location.origin;
  const ADMIN_STORAGE_KEY = 'arw_admin_token';
  let adminToken = '';

  // SSE status indicator
  try {
    const es = new EventSource(base + '/events');
    es.onopen = () => { const el = document.getElementById('sseBadge'); if (el) el.innerHTML = "<span class='dot ok'></span> SSE: on"; };
    es.onerror = () => { const el = document.getElementById('sseBadge'); if (el) el.innerHTML = "<span class='dot bad'></span> SSE: off"; };
  } catch {}

  function setAdminBadge(ok) {
    const el = document.getElementById('authBadge');
    if (!el) return;
    el.className = 'badge ' + (ok ? 'ok' : 'warn');
    el.innerHTML = '<span class="dot"></span> Admin: ' + (ok ? 'on' : 'off');
  }

  function setAdminToken() {
    const input = document.getElementById('adminTok');
    const value = (input?.value || '').trim();
    adminToken = value;
    if (value) { try { localStorage.setItem(ADMIN_STORAGE_KEY, value); } catch {} } else { try { localStorage.removeItem(ADMIN_STORAGE_KEY); } catch {} }
    setAdminBadge(!!value);
  }

  function loadAdminToken() {
    try {
      const stored = localStorage.getItem(ADMIN_STORAGE_KEY);
      if (stored) {
        adminToken = stored;
        const input = document.getElementById('adminTok');
        if (input) input.value = stored;
        setAdminBadge(true);
        return;
      }
    } catch {}
    setAdminBadge(false);
  }

  async function call(method, path, body) {
    const init = { method, headers: {} };
    if (body !== undefined) { init.body = JSON.stringify(body); init.headers['Content-Type'] = 'application/json'; }
    if (adminToken) init.headers['X-ARW-Admin'] = adminToken;
    const resp = await fetch(base + path, init);
    if (resp.status === 401) { setAdminBadge(false); throw new Error('unauthorized'); }
    const text = await resp.text();
    try { return JSON.parse(text); } catch { return text; }
  }

  function renderRuntime(metrics) {
    const card = document.getElementById('runtimeCard');
    const hints = document.getElementById('runtimeHints');
    const errors = document.getElementById('errorHints');
    if (!card || !hints || !errors) return;
    card.style.display = 'block';
    const runtime = metrics.runtime || {};
    const lines = [];
    if (runtime.idle_timeout_secs !== undefined && runtime.idle_timeout_secs !== null) {
      lines.push(`Idle timeout: ${runtime.idle_timeout_secs}s (ARW_DL_IDLE_TIMEOUT_SECS)`);
    } else { lines.push('Idle timeout: disabled (ARW_DL_IDLE_TIMEOUT_SECS=0)'); }
    lines.push(`Send retries: ${runtime.send_retries} · Stream retries: ${runtime.stream_retries}`);
    lines.push(`Backoff: ${runtime.retry_backoff_ms}ms · Preflight: ${runtime.preflight_enabled ? 'on' : 'off'}`);
    hints.innerHTML = `<strong>Download Runtime</strong><br>${lines.join('<br>')}`;
    let errorHtml = '';
    const errorsCount = metrics.errors || 0;
    if (errorsCount > 0) { errorHtml += `<p><strong>${errorsCount}</strong> download errors recorded. Inspect codes like <code>idle-timeout</code> or <code>resume-content-range</code>.</p>`; }
    errorHtml += '<p>Idle timeout failures emit <code>idle-timeout</code>. Adjust <code>ARW_DL_IDLE_TIMEOUT_SECS</code> if required.</p>';
    errorHtml += '<p>Network instability? Increase <code>ARW_DL_SEND_RETRIES</code> or tweak <code>ARW_DL_RETRY_BACKOFF_MS</code>. Resume mismatches may require deleting partial files under <code>state/models/tmp</code>.</p>';
    errors.innerHTML = errorHtml;
  }

  async function listM() {
    try {
      const j = await call('GET', '/admin/models');
      const b = document.getElementById('mdlBadge');
      if (Array.isArray(j) && b) { b.className = 'badge ok'; b.innerHTML = '<span class="dot"></span> ' + j.length; }
      document.getElementById('out').textContent = JSON.stringify(j, null, 2);
      setAdminBadge(!!adminToken);
    } catch (err) {
      document.getElementById('out').textContent = `error: ${err.message}`;
    }
  }
  async function addM() { const id = document.getElementById('mId').value.trim(); if (!id) return; const provider = (document.getElementById('mProv').value || 'local').trim(); await call('POST', '/admin/models/add', { id, provider }); await listM(); }
  async function delM() { const id = document.getElementById('mId').value.trim(); if (!id) return; await call('POST', '/admin/models/remove', { id }); await listM(); }
  async function defGet() { const j = await call('GET', '/admin/models/default'); document.getElementById('out').textContent = JSON.stringify(j, null, 2); }
  async function defSet() { const id = document.getElementById('mId').value.trim(); if (!id) return; await call('POST', '/admin/models/default', { id }); await listM(); }
  async function metricsM() { try { const j = await call('GET', '/state/models_metrics'); document.getElementById('out').textContent = JSON.stringify(j, null, 2); renderRuntime(j); } catch (err) { document.getElementById('out').textContent = `error: ${err.message}`; } }

  document.addEventListener('DOMContentLoaded', () => {
    loadAdminToken();
    const byId = (id) => document.getElementById(id);
    byId('adminTokApply')?.addEventListener('click', setAdminToken);
    byId('btnList')?.addEventListener('click', listM);
    byId('btnMetrics')?.addEventListener('click', metricsM);
    byId('btnAdd')?.addEventListener('click', addM);
    byId('btnDel')?.addEventListener('click', delM);
    byId('btnDefGet')?.addEventListener('click', defGet);
    byId('btnDefSet')?.addEventListener('click', defSet);
    listM();
  });
})();

