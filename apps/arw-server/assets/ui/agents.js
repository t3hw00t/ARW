/* Agents page script (no inline handlers) */
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

  function setAdminBadge(on) {
    const el = document.getElementById('authBadge'); if (!el) return;
    el.className = 'badge ' + (on ? 'ok' : 'warn');
    el.innerHTML = '<span class="dot"></span> Admin: ' + (on ? 'on' : 'off');
  }
  function setAdminToken() { const tok = (document.getElementById('adminTok')?.value || '').trim(); adminToken = tok; if (tok) { try { localStorage.setItem(ADMIN_STORAGE_KEY, tok); } catch {} } else { try { localStorage.removeItem(ADMIN_STORAGE_KEY); } catch {} } setAdminBadge(!!tok); }
  function loadAdminToken() { try { const stored = localStorage.getItem(ADMIN_STORAGE_KEY); if (stored) { adminToken = stored; const input = document.getElementById('adminTok'); if (input) input.value = stored; setAdminBadge(true); return; } } catch {} setAdminBadge(false); }
  async function adminFetch(path, options = {}) { const init = { ...options }; init.headers = { ...(options.headers || {}) }; if (adminToken) { init.headers['X-ARW-Admin'] = adminToken; }
    const resp = await fetch(base + path, init); if (resp.status === 401) { setAdminBadge(false); throw new Error('unauthorized'); } return resp; }

  async function roleApply() { const r = document.getElementById('roleSel').value; await adminFetch('/admin/hierarchy/role', { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ role: r }) }); await loadState(); }
  async function loadState() { try { const resp = await adminFetch('/admin/hierarchy/state'); const t = await resp.text(); try { document.getElementById('state').textContent = JSON.stringify(JSON.parse(t), null, 2);} catch { document.getElementById('state').textContent = t; } setAdminBadge(!!adminToken); } catch (err) { document.getElementById('state').textContent = 'error: ' + err.message; } }

  document.addEventListener('DOMContentLoaded', () => {
    loadAdminToken();
    const byId = (id) => document.getElementById(id);
    byId('adminTokApply')?.addEventListener('click', setAdminToken);
    byId('roleApply')?.addEventListener('click', roleApply);
    byId('refreshState')?.addEventListener('click', loadState);
    loadState();
  });
})();

