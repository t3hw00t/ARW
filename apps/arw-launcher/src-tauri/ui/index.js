const invoke = (cmd, args) => ARW.invoke(cmd, args);
const getPort = () => ARW.getPortFromInput('port');
const updateBaseMeta = () => ARW.applyBaseMeta({ portInputId: 'port', badgeId: 'baseBadge', label: 'Base' });
let baseMeta = null;
const effectivePort = () => getPort() || (baseMeta && baseMeta.port) || 8091;

let miniDownloadsSub = null;
let prefsDirty = false;
const prefBaseline = { port: '', autostart: false, notif: true, loginstart: false };
let lastHealthCheck = null;
let healthMetaTimer = null;

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
  return false;
}

function refreshPrefsDirty() {
  applyPrefsDirty(calculatePrefsDirty());
}

function bindPrefWatchers() {
  const portEl = document.getElementById('port');
  if (portEl) portEl.addEventListener('input', refreshPrefsDirty);
  ['autostart', 'notif', 'loginstart'].forEach((id) => {
    const el = document.getElementById(id);
    if (el) el.addEventListener('change', refreshPrefsDirty);
  });
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

function initStatusBadges() {
  const wrap = document.getElementById('statusBadges');
  if (!wrap) return;
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
    }
  } catch {}
  try {
    const enabled = await invoke('launcher_autostart_status');
    document.getElementById('loginstart').checked = !!enabled
  } catch {}
  snapshotPrefsBaseline();
  baseMeta = updateBaseMeta();
  connectSse({ replay: 5, resume: false });
  miniDownloads();
  health();
}

async function savePrefs() {
  const v = await ARW.getPrefs('launcher') || {};
  v.port = getPort();
  v.autostart = !!document.getElementById('autostart').checked;
  v.notifyOnStatus = !!document.getElementById('notif').checked;
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
  try {
    const ok = await invoke('check_service_health', { port: effectivePort() });
    if (dot) dot.className = 'dot ' + (ok ? 'ok' : 'bad');
    if (txt) txt.innerText = ok ? 'online' : 'offline';
    if (startBtn) startBtn.disabled = ok;
    if (stopBtn) stopBtn.disabled = !ok;
    if (statusLabel) {
      statusLabel.textContent = ok ? 'Service online' : 'Service offline';
      statusLabel.className = ok ? 'ok' : 'bad';
    }
    lastHealthCheck = Date.now();
    if (metaLabel) updateHealthMetaLabel();
  } catch {
    if (dot) dot.className = 'dot';
    if (txt) txt.innerText = 'unknown';
    if (startBtn) startBtn.disabled = false;
    if (stopBtn) stopBtn.disabled = true;
    if (statusLabel) {
      statusLabel.textContent = 'Status unavailable';
      statusLabel.className = 'bad';
    }
    lastHealthCheck = Date.now();
    if (metaLabel) updateHealthMetaLabel();
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
    try { await invoke('start_service', { port: effectivePort() }); ARW.toast('Service starting'); } catch (e) { console.error(e); }
  });
  document.getElementById('btn-stop').addEventListener('click', async () => {
    try { await invoke('stop_service', { port: effectivePort() }); ARW.toast('Service stop requested'); } catch (e) { console.error(e); }
  });
  const saveBtn = document.getElementById('btn-save');
  if (saveBtn) {
    saveBtn.addEventListener('click', async () => {
      if (saveBtn.disabled) return;
      try {
        const previousLoginBaseline = prefBaseline.loginstart;
        await savePrefs();
        snapshotPrefsBaseline();
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
