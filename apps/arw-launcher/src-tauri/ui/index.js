const invoke = (cmd, args) => ARW.invoke(cmd, args);
const getPort = () => ARW.getPortFromInput('port');
const updateBaseMeta = () => ARW.applyBaseMeta({ portInputId: 'port', badgeId: 'baseBadge', label: 'Base' });
let baseMeta = null;
const effectivePort = () => getPort() || (baseMeta && baseMeta.port) || 8091;

let miniDownloadsSub = null;

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
  try {
    const ok = await invoke('check_service_health', { port: effectivePort() });
    if (dot) dot.className = 'dot ' + (ok ? 'ok' : 'bad');
    if (txt) txt.innerText = ok ? 'online' : 'offline';
    if (startBtn) startBtn.disabled = ok;
    if (stopBtn) stopBtn.disabled = !ok;
  } catch {
    if (dot) dot.className = 'dot';
    if (txt) txt.innerText = 'unknown';
    if (startBtn) startBtn.disabled = false;
    if (stopBtn) stopBtn.disabled = true;
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
  document.getElementById('btn-save').addEventListener('click', async () => {
    try {
      await savePrefs(); ARW.toast('Preferences saved');
      const loginstart = document.getElementById('loginstart').checked;
      await invoke('set_launcher_autostart', { enabled: loginstart });
    } catch (e) { console.error(e); }
  });
  document.getElementById('btn-updates').addEventListener('click', async () => {
    try {
      await invoke('open_url', { url: 'https://github.com/t3hw00t/ARW/releases' });
    } catch (e) { console.error(e); }
  });
  const portInput = document.getElementById('port');
  if (portInput) {
    portInput.addEventListener('change', async () => {
      baseMeta = updateBaseMeta();
      try {
        const prefs = (await ARW.getPrefs('launcher')) || {};
        prefs.port = effectivePort();
        await ARW.setPrefs('launcher', prefs);
      } catch {}
      connectSse({ replay: 5, resume: false });
      miniDownloads();
    });
  }
  window.addEventListener('arw:base-override-changed', () => {
    connectSse({ replay: 5, resume: false });
    miniDownloads();
    health();
  });
  const healthBtn = document.getElementById('btn-health');
  if (healthBtn) healthBtn.addEventListener('click', async () => {
    const el = document.getElementById('health');
    if (el) { el.textContent = '…'; el.className = ''; }
    try {
      const ok = await invoke('check_service_health', { port: effectivePort() });
      if (el) { el.textContent = ok ? 'Service UP' : 'Service DOWN'; el.className = ok ? 'ok' : 'bad'; }
    } catch (e) {
      if (el) { el.textContent = 'Error'; el.className = 'bad'; }
      console.error(e);
    }
  });
  // Service health polling
  health(); setInterval(health, 4000);
  // Prefs and mini SSE downloads
  loadPrefs();
});
