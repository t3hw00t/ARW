const invoke = (cmd, args) => ARW.invoke(cmd, args);
const IS_DESKTOP = !!(ARW.env && ARW.env.isTauri);
const getPort = () => ARW.getPortFromInput('port');
const updateBaseMeta = () => ARW.applyBaseMeta({ portInputId: 'port', badgeId: 'baseBadge', label: 'Base' });
let baseMeta = null;
const effectivePort = () => getPort() || (baseMeta && baseMeta.port) || 8091;
const CONNECTION_SELECT_ID = 'connectionSelect';
const isExpertMode = () => !!(ARW.mode && ARW.mode.current === 'expert');
const slugify = (value) => String(value || '')
  .toLowerCase()
  .replace(/[^a-z0-9]+/g, '-')
  .replace(/^-+|-+$/g, '')
  || 'project';

let miniDownloadsSub = null;
let prefsDirty = false;
const prefBaseline = { port: '', autostart: false, notif: true, loginstart: false, adminToken: '', baseOverride: '', mascotEnabled: true };
let currentMascotPrefs = {};
let mascotConfigUnlisten = null;
let mascotStatusUnlisten = null;
const MASCOT_CHARACTERS = ['guide','engineer','researcher','navigator','guardian'];
const mascotRuntimeStatus = new Map();
let lastHealthCheck = null;
let healthMetaTimer = null;
let serviceLogPath = null;
let tokenProbeController = null;
let tokenProbeLast = { token: null, state: null, at: 0 };
let serviceOnline = false;
let controlButtonsInitialized = false;
let mascotEnabled = true;
let mascotChecklistState = null;
let mascotSseState = null;
let mascotSseSub = null;
const MASCOT_PRIORITY = {
  error: 3,
  concern: 2,
  thinking: 1,
  ready: 0,
};
const CONTROL_BUTTONS = [
  { id: 'btn-hub', requiresService: true, requiresToken: true },
  { id: 'btn-chat', requiresService: true, requiresToken: true },
  { id: 'btn-training', requiresService: true, requiresToken: true },
  { id: 'btn-trial', requiresService: true, requiresToken: true },
  { id: 'btn-events', requiresService: true, requiresToken: true },
  { id: 'btn-logs', requiresService: true, requiresToken: true },
  { id: 'btn-models', requiresService: true, requiresToken: true },
  { id: 'btn-connections', requiresService: true, requiresToken: true },
  { id: 'btn-open', requiresService: true, requiresToken: false },
  { id: 'btn-open-window', requiresService: true, requiresToken: false },
];
let tokenStatusState = { state: 'missing', message: '', token: '', context: 'saved' };
let connectionsList = [];
let tokenCalloutPrimed = false;
let anonymousAdminAccess = { state: 'unknown', at: 0, base: '' };
let allowAnonymousAdmin = false;

const MASCOT_DEFAULT_MESSAGE = {
  ready: 'All systems ready.',
  thinking: 'Working on it…',
  concern: 'Double-check your setup.',
  error: 'Something needs attention.',
};

function detectPreferredRestartShell() {
  try {
    const nav = window.navigator || {};
    const platform = String(nav.platform || '').toLowerCase();
    const ua = String(nav.userAgent || '').toLowerCase();
    if (platform.includes('win') || ua.includes('windows')) {
      return 'powershell';
    }
  } catch {}
  return 'bash';
}

function alternateRestartShell(shell) {
  return shell === 'powershell' ? 'bash' : 'powershell';
}

function restartShellLabel(shell) {
  return shell === 'powershell' ? 'PowerShell' : 'bash';
}

function quoteForBashEnv(value) {
  const str = String(value ?? '');
  if (!str) return "''";
  const escape = "'\"'\"'";
  return "'" + str.split("'").join(escape) + "'";
}

function quoteForPowerShellEnv(value) {
  const str = String(value ?? '');
  return "'" + str.replace(/'/g, "''") + "'";
}

function buildRestartCommand(token, shell) {
  const trimmed = String(token ?? '').trim();
  if (!trimmed) return '';
  if (shell === 'powershell') {
    const assign = `$env:ARW_ADMIN_TOKEN = ${quoteForPowerShellEnv(trimmed)}`;
    const start = 'powershell -ExecutionPolicy Bypass -File scripts\\start.ps1 -ServiceOnly -WaitHealth -AdminToken $env:ARW_ADMIN_TOKEN';
    return `${assign}\n${start}`;
  }
  const assign = `export ARW_ADMIN_TOKEN=${quoteForBashEnv(trimmed)}`;
  const start = 'bash scripts/start.sh --service-only --wait-health --admin-token "$ARW_ADMIN_TOKEN"';
  return `${assign}\n${start}`;
}

async function copyRestartCommandToClipboard(token, event) {
  const trimmed = String(token ?? '').trim();
  if (!trimmed) {
    ARW.toast('Set an admin token first');
    return;
  }
  const preferred = detectPreferredRestartShell();
  const shell = event && event.shiftKey ? alternateRestartShell(preferred) : preferred;
  const command = buildRestartCommand(trimmed, shell);
  if (!command) {
    ARW.toast('Unable to build restart command');
    return;
  }
  try {
    await navigator.clipboard.writeText(command);
    ARW.toast(`Restart command copied (${restartShellLabel(shell)})`);
  } catch (err) {
    console.error(err);
    ARW.toast('Copy failed — showing command');
    try {
      await ARW.modal.form({
        title: `Restart command (${restartShellLabel(shell)})`,
        description:
          'Copy this command manually. Use Shift with “Copy restart” to switch shells.',
        submitLabel: 'Close',
        hideCancel: true,
        fields: [
          {
            name: 'restart',
            label: 'Command',
            type: 'textarea',
            value: command,
            rows: 6,
            readonly: true,
            monospace: true,
            autoSelect: true,
          },
        ],
      });
    } catch (modalErr) {
      console.error(modalErr);
    }
  }
}

function composeMascotMood(state, message) {
  const normalized = (typeof state === 'string' ? state : '').trim().toLowerCase();
  const resolved = ['ready', 'thinking', 'concern', 'error'].includes(normalized)
    ? normalized
    : 'ready';
  const text = typeof message === 'string' && message.trim().length
    ? message.trim()
    : MASCOT_DEFAULT_MESSAGE[resolved] || MASCOT_DEFAULT_MESSAGE.ready;
  return {
    state: resolved,
    message: text,
    priority: MASCOT_PRIORITY[resolved] ?? 0,
  };
}

function triggerMascotState(state, hint, options = {}) {
  if (!mascotEnabled && !options.force) return;
  try {
    if (window.__TAURI__?.event?.emit) {
      window.__TAURI__.event.emit('mascot:state', {
        state,
        hint: hint && hint.trim ? hint.trim() : hint,
      });
    }
  } catch (err) {
    console.error(err);
  }
}

function applyMascotMood() {
  if (!mascotEnabled) return;
  const candidates = [];
  if (mascotChecklistState) candidates.push(mascotChecklistState);
  if (mascotSseState) candidates.push(mascotSseState);
  if (!candidates.length) {
    triggerMascotState('ready', MASCOT_DEFAULT_MESSAGE.ready);
    return;
  }
  let best = candidates[0];
  for (let i = 1; i < candidates.length; i += 1) {
    const current = candidates[i];
    if ((current?.priority ?? 0) > (best?.priority ?? 0)) {
      best = current;
    }
  }
  triggerMascotState(best.state, best.message);
}

function mapSseStatusToMood(status, env = {}) {
  const normalized = typeof status === 'string' ? status.toLowerCase() : '';
  switch (normalized) {
    case 'error': {
      const retryIn = Number(env.retryIn);
      const seconds = Number.isFinite(retryIn) ? Math.max(1, Math.round(retryIn / 1000)) : null;
      const msg = seconds
        ? `Event stream retrying in ${seconds}s…`
        : 'Event stream retrying…';
      return composeMascotMood('concern', msg);
    }
    case 'closed':
      return composeMascotMood('concern', 'Event stream offline.');
    case 'connecting':
      return composeMascotMood('thinking', 'Connecting to event stream…');
    case 'stale':
      return composeMascotMood('thinking', 'Waiting for fresh events…');
    case 'idle':
      return composeMascotMood('thinking', 'Event stream idle.');
    case 'open':
      return composeMascotMood('ready', 'Event stream live.');
    default:
      return null;
  }
}

function initMascotSseSubscription() {
  if (mascotSseSub || !ARW?.sse || typeof ARW.sse.subscribe !== 'function') return;
  mascotSseSub = ARW.sse.subscribe('*status*', ({ env }) => {
    const status = env?.status;
    const mood = mapSseStatusToMood(status, env);
    mascotSseState = mood;
    applyMascotMood();
  });
}

async function ensureMascotWindow({ focus = false, force = false } = {}) {
  if (!IS_DESKTOP) return;
  if (!mascotEnabled && !force) return;
  try {
    await invoke('open_mascot_window', { profile: 'global' });
    if (focus && window.__TAURI__?.window?.getWindow) {
      const w = window.__TAURI__.window.getWindow('mascot');
      if (w && typeof w.setFocus === 'function') {
        await w.setFocus();
      }
    }
  } catch (err) {
    console.error(err);
  }
}

async function closeMascotWindow() {
  if (!IS_DESKTOP) return;
  try {
    if (window.__TAURI__?.window?.getWindow) {
      const w = window.__TAURI__.window.getWindow('mascot');
      if (w && typeof w.close === 'function') {
        await w.close();
        return;
      }
    }
  } catch (err) {
    console.error(err);
  }
  try {
    await invoke('close_mascot_window', { label: 'mascot' });
  } catch (err) {
    console.error(err);
  }
}

function shouldOpenAdvancedPrefs() {
  if (isExpertMode()) return true;
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
  const baseOverrideActive = !!(typeof ARW !== 'undefined' && ARW.baseOverride && ARW.baseOverride());
  return portChanged || autostartOn || notificationsOff || loginOn || tokenSet || baseOverrideActive;
}

function syncAdvancedPrefsDisclosure() {
  const advanced = document.querySelector('.hero-preferences');
  if (!advanced) return;
  if (advanced.dataset.forceOpen === 'expert') {
    advanced.open = true;
    return;
  }
  if (advanced.dataset.forceOpen === 'true') {
    advanced.open = true;
    return;
  }
  advanced.open = shouldOpenAdvancedPrefs();
}

function ensureAdvancedOpen({ focusToken = false, scrollIntoView = false } = {}) {
  const advanced = document.querySelector('.hero-preferences');
  if (!advanced) return;
  if (!advanced.open) {
    advanced.open = true;
  }
  if (advanced.dataset.forceOpen !== 'expert') {
    advanced.dataset.forceOpen = 'true';
  }
  if (scrollIntoView) {
    try {
      advanced.scrollIntoView({ behavior: 'smooth', block: 'start' });
    } catch {}
  }
  if (focusToken) {
    const input = tokenInputEl();
    if (input) {
      window.requestAnimationFrame(() => {
        try {
          input.focus();
          const len = input.value.length;
          input.setSelectionRange(len, len);
        } catch {}
      });
    }
  }
}

function normalizedBaseForAnonymous(meta) {
  const raw = meta && typeof meta.base === 'string' ? meta.base.trim() : '';
  if (!raw) return '';
  return raw.replace(/\/+$/, '');
}

function resetAnonymousAdminAccess() {
  anonymousAdminAccess = { state: 'unknown', at: 0, base: '' };
  allowAnonymousAdmin = false;
}

async function probeAnonymousAdminAccess({ force = false, meta } = {}) {
  const info = meta || baseMeta || updateBaseMeta();
  const base = normalizedBaseForAnonymous(info);
  if (!base) {
    resetAnonymousAdminAccess();
    anonymousAdminAccess.at = Date.now();
    return false;
  }
  const now = Date.now();
  if (
    !force &&
    anonymousAdminAccess.base === base &&
    anonymousAdminAccess.state !== 'unknown' &&
    now - anonymousAdminAccess.at < 10000
  ) {
    allowAnonymousAdmin = anonymousAdminAccess.state === 'ok';
    return allowAnonymousAdmin;
  }
  anonymousAdminAccess = { state: 'checking', at: now, base };
  try {
    const resp = await fetch(`${base}/state/projects`, {
      method: 'GET',
      headers: { Accept: 'application/json' },
    });
    if (resp.ok) {
      anonymousAdminAccess = { state: 'ok', at: Date.now(), base };
      allowAnonymousAdmin = true;
      return true;
    }
    if (resp.status === 401 || resp.status === 403) {
      anonymousAdminAccess = { state: 'denied', at: Date.now(), base };
    } else {
      anonymousAdminAccess = { state: 'error', at: Date.now(), base };
    }
  } catch {
    anonymousAdminAccess = { state: 'offline', at: Date.now(), base };
  }
  allowAnonymousAdmin = false;
  return false;
}

function enterBrowserMode() {
  document.body.classList.add('browser-mode');
  const callout = document.getElementById('desktopCallout');
  if (callout) callout.hidden = false;
  const heroHint = document.querySelector('.status-hint');
  if (heroHint) {
    heroHint.textContent = 'Manage the service with CLI scripts or the desktop launcher.';
  }
  const markDisabled = (id, hint) => {
    const el = document.getElementById(id);
    if (!el) return;
    el.disabled = true;
    el.setAttribute('aria-disabled', 'true');
    if (hint) el.title = hint;
  };
  markDisabled('btn-start', 'Available in the desktop launcher');
  markDisabled('btn-stop', 'Available in the desktop launcher');
  markDisabled('btn-log-file', 'Available in the desktop launcher');
  markDisabled('autostart', 'Requires desktop launcher');
  markDisabled('loginstart', 'Requires desktop launcher');
  const auto = document.getElementById('autostart');
  if (auto) auto.checked = false;
  const login = document.getElementById('loginstart');
  if (login) login.checked = false;
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
  prefBaseline.mascotEnabled = getChecked('mascotEnabled');
  const tokenEl = document.getElementById('admintok');
  prefBaseline.adminToken = tokenEl ? String(tokenEl.value ?? '').trim() : '';
  prefBaseline.baseOverride =
    (typeof ARW.baseOverride === 'function' && ARW.baseOverride()) || '';
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
  if (isDirty('mascotEnabled', 'mascotEnabled')) return true;
  const tokenEl = document.getElementById('admintok');
  const tokenValue = tokenEl ? String(tokenEl.value ?? '').trim() : '';
  if (tokenValue !== prefBaseline.adminToken) return true;
  const currentOverride =
    (typeof ARW.baseOverride === 'function' && ARW.baseOverride()) || '';
  if (currentOverride !== prefBaseline.baseOverride) return true;
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
  const mascotToggle = document.getElementById('mascotEnabled');
  if (mascotToggle) {
    mascotToggle.addEventListener('change', () => {
      mascotEnabled = !!mascotToggle.checked;
      if (mascotEnabled) {
        ensureMascotWindow({ force: true });
      } else {
        void closeMascotWindow();
      }
      refreshPrefsDirty();
    });
  }
  const tokenEl = document.getElementById('admintok');
  if (tokenEl) tokenEl.addEventListener('input', refreshPrefsDirty);
}

async function fetchLauncherPrefs({ fresh = false } = {}) {
  if (!fresh) {
    return (await ARW.getPrefs('launcher')) || {};
  }
  try {
    const raw = await ARW.invoke('get_prefs', { namespace: 'launcher' });
    if (raw && typeof raw === 'object') {
      try {
        ARW._prefsCache?.set?.('launcher', { ...raw });
      } catch {}
      return raw;
    }
  } catch {}
  return {};
}

function normalizeConnectionEntry(entry) {
  const name = typeof entry?.name === 'string' ? entry.name.trim() : '';
  const baseRaw = typeof entry?.base === 'string' ? entry.base.trim() : '';
  const normalizedBase =
    (ARW.normalizeBase && ARW.normalizeBase(baseRaw)) || baseRaw || '';
  const hasToken =
    typeof entry?.token === 'string' && entry.token.trim().length > 0;
  return {
    name,
    base: baseRaw,
    normalizedBase,
    hasToken,
  };
}

function renderConnectionSelect() {
  const select = document.getElementById(CONNECTION_SELECT_ID);
  if (!select) return;
  const override = (ARW.baseOverride && ARW.baseOverride()) || '';
  const options = [
    {
      value: '',
      label: 'Local default (127.0.0.1)',
    },
  ];
  connectionsList.forEach((entry) => {
    if (!entry.normalizedBase) return;
    let host = entry.normalizedBase;
    try {
      const parsed = new URL(entry.normalizedBase);
      host = parsed.host || parsed.hostname || entry.normalizedBase;
    } catch {}
    const prefix = entry.name ? `${entry.name} — ` : '';
    const suffix = entry.hasToken ? ' (token saved)' : '';
    options.push({
      value: entry.normalizedBase,
      label: `${prefix}${host}${suffix}`,
    });
  });
  const currentValue = override || '';
  const previousFocus = document.activeElement === select;
  select.innerHTML = '';
  options.forEach((opt) => {
    const node = document.createElement('option');
    node.value = opt.value;
    node.textContent = opt.label;
    select.appendChild(node);
  });
  select.value = currentValue;
  if (select.value !== currentValue) {
    select.value = '';
  }
  select.disabled = options.length === 1 && !override;
  if (previousFocus) {
    try {
      select.focus();
    } catch {}
  }
}

function ensureMascotProfile(prefs, profile) {
  if (!prefs.profiles || typeof prefs.profiles !== 'object') {
    prefs.profiles = {};
  }
  if (!prefs.profiles[profile] || typeof prefs.profiles[profile] !== 'object') {
    prefs.profiles[profile] = {};
  }
  const entry = prefs.profiles[profile];
  entry.slug = entry.slug || slugify(profile.replace(/^project:|^custom:/, ''));
  entry.name = entry.name || (profile === 'global' ? 'Global mascot' : profile.replace(/^project:/, ''));
  entry.character = entry.character || prefs.character || 'guide';
  return entry;
}

async function loadMascotPrefs() {
  try {
    const prefs = await ARW.getPrefs('mascot');
    return prefs && typeof prefs === 'object' ? { ...prefs } : {};
  } catch (err) {
    console.error(err);
    return {};
  }
}

async function storeMascotPrefs(prefs) {
  try {
    await ARW.setPrefs('mascot', prefs);
  } catch (err) {
    console.error(err);
  }
}

async function openMascotProfile(profile) {
  const prefs = await loadMascotPrefs();
  const entry = ensureMascotProfile(prefs, profile);
  const quiet = entry.quietMode ?? prefs.quietMode ?? false;
  const compact = entry.compactMode ?? prefs.compactMode ?? false;
  const character = entry.character || prefs.character || 'guide';
  const slug = entry.slug || slugify(profile.replace(/^project:|^custom:/, ''));
  const label = profile === 'global' ? 'mascot' : `mascot-${slug}`;
  try {
    await invoke('open_mascot_window', {
      label,
      profile,
      character,
      quiet,
      compact,
    });
    await sendMascotConfig(profile, {
      quietMode: quiet,
      compactMode: compact,
      character,
      name: entry.name,
    });
    await renderMascotProfiles(prefs);
  } catch (err) {
    console.error(err);
    ARW.toast?.('Unable to open mascot');
  }
}

async function closeMascotProfile(profile) {
  const prefs = await loadMascotPrefs();
  const entry = ensureMascotProfile(prefs, profile);
  const slug = entry.slug || slugify(profile.replace(/^project:|^custom:/, ''));
  const label = profile === 'global' ? 'mascot' : `mascot-${slug}`;
  try {
    await invoke('close_mascot_window', { label });
    mascotRuntimeStatus.set(profile, { ...(mascotRuntimeStatus.get(profile) || {}), streaming: 0, state: 'ready', open: false, name: entry.name });
    await renderMascotProfiles(prefs);
    ARW.toast?.(`Closed mascot for ${entry.name}`);
  } catch (err) {
    console.error(err);
    ARW.toast?.('Unable to close mascot');
  }
}

async function updateMascotProfile(profile, updater, { emitConfig = true } = {}) {
  const prefs = await loadMascotPrefs();
  const entry = ensureMascotProfile(prefs, profile);
  const result = await updater(entry, prefs);
  await storeMascotPrefs(prefs);
  currentMascotPrefs = prefs;
  await renderMascotProfiles(prefs);
  if (emitConfig) {
    await sendMascotConfig(profile, {
      quietMode: entry.quietMode ?? prefs.quietMode ?? false,
      compactMode: entry.compactMode ?? prefs.compactMode ?? false,
      character: entry.character || prefs.character || 'guide',
      name: entry.name,
    });
  }
  return result;
}

async function removeMascotProfile(profile) {
  const prefs = await loadMascotPrefs();
  if (!prefs.profiles || !prefs.profiles[profile]) return;
  delete prefs.profiles[profile];
  await storeMascotPrefs(prefs);
  currentMascotPrefs = prefs;
  mascotRuntimeStatus.delete(profile);
  await renderMascotProfiles(prefs);
  const slug = slugify(profile.replace(/^project:|^custom:/, ''));
  const label = profile === 'global' ? 'mascot' : `mascot-${slug}`;
  try { await invoke('close_mascot_window', { label }); } catch {}
}

function formatProfileTags(entry) {
  const tags = [];
  if (entry.character) tags.push(entry.character);
  if (entry.quietMode) tags.push('quiet');
  if (entry.compactMode) tags.push('compact');
  if (entry.autoOpen) tags.push('auto-open');
  return tags;
}

async function renderMascotProfiles(prefs = currentMascotPrefs) {
  currentMascotPrefs = prefs || {};
  const container = document.getElementById('mascotProfileList');
  if (!container) return;
  container.innerHTML = '';
  const profiles = (currentMascotPrefs.profiles && typeof currentMascotPrefs.profiles === 'object')
    ? Object.entries(currentMascotPrefs.profiles)
    : [];
  if (!profiles.length) {
    const empty = document.createElement('div');
    empty.className = 'mascot-profile-empty';
    empty.textContent = 'No project mascots yet. Create one to keep a role-focused companion on screen.';
    container.appendChild(empty);
    return;
  }
  profiles.sort(([aKey, aVal], [bKey, bVal]) => {
    const aName = (aVal?.name || aKey).toLowerCase();
    const bName = (bVal?.name || bKey).toLowerCase();
    return aName.localeCompare(bName);
  });
  for (const [profile, entryRaw] of profiles) {
    const entry = ensureMascotProfile(currentMascotPrefs, profile);
    const runtime = mascotRuntimeStatus.get(profile) || {};
    const item = document.createElement('div');
    item.className = 'mascot-profile-item';
    item.dataset.profile = profile;
    const info = document.createElement('div');
    info.className = 'mascot-profile-info';
    const title = document.createElement('div');
    title.className = 'mascot-profile-name';
    title.textContent = entry.name || profile;
    const tagsWrap = document.createElement('div');
    tagsWrap.className = 'mascot-profile-tags';
    const slug = entry.slug || slugify(profile.replace(/^project:|^custom:/, ''));
    const label = profile === 'global' ? 'mascot' : `mascot-${slug}`;
    let isOpen = !!runtime.open;
    if (!isOpen) {
      try {
        isOpen = !!(window.__TAURI__?.window?.getWindow?.(label));
      } catch {}
    }
    const statusTag = document.createElement('span');
    statusTag.className = `mascot-tag ${isOpen ? 'status-open' : 'status-closed'}`;
    statusTag.textContent = isOpen ? 'open' : 'closed';
    statusTag.title = isOpen ? 'Mascot window is open' : 'Mascot window is closed';
    tagsWrap.appendChild(statusTag);
    const tags = formatProfileTags(entry);
    tags.forEach((tag) => {
      const pill = document.createElement('span');
      pill.className = 'mascot-tag';
      pill.textContent = tag;
      tagsWrap.appendChild(pill);
    });
    if (runtime.streaming > 0) {
      const pill = document.createElement('span');
      pill.className = 'mascot-tag';
      pill.textContent = `streaming ×${runtime.streaming}`;
      tagsWrap.appendChild(pill);
    }
    if (runtime.state && runtime.state !== 'ready') {
      const pill = document.createElement('span');
      pill.className = 'mascot-tag';
      pill.textContent = runtime.state;
      tagsWrap.appendChild(pill);
    }
    if (!tags.length) {
      const pill = document.createElement('span');
      pill.className = 'mascot-tag';
      pill.textContent = 'default';
      tagsWrap.appendChild(pill);
    }
    info.appendChild(title);
    info.appendChild(tagsWrap);
    const actions = document.createElement('div');
    actions.className = 'mascot-profile-actions';
    const makeBtn = (label, action, className = 'ghost mini') => {
      const btn = document.createElement('button');
      btn.type = 'button';
      btn.className = className;
      btn.dataset.action = action;
      btn.textContent = label;
      return btn;
    };
    actions.appendChild(makeBtn(isOpen ? 'Focus' : 'Open', 'open', 'primary mini'));
    const focusBtn = actions.lastChild;
    focusBtn.title = isOpen ? 'Bring this mascot to the front' : 'Open mascot window';
    if (isOpen) {
      const closeBtn = makeBtn('Close', 'close');
      closeBtn.title = 'Close mascot window';
      actions.appendChild(closeBtn);
    }
    actions.appendChild(makeBtn(entry.quietMode ? 'Quiet on' : 'Quiet off', 'toggle-quiet'));
    actions.appendChild(makeBtn(entry.compactMode ? 'Compact on' : 'Compact off', 'toggle-compact'));
    actions.appendChild(makeBtn(entry.autoOpen ? 'Auto-on' : 'Auto-off', 'toggle-auto'));
    const charBtn = makeBtn('Character', 'cycle-character');
    charBtn.title = 'Cycle character';
    actions.appendChild(charBtn);
    const renameBtn = makeBtn('Rename', 'rename');
    renameBtn.title = 'Rename profile';
    actions.appendChild(renameBtn);
    const deleteBtn = makeBtn('Remove', 'delete');
    deleteBtn.title = 'Remove profile';
    actions.appendChild(deleteBtn);
    item.appendChild(info);
    item.appendChild(actions);
    container.appendChild(item);
  }
}

async function renameMascotProfile(profile) {
  const prefs = await loadMascotPrefs();
  const entry = ensureMascotProfile(prefs, profile);
  if (!ARW.modal?.form) {
    const next = window.prompt('Profile name', entry.name || profile);
    if (!next) return;
    await updateMascotProfile(profile, (entry) => { entry.name = next.trim(); return entry.name; });
    ARW.toast?.(`Profile renamed to ${ensureMascotProfile(currentMascotPrefs, profile).name}`);
    return;
  }
  const result = await ARW.modal.form({
    title: 'Rename Mascot',
    submitLabel: 'Save',
    fields: [
      { name: 'name', label: 'Profile name', required: true, value: entry.name || profile, placeholder: 'Project Alpha' },
    ],
  });
  if (!result || !result.name) return;
  const nextName = String(result.name || '').trim();
  if (!nextName) return;
  await updateMascotProfile(profile, (entry) => { entry.name = nextName; return entry.name; });
  ARW.toast?.(`Profile renamed to ${ensureMascotProfile(currentMascotPrefs, profile).name}`);
}

async function handleDeleteMascotProfile(profile) {
  const prefs = await loadMascotPrefs();
  const entry = ensureMascotProfile(prefs, profile);
  let confirmed = true;
  if (ARW.modal?.form) {
    const result = await ARW.modal.form({
      title: 'Remove Mascot Profile',
      description: `Remove the mascot profile for “${entry.name || profile}”?`,
      submitLabel: 'Remove',
      destructive: true,
      fields: [],
    });
    confirmed = !!result;
  } else {
    confirmed = window.confirm(`Remove mascot profile ${entry.name || profile}?`);
  }
  if (!confirmed) return;
  await removeMascotProfile(profile);
  ARW.toast?.(`Removed mascot profile ${entry.name || profile}`);
}

async function fetchProjectsForMascot() {
  try {
    baseMeta = updateBaseMeta();
    const resolvedBase = (baseMeta && baseMeta.base) || ARW.base(effectivePort());
    if (!resolvedBase) return [];
    let data = null;
    if (ARW.http?.json) {
      data = await ARW.http.json(resolvedBase, '/state/projects');
    } else {
      const resp = await fetch(`${String(resolvedBase).replace(/\/$/, '')}/state/projects`, {
        headers: { Accept: 'application/json' },
      });
      if (!resp.ok) return [];
      data = await resp.json();
    }
    const items = Array.isArray(data?.items) ? data.items : Array.isArray(data) ? data : [];
    return items
      .map((item) => {
        if (!item) return '';
        if (typeof item === 'string') return item;
        if (typeof item === 'object') return item.name || item.id || '';
        return '';
      })
      .filter(Boolean)
      .filter((value, index, arr) => arr.indexOf(value) === index)
      .sort((a, b) => a.localeCompare(b));
  } catch (err) {
    console.error(err);
    return [];
  }
}

async function createMascotProfileManual() {
  if (!ARW.modal?.form) {
    ARW.toast?.('Desktop launcher modal unavailable');
    return;
  }
  const result = await ARW.modal.form({
    title: 'New Mascot Profile',
    submitLabel: 'Create',
    fields: [
      { name: 'name', label: 'Profile name', placeholder: 'Support crew', required: true },
      { name: 'slug', label: 'Identifier (optional)', placeholder: 'support' },
      { name: 'character', label: 'Character', type: 'select', value: 'guide', options: MASCOT_CHARACTERS.map((value) => ({ value, label: value.charAt(0).toUpperCase() + value.slice(1) })) },
      { name: 'quietMode', label: 'Start in quiet mode', type: 'checkbox', value: false },
      { name: 'compactMode', label: 'Start in compact mode', type: 'checkbox', value: false },
      { name: 'autoOpen', label: 'Reopen automatically on launch', type: 'checkbox', value: false },
      { name: 'openNow', label: 'Open immediately', type: 'checkbox', value: true },
    ],
  });
  if (!result || !result.name) return;
  const name = String(result.name || '').trim();
  if (!name) return;
  const slug = result.slug ? slugify(result.slug) : slugify(name);
  const profile = `custom:${slug}`;
  const prefs = await loadMascotPrefs();
  const entry = ensureMascotProfile(prefs, profile);
  entry.name = name;
  entry.slug = slug;
  entry.character = result.character || entry.character || prefs.character || 'guide';
  entry.quietMode = !!result.quietMode;
  entry.compactMode = !!result.compactMode;
  entry.autoOpen = !!result.autoOpen;
  await storeMascotPrefs(prefs);
  currentMascotPrefs = prefs;
  await renderMascotProfiles(prefs);
  if (result.openNow !== false) {
    await openMascotProfile(profile);
  }
}

async function createMascotProfileFromProject() {
  if (!ARW.modal?.form) {
    ARW.toast?.('Desktop launcher modal unavailable');
    return;
  }
  const projects = await fetchProjectsForMascot();
  if (!projects.length) {
    ARW.toast?.('No projects found. Create a project first, or add a manual profile.');
    await createMascotProfileManual();
    return;
  }
  const options = projects.map((name) => ({ value: name, label: name }));
  const result = await ARW.modal.form({
    title: 'New Project Mascot',
    submitLabel: 'Create',
    fields: [
      { name: 'project', label: 'Project', type: 'select', value: options[0].value, options },
      { name: 'character', label: 'Character', type: 'select', value: 'guide', options: MASCOT_CHARACTERS.map((value) => ({ value, label: value.charAt(0).toUpperCase() + value.slice(1) })) },
      { name: 'quietMode', label: 'Start in quiet mode', type: 'checkbox', value: false },
      { name: 'compactMode', label: 'Start in compact mode', type: 'checkbox', value: false },
      { name: 'autoOpen', label: 'Reopen automatically on launch', type: 'checkbox', value: true },
      { name: 'openNow', label: 'Open immediately', type: 'checkbox', value: true },
    ],
  });
  if (!result || !result.project) return;
  const name = String(result.project || '').trim();
  if (!name) return;
  const slug = slugify(name);
  const profile = `project:${slug}`;
  const prefs = await loadMascotPrefs();
  const entry = ensureMascotProfile(prefs, profile);
  entry.name = name;
  entry.slug = slug;
  entry.character = result.character || entry.character || prefs.character || 'guide';
  entry.quietMode = !!result.quietMode;
  entry.compactMode = !!result.compactMode;
  entry.autoOpen = !!result.autoOpen;
  await storeMascotPrefs(prefs);
  currentMascotPrefs = prefs;
  await renderMascotProfiles(prefs);
  if (result.openNow !== false) {
    await openMascotProfile(profile);
  }
}

async function handleMascotProfileListClick(event) {
  const button = event.target.closest('button[data-action]');
  if (!button) return;
  const row = button.closest('.mascot-profile-item');
  if (!row || !row.dataset.profile) return;
  const profile = row.dataset.profile;
  const action = button.dataset.action;
  event.preventDefault();
  switch (action) {
    case 'open':
      await openMascotProfile(profile);
      {
        const entry = ensureMascotProfile(currentMascotPrefs, profile);
        ARW.toast?.(`Mascot opened for ${entry.name}`);
      }
      break;
    case 'toggle-quiet':
      await updateMascotProfile(profile, (entry, prefs) => {
        entry.quietMode = !(entry.quietMode ?? prefs.quietMode ?? false);
      });
      {
        const entry = ensureMascotProfile(currentMascotPrefs, profile);
        ARW.toast?.(`Quiet mode ${entry.quietMode ? 'enabled' : 'disabled'} for ${entry.name}`);
      }
      break;
    case 'toggle-compact':
      await updateMascotProfile(profile, (entry, prefs) => {
        entry.compactMode = !(entry.compactMode ?? prefs.compactMode ?? false);
      });
      {
        const entry = ensureMascotProfile(currentMascotPrefs, profile);
        ARW.toast?.(`Compact mode ${entry.compactMode ? 'enabled' : 'disabled'} for ${entry.name}`);
      }
      break;
    case 'toggle-auto':
      await updateMascotProfile(profile, (entry) => {
        entry.autoOpen = !(entry.autoOpen ?? false);
      }, { emitConfig: false });
      {
        const entry = ensureMascotProfile(currentMascotPrefs, profile);
        ARW.toast?.(`Auto reopen ${entry.autoOpen ? 'enabled' : 'disabled'} for ${entry.name}`);
      }
      break;
    case 'cycle-character':
      await updateMascotProfile(profile, (entry, prefs) => {
        const current = entry.character || prefs.character || 'guide';
        const idx = MASCOT_CHARACTERS.indexOf(current);
        entry.character = MASCOT_CHARACTERS[(idx + 1) % MASCOT_CHARACTERS.length];
      });
      {
        const entry = ensureMascotProfile(currentMascotPrefs, profile);
        ARW.toast?.(`Character set to ${entry.character} for ${entry.name}`);
      }
      break;
    case 'rename':
      await renameMascotProfile(profile);
      break;
    case 'delete':
      await handleDeleteMascotProfile(profile);
      break;
    case 'close':
      await closeMascotProfile(profile);
      break;
    default:
      break;
  }
}

async function refreshConnections({ fresh = false } = {}) {
  const prefs = await fetchLauncherPrefs({ fresh });
  const rawList = Array.isArray(prefs.connections) ? prefs.connections : [];
  connectionsList = rawList.map(normalizeConnectionEntry);
  renderConnectionSelect();
  syncAdvancedPrefsDisclosure();
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

function syncTokenCallout({ tokenValue, state, context } = {}) {
  const callout = document.getElementById('tokenCallout');
  const body = document.getElementById('tokenCalloutBody');
  if (!callout) return;
  const trimmed = typeof tokenValue === 'string' ? tokenValue.trim() : '';
  const currentState = state || (trimmed ? 'saved' : 'missing');
  let show = false;
  let message = '';
  switch (currentState) {
    case 'pending':
      show = true;
      message =
        'Save your changes to update the admin token, then restart the service when prompted so workspaces stay authorized.';
      break;
    case 'missing':
      show = true;
      message =
        'Paste an existing token or use Generate to create a new secret. Tokens gate access to admin surfaces and should remain private.';
      break;
    case 'invalid':
      show = true;
      message =
        'Token rejected. Generate a new secret or double-check the value, then save your changes.';
      break;
    case 'offline':
      show = true;
      message = 'Start the service so the launcher can verify your admin token.';
      break;
    case 'error':
      show = true;
      message = 'Unable to verify the admin token. Check service logs and retry.';
      break;
    case 'open':
      show = true;
      message =
        'Running in debug mode. Admin surfaces are open without a token—set one before exposing the service.';
      break;
    default:
      if (!trimmed) {
        show = true;
        message =
          'Paste an existing token or use Generate to create a new secret. Tokens gate access to admin surfaces and should remain private.';
      }
      break;
  }
  callout.hidden = !show;
  callout.setAttribute('aria-hidden', show ? 'false' : 'true');
  if (!show) {
    tokenCalloutPrimed = false;
    const advanced = document.querySelector('.hero-preferences');
    if (advanced && advanced.dataset.forceOpen === 'true') {
      delete advanced.dataset.forceOpen;
      syncAdvancedPrefsDisclosure();
    }
    return;
  }
  const needsFocusStates = new Set(['missing', 'pending', 'invalid', 'offline', 'error']);
  if (!tokenCalloutPrimed && needsFocusStates.has(currentState)) {
    ensureAdvancedOpen({ focusToken: true, scrollIntoView: true });
    tokenCalloutPrimed = true;
  }
  if (!body) return;
  body.textContent = message;
}

function tokenInputEl() {
  return document.getElementById('admintok');
}

function setTokenVisibility(show) {
  const input = tokenInputEl();
  const toggle = document.getElementById('btn-token-toggle');
  if (!input || !toggle) return;
  const shouldShow = !!show;
  input.type = shouldShow ? 'text' : 'password';
  toggle.textContent = shouldShow ? 'Hide' : 'Show';
  toggle.setAttribute('aria-pressed', shouldShow ? 'true' : 'false');
  toggle.setAttribute(
    'aria-label',
    shouldShow ? 'Hide admin token' : 'Show admin token',
  );
}

function toggleTokenVisibility() {
  const input = tokenInputEl();
  if (!input) return;
  const shouldShow = input.type === 'password';
  setTokenVisibility(shouldShow);
}

function setTokenValue(value, { focusEnd = false } = {}) {
  const input = tokenInputEl();
  if (!input) return '';
  const trimmed = typeof value === 'string' ? value.trim() : '';
  input.value = trimmed;
  refreshPrefsDirty();
  if (focusEnd) {
    window.requestAnimationFrame(() => {
      try {
        input.focus();
        const len = input.value.length;
        input.setSelectionRange(len, len);
      } catch {}
    });
  }
  return trimmed;
}

const TOKEN_STATUS_MESSAGES = {
  saved: {
    missing: 'No admin token saved. Generate or paste one to unlock Projects, Conversations, and Training.',
    saved: 'Admin token saved. Start the service or use Test to verify.',
    valid: 'Admin token accepted.',
    invalid: 'Admin token rejected. Generate a new secret or paste the correct value.',
    testing: 'Testing admin token…',
    offline: 'Service unreachable. Start the service, then try again.',
    error: 'Unable to verify admin token. Check the service logs and retry.',
    open: 'Admin surfaces unlocked (debug mode). Set a token before you invite others.',
  },
  pending: {
    pending: 'Unsaved admin token. Save preferences to apply it.',
    valid: 'Token accepted. Save to apply.',
    invalid: 'Token rejected. Update the value before saving.',
    testing: 'Testing unsaved admin token…',
    offline: 'Service unreachable. Save once the service is reachable, then retry.',
    error: 'Unable to verify unsaved token. Adjust and retry.',
    missing: 'No admin token in progress.',
    saved: 'Admin token saved.',
    open: 'Debug mode active. Save a token when you are ready to lock access.',
  },
};

const STEP_STATUS_VARIANTS = ['pending', 'active', 'ready', 'attention'];
const STEP_STATUS_DEFAULT_TEXT = {
  pending: 'Not started',
  active: 'In progress',
  ready: 'Ready',
  attention: 'Action needed',
};

function setStepStatus(id, state, message) {
  const el = document.getElementById(id);
  if (!el) return;
  const normalized = STEP_STATUS_VARIANTS.includes(state) ? state : 'pending';
  for (const variant of STEP_STATUS_VARIANTS) {
    el.classList.remove(`step-status--${variant}`);
  }
  el.classList.add(`step-status--${normalized}`);
  const text = el.querySelector('.step-status-text');
  if (text) {
    const fallback = STEP_STATUS_DEFAULT_TEXT[normalized] || STEP_STATUS_DEFAULT_TEXT.pending;
    text.textContent = message && message.trim() ? message : fallback;
  }
}

function updateSetupChecklist({
  serviceOnline,
  tokenReady,
  tokenNeedsSave,
  tokenTesting,
  tokenMissing,
  tokenInvalid,
  tokenOffline,
  tokenErrored,
  openAccessActive,
  gateMessage,
}) {
  // Step 1 — Service
  let step1State = 'pending';
  let step1Message = 'Start the service to continue.';
  if (serviceOnline) {
    step1State = 'ready';
    step1Message = 'Service online.';
  } else if (lastHealthCheck == null) {
    step1State = 'active';
    step1Message = 'Checking service…';
  } else {
    step1State = 'attention';
    step1Message = 'Start the service to continue.';
  }
  setStepStatus('status-step-service', step1State, step1Message);

  // Step 2 — Token
  let step2State = 'pending';
  let step2Message = 'Paste or generate an admin token.';
  if (tokenReady) {
    step2State = 'ready';
    step2Message = openAccessActive
      ? 'Debug mode active — token optional.'
      : 'Admin token saved.';
  } else if (tokenNeedsSave) {
    step2State = 'active';
    step2Message = 'Save your admin token to apply changes.';
  } else if (tokenTesting) {
    step2State = 'active';
    step2Message = 'Testing admin token…';
  } else if (tokenInvalid) {
    step2State = 'attention';
    step2Message = 'Fix the admin token to continue.';
  } else if (tokenOffline) {
    step2State = 'attention';
    step2Message = 'Bring the service online to verify the token.';
  } else if (tokenErrored) {
    step2State = 'attention';
    step2Message = 'Resolve token errors, then retry.';
  } else if (!tokenMissing) {
    step2State = 'pending';
    step2Message = 'Paste or generate an admin token.';
  }
  setStepStatus('status-step-token', step2State, step2Message);

  // Step 3 — Workspaces
  let step3State = 'pending';
  let step3Message = 'Complete Steps 1 and 2 first.';
  if (serviceOnline && tokenReady) {
    step3State = 'ready';
    step3Message = 'Workspaces unlocked.';
  } else if (!serviceOnline) {
    step3State = 'pending';
    step3Message = 'Complete Step 1 to unlock workspaces.';
  } else if (tokenNeedsSave) {
    step3State = 'active';
    step3Message = 'Save your admin token to unlock workspaces.';
  } else if (tokenTesting) {
    step3State = 'active';
    step3Message = 'Waiting on token test…';
  } else if (tokenMissing) {
    step3State = 'pending';
    step3Message = 'Complete Step 2 to unlock workspaces.';
  } else if (tokenInvalid) {
    step3State = 'attention';
    step3Message = 'Fix the admin token before launching workspaces.';
  } else if (tokenOffline) {
    step3State = 'attention';
    step3Message = 'Bring the service online to unlock workspaces.';
  } else if (tokenErrored) {
    step3State = 'attention';
    step3Message = 'Resolve token errors to continue.';
  }
  if (gateMessage && step3State !== 'ready') {
    step3Message = gateMessage;
  }
  setStepStatus('status-step-workspaces', step3State, step3Message);

  let mascotState = 'ready';
  let mascotMessage = 'All systems ready.';
  if (!serviceOnline) {
    mascotState = 'concern';
    mascotMessage = 'Start the service to continue.';
  } else if (tokenTesting) {
    mascotState = 'thinking';
    mascotMessage = 'Testing admin token…';
  } else if (tokenNeedsSave) {
    mascotState = 'thinking';
    mascotMessage = 'Save the admin token to apply changes.';
  } else if (tokenInvalid) {
    mascotState = 'error';
    mascotMessage = 'Admin token invalid — update it before you continue.';
  } else if (tokenOffline) {
    mascotState = 'concern';
    mascotMessage = 'Bring the service online to verify the token.';
  } else if (tokenErrored) {
    mascotState = 'concern';
    mascotMessage = 'Resolve token errors to unlock workspaces.';
  } else if (!tokenReady && !openAccessActive) {
    mascotState = 'thinking';
    mascotMessage = 'Paste or generate an admin token.';
  } else if (openAccessActive) {
    mascotState = 'thinking';
    mascotMessage = 'Debug mode active — set a token before sharing.';
  }
  mascotChecklistState = composeMascotMood(mascotState, mascotMessage);
  applyMascotMood();
}

function tokenBadgeClass(state) {
  switch (state) {
    case 'valid':
      return 'badge ok';
    case 'invalid':
    case 'error':
      return 'badge bad';
    case 'open':
      return 'badge ok';
    case 'pending':
    case 'missing':
    case 'testing':
    case 'offline':
      return 'badge warn';
    default:
      return 'badge';
  }
}

function tokenBadgeText(state) {
  switch (state) {
    case 'pending':
      return 'Admin token: unsaved';
    case 'missing':
      return 'Admin token: not set';
    case 'testing':
      return 'Admin token: testing…';
    case 'valid':
      return 'Admin token: valid';
    case 'invalid':
      return 'Admin token: invalid';
    case 'error':
      return 'Admin token: error';
    case 'offline':
      return 'Admin token: awaiting service';
    case 'open':
      return 'Admin token: debug access';
    case 'saved':
    default:
      return 'Admin token: saved';
  }
}

function tokenBadgeAria(state) {
  switch (state) {
    case 'pending':
      return 'Admin token has unsaved changes';
    case 'missing':
      return 'Admin token not set';
    case 'testing':
      return 'Admin token verification in progress';
    case 'valid':
      return 'Admin token verified';
    case 'invalid':
      return 'Admin token invalid';
    case 'error':
      return 'Admin token verification failed';
    case 'offline':
      return 'Admin token saved; waiting for service';
    case 'open':
      return 'Admin token not required (debug mode active)';
    case 'saved':
    default:
      return 'Admin token saved';
  }
}

function tokenStatusTone(state) {
  switch (state) {
    case 'valid':
      return 'ok';
    case 'invalid':
    case 'error':
      return 'bad';
    case 'open':
      return 'ok';
    default:
      return 'warn';
  }
}

function defaultTokenStatusMessage(state, context) {
  const scope = context === 'pending' ? 'pending' : 'saved';
  const table = TOKEN_STATUS_MESSAGES[scope] || TOKEN_STATUS_MESSAGES.saved;
  return table[state] || (scope === 'pending'
    ? 'Admin token pending save.'
    : 'Admin token status pending verification.');
}

function setTokenStatus(state, options = {}) {
  const { message, tokenValue = '', context = 'saved' } = options;
  const trimmed = typeof tokenValue === 'string' ? tokenValue.trim() : '';
  const resolvedMessage = typeof message === 'string' && message.trim()
    ? message
    : defaultTokenStatusMessage(state, context);
  if (
    tokenStatusState.state === state &&
    tokenStatusState.context === context &&
    tokenStatusState.token === trimmed &&
    tokenStatusState.message === resolvedMessage
  ) {
    return;
  }
  tokenStatusState = {
    state,
    context,
    token: trimmed,
    message: resolvedMessage,
  };
  const statusEl = document.getElementById('tokenStatus');
  if (statusEl) {
    statusEl.textContent = resolvedMessage;
    statusEl.className = `field-note token-status ${tokenStatusTone(state)}`;
  }
  syncTokenCallout({ tokenValue: trimmed, state, context });
  updateWorkspaceAvailability();
}

function updateTokenBadge(tokenValue, { pending = false, state, message, context } = {}) {
  const wrap = document.getElementById('statusBadges');
  if (!wrap) return;
  let badge = document.getElementById('tokenBadge');
  if (!badge) {
    badge = document.createElement('span');
    badge.id = 'tokenBadge';
    badge.className = 'badge';
    wrap.appendChild(badge);
  }
  const trimmed = typeof tokenValue === 'string' ? tokenValue.trim() : '';
  const resolvedContext = context || (pending ? 'pending' : 'saved');
  let effectiveState = state;
  if (!effectiveState) {
    if (pending) effectiveState = 'pending';
    else if (!trimmed) effectiveState = 'missing';
    else effectiveState = 'saved';
  }
  badge.className = tokenBadgeClass(effectiveState);
  badge.textContent = tokenBadgeText(effectiveState);
  badge.setAttribute('aria-label', tokenBadgeAria(effectiveState));
  badge.setAttribute('data-token-state', effectiveState);
  setTokenStatus(effectiveState, { message, tokenValue: trimmed, context: resolvedContext });
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

function markTokenProbeState(token, state) {
  tokenProbeLast = {
    token: typeof token === 'string' ? token : null,
    state: state || null,
    at: Date.now(),
  };
}

async function probeAdminToken(tokenValue, { context = 'saved', updateBadge = context === 'saved', reason = 'manual' } = {}) {
  const trimmed = typeof tokenValue === 'string' ? tokenValue.trim() : '';
  if (!trimmed) {
    markTokenProbeState(null, null);
    if (updateBadge) {
      updateTokenBadge('', { state: 'missing', context });
    } else {
      setTokenStatus('missing', { tokenValue: '', context });
    }
    return null;
  }
  if (tokenProbeController) {
    try {
      tokenProbeController.abort();
    } catch {}
  }
  const meta = baseMeta || updateBaseMeta();
  const base = (meta && meta.base) || ARW.base(effectivePort());
  const url = `${String(base).replace(/\/$/, '')}/state/projects`;
  const controller = new AbortController();
  tokenProbeController = controller;
  const signal = controller.signal;
  const testingMessage = context === 'pending'
    ? 'Testing unsaved admin token…'
    : 'Testing admin token…';
  if (updateBadge) {
    updateTokenBadge(trimmed, { state: 'testing', message: testingMessage, context });
  } else {
    setTokenStatus('testing', { tokenValue: trimmed, context, message: testingMessage });
  }
  try {
    const resp = await fetch(url, {
      method: 'GET',
      headers: {
        Accept: 'application/json',
        Authorization: `Bearer ${trimmed}`,
        'X-ARW-Admin': trimmed,
      },
      signal,
    });
    if (resp.ok) {
      if (updateBadge) {
        updateTokenBadge(trimmed, { state: 'valid', message: 'Admin token accepted.', context });
      } else {
        const msg = context === 'pending'
          ? 'Token accepted. Save to apply.'
          : 'Admin token accepted.';
        setTokenStatus('valid', { tokenValue: trimmed, context, message: msg });
      }
      markTokenProbeState(trimmed, 'valid');
      return true;
    }
    if (resp.status === 401 || resp.status === 403) {
      if (updateBadge) {
        updateTokenBadge(trimmed, {
          state: 'invalid',
          message: 'Admin token rejected. Generate a new secret or double-check the value.',
          context,
        });
      } else {
        const msg = context === 'pending'
          ? 'Token rejected. Update the value before saving.'
          : 'Admin token rejected. Generate a new secret or double-check the value.';
        setTokenStatus('invalid', { tokenValue: trimmed, context, message: msg });
      }
      markTokenProbeState(trimmed, 'invalid');
      return false;
    }
    const msg = `Unexpected response: ${resp.status}`;
    if (updateBadge) {
      updateTokenBadge(trimmed, { state: 'error', message: msg, context });
    } else {
      setTokenStatus('error', { tokenValue: trimmed, context, message: msg });
    }
    markTokenProbeState(trimmed, 'error');
    return false;
  } catch (err) {
    if (err && err.name === 'AbortError') {
      return null;
    }
    const msg = 'Service unreachable. Start the service, then try again.';
    if (updateBadge) {
      updateTokenBadge(trimmed, { state: 'offline', message: msg, context });
    } else {
      setTokenStatus('offline', { tokenValue: trimmed, context, message: msg });
    }
    markTokenProbeState(trimmed, 'offline');
    return null;
  } finally {
    if (tokenProbeController === controller) {
      tokenProbeController = null;
    }
  }
}

async function maybeProbeSavedToken({ force = false, reason = 'health' } = {}) {
  const savedToken = typeof prefBaseline.adminToken === 'string'
    ? prefBaseline.adminToken.trim()
    : '';
  if (!savedToken) return;
  if (prefsDirty) return;
  const last = tokenProbeLast || {};
  if (!force && last.token === savedToken) {
    if (last.state === 'valid') {
      updateTokenBadge(savedToken, { state: 'valid' });
      return;
    }
    if (last.state === 'invalid') {
      updateTokenBadge(savedToken, { state: 'invalid' });
      return;
    }
    if (last.state === 'offline' && Date.now() - (last.at || 0) < 5000) {
      updateTokenBadge(savedToken, { state: 'offline' });
      return;
    }
    if (last.state === 'error' && Date.now() - (last.at || 0) < 5000) {
      updateTokenBadge(savedToken, { state: 'error' });
      return;
    }
  }
  await probeAdminToken(savedToken, { context: 'saved', updateBadge: true, reason });
}

function initControlButtons() {
  if (controlButtonsInitialized) return;
  CONTROL_BUTTONS.forEach((cfg) => {
    const el = document.getElementById(cfg.id);
    if (!el) return;
    cfg.defaultTitle = el.getAttribute('title') || '';
  });
  controlButtonsInitialized = true;
}

function syncRestartButtonState() {
  const button = document.getElementById('btn-token-restart');
  if (!button) return;
  const input = tokenInputEl();
  const tokenValue = input ? String(input.value || '').trim() : '';
  const enabled = tokenValue.length > 0;
  button.disabled = !enabled;
  if (enabled) {
    button.removeAttribute('aria-disabled');
  } else {
    button.setAttribute('aria-disabled', 'true');
  }
  const preferred = detectPreferredRestartShell();
  const alt = alternateRestartShell(preferred);
  const preferredLabel = restartShellLabel(preferred);
  const altLabel = restartShellLabel(alt);
  const hint = enabled
    ? `Copy restart command (${preferredLabel}; Shift for ${altLabel})`
    : 'Set an admin token to copy a restart command';
  button.title = hint;
  button.setAttribute('data-preferred-shell', preferred);
  button.setAttribute('data-alt-shell', alt);
}

function updateWorkspaceAvailability() {
  initControlButtons();
  const hint = document.getElementById('workspaceHint');
  const tokenState = tokenStatusState.state;
  const tokenValue = typeof tokenStatusState.token === 'string' ? tokenStatusState.token : '';
  const tokenReadyStates = new Set(['valid', 'saved']);
  const credentialReady = tokenValue && tokenReadyStates.has(tokenState);
  const openAccessActive = tokenState === 'open' && allowAnonymousAdmin;
  const tokenReady = credentialReady || openAccessActive;
  const tokenNeedsSave = tokenState === 'pending';
  const tokenTesting = tokenState === 'testing';
  const tokenMissing = !credentialReady && !openAccessActive && (tokenState === 'missing' || !tokenValue);
  const tokenInvalid = tokenState === 'invalid';
  const tokenOffline = tokenState === 'offline';
  const tokenErrored = tokenState === 'error';

  CONTROL_BUTTONS.forEach((cfg) => {
    const el = document.getElementById(cfg.id);
    if (!el) return;
    const defaultTitle = typeof cfg.defaultTitle === 'string' ? cfg.defaultTitle : '';
    let disabled = false;
    let reason = '';

    if (cfg.requiresService && !serviceOnline) {
      disabled = true;
      reason = 'Start the service to enable this action.';
    } else if (cfg.requiresToken) {
      if (tokenNeedsSave) {
        disabled = true;
        reason = 'Save your admin token before opening this surface.';
      } else if (tokenTesting) {
        disabled = true;
        reason = 'Wait for token verification to finish.';
      } else if (!tokenReady) {
        disabled = true;
        if (tokenMissing) {
          reason = 'Paste and save an admin token first.';
        } else if (tokenInvalid) {
          reason = 'Admin token invalid. Update it before continuing.';
        } else if (tokenOffline) {
          reason = 'Bring the service online to verify your admin token.';
        } else if (tokenErrored) {
          reason = 'Resolve admin token errors before continuing.';
        } else {
          reason = 'Admin token not ready yet.';
        }
      }
    }

    if (disabled) {
      el.disabled = true;
      el.setAttribute('aria-disabled', 'true');
      el.title = reason || defaultTitle || 'Unavailable';
    } else {
      el.disabled = false;
      el.removeAttribute('aria-disabled');
      el.title = defaultTitle;
    }
  });

  syncRestartButtonState();

  let gateMessage = '';
  if (!serviceOnline) {
    gateMessage = 'Start the service to enable Projects, Conversations, Training, and diagnostics.';
  } else if (tokenNeedsSave) {
    gateMessage = 'Save your admin token before opening workspaces.';
  } else if (tokenTesting) {
    gateMessage = 'Testing admin token…';
  } else if (openAccessActive) {
    gateMessage = 'Debug mode active — admin surfaces are open. Set a token before sharing this service.';
  } else if (tokenMissing) {
    gateMessage = 'Paste an admin token to unlock Projects, Conversations, and Training.';
  } else if (tokenInvalid) {
    gateMessage = 'Fix the admin token (Test shows invalid) before opening workspaces.';
  } else if (tokenOffline) {
    gateMessage = 'Bring the service online to verify your admin token.';
  } else if (tokenErrored) {
    gateMessage = 'Resolve admin token errors before continuing.';
  }

  if (hint) {
    if (gateMessage) {
      hint.textContent = gateMessage;
      hint.hidden = false;
    } else {
      hint.textContent = '';
      hint.hidden = true;
    }
  }

  updateSetupChecklist({
    serviceOnline,
    tokenReady,
    tokenNeedsSave,
    tokenTesting,
    tokenMissing,
    tokenInvalid,
    tokenOffline,
    tokenErrored,
    openAccessActive,
    gateMessage,
  });
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
  const prefs = await fetchLauncherPrefs({ fresh: true });
  if (prefs && typeof prefs === 'object') {
    if (prefs.port) document.getElementById('port').value = prefs.port;
    if (typeof prefs.autostart === 'boolean') document.getElementById('autostart').checked = prefs.autostart;
    if (typeof prefs.notifyOnStatus === 'boolean') document.getElementById('notif').checked = prefs.notifyOnStatus;
    if (typeof prefs.adminToken === 'string') document.getElementById('admintok').value = String(prefs.adminToken).trim();
    if (typeof prefs.baseOverride === 'string') {
      const override = prefs.baseOverride.trim();
      if (override) {
        ARW.setBaseOverride(override, { persist: false });
      } else {
        ARW.clearBaseOverride({ persist: false });
      }
    }
  }
  setTokenVisibility(false);
  try {
    const enabled = await invoke('launcher_autostart_status');
    document.getElementById('loginstart').checked = !!enabled
  } catch {}
  let mascotPrefs = {};
  try {
    const stored = await ARW.getPrefs('mascot');
    if (stored && typeof stored === 'object') {
      mascotPrefs = stored;
    }
  } catch {}
  currentMascotPrefs = mascotPrefs;
  const mascotToggle = document.getElementById('mascotEnabled');
  const openMascotBtn = document.getElementById('btn-open-mascot');
  const supportMascotBtn = document.getElementById('btn-mascot');
  const mascotIntensity = document.getElementById('mascotIntensity');
  const mascotCharacter = document.getElementById('mascotCharacter');
  const mascotClickThrough = document.getElementById('mascotClickThrough');
  const mascotSnapWindows = document.getElementById('mascotSnapWindows');
  const mascotQuietMode = document.getElementById('mascotQuietMode');
  const mascotCompactMode = document.getElementById('mascotCompactMode');
  const resolvedMascotEnabled =
    mascotPrefs && typeof mascotPrefs.enabled === 'boolean'
      ? !!mascotPrefs.enabled
      : true;
  mascotEnabled = resolvedMascotEnabled;
  if (mascotToggle) {
    mascotToggle.checked = resolvedMascotEnabled;
    if (!IS_DESKTOP) {
      mascotToggle.disabled = true;
      mascotToggle.setAttribute('aria-disabled', 'true');
      mascotToggle.title = 'Mascot overlay requires the desktop launcher.';
    } else {
      mascotToggle.removeAttribute('aria-disabled');
      mascotToggle.removeAttribute('title');
    }
  }
  // Apply stored mascot prefs
  const intensity = (mascotPrefs && typeof mascotPrefs.intensity === 'string') ? mascotPrefs.intensity : 'normal';
  const clickThrough = mascotPrefs && typeof mascotPrefs.clickThrough === 'boolean' ? !!mascotPrefs.clickThrough : true;
  const snapWindows = mascotPrefs && typeof mascotPrefs.snapWindows === 'boolean' ? !!mascotPrefs.snapWindows : true;
  const quietMode = mascotPrefs && typeof mascotPrefs.quietMode === 'boolean' ? !!mascotPrefs.quietMode : false;
  const compactMode = mascotPrefs && typeof mascotPrefs.compactMode === 'boolean' ? !!mascotPrefs.compactMode : false;
  const character = mascotPrefs && typeof mascotPrefs.character === 'string' ? mascotPrefs.character : 'guide';
  if (mascotIntensity) mascotIntensity.value = intensity;
  if (mascotClickThrough) mascotClickThrough.checked = clickThrough;
  if (mascotCharacter) mascotCharacter.value = character;
  if (mascotSnapWindows) mascotSnapWindows.checked = snapWindows;
  if (mascotQuietMode) mascotQuietMode.checked = quietMode;
  if (mascotCompactMode) mascotCompactMode.checked = compactMode;
  await renderMascotProfiles(mascotPrefs);

  const sendMascotConfig = async (profile = 'global', overrides = {}) => {
    try {
      if (window.__TAURI__?.event?.emit) {
        const effectiveProfile = typeof profile === 'string' && profile.trim() ? profile.trim() : 'global';
        await window.__TAURI__.event.emit('mascot:config', {
          profile: effectiveProfile,
          allowInteractions: !(mascotClickThrough && mascotClickThrough.checked),
          intensity: mascotIntensity ? mascotIntensity.value : 'normal',
          snapWindows: mascotSnapWindows ? !!mascotSnapWindows.checked : true,
          quietMode: mascotQuietMode ? !!mascotQuietMode.checked : false,
          compactMode: mascotCompactMode ? !!mascotCompactMode.checked : false,
          character: mascotCharacter ? mascotCharacter.value : 'guide',
          ...overrides,
        });
      }
    } catch (err) { console.error(err); }
  };

  const wireOpenButton = (button) => {
    if (!button) return;
    if (!IS_DESKTOP) {
      button.disabled = true;
      button.setAttribute('aria-disabled', 'true');
      button.title = 'Mascot overlay requires the desktop launcher.';
      return;
    }
    button.disabled = false;
    button.removeAttribute('aria-disabled');
    button.title = 'Show mascot overlay';
    button.addEventListener('click', async () => {
      await ensureMascotWindow({ focus: true, force: true });
      mascotChecklistState = composeMascotMood('ready', 'Mascot on duty.');
      applyMascotMood();
      await sendMascotConfig('global');
    });
  };
  wireOpenButton(openMascotBtn);
  wireOpenButton(supportMascotBtn);
  if (IS_DESKTOP) {
    if (resolvedMascotEnabled) {
      await ensureMascotWindow({ force: true });
      mascotChecklistState = composeMascotMood('thinking', 'Launcher starting…');
      applyMascotMood();
      await sendMascotConfig('global');
    } else {
      await closeMascotWindow();
      mascotChecklistState = null;
      mascotSseState = null;
      applyMascotMood();
    }
    const projectProfiles = mascotPrefs && typeof mascotPrefs.profiles === 'object'
      ? Object.entries(mascotPrefs.profiles)
      : [];
    for (const [profileKey, entry] of projectProfiles) {
      if (!entry || !entry.autoOpen) continue;
      const slug = entry.slug || slugify(profileKey.replace(/^project:/, ''));
      try {
        await invoke('open_mascot_window', {
          label: `mascot-${slug}`,
          profile: profileKey,
          character: entry.character || mascotPrefs.character || 'guide',
          quiet: entry.quietMode ?? mascotPrefs.quietMode ?? false,
          compact: entry.compactMode ?? mascotPrefs.compactMode ?? false,
        });
        await sendMascotConfig(profileKey, entry);
      } catch (err) {
        console.error(err);
      }
    }
    if (window.__TAURI__?.event) {
      try {
        if (!mascotConfigUnlisten) {
          mascotConfigUnlisten = await window.__TAURI__.event.listen('mascot:config', async () => {
            const prefs = await loadMascotPrefs();
            currentMascotPrefs = prefs;
            await renderMascotProfiles(prefs);
          });
        }
        if (!mascotStatusUnlisten) {
          mascotStatusUnlisten = await window.__TAURI__.event.listen('mascot:profile-status', (evt) => {
            const payload = evt?.payload || {};
            const key = payload.profile || 'global';
            mascotRuntimeStatus.set(key, payload);
            renderMascotProfiles();
          });
        }
      } catch (err) {
        console.error(err);
      }
    }
  }

  const rawConnections = Array.isArray(prefs.connections) ? prefs.connections : [];
  connectionsList = rawConnections.map(normalizeConnectionEntry);
  renderConnectionSelect();
  snapshotPrefsBaseline();
  updateTokenBadge(prefBaseline.adminToken);
  syncAdvancedPrefsDisclosure();
  baseMeta = updateBaseMeta();
  connectSse({ replay: 5, resume: false });
  initMascotSseSubscription();
  miniDownloads();
  await health();
  await refreshServiceLogPath();
  if (!IS_DESKTOP) {
    enterBrowserMode();
  }
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
  const mascotToggle = document.getElementById('mascotEnabled');
  const desiredMascot = !!(mascotToggle && mascotToggle.checked);
  try {
    const existingMascotPrefs = await ARW.getPrefs('mascot') || {};
    const profiles = (existingMascotPrefs.profiles && typeof existingMascotPrefs.profiles === 'object')
      ? existingMascotPrefs.profiles
      : {};
    const prefs = {
      ...existingMascotPrefs,
      enabled: desiredMascot,
      intensity: (document.getElementById('mascotIntensity') || {}).value || 'normal',
      clickThrough: !!(document.getElementById('mascotClickThrough') || { checked: true }).checked,
      snapWindows: !!(document.getElementById('mascotSnapWindows') || { checked: true }).checked,
      quietMode: !!(document.getElementById('mascotQuietMode') || { checked: false }).checked,
      compactMode: !!(document.getElementById('mascotCompactMode') || { checked: false }).checked,
      character: (document.getElementById('mascotCharacter') || {}).value || 'guide',
      profiles,
    };
    await ARW.setPrefs('mascot', prefs);
    mascotEnabled = desiredMascot;
    if (desiredMascot) {
      await ensureMascotWindow({ force: true });
      mascotChecklistState = composeMascotMood('thinking', 'Mascot enabled — gathering status…');
      initMascotSseSubscription();
      applyMascotMood();
      // Send config to mascot
      try{
        if (window.__TAURI__?.event?.emit) {
          await window.__TAURI__.event.emit('mascot:config', {
            profile: 'global',
            allowInteractions: !prefs.clickThrough,
            intensity: prefs.intensity,
            snapWindows: prefs.snapWindows,
            quietMode: prefs.quietMode,
            compactMode: prefs.compactMode,
            character: prefs.character,
          });
        }
      }catch{}
    } else {
      await closeMascotWindow();
      mascotChecklistState = null;
      mascotSseState = null;
      applyMascotMood();
    }
  } catch (err) {
    console.error(err);
    ARW.toast('Unable to update mascot preference');
  }
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
    const metaInfo = baseMeta || updateBaseMeta();
    const healthArgs = { port: effectivePort() };
    if (metaInfo && typeof metaInfo.base === 'string' && metaInfo.base.trim()) {
      healthArgs.base = metaInfo.base;
    }
    const ok = await invoke('check_service_health', healthArgs);
    serviceOnline = !!ok;
    const savedToken = typeof prefBaseline.adminToken === 'string'
      ? prefBaseline.adminToken.trim()
      : '';
    const tokenInput = tokenInputEl();
    const inputValue = tokenInput ? String(tokenInput.value || '').trim() : '';
    const pendingToken =
      prefsDirty &&
      inputValue &&
      inputValue !== savedToken;
    const hasToken =
      typeof prefBaseline.adminToken === 'string' &&
      prefBaseline.adminToken.trim().length > 0;
    if (dot) dot.className = 'dot ' + (ok ? 'ok' : 'bad');
    if (txt) txt.innerText = ok ? 'online' : 'offline';
    if (IS_DESKTOP) {
      if (startBtn) startBtn.disabled = ok;
      if (stopBtn) stopBtn.disabled = !ok;
    } else {
      if (startBtn) startBtn.disabled = true;
      if (stopBtn) stopBtn.disabled = true;
    }
    if (statusLabel) {
      statusLabel.textContent = ok ? 'Service online' : 'Service offline';
      statusLabel.className = ok ? 'ok' : 'bad';
    }
    lastHealthCheck = Date.now();
    if (metaLabel) updateHealthMetaLabel();
    if (ok) {
      await refreshServiceLogPath();
      const shouldForceProbe =
        tokenStatusState.state === 'offline' ||
        (savedToken &&
          tokenProbeLast.token === savedToken &&
          (tokenProbeLast.state === 'offline' || tokenProbeLast.state === 'error'));
      await maybeProbeSavedToken({ force: shouldForceProbe, reason: 'health' });
      if (!hasToken && !pendingToken) {
        const normalizedBase = normalizedBaseForAnonymous(metaInfo);
        const forceAnon =
          anonymousAdminAccess.base !== normalizedBase ||
          anonymousAdminAccess.state === 'offline' ||
          anonymousAdminAccess.state === 'error';
        const openAccess = await probeAnonymousAdminAccess({
          force: forceAnon,
          meta: metaInfo,
        });
        allowAnonymousAdmin = openAccess;
        if (openAccess) {
          updateTokenBadge('', {
            state: 'open',
            message: 'Admin surfaces unlocked (debug mode). Set a token before sharing.',
            context: 'saved',
          });
        } else if (tokenStatusState.state === 'open') {
          updateTokenBadge('', { state: 'missing' });
        }
      } else {
        allowAnonymousAdmin = false;
      }
    } else {
      allowAnonymousAdmin = false;
      resetAnonymousAdminAccess();
    }
    if (heroHint) {
      if (IS_DESKTOP) {
        heroHint.textContent = ok
          ? hasToken
            ? 'Stack online. Launch a workspace when you are ready.'
            : allowAnonymousAdmin
              ? 'Stack online (debug mode). Admin surfaces are open; set a token before sharing.'
              : 'Stack online. Paste or generate an admin token to unlock Hub, Chat, and Training.'
          : 'Start the service, then paste or generate an admin token to unlock workspaces.';
      } else {
        heroHint.textContent = ok
          ? 'Stack online. Desktop launcher controls are disabled in browser mode.'
          : 'Start the service with CLI scripts or the desktop launcher.';
      }
    }
    updateWorkspaceAvailability();
  } catch {
    serviceOnline = false;
    allowAnonymousAdmin = false;
    resetAnonymousAdminAccess();
    if (dot) dot.className = 'dot';
    if (txt) txt.innerText = 'unknown';
    if (startBtn) startBtn.disabled = false;
    if (stopBtn) stopBtn.disabled = true;
    if (statusLabel) {
      statusLabel.textContent = 'Status unavailable';
      statusLabel.className = 'bad';
    }
    if (heroHint) {
      heroHint.textContent = 'Health check failed. Confirm the port, restart the service, then paste your admin token.';
    }
    lastHealthCheck = Date.now();
    if (metaLabel) updateHealthMetaLabel();
    await refreshServiceLogPath();
    const savedToken = typeof prefBaseline.adminToken === 'string'
      ? prefBaseline.adminToken.trim()
      : '';
    const offlineMessage = 'Service unreachable. Start the service, then test again.';
    if (prefsDirty) {
      setTokenStatus('offline', {
        tokenValue: savedToken,
        context: 'pending',
        message: offlineMessage,
      });
    } else if (savedToken) {
      updateTokenBadge(savedToken, { state: 'offline', message: offlineMessage });
      markTokenProbeState(savedToken, 'offline');
    } else {
      updateTokenBadge('', { state: 'missing' });
      markTokenProbeState(null, null);
    }
    updateWorkspaceAvailability();
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
  if (!IS_DESKTOP) {
    enterBrowserMode();
  }
  initStatusBadges();
  bindPrefWatchers();
  setTokenVisibility(false);
  initControlButtons();
  const advanced = document.querySelector('.hero-preferences');
  if (ARW.mode && typeof ARW.mode.subscribe === 'function') {
    ARW.mode.subscribe((mode) => {
      if (!advanced) return;
      if (mode === 'expert') {
        advanced.open = true;
        advanced.dataset.forceOpen = 'expert';
      } else {
        if (advanced.dataset.forceOpen === 'expert') {
          delete advanced.dataset.forceOpen;
        }
        syncAdvancedPrefsDisclosure();
      }
    });
  }
  syncAdvancedPrefsDisclosure();
  if (advanced) {
    advanced.addEventListener('toggle', () => {
      if (!advanced.open) {
        delete advanced.dataset.forceOpen;
        syncAdvancedPrefsDisclosure();
      }
    });
  }
  renderConnectionSelect();
  updateWorkspaceAvailability();
  const tokenToggle = document.getElementById('btn-token-toggle');
  if (tokenToggle) {
    tokenToggle.addEventListener('click', () => {
      toggleTokenVisibility();
    });
  }
  const tokenGenerate = document.getElementById('btn-token-generate');
  if (tokenGenerate) {
    tokenGenerate.addEventListener('click', async () => {
      const token = ARW.tokens.generateHex(32);
      if (!token) return;
      setTokenVisibility(true);
      const value = setTokenValue(token, { focusEnd: true });
      try {
        await navigator.clipboard.writeText(value);
        ARW.toast('Token generated and copied');
      } catch {
        ARW.toast('Token generated');
      }
    });
  }
  const tokenCopy = document.getElementById('btn-token-copy');
  if (tokenCopy) {
    tokenCopy.addEventListener('click', async () => {
      const input = tokenInputEl();
      const value = input ? String(input.value || '').trim() : '';
      if (!value) {
        ARW.toast('No token to copy');
        return;
      }
      try {
        await navigator.clipboard.writeText(value);
        ARW.toast('Token copied');
      } catch {
        ARW.toast('Copy failed');
      }
    });
  }
  const tokenRestart = document.getElementById('btn-token-restart');
  if (tokenRestart) {
    tokenRestart.addEventListener('click', async (event) => {
      const input = tokenInputEl();
      const value = input ? String(input.value || '').trim() : '';
      await copyRestartCommandToClipboard(value, event);
    });
  }
  const tokenCalloutBtn = document.getElementById('btn-token-callout');
  if (tokenCalloutBtn) {
    tokenCalloutBtn.addEventListener('click', () => {
      tokenCalloutPrimed = true;
      ensureAdvancedOpen({ focusToken: true, scrollIntoView: true });
    });
  }
  const tokenTest = document.getElementById('btn-token-test');
  if (tokenTest) {
    tokenTest.addEventListener('click', async () => {
      if (tokenTest.disabled) return;
      const input = tokenInputEl();
      const value = input ? String(input.value || '').trim() : '';
      const dirtyToken = prefsDirty || value !== prefBaseline.adminToken;
      const context = dirtyToken ? 'pending' : 'saved';
      if (!value) {
        setTokenStatus('missing', { tokenValue: value, context });
        ARW.toast('Paste or generate a token first');
        return;
      }
      tokenTest.disabled = true;
      try {
        await probeAdminToken(value, {
          context,
          updateBadge: context === 'saved',
          reason: 'manual-test',
        });
      } finally {
        tokenTest.disabled = false;
      }
    });
  }
  const tokenGuide = document.getElementById('btn-token-guide');
  if (tokenGuide) {
    tokenGuide.addEventListener('click', async () => {
      try {
        await invoke('open_url', { url: 'https://t3hw00t.github.io/ARW/guide/quickstart/#minimum-secure-setup' });
      } catch (err) {
        console.error(err);
        ARW.toast('Unable to open docs');
      }
    });
  }
  const manageConnectionsBtn = document.getElementById('btn-open-connections');
  if (manageConnectionsBtn) {
    manageConnectionsBtn.addEventListener('click', async () => {
      try {
        await invoke('open_connections_window');
      } catch (err) {
        console.error(err);
        ARW.toast('Unable to open connections');
      }
    });
  }
  const mascotProfileListEl = document.getElementById('mascotProfileList');
  if (mascotProfileListEl) {
    mascotProfileListEl.addEventListener('click', handleMascotProfileListClick);
  }
  const mascotProfileNewBtn = document.getElementById('mascotProfileNew');
  if (mascotProfileNewBtn) {
    mascotProfileNewBtn.addEventListener('click', createMascotProfileManual);
  }
  const mascotProfileProjectBtn = document.getElementById('mascotProfileProject');
  if (mascotProfileProjectBtn) {
    mascotProfileProjectBtn.addEventListener('click', createMascotProfileFromProject);
  }
  const connectionSelect = document.getElementById(CONNECTION_SELECT_ID);
  if (connectionSelect) {
    connectionSelect.addEventListener('change', () => {
      const currentOverride = (ARW.baseOverride && ARW.baseOverride()) || '';
      const next = connectionSelect.value;
      if (next === currentOverride) return;
      if (!next) {
        ARW.clearBaseOverride();
        ARW.toast('Switched to local stack');
        return;
      }
      const normalized = ARW.setBaseOverride(next);
      if (normalized) {
        ARW.toast(`Active base: ${normalized}`);
      } else {
        connectionSelect.value = currentOverride || '';
        ARW.toast('Unable to activate connection');
      }
    });
  }
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
        if (IS_DESKTOP) {
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
        } else {
          prefBaseline.loginstart = false;
        }
        const tokenChanged = previousTokenBaseline !== prefBaseline.adminToken;
        if (tokenChanged) {
          const restart = await ARW.modal.confirm({
            title: 'Restart required',
            body: 'Admin token updated. Restart the local service now to apply the new credentials?',
            submitLabel: 'Restart now',
            cancelLabel: 'Later',
          });
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
        if (prefBaseline.adminToken) {
          await probeAdminToken(prefBaseline.adminToken, {
            context: 'saved',
            updateBadge: true,
            reason: tokenChanged ? 'save-token-changed' : 'save',
          });
        } else {
          markTokenProbeState(null, null);
          updateTokenBadge('', { state: 'missing' });
        }
        ARW.toast('Preferences saved');
      } catch (e) {
        console.error(e);
        ARW.toast('Save failed');
        refreshPrefsDirty();
      }
    });
  }
  const settingsBtn = document.getElementById('btn-settings');
  if (settingsBtn) {
    settingsBtn.addEventListener('click', async () => {
      try {
        await invoke('open_settings_window');
      } catch (e) {
        console.error(e);
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
      updateWorkspaceAvailability();
    });
  }

  let settingsEventUnlisten = null;
  async function handleLauncherSettingsUpdated(event) {
    try {
      const payload = event?.payload || {};
      const settings = payload.settings || {};
      if (!settings || typeof settings !== 'object') return;
      if (prefsDirty) {
        ARW.toast('Launcher settings updated in Settings. Save or discard local edits to sync.');
        return;
      }
      if (typeof settings.default_port === 'number') {
        const portEl = document.getElementById('port');
        if (portEl) portEl.value = settings.default_port;
      }
      const applyCheckbox = (id, value) => {
        const el = document.getElementById(id);
        if (el && typeof value === 'boolean') el.checked = value;
      };
      applyCheckbox('autostart', settings.autostart_service);
      applyCheckbox('notif', settings.notify_on_status);
      applyCheckbox('loginstart', settings.launch_at_login);
      if (typeof settings.base_override === 'string') {
        const normalized = settings.base_override.trim();
        if (normalized) {
          ARW.setBaseOverride(normalized, { persist: false });
        } else {
          ARW.clearBaseOverride({ persist: false });
        }
      } else if (settings.base_override == null) {
        ARW.clearBaseOverride({ persist: false });
      }
      snapshotPrefsBaseline();
      refreshPrefsDirty();
      baseMeta = updateBaseMeta();
      renderConnectionSelect();
      syncAdvancedPrefsDisclosure();
    } catch (err) {
      console.error('settings update application failed', err);
    }
  }

  if (IS_DESKTOP && window.__TAURI__?.event) {
    window.__TAURI__.event
      .listen('launcher://settings-updated', handleLauncherSettingsUpdated)
      .then((unlisten) => {
        settingsEventUnlisten = unlisten;
      })
      .catch((err) => {
        console.error('launcher settings listener failed', err);
      });
  }

  window.addEventListener('arw:base-override-changed', () => {
    baseMeta = updateBaseMeta();
    resetAnonymousAdminAccess();
    allowAnonymousAdmin = false;
    connectSse({ replay: 5, resume: false });
    miniDownloads();
    health();
    refreshPrefsDirty();
    renderConnectionSelect();
    syncAdvancedPrefsDisclosure();
    updateWorkspaceAvailability();
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

  window.addEventListener('beforeunload', () => {
    if (typeof settingsEventUnlisten === 'function') {
      try {
        settingsEventUnlisten();
      } catch {}
      settingsEventUnlisten = null;
    }
  });
});

window.addEventListener('focus', () => {
  if (document.hidden) return;
  refreshConnections({ fresh: true });
});

window.addEventListener('beforeunload', () => {
  if (healthMetaTimer) {
    clearInterval(healthMetaTimer);
    healthMetaTimer = null;
  }
});
