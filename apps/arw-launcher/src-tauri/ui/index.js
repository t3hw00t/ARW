const invoke = (cmd, args) => ARW.invoke(cmd, args);
const getPort = () => ARW.getPortFromInput('port');
const updateBaseMeta = () => ARW.applyBaseMeta({ portInputId: 'port', badgeId: 'baseBadge', label: 'Base' });
let baseMeta = null;
const effectivePort = () => getPort() || (baseMeta && baseMeta.port) || 8091;

let miniDownloadsSub = null;
let prefsDirty = false;
const prefBaseline = { port: '', autostart: false, notif: true, loginstart: false, adminToken: '' };
let lastHealthCheck = null;
let healthMetaTimer = null;
let serviceLogPath = null;

function shouldOpenAdvancedPrefs() {
  const portEl = document.getElementById('port');
  const auto = document.getElementById('autostart');
  const notif = document.getElementById('notif');
  const login = document.getElementById('loginstart');
  const portVal = portEl ? String(portEl.value ?? '') : '';
  const portChanged = portVal && portVal !== '8091';
  const autostartOn = !!(auto && auto.checked);
  const notificationsOff = !!(notif && !notif.checked);
  const loginOn = !!(login && login.checked);
  const token = document.getElementById('admintok');
  const tokenSet = !!(token && String(token.value || '').trim());
  return portChanged || autostartOn || notificationsOff || loginOn || tokenSet;
}

function syncAdvancedPrefsDisclosure() {
  const advanced = document.querySelector('.hero-preferences');
  if (!advanced) return;
  advanced.open = shouldOpenAdvancedPrefs();
}

function applyPrefsDirty(state) {
  prefsDirty = !!state;
  const btn = document.getElementById('btn-save');
  if (btn) btn.disabled = !prefsDirty;
  const status = document.getElementById('prefsStatus');
  if (status) {
    status.textContent = prefsDirty ? 'Unsaved changes' : 'All changes saved';
    status.dataset.state = prefsDirty ? 'dirty' : 'clean';
  }
}

function snapshotPrefsBaseline() {
  const portEl = document.getElementById('port');
  prefBaseline.port = portEl ? String(portEl.value ?? '') : '';
  const getChecked = (id) => {
    const el = document.getElementById(id);
    return !!(el && el.checked);
  };
  prefBaseline.autostart = getChecked('autostart');
  prefBaseline.notif = getChecked('notif');
  prefBaseline.loginstart = getChecked('loginstart');
  const tokenEl = document.getElementById('admintok');
  prefBaseline.adminToken = tokenEl ? String(tokenEl.value ?? '').trim() : '';
  applyPrefsDirty(false);
}

function calculatePrefsDirty() {
  const portEl = document.getElementById('port');
  const portValue = portEl ? String(portEl.value ?? '') : '';
  if (portValue !== prefBaseline.port) return true;
  const isDirty = (id, key) => {
    const el = document.getElementById(id);
    const checked = !!(el && el.checked);
    return checked !== !!prefBaseline[key];
  };
  if (isDirty('autostart', 'autostart')) return true;
  if (isDirty('notif', 'notif')) return true;
  if (isDirty('loginstart', 'loginstart')) return true;
  const tokenEl = document.getElementById('admintok');
  const tokenValue = tokenEl ? String(tokenEl.value ?? '').trim() : '';
  if (tokenValue !== prefBaseline.adminToken) return true;
  return false;
}

function refreshPrefsDirty() {
  const dirty = calculatePrefsDirty();
  applyPrefsDirty(dirty);
  const tokenEl = document.getElementById('admintok');
  const tokenValue = tokenEl ? String(tokenEl.value ?? '').trim() : '';
  const pending = dirty && tokenValue !== prefBaseline.adminToken;
  updateTokenBadge(pending ? tokenValue : prefBaseline.adminToken, { pending });
  syncAdvancedPrefsDisclosure();
}

function bindPrefWatchers() {
  const portEl = document.getElementById('port');
  if (portEl) portEl.addEventListener('input', refreshPrefsDirty);
  ['autostart', 'notif', 'loginstart'].forEach((id) => {
    const el = document.getElementById(id);
    if (el) el.addEventListener('change', refreshPrefsDirty);
  });
  const tokenEl = document.getElementById('admintok');
  if (tokenEl) tokenEl.addEventListener('input', refreshPrefsDirty);
}

function updateHealthMetaLabel() {
  const metaLabel = document.getElementById('healthMeta');
  if (!metaLabel) return;
  if (!lastHealthCheck) {
    metaLabel.textContent = 'Waiting for first check…';
    return;
  }
  const diff = Date.now() - lastHealthCheck;
  if (diff < 30_000) {
    metaLabel.textContent = 'Checked just now';
    return;
  }
  const minutes = Math.floor(diff / 60_000);
  if (minutes < 60) {
    metaLabel.textContent = `Checked ${minutes} minute${minutes === 1 ? '' : 's'} ago`;
    return;
  }
  const hours = Math.floor(minutes / 60);
  const minutesRemainder = minutes % 60;
  const parts = [`${hours} hour${hours === 1 ? '' : 's'}`];
  if (minutesRemainder) parts.push(`${minutesRemainder} minute${minutesRemainder === 1 ? '' : 's'}`);
  metaLabel.textContent = `Checked ${parts.join(' ')} ago`;
}

function connectSse({ replay = 0, resume = true } = {}) {
  baseMeta = updateBaseMeta();
  const base = (baseMeta && baseMeta.base) || ARW.base(effectivePort());
  const opts = { prefix: 'models.' };
  if (replay > 0) opts.replay = replay;
  ARW.sse.connect(base, opts, resume);
}

function updateTokenBadge(tokenValue, { pending = false } = {}) {
  const wrap = document.getElementById('statusBadges');
  if (!wrap) return;
  let badge = document.getElementById('tokenBadge');
  if (!badge) {
    badge = document.createElement('span');
    badge.id = 'tokenBadge';
    badge.className = 'badge';
    wrap.appendChild(badge);
  }
  if (pending) {
    badge.className = 'badge warn';
    badge.textContent = 'Admin token: unsaved';
    badge.setAttribute('aria-label', 'Admin token has unsaved changes');
    return;
  }
  const hasToken =
    typeof tokenValue === 'string' && tokenValue.trim().length > 0;
  badge.className = hasToken ? 'badge ok' : 'badge warn';
  badge.textContent = hasToken ? 'Admin token: set' : 'Admin token: not set';
  badge.setAttribute(
    'aria-label',
    hasToken ? 'Admin token saved' : 'Admin token not set',
  );
}

async function refreshServiceLogPath({ toastOnError = false } = {}) {
  try {
    const path = await invoke('launcher_service_log_path');
    serviceLogPath =
      typeof path === 'string' && path.trim().length > 0 ? path : null;
  } catch (err) {
    serviceLogPath = null;
    if (toastOnError) {
      ARW.toast('Unable to resolve service log');
    }
  }
  const btn = document.getElementById('btn-log-file');
  if (btn) btn.disabled = !serviceLogPath;
  return serviceLogPath;
}

function initStatusBadges() {
  const wrap = document.getElementById('statusBadges');
  if (!wrap) return;
  updateTokenBadge(prefBaseline.adminToken);
  let badge = document.getElementById('sseBadge');
  if (!badge) {
    badge = document.createElement('span');
    badge.id = 'sseBadge';
    badge.className = 'badge';
    wrap.appendChild(badge);
  }
  ARW.sse.indicator(badge, { prefix: 'SSE' });
}

async function loadPrefs() {
  try {
    const v = await ARW.getPrefs('launcher');
    if (v && typeof v === 'object') {
      if (v.port) document.getElementById('port').value = v.port;
      if (typeof v.autostart === 'boolean') document.getElementById('autostart').checked = v.autostart;
      if (typeof v.notifyOnStatus === 'boolean') document.getElementById('notif').checked = v.notifyOnStatus;
      if (typeof v.adminToken === 'string') document.getElementById('admintok').value = String(v.adminToken).trim();
    }
  } catch {}
  try {
    const enabled = await invoke('launcher_autostart_status');
    document.getElementById('loginstart').checked = !!enabled
  } catch {}
  snapshotPrefsBaseline();
  updateTokenBadge(prefBaseline.adminToken);
  syncAdvancedPrefsDisclosure();
  baseMeta = updateBaseMeta();
  connectSse({ replay: 5, resume: false });
  miniDownloads();
  health();
  await refreshServiceLogPath();
}

async function savePrefs() {
  const v = await ARW.getPrefs('launcher') || {};
  v.port = getPort();
  v.autostart = !!document.getElementById('autostart').checked;
  v.notifyOnStatus = !!document.getElementById('notif').checked;
  const tokenEl = document.getElementById('admintok');
  const tokenValue = String(tokenEl && tokenEl.value ? tokenEl.value : '').trim();
  v.adminToken = tokenValue;
  if (tokenEl) tokenEl.value = tokenValue;
  await ARW.setPrefs('launcher', v);
  connectSse({ replay: 5, resume: false });
  miniDownloads();
}

async function health() {
  const dot = document.getElementById('svc-dot');
  const txt = document.getElementById('svc-text');
  const startBtn = document.getElementById('btn-start');
  const stopBtn = document.getElementById('btn-stop');
  const statusLabel = document.getElementById('health');
  const metaLabel = document.getElementById('healthMeta');
  const heroHint = document.querySelector('.status-hint');
  try {
    const ok = await invoke('check_service_health', { port: effectivePort() });
    const hasToken =
      typeof prefBaseline.adminToken === 'string' &&
      prefBaseline.adminToken.trim().length > 0;
    if (dot) dot.className = 'dot ' + (ok ? 'ok' : 'bad');
    if (txt) txt.innerText = ok ? 'online' : 'offline';
    if (startBtn) startBtn.disabled = ok;
    if (stopBtn) stopBtn.disabled = !ok;
    if (statusLabel) {
      statusLabel.textContent = ok ? 'Service online' : 'Service offline';
      statusLabel.className = ok ? 'ok' : 'bad';
    }
    if (heroHint) {
      heroHint.textContent = ok
        ? hasToken
          ? 'Stack is ready. Open a workspace to dive in.'
          : 'Stack is ready. Set an admin token to secure admin-only surfaces.'
        : 'Start the service to unlock workspaces and live telemetry.';
    }
    lastHealthCheck = Date.now();
    if (metaLabel) updateHealthMetaLabel();
    if (ok) {
      await refreshServiceLogPath();
    }
  } catch {
    if (dot) dot.className = 'dot';
    if (txt) txt.innerText = 'unknown';
    if (startBtn) startBtn.disabled = false;
    if (stopBtn) stopBtn.disabled = true;
    if (statusLabel) {
      statusLabel.textContent = 'Status unavailable';
      statusLabel.className = 'bad';
    }
    if (heroHint) {
      heroHint.textContent = 'Health check failed. Verify the port and try again.';
    }
    lastHealthCheck = Date.now();
    if (metaLabel) updateHealthMetaLabel();
    await refreshServiceLogPath();
  }
}

// Mini downloads widget (models.*)
function miniDownloads() {
  const root = document.getElementById('dlmini');
  if (!root) return;
  root.innerHTML = '';
  if (miniDownloadsSub) {
    ARW.sse.unsubscribe(miniDownloadsSub);
    miniDownloadsSub = null;
  }
  const badges = new Map();
  const last = new Map();
  const ensure = (id) => {
    if (badges.has(id)) return badges.get(id);
    const el = document.createElement('span');
    el.className = 'badge';
    root.appendChild(el);
    badges.set(id, el);
    return el;
  };
  const remove = (id) => {
    const el = badges.get(id);
    if (!el) return;
    if (el.parentNode) el.parentNode.removeChild(el);
    badges.delete(id);
    last.delete(id);
  };
  const render = (el, dotClass, label) => {
    el.innerHTML = '';
    const dot = document.createElement('span');
    dot.className = `dot ${dotClass || ''}`.trim();
    el.appendChild(dot);
    const text = document.createElement('span');
    text.textContent = ` ${label}`;
    el.appendChild(text);
  };
  miniDownloadsSub = ARW.sse.subscribe((kind) => kind.startsWith('models.'), ({ kind, env }) => {
    if (kind !== 'models.download.progress') return;
    const payload = env?.payload || {};
    const id = String(payload.id || '').trim();
    if (!id) return;
    const el = ensure(id);
    if (payload.status === 'complete') {
      render(el, '', `${id} complete`);
      setTimeout(() => remove(id), 1500);
      return;
    }
    if (payload.error || payload.code) {
      const code = payload.code || 'error';
      render(el, 'bad', `${id} ${code}`);
      return;
    }
    const progressNum = ARW.util.downloadPercent(payload);
    const progress = Number.isFinite(progressNum)
      ? `${Math.round(progressNum)}%`
      : (payload.status || '…');
    let tail = '';
    const now = Date.now();
    const downloaded = Number(payload.downloaded || 0);
    const prev = last.get(id);
    last.set(id, { t: now, bytes: downloaded });
    if (prev && downloaded >= prev.bytes) {
      const dt = Math.max(0.001, (now - prev.t) / 1000);
      const rate = (downloaded - prev.bytes) / dt / (1024 * 1024);
      if (rate > 0.05) {
        tail = ` · ${rate.toFixed(2)} MiB/s`;
      }
    }
    const dotClass = payload.status === 'canceled' ? 'bad' : '';
    render(el, dotClass, `${id} ${progress}${tail}`);
  });
}

document.addEventListener('DOMContentLoaded', () => {
  initStatusBadges();
  bindPrefWatchers();
  // Buttons
  document.getElementById('btn-open').addEventListener('click', async () => {
    try { await invoke('open_debug_ui', { port: effectivePort() }); } catch (e) { console.error(e); }
  });
  document.getElementById('btn-open-window').addEventListener('click', async () => {
    try { await invoke('open_debug_window', { port: effectivePort() }); } catch (e) { console.error(e); }
  });
  const logBtn = document.getElementById('btn-log-file');
  if (logBtn) {
    logBtn.addEventListener('click', async () => {
      try {
        const path = await refreshServiceLogPath({ toastOnError: true });
        if (!path) return;
        await invoke('open_path', { path });
      } catch (err) {
        console.error(err);
        ARW.toast('Unable to open service log');
      }
    });
  }
  document.getElementById('btn-events').addEventListener('click', async () => {
    try { await invoke('open_events_window'); } catch (e) { console.error(e); }
  });
  document.getElementById('btn-logs').addEventListener('click', async () => {
    try { await invoke('open_logs_window'); } catch (e) { console.error(e); }
  });
  document.getElementById('btn-models').addEventListener('click', async () => {
    try { await invoke('open_models_window'); } catch (e) { console.error(e); }
  });
  document.getElementById('btn-connections').addEventListener('click', async () => {
    try { await invoke('open_connections_window'); } catch (e) { console.error(e); }
  });
  document.getElementById('btn-hub').addEventListener('click', async () => {
    try { await invoke('open_hub_window'); } catch (e) { console.error(e); }
  });
  document.getElementById('btn-chat').addEventListener('click', async () => {
    try { await invoke('open_chat_window'); } catch (e) { console.error(e); }
  });
  document.getElementById('btn-training').addEventListener('click', async () => {
    try { await invoke('open_training_window'); } catch (e) { console.error(e); }
  });
  const trialBtn = document.getElementById('btn-trial');
  if (trialBtn) trialBtn.addEventListener('click', async () => {
    try { await invoke('open_trial_window'); } catch (e) { console.error(e); }
  });
  document.getElementById('btn-start').addEventListener('click', async () => {
    try {
      await invoke('start_service', { port: effectivePort() });
      ARW.toast('Service starting');
      await refreshServiceLogPath();
    } catch (e) {
      console.error(e);
      const message = e && e.toString ? e.toString() : '';
      if (message && message.includes('service binary not found')) {
        ARW.toast('Build arw-server first (cargo build --release -p arw-server)');
      } else {
        ARW.toast('Unable to start service');
      }
    }
  });
  document.getElementById('btn-stop').addEventListener('click', async () => {
    try {
      await invoke('stop_service', { port: effectivePort() });
      ARW.toast('Service stop requested');
      await refreshServiceLogPath();
    } catch (e) { console.error(e); }
  });
  const saveBtn = document.getElementById('btn-save');
  if (saveBtn) {
    saveBtn.addEventListener('click', async () => {
      if (saveBtn.disabled) return;
      try {
        const previousLoginBaseline = prefBaseline.loginstart;
        const previousTokenBaseline = prefBaseline.adminToken;
        await savePrefs();
        snapshotPrefsBaseline();
        updateTokenBadge(prefBaseline.adminToken);
        const loginstart = document.getElementById('loginstart').checked;
        try {
          await invoke('set_launcher_autostart', { enabled: loginstart });
        } catch (err) {
          console.error(err);
          ARW.toast('Unable to update launch at login');
          const loginToggle = document.getElementById('loginstart');
          if (loginToggle) loginToggle.checked = !!previousLoginBaseline;
          prefBaseline.loginstart = previousLoginBaseline;
          refreshPrefsDirty();
          return;
        }
        const tokenChanged = previousTokenBaseline !== prefBaseline.adminToken;
        if (tokenChanged) {
          const restart = window.confirm('Admin token updated. Restart the local service now to apply the new credentials?');
          if (restart) {
            try {
              await invoke('stop_service', { port: effectivePort() });
            } catch (err) {
              console.error(err);
            }
            try {
              await invoke('start_service', { port: effectivePort() });
              ARW.toast('Service restarted with new token');
            } catch (err) {
              console.error(err);
              ARW.toast('Unable to restart service');
            }
          } else {
            ARW.toast('Restart required to apply token');
          }
          await refreshServiceLogPath();
          await health();
        }
        ARW.toast('Preferences saved');
      } catch (e) {
        console.error(e);
        ARW.toast('Save failed');
        refreshPrefsDirty();
      }
    });
  }
  document.getElementById('btn-updates').addEventListener('click', async () => {
    try {
      await invoke('open_url', { url: 'https://github.com/t3hw00t/ARW/releases' });
    } catch (e) { console.error(e); }
  });
  const portInput = document.getElementById('port');
  if (portInput) {
    portInput.addEventListener('change', () => {
      baseMeta = updateBaseMeta();
      connectSse({ replay: 5, resume: false });
      miniDownloads();
      health();
      refreshPrefsDirty();
    });
  }
  window.addEventListener('arw:base-override-changed', () => {
    connectSse({ replay: 5, resume: false });
    miniDownloads();
    health();
    refreshPrefsDirty();
  });
  const healthBtn = document.getElementById('btn-health');
  if (healthBtn) healthBtn.addEventListener('click', async () => {
    const el = document.getElementById('health');
    if (el) { el.textContent = 'Checking…'; el.className = 'dim'; }
    try {
      await health();
    } catch (e) {
      console.error(e);
    }
  });
  // Service health polling
  health();
  setInterval(health, 4000);
  if (healthMetaTimer) clearInterval(healthMetaTimer);
  healthMetaTimer = setInterval(updateHealthMetaLabel, 30_000);
  // Prefs and mini SSE downloads
  loadPrefs().then(() => {
    updateHealthMetaLabel();
  });
});

window.addEventListener('beforeunload', () => {
  if (healthMetaTimer) {
    clearInterval(healthMetaTimer);
    healthMetaTimer = null;
  }
});
