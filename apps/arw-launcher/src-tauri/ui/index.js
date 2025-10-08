const invoke = (cmd, args) => ARW.invoke(cmd, args);
const IS_DESKTOP = !!(ARW.env && ARW.env.isTauri);
const getPort = () => ARW.getPortFromInput('port');
const updateBaseMeta = () => ARW.applyBaseMeta({ portInputId: 'port', badgeId: 'baseBadge', label: 'Base' });
let baseMeta = null;
const effectivePort = () => getPort() || (baseMeta && baseMeta.port) || 8091;
const CONNECTION_SELECT_ID = 'connectionSelect';

let miniDownloadsSub = null;
let prefsDirty = false;
const prefBaseline = { port: '', autostart: false, notif: true, loginstart: false, adminToken: '', baseOverride: '' };
let lastHealthCheck = null;
let healthMetaTimer = null;
let serviceLogPath = null;
let tokenProbeController = null;
let tokenProbeLast = { token: null, state: null, at: 0 };
let serviceOnline = false;
let controlButtonsInitialized = false;
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
  const baseOverrideActive = !!(typeof ARW !== 'undefined' && ARW.baseOverride && ARW.baseOverride());
  return portChanged || autostartOn || notificationsOff || loginOn || tokenSet || baseOverrideActive;
}

function syncAdvancedPrefsDisclosure() {
  const advanced = document.querySelector('.hero-preferences');
  if (!advanced) return;
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
  advanced.dataset.forceOpen = 'true';
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
    missing: 'No admin token saved. Generate or paste one to unlock Project Hub, Chat, and Training.',
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
    gateMessage = 'Start the service to enable Project Hub, Chat, Training, and diagnostics.';
  } else if (tokenNeedsSave) {
    gateMessage = 'Save your admin token before opening workspaces.';
  } else if (tokenTesting) {
    gateMessage = 'Testing admin token…';
  } else if (openAccessActive) {
    gateMessage = 'Debug mode active — admin surfaces are open. Set a token before sharing this service.';
  } else if (tokenMissing) {
    gateMessage = 'Paste and save an admin token to unlock Project Hub, Chat, and Training.';
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
  const rawConnections = Array.isArray(prefs.connections) ? prefs.connections : [];
  connectionsList = rawConnections.map(normalizeConnectionEntry);
  renderConnectionSelect();
  snapshotPrefsBaseline();
  updateTokenBadge(prefBaseline.adminToken);
  syncAdvancedPrefsDisclosure();
  baseMeta = updateBaseMeta();
  connectSse({ replay: 5, resume: false });
  miniDownloads();
  health();
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
