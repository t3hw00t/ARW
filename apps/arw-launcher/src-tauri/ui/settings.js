const invoke = (cmd, args) => ARW.invoke(cmd, args);

const state = {
  bundle: null,
  dirty: false,
  saving: false,
  installing: false,
};
let activeMode = (window.ARW?.mode?.current === 'expert') ? 'expert' : 'guided';

const defaults = () => ({
  default_port: 8091,
  autostart_service: false,
  notify_on_status: true,
  launch_at_login: false,
  base_override: '',
});

function setStatus(message, variant = 'info') {
  const el = document.getElementById('settingsStatus');
  if (!el) return;
  el.textContent = message;
  el.dataset.state = variant;
}

function toggleSaveDisabled(disabled) {
  const btn = document.getElementById('btn-settings-save');
  if (btn) btn.disabled = disabled;
}

function markDirty(flag) {
  state.dirty = !!flag;
  toggleSaveDisabled(!state.dirty || state.saving);
  if (state.dirty) {
    setStatus('Unsaved changes', 'dirty');
  } else {
    setStatus('All changes saved', 'clean');
  }
}

function updateWebView2Status(status) {
  const badge = document.getElementById('webview2StatusBadge');
  const detail = document.getElementById('webview2StatusDetail');
  const installBtn = document.getElementById('btn-webview2-install');
  const refreshBtn = document.getElementById('btn-webview2-refresh');
  const supported = !!status?.supported;
  const installed = !!status?.installed;
  if (badge) {
    let label = installed ? 'Installed' : 'Not installed';
    if (!supported) {
      label = 'Not required';
    }
    badge.textContent = label;
    badge.className = `badge ${installed ? 'ok' : supported ? 'warn' : 'neutral'}`;
    badge.dataset.state = installed ? 'ok' : supported ? 'warn' : 'neutral';
  }
  if (detail) {
    const info = status?.detail ? String(status.detail) : '';
    if (supported) {
      detail.textContent = installed
        ? info || 'Evergreen runtime detected.'
        : info || 'Install the Evergreen runtime to enable the desktop launcher.';
    } else {
      detail.textContent = info || 'WebView2 runtime is only required on Windows.';
    }
  }
  if (installBtn) {
    installBtn.disabled = !supported || installed || state.installing;
    installBtn.title = supported
      ? installed
        ? 'Runtime already installed'
        : 'Install or repair the Evergreen runtime'
      : 'WebView2 is not required on this platform';
  }
  if (refreshBtn) {
    refreshBtn.disabled = !supported && !state.installing;
  }
}

function updateLogsPath(path) {
  const el = document.getElementById('logsPath');
  state.logsDir = path || '';
  if (!el) return;
  if (state.logsDir) {
    el.textContent = state.logsDir;
  } else {
    el.textContent = 'Log directory not available yet.';
  }
}

function applySettingsToForm(settings) {
  const port = document.getElementById('setting-port');
  if (port) port.value = settings.default_port ?? 8091;
  const auto = document.getElementById('setting-autostart-service');
  if (auto) auto.checked = !!settings.autostart_service;
  const notify = document.getElementById('setting-notify');
  if (notify) notify.checked = !!settings.notify_on_status;
  const login = document.getElementById('setting-launch-at-login');
  if (login) login.checked = !!settings.launch_at_login;
  const base = document.getElementById('setting-base');
  if (base) base.value = settings.base_override || '';
}

function readSettingsFromForm() {
  const settings = defaults();
  const port = document.getElementById('setting-port');
  const parsed = port ? parseInt(port.value, 10) : NaN;
  if (Number.isInteger(parsed) && parsed >= 1 && parsed <= 65535) {
    settings.default_port = parsed;
  }
  const auto = document.getElementById('setting-autostart-service');
  settings.autostart_service = !!(auto && auto.checked);
  const notify = document.getElementById('setting-notify');
  settings.notify_on_status = !!(notify && notify.checked);
  const login = document.getElementById('setting-launch-at-login');
  settings.launch_at_login = !!(login && login.checked);
  const base = document.getElementById('setting-base');
  if (base) {
    const normalized = ARW.normalizeBase ? ARW.normalizeBase(base.value || '') : (base.value || '').trim();
    settings.base_override = normalized || '';
  }
  return settings;
}

function bindInputs() {
  ['setting-autostart-service', 'setting-notify', 'setting-launch-at-login'].forEach((id) => {
    const el = document.getElementById(id);
    if (el) {
      el.addEventListener('change', () => markDirty(true));
    }
  });
  ['setting-port', 'setting-base'].forEach((id) => {
    const el = document.getElementById(id);
    if (el) {
      el.addEventListener('input', () => markDirty(true));
    }
  });
}

function settingsEqual(a, b) {
  return (
    a.default_port === b.default_port &&
    !!a.autostart_service === !!b.autostart_service &&
    !!a.notify_on_status === !!b.notify_on_status &&
    !!a.launch_at_login === !!b.launch_at_login &&
    (a.base_override || '') === (b.base_override || '')
  );
}

async function loadBundle({ refresh = false } = {}) {
  if (!refresh && state.bundle) {
    applySettingsToForm(state.bundle.settings);
    updateWebView2Status(state.bundle.webview2);
    updateLogsPath(state.bundle.logs_dir);
    markDirty(false);
    return;
  }
  try {
    setStatus('Loading…', 'info');
    const bundle = await invoke('get_launcher_settings');
    state.bundle = bundle;
    applySettingsToForm(bundle.settings);
    updateWebView2Status(bundle.webview2);
    updateLogsPath(bundle.logs_dir);
    markDirty(false);
  } catch (err) {
    console.error(err);
    setStatus('Failed to load settings', 'error');
    ARW.toast('Failed to load settings');
  }
}

async function saveSettings() {
  if (state.saving) return;
  const current = readSettingsFromForm();
  if (state.bundle && settingsEqual(current, state.bundle.settings)) {
    markDirty(false);
    return;
  }
  state.saving = true;
  toggleSaveDisabled(true);
  setStatus('Saving…', 'info');
  try {
    const bundle = await invoke('save_launcher_settings', { settings: current });
    state.bundle = bundle;
    applySettingsToForm(bundle.settings);
    updateWebView2Status(bundle.webview2);
    updateLogsPath(bundle.logs_dir);
    markDirty(false);
    ARW.toast('Settings saved');
  } catch (err) {
    console.error(err);
    setStatus('Save failed', 'error');
    ARW.toast('Save failed');
    markDirty(true);
  } finally {
    state.saving = false;
    toggleSaveDisabled(!state.dirty);
  }
}

function restoreDefaults() {
  const defaultsValue = defaults();
  applySettingsToForm(defaultsValue);
  markDirty(true);
}

async function installWebView2() {
  if (state.installing) return;
  state.installing = true;
  const btn = document.getElementById('btn-webview2-install');
  if (btn) btn.disabled = true;
  setStatus('Installing WebView2…', 'info');
  try {
    const status = await invoke('install_webview2_runtime');
    updateWebView2Status(status);
    ARW.toast(status?.installed ? 'WebView2 runtime installed' : 'Install completed');
  } catch (err) {
    console.error(err);
    ARW.toast('WebView2 install failed');
    setStatus('WebView2 install failed', 'error');
  } finally {
    state.installing = false;
    const refresh = document.getElementById('btn-webview2-refresh');
    if (refresh) refresh.disabled = false;
    loadBundle({ refresh: true }).catch(() => {});
  }
}

async function refreshWebViewStatus() {
  try {
    const bundle = await invoke('get_launcher_settings');
    if (bundle?.webview2) {
      updateWebView2Status(bundle.webview2);
    }
  } catch (err) {
    console.error(err);
    ARW.toast('Failed to refresh WebView2 status');
  }
}

async function openLogsDir() {
  if (!state.logsDir) {
    ARW.toast('Log directory unavailable');
    return;
  }
  try {
    await invoke('open_path', { path: state.logsDir });
  } catch (err) {
    console.error(err);
    ARW.toast('Open failed');
  }
}

async function openServiceLog() {
  try {
    const path = await invoke('launcher_service_log_path');
    if (path) {
      await invoke('open_path', { path });
    } else {
      ARW.toast('No service log found yet');
    }
  } catch (err) {
    console.error(err);
    ARW.toast('Unable to open service log');
  }
}

function applyMode(mode, { force = false } = {}) {
  const normalized = mode === 'expert' ? 'expert' : 'guided';
  if (!force && normalized === activeMode) return;
  activeMode = normalized;
  const hideExpert = normalized !== 'expert';
  document.querySelectorAll('[data-mode="expert-only"]').forEach((el) => {
    if (!(el instanceof HTMLElement)) return;
    if (hideExpert) el.setAttribute('aria-hidden', 'true');
    else el.removeAttribute('aria-hidden');
  });
  document.querySelectorAll('[data-mode="guided-only"]').forEach((el) => {
    if (!(el instanceof HTMLElement)) return;
    if (hideExpert) el.removeAttribute('aria-hidden');
    else el.setAttribute('aria-hidden', 'true');
  });
}

document.addEventListener('DOMContentLoaded', () => {
  applyMode(window.ARW?.mode?.current || activeMode, { force: true });
  if (ARW.mode && typeof ARW.mode.subscribe === 'function') {
    ARW.mode.subscribe((modeValue) => {
      applyMode(modeValue);
    });
  }
  bindInputs();
  document.getElementById('btn-settings-save')?.addEventListener('click', () => {
    saveSettings();
  });
  document.getElementById('btn-settings-reset')?.addEventListener('click', () => {
    restoreDefaults();
  });
  document.getElementById('btn-webview2-install')?.addEventListener('click', () => {
    installWebView2();
  });
  document.getElementById('btn-webview2-refresh')?.addEventListener('click', () => {
    refreshWebViewStatus();
  });
  document.getElementById('btn-open-logs')?.addEventListener('click', () => {
    openLogsDir();
  });
  document.getElementById('btn-open-service-log')?.addEventListener('click', () => {
    openServiceLog();
  });

  if (window.__TAURI__?.event) {
    window.__TAURI__.event
      .listen('launcher://settings-updated', (event) => {
        const payload = event?.payload || {};
        if (payload.settings && !state.saving) {
          state.bundle = {
            settings: payload.settings,
            webview2: payload.webview2 || state.bundle?.webview2 || {},
            logs_dir: payload.logsDir || state.bundle?.logs_dir || null,
          };
          applySettingsToForm(state.bundle.settings);
          updateWebView2Status(state.bundle.webview2);
          updateLogsPath(state.bundle.logs_dir);
          markDirty(false);
        }
      })
      .catch(() => {});
  }

  loadBundle({ refresh: true });
});
