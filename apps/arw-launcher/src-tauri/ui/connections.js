const invoke = (cmd, args) => ARW.invoke(cmd, args);
let sseIndicatorHandle = null;
let sseIndicatorPrefix = '';
let sseIndicatorNode = null;
let currentSseBase = null;

const ESCAPE_ENTITIES = {
  '&': '&amp;',
  '<': '&lt;',
  '>': '&gt;',
  '"': '&quot;',
  "'": '&#39;',
  '`': '&#96;',
};

const esc = (value) => String(value ?? '').replace(/[&<>"'`]/g, (ch) => ESCAPE_ENTITIES[ch] || ch);
const trimOrEmpty = (value) => (typeof value === 'string' ? value.trim() : '');
const hasToken = (token) => trimOrEmpty(token).length > 0;
const normalizedBase = (value) => ARW.normalizeBase(value) || '';

const cloneConn = (raw) => ({
  name: trimOrEmpty(raw?.name),
  base: trimOrEmpty(raw?.base),
  token: trimOrEmpty(raw?.token),
});

const prepareConn = (raw) => {
  const clone = cloneConn(raw);
  const norm = normalizedBase(clone.base);
  return {
    ...clone,
    normalizedBase: norm,
    displayBase: norm || clone.base,
    hasToken: hasToken(clone.token),
  };
};

function setTokenVisibility(visible) {
  const input = document.getElementById('ctok');
  const toggle = document.getElementById('ctokReveal');
  if (!input || !toggle) return;
  input.type = visible ? 'text' : 'password';
  toggle.textContent = visible ? 'Hide' : 'Show';
  toggle.setAttribute('aria-pressed', visible ? 'true' : 'false');
}

function ensureSseIndicator(prefix = 'SSE') {
  const wrap = document.getElementById('statusBadges');
  if (!wrap) return null;
  if (!sseIndicatorNode) {
    sseIndicatorNode = document.getElementById('connectionsSseBadge');
    if (!sseIndicatorNode) {
      sseIndicatorNode = document.createElement('span');
      sseIndicatorNode.id = 'connectionsSseBadge';
      sseIndicatorNode.className = 'badge';
      wrap.appendChild(sseIndicatorNode);
    }
  }
  if (sseIndicatorHandle && sseIndicatorPrefix === prefix) {
    return sseIndicatorNode;
  }
  if (sseIndicatorHandle) {
    try { sseIndicatorHandle.dispose(); } catch {}
    sseIndicatorHandle = null;
  }
  sseIndicatorPrefix = prefix;
  sseIndicatorHandle = ARW.sse.indicator(sseIndicatorNode, { prefix });
  return sseIndicatorNode;
}

function resolveSseBase() {
  const override = ARW.baseOverride();
  if (override) return override;
  const formInput = document.getElementById('curl');
  const formBase = formInput ? normalizedBase(formInput.value) : '';
  if (formBase) return formBase;
  return ARW.base(8091);
}

function connectSse({ replay = 5, resume = false } = {}) {
  const base = resolveSseBase();
  let hostLabel = 'SSE';
  if (base) {
    try {
      const parsed = new URL(base);
      const host = parsed.host || parsed.hostname;
      if (host) hostLabel = `SSE ${host}`;
    } catch {
      hostLabel = `SSE ${base}`;
    }
  }
  ensureSseIndicator(hostLabel);
  if (!base) {
    currentSseBase = null;
    ARW.sse.close();
    return;
  }
  const sameBase = currentSseBase === base;
  currentSseBase = base;
  ARW.sse.connect(base, { replay, prefix: 'probe.metrics' }, resume && sameBase);
}

async function load() {
  const prefs = (await ARW.getPrefs('launcher')) || {};
  const list = Array.isArray(prefs.connections) ? prefs.connections : [];
  return list.map(cloneConn);
}

async function save(conns) {
  const prefs = (await ARW.getPrefs('launcher')) || {};
  prefs.connections = conns.map(cloneConn);
  await ARW.setPrefs('launcher', prefs);
}

function renderEmpty(rows) {
  const tr = document.createElement('tr');
  const td = document.createElement('td');
  td.colSpan = 4;
  td.className = 'dim';
  td.textContent = 'No saved connections yet.';
  tr.appendChild(td);
  rows.appendChild(tr);
}

async function refresh() {
  const rows = document.getElementById('rows');
  if (!rows) return;
  rows.innerHTML = '';
  const conns = (await load()).map(prepareConn);
  const activeBase = ARW.baseOverride();
  if (!conns.length) {
    const info = document.getElementById('activeBaseInfo');
    if (info) info.textContent = activeBase ? `Active override: ${activeBase}` : 'Active override: none';
    renderEmpty(rows);
    return;
  }
  for (const entry of conns) {
    const tr = document.createElement('tr');
    tr.dataset.clickable = 'true';
    const isActive = activeBase && entry.normalizedBase && activeBase === entry.normalizedBase;
    tr.dataset.active = isActive ? 'true' : 'false';
    const nameCell = entry.name ? esc(entry.name) : '<span class="dim">(unnamed)</span>';
    const tokenBadge = entry.hasToken
      ? ' <span class="pill pill-token" title="Admin token saved" aria-label="Admin token saved">token</span>'
      : '';
    const baseCell = entry.displayBase
      ? `<span class="mono">${esc(entry.displayBase)}</span>`
      : '<span class="dim">—</span>';
    tr.innerHTML = `
      <td>${nameCell}${tokenBadge}</td>
      <td>${baseCell}</td>
      <td data-st class="dim" data-status="pending">…</td>
      <td>
        <button data-activate title="Set as active base">${isActive ? 'Deactivate' : 'Activate'}</button>
        <button data-ev title="Open Events window">Events</button>
        <button data-logs title="Open Logs window">Logs</button>
        <button data-models title="Open Models window">Models</button>
        <button data-open title="Open Debug UI">Open Debug</button>
        <button data-ping title="Ping connection">Ping</button>
        <button data-del title="Delete connection">Delete</button>
      </td>`;
    const statusCell = tr.querySelector('[data-st]');
    if (statusCell) {
      statusCell.setAttribute('role', 'status');
      statusCell.setAttribute('aria-live', 'polite');
    }
    const baseForOps = entry.normalizedBase;
    const setButtonState = (selector, handler) => {
      const button = tr.querySelector(selector);
      if (!button) return;
      if (!baseForOps && selector !== '[data-del]') {
        button.disabled = true;
        return;
      }
      button.addEventListener('click', handler);
    };
    const activateBtn = tr.querySelector('[data-activate]');
    if (activateBtn) {
      activateBtn.disabled = !baseForOps;
      activateBtn.textContent = isActive ? 'Deactivate' : 'Activate';
      activateBtn.addEventListener('click', () => {
        if (!baseForOps) return;
        if (isActive) {
          ARW.clearBaseOverride();
          ARW.toast('Base override cleared');
        } else {
          const normalized = ARW.setBaseOverride(baseForOps);
          ARW.toast(normalized ? `Base override active: ${normalized}` : 'Override cleared');
        }
      });
    }
    setButtonState('[data-ev]', async () => {
      if (!baseForOps) return;
      try {
        await invoke('open_events_window_base', { base: baseForOps, labelSuffix: entry.name || '' });
      } catch {}
    });
    setButtonState('[data-logs]', async () => {
      if (!baseForOps) return;
      try {
        await invoke('open_logs_window_base', { base: baseForOps, labelSuffix: entry.name || '' });
      } catch {}
    });
    setButtonState('[data-models]', async () => {
      if (!baseForOps) return;
      try {
        await invoke('open_models_window_base', { base: baseForOps, labelSuffix: entry.name || '' });
      } catch {}
    });
    setButtonState('[data-open]', async () => {
      if (!baseForOps) return;
      try {
        await invoke('open_url', { url: `${baseForOps}/admin/debug` });
      } catch {}
    });
    setButtonState('[data-ping]', async () => {
      await pingRow(tr, entry);
    });
    const delBtn = tr.querySelector('[data-del]');
    if (delBtn) {
      delBtn.addEventListener('click', async () => {
        const current = await load();
        const filtered = current.filter((c) => {
          return !(c.name === entry.name && normalizedBase(c.base) === entry.normalizedBase);
        });
        await save(filtered);
        const stat = document.getElementById('stat');
        if (stat) stat.textContent = `Removed ${entry.name || entry.displayBase || 'connection'}`;
        await refresh();
      });
    }
    tr.addEventListener('click', (ev) => {
      if (ev.target.closest('button')) return;
      const nameInput = document.getElementById('cname');
      const baseInput = document.getElementById('curl');
      const tokenInput = document.getElementById('ctok');
      if (nameInput) nameInput.value = entry.name;
      if (baseInput) baseInput.value = entry.displayBase || entry.normalizedBase || '';
      if (tokenInput) tokenInput.value = entry.token || '';
      const stat = document.getElementById('stat');
      if (stat) stat.textContent = `Loaded ${entry.name || entry.displayBase || 'connection'}`;
    });
    rows.appendChild(tr);
    pingRow(tr, entry);
  }
  const info = document.getElementById('activeBaseInfo');
  if (info) {
    if (activeBase) {
      info.textContent = `Active override: ${activeBase}`;
    } else {
      info.textContent = 'Active override: none';
    }
  }
  const clearBtn = document.getElementById('btn-clear-override');
  if (clearBtn) clearBtn.disabled = !activeBase;
}

async function pingRow(tr, entry) {
  const st = tr.querySelector('[data-st]');
  if (!st) return;
  st.textContent = '…';
  st.className = 'dim';
  st.dataset.status = 'checking';
  st.removeAttribute('title');
  const baseUrl = entry.normalizedBase || normalizedBase(entry.base);
  if (!baseUrl) {
    st.innerHTML = '<span class="dot bad"></span> invalid base';
    st.className = 'bad';
    st.dataset.status = 'invalid';
    return;
  }
  try {
    const resp = await ARW.http.fetch(baseUrl, '/healthz', { method: 'GET' });
    if (resp.ok) {
      st.innerHTML = '<span class="dot ok"></span> online';
      st.className = 'ok';
      st.dataset.status = 'online';
    } else if (resp.status === 401 || resp.status === 403) {
      if (entry.hasToken) {
        st.innerHTML = '<span class="dot bad"></span> token rejected';
        st.className = 'bad';
        st.dataset.status = 'token';
        st.title = 'Token rejected by remote';
      } else {
        st.innerHTML = '<span class="dot warn"></span> auth required';
        st.className = 'warn';
        st.dataset.status = 'auth';
        st.title = 'Set an admin token (Control Room → Connection & alerts)';
      }
    } else {
      st.innerHTML = `<span class="dot bad"></span> error (${resp.status})`;
      st.className = 'bad';
      st.dataset.status = 'error';
    }
  } catch (err) {
    st.innerHTML = '<span class="dot bad"></span> offline';
    st.className = 'bad';
    st.dataset.status = 'offline';
  }
}

document.addEventListener('DOMContentLoaded', () => {
  const addBtn = document.getElementById('btn-add');
  if (addBtn) {
    addBtn.addEventListener('click', async () => {
      const nameInput = document.getElementById('cname');
      const baseInput = document.getElementById('curl');
      const tokenInput = document.getElementById('ctok');
      const name = trimOrEmpty(nameInput?.value);
      const baseRaw = trimOrEmpty(baseInput?.value);
      const token = trimOrEmpty(tokenInput?.value);
      if (!name) {
        ARW.toast('Connection name required');
        nameInput?.focus();
        return;
      }
      const normalized = normalizedBase(baseRaw);
      if (!normalized) {
        ARW.toast('Enter a valid base URL (http or https)');
        baseInput?.focus();
        return;
      }
      const conns = await load();
      const existing = conns.find((c) => c.name === name);
      if (existing) {
        existing.base = normalized;
        existing.token = token;
      } else {
        conns.push({ name, base: normalized, token });
      }
      await save(conns);
      if (baseInput) baseInput.value = normalized;
      const stat = document.getElementById('stat');
      if (stat) stat.textContent = `Saved ${name}`;
      ARW.toast('Connection saved');
      await refresh();
    });
    ['cname', 'curl', 'ctok'].forEach((id) => {
      const field = document.getElementById(id);
      if (!field) return;
      field.addEventListener('keydown', (ev) => {
        if (ev.key === 'Enter') {
          ev.preventDefault();
          addBtn.click();
        }
      });
    });
  }

  const saveBtn = document.getElementById('btn-save');
  if (saveBtn) {
    saveBtn.addEventListener('click', async () => {
      const conns = await load();
      await save(conns);
      const stat = document.getElementById('stat');
      if (stat) stat.textContent = 'Saved';
      ARW.toast('Connections saved');
    });
  }

  (async () => {
    connectSse({ replay: 5, resume: false });
    ARW.syncBaseCallout();
    await refresh();
    setInterval(refresh, 10000);
  })();
  const clearBtn = document.getElementById('btn-clear-override');
  if (clearBtn) {
    clearBtn.addEventListener('click', () => {
      ARW.clearBaseOverride();
      ARW.toast('Base override cleared');
    });
  }
  const revealBtn = document.getElementById('ctokReveal');
  if (revealBtn) {
    setTokenVisibility(false);
    revealBtn.addEventListener('click', () => {
      const current = revealBtn.getAttribute('aria-pressed') === 'true';
      setTokenVisibility(!current);
    });
  }
  const baseField = document.getElementById('curl');
  if (baseField) {
    baseField.addEventListener('change', () => {
      currentSseBase = null;
      connectSse({ replay: 5, resume: false });
      ARW.syncBaseCallout();
    });
  }
});

window.addEventListener('arw:base-override-changed', () => {
  connectSse({ replay: 5, resume: true });
  ARW.syncBaseCallout();
  refresh();
});
