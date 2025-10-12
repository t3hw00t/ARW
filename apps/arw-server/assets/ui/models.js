/* Models page script (no inline handlers) */
(() => {
  const base = location.origin;
  const ADMIN_STORAGE_KEY = 'arw_admin_token';
  let adminToken = '';
  let capsulePresets = [];

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
    loadCapsulePresets();
    refreshCapsuleStatus();
    refreshCapsuleAudit();
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
    card.hidden = false;
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

  function setCapsuleActionMessage(text, variant = 'info') {
    const el = document.getElementById('capsuleActionStatus');
    if (!el) return;
    el.textContent = text || '';
    el.classList.toggle('error', variant === 'error');
  }

  function capsuleReason() {
    const input = document.getElementById('capsuleReason');
    return (input?.value || '').trim();
  }

  function formatDuration(ms) {
    if (typeof ms !== 'number' || !Number.isFinite(ms) || ms <= 0) return 'now';
    const seconds = Math.floor(ms / 1000);
    const minutes = Math.floor(seconds / 60);
    const hours = Math.floor(minutes / 60);
    const days = Math.floor(hours / 24);
    if (days > 0) return `${days}d ${hours % 24}h`;
    if (hours > 0) return `${hours}h ${minutes % 60}m`;
    if (minutes > 0) return `${minutes}m ${seconds % 60}s`;
    return `${seconds}s`;
  }

  function formatTimestamp(value) {
    if (value === undefined || value === null) return '';
    const date = typeof value === 'number' ? new Date(value) : new Date(String(value));
    if (Number.isNaN(date.getTime())) return '';
    return date.toLocaleString();
  }

  function truncate(text, limit) {
    if (!text) return '';
    return text.length > limit ? `${text.slice(0, limit - 1)}…` : text;
  }

  async function loadCapsulePresets() {
    const select = document.getElementById('capsulePresetSelect');
    const adoptBtn = document.getElementById('capsuleAdoptBtn');
    if (!select) return;
    select.innerHTML = '';
    select.disabled = true;
    if (adoptBtn) adoptBtn.disabled = true;
    try {
      const data = await call('GET', '/admin/policy/capsules/presets');
      const presets = Array.isArray(data?.presets) ? data.presets : [];
      capsulePresets = presets;
      const placeholder = document.createElement('option');
      placeholder.value = '';
      placeholder.textContent = presets.length ? 'Select preset…' : 'No presets available';
      select.appendChild(placeholder);
      presets.forEach((preset) => {
        const opt = document.createElement('option');
        opt.value = preset.id || '';
        const issuer = preset.issuer ? ` · ${preset.issuer}` : '';
        opt.textContent = `${preset.id || 'capsule'} (v${preset.version || '1'}${issuer})`;
        select.appendChild(opt);
      });
      select.disabled = presets.length === 0;
      if (adoptBtn) adoptBtn.disabled = presets.length === 0;
      setCapsuleActionMessage(
        presets.length ? '' : 'No capsule presets available on this server.',
        presets.length ? 'info' : 'info'
      );
    } catch (err) {
      setCapsuleActionMessage(`Failed to load capsule presets: ${err.message}`, 'error');
    }
  }

  function renderCapsuleStatus(snapshot) {
    const summaryEl = document.getElementById('capsuleStatusSummary');
    const listEl = document.getElementById('capsuleStatusList');
    if (!summaryEl || !listEl) return;
    const count = snapshot?.count ?? 0;
    const generated = formatTimestamp(snapshot?.generated ?? snapshot?.generated_ms);
    summaryEl.textContent = count === 0 ? 'No active policy capsules.' : `Active policy capsules: ${count}${generated ? ` — updated ${generated}` : ''}`;
    listEl.innerHTML = '';
    const items = Array.isArray(snapshot?.items) ? snapshot.items : [];
    if (!items.length) {
      const empty = document.createElement('div');
      empty.className = 'capsule-entry';
      const meta = document.createElement('div');
      meta.className = 'capsule-entry__meta';
      meta.textContent = 'Capsules will appear here when adopted.';
      empty.appendChild(meta);
      listEl.appendChild(empty);
      return;
    }
    items.forEach((item) => {
      const entry = document.createElement('div');
      entry.className = 'capsule-entry';
      const meta = document.createElement('div');
      meta.className = 'capsule-entry__meta';
      const title = document.createElement('strong');
      title.textContent = item.id || 'capsule';
      meta.appendChild(title);
      const details = document.createElement('span');
      const issuer = item.issuer ? ` · issuer ${item.issuer}` : '';
      details.textContent = `version ${item.version || '1'}${issuer}`;
      meta.appendChild(details);
      if (item.status_label) {
        const status = document.createElement('span');
        status.textContent = `status: ${item.status_label}`;
        meta.appendChild(status);
      }
      if (item.expires_in_ms !== undefined) {
        const lease = document.createElement('span');
        if (item.expires_in_ms <= 0) {
          lease.textContent = 'lease expired';
        } else {
          const until = formatTimestamp(item.lease_until || item.lease_until_ms);
          lease.textContent = `lease expires in ${formatDuration(item.expires_in_ms)}${until ? ` (${until})` : ''}`;
        }
        meta.appendChild(lease);
      }
      if (item.renew_in_ms !== undefined && item.renew_in_ms > 0) {
        const renew = document.createElement('span');
        renew.textContent = `renew within ${formatDuration(item.renew_in_ms)}`;
        meta.appendChild(renew);
      }
      const buttons = document.createElement('div');
      buttons.className = 'capsule-entry__buttons';
      const btn = document.createElement('button');
      btn.type = 'button';
      btn.textContent = 'Tear down';
      btn.dataset.capsuleId = item.id || '';
      buttons.appendChild(btn);
      entry.appendChild(meta);
      entry.appendChild(buttons);
      listEl.appendChild(entry);
    });
  }

  async function refreshCapsuleStatus() {
    const summaryEl = document.getElementById('capsuleStatusSummary');
    const listEl = document.getElementById('capsuleStatusList');
    if (summaryEl) summaryEl.textContent = 'Loading capsules…';
    if (listEl) listEl.innerHTML = '';
    try {
      const snapshot = await call('GET', '/state/policy/capsules');
      if (typeof snapshot !== 'object' || snapshot === null) throw new Error('unexpected response');
      renderCapsuleStatus(snapshot);
    } catch (err) {
      if (summaryEl) summaryEl.textContent = `Failed to load capsules: ${err.message}`;
      setCapsuleActionMessage(`Capsule status fetch failed: ${err.message}`, 'error');
    }
  }

  async function adoptPreset() {
    const select = document.getElementById('capsulePresetSelect');
    const adoptBtn = document.getElementById('capsuleAdoptBtn');
    if (!select) return;
    const presetId = (select.value || '').trim();
    if (!presetId) {
      setCapsuleActionMessage('Select a preset before adopting.', 'error');
      return;
    }
    if (adoptBtn) adoptBtn.disabled = true;
    setCapsuleActionMessage(`Adopting preset ${presetId}…`);
    try {
      const body = { preset_id: presetId };
      const reason = capsuleReason();
      if (reason) body.reason = reason;
      const resp = await call('POST', '/admin/policy/capsules/adopt', body);
      if (!resp || typeof resp !== 'object' || resp.ok !== true) {
        throw new Error('unexpected response');
      }
      setCapsuleActionMessage(`Adopted preset ${presetId}.`, 'info');
      await refreshCapsuleStatus();
      await refreshCapsuleAudit();
    } catch (err) {
      setCapsuleActionMessage(`Preset adoption failed: ${err.message}`, 'error');
    } finally {
      if (adoptBtn) adoptBtn.disabled = capsulePresets.length === 0;
    }
  }

  async function teardownCapsules(ids, all) {
    setCapsuleActionMessage(all ? 'Tearing down all capsules…' : 'Tearing down capsule…');
    const body = {
      ids: Array.isArray(ids) ? ids : [],
      all: !!all,
      dry_run: false,
    };
    const reason = capsuleReason();
    if (reason) body.reason = reason;
    try {
      const resp = await call('POST', '/admin/policy/capsules/teardown', body);
      if (!resp || typeof resp !== 'object') throw new Error('unexpected response');
      const removed = Array.isArray(resp.removed) ? resp.removed.length : 0;
      setCapsuleActionMessage(
        resp.dry_run ? `Dry-run: would remove ${removed} capsule(s).` : `Removed ${removed} capsule(s).`,
        'info'
      );
      await refreshCapsuleStatus();
      await refreshCapsuleAudit();
    } catch (err) {
      setCapsuleActionMessage(`Teardown failed: ${err.message}`, 'error');
    }
  }

  async function refreshCapsuleAudit() {
    const logEl = document.getElementById('capsuleAuditLog');
    if (logEl) logEl.textContent = 'Loading audit…';
    try {
      const audit = await call('GET', '/admin/policy/capsules/audit?limit=25');
      if (typeof audit !== 'object' || audit === null) throw new Error('unexpected response');
      renderCapsuleAudit(audit);
    } catch (err) {
      if (logEl) logEl.textContent = `Failed to load audit: ${err.message}`;
      setCapsuleActionMessage(`Audit fetch failed: ${err.message}`, 'error');
    }
  }

  function renderCapsuleAudit(data) {
    const logEl = document.getElementById('capsuleAuditLog');
    if (!logEl) return;
    logEl.innerHTML = '';
    const entries = Array.isArray(data?.entries) ? data.entries : [];
    if (!entries.length) {
      logEl.textContent = 'No recent capsule events.';
      return;
    }
    entries.forEach((entry) => {
      const payload = entry?.payload || {};
      const wrapper = document.createElement('div');
      wrapper.className = 'capsule-audit-entry';
      const header = document.createElement('div');
      const headerStrong = document.createElement('strong');
      headerStrong.textContent = truncate(formatTimestamp(entry.time), 48);
      header.appendChild(headerStrong);
      header.appendChild(document.createTextNode(` — ${entry.kind || 'event'}`));
      wrapper.appendChild(header);
      if (payload.id) {
        const idLine = document.createElement('div');
        idLine.textContent = `Capsule: ${payload.id}`;
        wrapper.appendChild(idLine);
      }
      if (payload.status_label) {
        const statusLine = document.createElement('div');
        statusLine.textContent = `Status: ${payload.status_label}`;
        wrapper.appendChild(statusLine);
      }
      if (Array.isArray(payload.removed) && payload.removed.length) {
        const removedLine = document.createElement('div');
        const ids = payload.removed
          .map((item) => item?.id || 'capsule')
          .filter(Boolean)
          .join(', ');
        removedLine.textContent = `Removed: ${truncate(ids, 80)}`;
        wrapper.appendChild(removedLine);
      }
      if (payload.reason) {
        const reasonLine = document.createElement('div');
        reasonLine.textContent = `Reason: ${truncate(payload.reason, 80)}`;
        wrapper.appendChild(reasonLine);
      }
      logEl.appendChild(wrapper);
    });
  }

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
    loadCapsulePresets();
    refreshCapsuleStatus();
    refreshCapsuleAudit();
    byId('capsuleAdoptBtn')?.addEventListener('click', adoptPreset);
    byId('capsuleRefreshBtn')?.addEventListener('click', () => {
      loadCapsulePresets();
      refreshCapsuleStatus();
      refreshCapsuleAudit();
    });
    byId('capsuleTeardownAllBtn')?.addEventListener('click', () => teardownCapsules([], true));
    byId('capsuleAuditRefresh')?.addEventListener('click', refreshCapsuleAudit);
    byId('capsuleStatusList')?.addEventListener('click', (event) => {
      const target = event.target;
      if (!(target instanceof HTMLElement)) return;
      const button = target.closest('button[data-capsule-id]');
      if (button) {
        const id = button.getAttribute('data-capsule-id');
        if (id) teardownCapsules([id], false);
      }
    });
  });
})();
