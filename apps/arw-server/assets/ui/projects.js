/* Projects page script (no inline handlers) */
(() => {
  const base = location.origin;
  const ADMIN_STORAGE_KEY = 'arw_admin_token';
  let adminToken = '';
  let current = null;

  // SSE status indicator
  try {
    const es = new EventSource(base + '/events');
    es.onopen = () => { const el = document.getElementById('sseBadge'); if (el) el.innerHTML = "<span class='dot ok'></span> SSE: on"; };
    es.onerror = () => { const el = document.getElementById('sseBadge'); if (el) el.innerHTML = "<span class='dot bad'></span> SSE: off"; };
  } catch {}

  function setAdminBadge(on) { const el = document.getElementById('authBadge'); if (!el) return; el.className = 'badge ' + (on ? 'ok' : 'warn'); el.innerHTML = '<span class="dot"></span> Admin: ' + (on ? 'on' : 'off'); }
  function setAdminToken() { const tok = (document.getElementById('adminTok')?.value || '').trim(); adminToken = tok; if (tok) { try { localStorage.setItem(ADMIN_STORAGE_KEY, tok); } catch {} } else { try { localStorage.removeItem(ADMIN_STORAGE_KEY); } catch {} } setAdminBadge(!!tok); }
  function loadAdminToken() { try { const stored = localStorage.getItem(ADMIN_STORAGE_KEY); if (stored) { adminToken = stored; const input = document.getElementById('adminTok'); if (input) input.value = stored; setAdminBadge(true); return; } } catch {} setAdminBadge(false); }
  async function adminFetch(path, options = {}) { const init = { ...options }; init.headers = { ...(options.headers || {}) }; if (adminToken) { init.headers['X-ARW-Admin'] = adminToken; } const resp = await fetch(base + path, init); if (resp.status === 401) { setAdminBadge(false); throw new Error('unauthorized'); } return resp; }
  function show(s) { document.getElementById('out').textContent = s; setTimeout(() => { document.getElementById('out').textContent = '{}'; }, 1200); }

  function setProj(n) { current = n; document.getElementById('curProj').textContent = n || '–'; if (n) { loadNotes(); loadTree(''); } }

  async function listProjs() {
    try {
      const resp = await adminFetch('/state/projects');
      const j = await resp.json().catch(() => ({ items: [] }));
      const ul = document.getElementById('projList');
      ul.innerHTML = '';
      (j.items || []).forEach(item => {
        const name = (item && typeof item === 'object') ? item.name : item;
        if (!name) return;
        const li = document.createElement('li');
        li.textContent = name;
        li.addEventListener('click', () => setProj(name));
        ul.appendChild(li);
      });
      setAdminBadge(!!adminToken);
    } catch (err) {
      document.getElementById('out').textContent = 'error: ' + err.message;
    }
  }

  async function createProj() { const n = document.getElementById('projName').value.trim(); if (!n) return; await adminFetch('/projects', { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ name: n }) }); await listProjs(); setProj(n); }
  async function loadNotes() { if (!current) return; try { const resp = await adminFetch('/state/projects/' + encodeURIComponent(current) + '/notes'); const t = await resp.text(); document.getElementById('notes').value = t; } catch (err) { show('error: ' + err.message); } }
  async function saveNotes() { if (!current) return; const t = document.getElementById('notes').value || ''; try { await adminFetch('/projects/' + encodeURIComponent(current) + '/notes', { method: 'PUT', headers: { 'Content-Type': 'text/plain' }, body: t + '\n' }); show('saved'); } catch (err) { show('error: ' + err.message); } }
  async function loadTree(p) { if (!current) return; try { const resp = await adminFetch('/state/projects/' + encodeURIComponent(current) + '/tree?path=' + encodeURIComponent(p || '')); const j = await resp.json().catch(() => ({ items: [] })); const el = document.getElementById('tree'); el.innerHTML = ''; (j.items || []).forEach(it => { const d = document.createElement('div'); d.textContent = (it.dir ? '📁 ' : '📄 ') + it.name; d.style.cursor = 'pointer'; d.addEventListener('click', () => { if (it.dir) loadTree(it.rel); }); el.appendChild(d); }); } catch (err) { show('error: ' + err.message); } }

  document.addEventListener('DOMContentLoaded', () => {
    loadAdminToken();
    const byId = (id) => document.getElementById(id);
    byId('adminTokApply')?.addEventListener('click', setAdminToken);
    byId('createProj')?.addEventListener('click', createProj);
    byId('saveNotes')?.addEventListener('click', saveNotes);
    byId('reloadNotes')?.addEventListener('click', loadNotes);
    listProjs();
  });
})();

