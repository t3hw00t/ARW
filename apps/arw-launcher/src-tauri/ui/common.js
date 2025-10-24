// Lightweight helpers shared by launcher pages
// Capture optional base override from query string (`?base=http://host:port`)
(() => {
  try {
    const current = new URL(window.location.href);
    const raw = current.searchParams.get('base');
    if (!raw) return;
    const cleaned = (() => {
      const str = String(raw).trim();
      if (!str) return '';
      const strip = (val) => val.replace(/\/+$/, '');
      try {
        return strip(new URL(str).origin || str);
      } catch {
        if (!/^[a-zA-Z][a-zA-Z0-9+\-.]*:\/\//.test(str)) {
          try { return strip(new URL(`http://${str}`).origin || str); }
          catch { return strip(str); }
        }
        return strip(str);
      }
    })();
    if (cleaned) {
      window.__ARW_BASE_OVERRIDE = cleaned;
    }
  } catch {}
})();

const HAS_TAURI =
  typeof window !== 'undefined' &&
  typeof window.__TAURI__ === 'object' &&
  typeof window.__TAURI__.invoke === 'function';
const fallbackHandlers = Object.create(null);

function escapeHtml(value) {
  return String(value ?? '')
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
    .replace(/"/g, '&quot;')
    .replace(/'/g, '&#39;');
}

const RUNTIME_STATE_DEFS = [
  { slug: 'ready', label: 'Ready', synonyms: ['ready', 'ok'] },
  { slug: 'starting', label: 'Starting', synonyms: ['starting', 'start'] },
  { slug: 'degraded', label: 'Degraded', synonyms: ['degraded'] },
  { slug: 'offline', label: 'Offline', synonyms: ['offline', 'disabled'] },
  { slug: 'error', label: 'Error', synonyms: ['error', 'failed'] },
  { slug: 'unknown', label: 'Unknown', synonyms: ['unknown'] },
];

const RUNTIME_SEVERITY_DEFS = [
  { slug: 'info', label: 'Info', synonyms: ['info'] },
  { slug: 'warn', label: 'Warn', synonyms: ['warn', 'warning'] },
  { slug: 'error', label: 'Error', synonyms: ['error'] },
];

function normalizeEnum(defs, value, fallbackSlug) {
  const raw = String(value ?? '').trim().toLowerCase();
  if (!raw) {
    return defs.find((def) => def.slug === fallbackSlug) || defs[0];
  }
  const match = defs.find((def) => def.slug === raw || def.synonyms.includes(raw));
  if (match) return match;
  return defs.find((def) => def.slug === fallbackSlug) || defs[0];
}

window.ARW = {
  env: {
    isTauri: HAS_TAURI,
    isBrowser: !HAS_TAURI,
  },
  _prefsCache: new Map(),
  _prefsTimers: new Map(),
  _ocrCache: new Map(),
  unsupported(feature) {
    const label =
      typeof feature === 'string' && feature.trim().length
        ? feature.trim()
        : 'This action';
    try {
      this.toast(`${label} requires the desktop launcher.`);
    } catch {}
    throw new Error('unsupported_command');
  },
  runtime: {
    state(value) {
      const def = normalizeEnum(RUNTIME_STATE_DEFS, value, 'unknown');
      return { slug: def.slug, label: def.label };
    },
    severity(value) {
      const def = normalizeEnum(RUNTIME_SEVERITY_DEFS, value, 'info');
      return { slug: def.slug, label: def.label };
    },
  },
  ui: {
    updateRatioBar(target, value, options = {}) {
      const node = typeof target === 'string' ? document.getElementById(target) : target;
      if (!node) return;
      const fill = node.querySelector('i');
      const {
        preferLow = false,
        warn = preferLow ? 0.25 : 0.65,
        bad = preferLow ? 0.45 : 0.4,
        formatText,
      } = options;

      node.classList.remove('ok', 'warn', 'bad', 'empty');

      const numeric = typeof value === 'number' && Number.isFinite(value) ? value : null;
      if (numeric == null) {
        if (fill) fill.style.width = '0%';
        node.classList.add('empty');
        node.setAttribute('aria-valuenow', '0');
        node.setAttribute('aria-valuetext', 'No data');
        node.title = 'No data';
        return;
      }

      const clamped = Math.min(1, Math.max(0, numeric));
      const percent = Math.round(clamped * 100);
      if (fill) fill.style.width = `${percent}%`;
      node.setAttribute('aria-valuenow', clamped.toFixed(2));
      const text = typeof formatText === 'function'
        ? formatText(clamped, percent)
        : `${percent}%`;
      node.setAttribute('aria-valuetext', text);
      node.title = text;

      let state = 'ok';
      if (preferLow) {
        if (clamped >= bad) state = 'bad';
        else if (clamped >= warn) state = 'warn';
      } else {
        if (clamped <= bad) state = 'bad';
        else if (clamped <= warn) state = 'warn';
      }

      node.classList.add(state);
    },
  },
  modal: {
    _overlay: null,
    _active: null,
    _ensureOverlay() {
      if (this._overlay && document.body.contains(this._overlay)) {
        return this._overlay;
      }
      const overlay = document.createElement('div');
      overlay.className = 'modal-overlay';
      overlay.hidden = true;
      overlay.setAttribute('data-arw-modal', 'overlay');
      document.body.appendChild(overlay);
      this._overlay = overlay;
      return overlay;
    },
    close(result = null) {
      if (this._active && typeof this._active.close === 'function') {
        this._active.close(result);
      }
    },
    async form(rawOptions = {}) {
      const options = Object.assign(
        {
          title: 'Confirm',
          description: '',
          body: null,
          fields: [],
          submitLabel: 'Save',
          cancelLabel: 'Cancel',
          focusField: null,
          destructive: false,
          hideCancel: false,
        },
        rawOptions || {},
      );
      const overlay = this._ensureOverlay();
      return new Promise((resolve) => {
        const modal = this;
        if (modal._active && typeof modal._active.close === 'function') {
          try {
            modal._active.close(null);
          } catch {}
        }

        const uid = `arw-modal-${Date.now()}-${Math.random().toString(36).slice(2, 7)}`;
        const previous = document.activeElement instanceof HTMLElement ? document.activeElement : null;
        overlay.innerHTML = '';
        overlay.hidden = false;

        const dialog = document.createElement('div');
        dialog.className = 'modal-dialog';
        dialog.tabIndex = -1;
        dialog.setAttribute('role', 'dialog');
        dialog.setAttribute('aria-modal', 'true');
        overlay.appendChild(dialog);

        const header = document.createElement('header');
        const heading = document.createElement('h2');
        heading.id = `${uid}-title`;
        heading.textContent = options.title;
        header.appendChild(heading);
        if (options.description) {
          const desc = document.createElement('p');
          desc.id = `${uid}-desc`;
          desc.textContent = options.description;
          header.appendChild(desc);
          dialog.setAttribute('aria-describedby', desc.id);
        }
        dialog.setAttribute('aria-labelledby', heading.id);
        dialog.appendChild(header);

        const form = document.createElement('form');
        form.noValidate = true;
        const body = document.createElement('div');
        body.className = 'modal-body';
        if (options.body) {
          const info = document.createElement('p');
          info.className = 'modal-body-text';
          if (typeof options.body === 'string') {
            info.textContent = options.body;
          } else if (options.body instanceof Node) {
            info.appendChild(options.body);
          }
          body.appendChild(info);
        }

        const generalError = document.createElement('div');
        generalError.className = 'field-error';
        generalError.id = `${uid}-general-error`;
        generalError.hidden = true;
        generalError.setAttribute('aria-live', 'polite');

        const inputs = new Map();
        const fields = Array.isArray(options.fields) ? options.fields : [];
        for (const field of fields) {
          if (!field || !field.name) continue;
          const fieldId = `${uid}-${field.name}`;
          const wrap = document.createElement('div');
          wrap.className = 'modal-field';

          const label = document.createElement('label');
          label.setAttribute('for', fieldId);
          label.textContent = field.label || field.name;
          wrap.appendChild(label);

          let control;
          if (field.type === 'textarea') {
            control = document.createElement('textarea');
            control.rows = field.rows || 3;
          } else if (field.type === 'select') {
            control = document.createElement('select');
            const options = Array.isArray(field.options) ? field.options : [];
            for (const opt of options) {
              if (!opt) continue;
              const optionEl = document.createElement('option');
              optionEl.value = opt.value != null ? String(opt.value) : '';
              optionEl.textContent = opt.label != null ? String(opt.label) : optionEl.value;
              if (opt.disabled) optionEl.disabled = true;
              if (opt.selected) optionEl.selected = true;
              control.appendChild(optionEl);
            }
          } else {
            control = document.createElement('input');
            control.type = field.type || 'text';
          }

          control.id = fieldId;
          control.name = field.name;
          if (field.type === 'checkbox') {
            const checked = field.value != null ? Boolean(field.value) : Boolean(field.defaultValue);
            control.checked = checked;
            if (field.value != null) control.value = String(field.value);
          } else if (field.type === 'select') {
            const current = field.value != null ? String(field.value) : field.defaultValue != null ? String(field.defaultValue) : null;
            if (current != null) control.value = current;
          } else if (field.value != null) {
            control.value = String(field.value);
          } else if (field.defaultValue != null) {
            control.value = String(field.defaultValue);
          }
          if (field.placeholder) control.placeholder = field.placeholder;
          if (field.autocomplete) control.autocomplete = field.autocomplete;
          if (field.required) control.required = true;
          if (field.disabled) control.disabled = true;
          if (field.maxlength) control.maxLength = field.maxlength;
          if (field.pattern) control.pattern = field.pattern;
          if (field.inputmode) control.setAttribute('inputmode', field.inputmode);
          if (field.spellcheck === false) control.spellcheck = false;
          if (field.rows) control.rows = field.rows;
          if (field.readonly) {
            control.readOnly = true;
            control.setAttribute('aria-readonly', 'true');
          }
          if (field.monospace) {
            control.classList.add('mono');
          }
          wrap.appendChild(control);

          const describedBy = [];
          if (field.hint) {
            const hint = document.createElement('small');
            hint.id = `${fieldId}-hint`;
            hint.textContent = field.hint;
            wrap.appendChild(hint);
            describedBy.push(hint.id);
          }

          const error = document.createElement('div');
          error.className = 'field-error';
          error.id = `${fieldId}-error`;
          error.hidden = true;
          error.setAttribute('aria-live', 'polite');
          wrap.appendChild(error);
          describedBy.push(error.id);
          control.setAttribute('aria-describedby', describedBy.join(' '));

          const clearError = () => {
            wrap.classList.remove('has-error');
            control.removeAttribute('aria-invalid');
            error.textContent = '';
            error.hidden = true;
            generalError.textContent = '';
            generalError.hidden = true;
          };
          control.addEventListener('input', clearError);
          control.addEventListener('change', clearError);
          if (field.autoSelect) {
            control.addEventListener('focus', () => {
              try {
                control.select();
              } catch {}
            });
          }

          inputs.set(field.name, { control, wrap, error, field });
          body.appendChild(wrap);
        }

        body.appendChild(generalError);
        form.appendChild(body);

        const footer = document.createElement('div');
        footer.className = 'modal-footer';

        const cancelBtn = document.createElement('button');
        cancelBtn.type = 'button';
        cancelBtn.className = 'ghost';
        cancelBtn.textContent = options.cancelLabel || 'Cancel';
        if (!options.hideCancel) {
          footer.appendChild(cancelBtn);
        }

        const submitBtn = document.createElement('button');
        submitBtn.type = 'submit';
        submitBtn.className = options.primary === false ? 'ghost' : 'primary';
        if (options.destructive) {
          submitBtn.classList.add('bad');
        }
        submitBtn.textContent = options.submitLabel || 'Save';
        footer.appendChild(submitBtn);

        form.appendChild(footer);
        dialog.appendChild(form);

        const focusablesSelector = 'button, [href], input, select, textarea, [tabindex]:not([tabindex="-1"])';
        const getFocusables = () =>
          Array.from(dialog.querySelectorAll(focusablesSelector)).filter((el) => {
            if (el.hasAttribute('disabled')) return false;
            if (el.getAttribute('aria-hidden') === 'true') return false;
            const rect = el.getBoundingClientRect();
            return rect.width > 0 || rect.height > 0;
          });

        let closed = false;
        const close = (result) => {
          if (closed) return;
          closed = true;
          dialog.removeEventListener('keydown', handleKeydown);
          overlay.removeEventListener('mousedown', handlePointerDown);
          overlay.hidden = true;
          overlay.innerHTML = '';
          modal._active = null;
          if (previous && typeof previous.focus === 'function') {
            setTimeout(() => {
              try {
                previous.focus();
              } catch {}
            }, 0);
          }
          resolve(result);
        };

        const showErrors = (errors = {}) => {
          const err = errors && typeof errors === 'object' ? errors : {};
          const fieldNames = Object.keys(err).filter((key) => key !== '_');
          let focused = false;
          inputs.forEach((entry, name) => {
            const message = err[name];
            if (message) {
              entry.wrap.classList.add('has-error');
              entry.error.textContent = message;
              entry.error.hidden = false;
              entry.control.setAttribute('aria-invalid', 'true');
              if (!focused) {
                focused = true;
                queueMicrotask(() => {
                  try {
                    entry.control.focus();
                  } catch {}
                });
              }
            } else {
              entry.wrap.classList.remove('has-error');
              entry.error.textContent = '';
              entry.error.hidden = true;
              entry.control.removeAttribute('aria-invalid');
            }
          });
          if (err._) {
            generalError.textContent = err._;
            generalError.hidden = false;
            if (!focused) {
              queueMicrotask(() => {
                try {
                  submitBtn.focus();
                } catch {}
              });
            }
          } else {
            generalError.textContent = '';
            generalError.hidden = true;
          }
          return fieldNames.length > 0 || Boolean(err._);
        };

        const handleSubmit = async (event) => {
          event.preventDefault();
          const rawValues = {};
          inputs.forEach((entry, name) => {
            let raw;
            if (entry.field && entry.field.type === 'checkbox') {
              raw = entry.control.checked;
            } else if (entry.field && entry.field.type === 'select') {
              raw = entry.control.value != null ? String(entry.control.value) : '';
            } else {
              raw = entry.control.value != null ? String(entry.control.value) : '';
            }
            if (typeof raw === 'string' && (!entry.field || entry.field.trim !== false)) {
              raw = raw.trim();
            }
            rawValues[name] = raw;
          });
          showErrors({});
          let values = { ...rawValues };
          let errors = {};
          try {
            if (typeof options.validate === 'function') {
              const result = await options.validate({ ...rawValues });
              if (result && typeof result === 'object') {
                if (result.values && typeof result.values === 'object') {
                  values = result.values;
                }
                if (result.errors && typeof result.errors === 'object') {
                  errors = result.errors;
                }
              }
            }
          } catch (err) {
            console.error(err);
            errors = Object.assign({}, errors, {
              _: err && typeof err.message === 'string' ? err.message : 'Unable to submit',
            });
          }
          if (showErrors(errors)) {
            return;
          }
          close(values);
        };

        const handlePointerDown = (event) => {
          if (event.target === overlay) {
            event.preventDefault();
            close(null);
          }
        };

        const handleKeydown = (event) => {
          if (event.key === 'Escape') {
            event.preventDefault();
            close(null);
            return;
          }
          if (event.key === 'Tab') {
            const focusables = getFocusables();
            if (!focusables.length) {
              event.preventDefault();
              return;
            }
            const current = document.activeElement;
            let index = focusables.indexOf(current);
            if (index === -1) {
              index = 0;
            }
            if (event.shiftKey) {
              if (index === 0) {
                event.preventDefault();
                focusables[focusables.length - 1].focus();
              }
            } else if (index === focusables.length - 1) {
              event.preventDefault();
              focusables[0].focus();
            }
          }
        };

        form.addEventListener('submit', handleSubmit);
        if (!options.hideCancel) {
          cancelBtn.addEventListener('click', (event) => {
            event.preventDefault();
            close(null);
          });
        }
        overlay.addEventListener('mousedown', handlePointerDown);
        dialog.addEventListener('keydown', handleKeydown);

        modal._active = { close };

        const focusCandidates = [];
        if (options.focusField && inputs.has(options.focusField)) {
          focusCandidates.push(inputs.get(options.focusField).control);
        }
        inputs.forEach((entry) => {
          focusCandidates.push(entry.control);
        });
        if (!options.hideCancel) {
          focusCandidates.push(cancelBtn);
        }
        focusCandidates.push(submitBtn);
        const target = focusCandidates.find((el) => el && !el.hasAttribute('disabled'));
        if (target && typeof target.focus === 'function') {
          setTimeout(() => {
            try {
              target.focus();
            } catch {}
          }, 0);
        }
      });
    },
    async confirm(options = {}) {
      const result = await this.form(
        Object.assign(
          {
            fields: [],
            submitLabel: options.confirmLabel || options.submitLabel || 'Confirm',
            cancelLabel: options.cancelLabel || 'Cancel',
          },
          options || {},
        ),
      );
      return result != null;
    },
  },
  util: {
    pageId(){
      try{
        const d = document.body?.dataset?.page; if (d) return String(d);
        const p = (window.location.pathname||'').split('/').pop() || 'index.html';
        return p.replace(/\.html?$/i,'') || 'index';
      }catch{ return 'index' }
    },
    downloadPercent(payload){
      if (!payload || typeof payload !== 'object') return null;
      const clamp = (value) => Math.max(0, Math.min(100, value));
      const candidates = [payload.progress, payload.percent];
      for (const candidate of candidates) {
        if (candidate == null) continue;
        const raw = typeof candidate === 'string' ? candidate.replace(/%$/, '') : candidate;
        const num = Number(raw);
        if (Number.isFinite(num)) {
          return clamp(num);
        }
      }
      const downloaded = Number(payload.downloaded);
      const total = Number(payload.total);
      if (Number.isFinite(downloaded) && Number.isFinite(total) && total > 0) {
        const pct = (downloaded / total) * 100;
        if (Number.isFinite(pct)) return clamp(pct);
      }
      return null;
    }
  },
  metrics: {
    async routeStats({ base, signal, headers, store = true } = {}) {
      try {
        const resolvedBase = base || (() => {
          try {
            const meta = ARW.baseMeta(ARW.getPortFromInput('port'));
            return meta.base;
          } catch {
            return ARW.base();
          }
        })();
        const init = {};
        if (signal) init.signal = signal;
        const mergedHeaders = Object.assign({ Accept: 'application/json' }, headers || {});
        init.headers = mergedHeaders;
        const snapshot = await ARW.http.json(resolvedBase, '/state/route_stats', init);
        const safe = snapshot && typeof snapshot === 'object' ? snapshot : {};
        if (store && ARW.read && ARW.read._store && typeof ARW.read._emit === 'function') {
          try {
            ARW.read._store.set('route_stats', safe);
            ARW.read._emit('route_stats');
          } catch {}
        }
        return safe;
      } catch (err) {
        throw err;
      }
    },
  },
  auth: {
    _recent: new Map(),
    notify(base) {
      try {
        const key = base ? ARW.normalizeBase(base) : '';
        const now = Date.now();
        const last = this._recent.get(key);
        if (last && (now - last) < 30000) return;
        this._recent.set(key, now);
        const target = key || 'http://127.0.0.1:8091';
        ARW.toast(`Authorization required for ${target}. Set your admin token in Home → Connection & alerts.`);
      } catch {
        // best-effort notification
      }
    },
  },
  validateProjectName(name) {
    const raw = String(name ?? '').trim();
    if (!raw) return { ok: false, error: 'Project name cannot be empty' };
    if (raw.length > 120) return { ok: false, error: 'Project name must be 120 characters or fewer' };
    if (raw.startsWith('.')) return { ok: false, error: 'Project name cannot start with a dot' };
    const valid = /^[A-Za-z0-9 _.-]+$/.test(raw);
    if (!valid) return { ok: false, error: 'Project name may only contain letters, numbers, spaces, ., -, _' };
    return { ok: true, value: raw };
  },
  validateProjectRelPath(rel) {
    const raw = String(rel ?? '').trim();
    if (!raw) return { ok: false, error: 'Destination path cannot be empty' };
    if (/^[\\/]/.test(raw)) return { ok: false, error: 'Destination must be relative (no leading / or \\)' };
    if (/^[A-Za-z]:/.test(raw)) return { ok: false, error: 'Destination must not include a drive prefix' };
    if (/^\\\\/.test(raw)) return { ok: false, error: 'Destination must not include a UNC prefix' };
    const parts = raw.split(/[\\/]+/).filter(Boolean);
    if (!parts.length) return { ok: false, error: 'Destination path cannot be empty' };
    if (parts.some(seg => seg === '.' || seg === '..')) {
      return { ok: false, error: 'Destination must not contain . or .. segments' };
    }
    return { ok: true, value: parts.join('/') };
  },
  invoke(cmd, args) {
    if (HAS_TAURI) {
      return window.__TAURI__.invoke(cmd, args);
    }
    const handler = fallbackHandlers[cmd];
    if (!handler) {
      const err = new Error(`Command ${cmd} not available in browser mode`);
      return Promise.reject(err);
    }
    try {
      const result = handler.call(this, args || {});
      if (result && typeof result.then === 'function') {
        return result;
      }
      return Promise.resolve(result);
    } catch (err) {
      return Promise.reject(err);
    }
  },
  // Clipboard helper
  async copy(text){ try{ await navigator.clipboard.writeText(text); this.toast('Copied'); }catch{} },
  templates: {
    async save(ns, tpl){
      try{
        const key = 'ui:'+ns;
        const cur = await ARW.getPrefs(key);
        const next = { ...(cur&&typeof cur==='object'?cur:{}), template: tpl };
        await ARW.setPrefs(key, next);
        ARW.toast('Layout saved');
      }catch(e){ ARW.toast('Save failed'); }
    },
    async load(ns){
      try{
        const key = 'ui:'+ns; const v = await ARW.getPrefs(key); return v?.template || null;
      }catch{ return null }
    }
  },
  tokens: {
    generateHex(byteLength = 32) {
      const size = Number.isFinite(byteLength) && byteLength > 0 ? Math.min(Math.floor(byteLength), 1024) : 32;
      const buffer = new Uint8Array(size);
      if (globalThis.crypto && typeof globalThis.crypto.getRandomValues === 'function') {
        globalThis.crypto.getRandomValues(buffer);
      } else {
        for (let i = 0; i < size; i += 1) {
          buffer[i] = Math.floor(Math.random() * 256);
        }
      }
      let out = '';
      for (let i = 0; i < buffer.length; i += 1) {
        out += buffer[i].toString(16).padStart(2, '0');
      }
      return out;
    },
  },
  connections: {
    _norm(b){
      try{
        const normalized = ARW.normalizeBase(b);
        return normalized || '';
      }catch{ return ''; }
    },
    async tokenFor(base){
      try{
        const prefs = await ARW.getPrefs('launcher') || {};
        const norm = this._norm(base);
        const list = Array.isArray(prefs.connections) ? prefs.connections : [];
        const hit = list.find(c => this._norm(c.base) === norm);
        const connToken = typeof hit?.token === 'string' ? hit.token.trim() : '';
        if (connToken) return connToken;
        const fallback = typeof prefs.adminToken === 'string' ? prefs.adminToken.trim() : '';
        return fallback || null;
      }catch{ return null }
    }
  },
  http: {
    _norm(base){
      try{
        const norm = ARW.normalizeBase(base);
        if (norm) return norm;
      }catch{}
      try{ return String(base||'').replace(/\/+$/,''); }catch{ return ''; }
    },
    async _headers(base, extra){
      const headers = Object.assign({}, extra || {});
      let token = null;
      try {
        if (base) token = await ARW.connections.tokenFor(base);
      } catch {}
      if (token) {
        const hasAuth = Object.keys(headers).some(k => k.toLowerCase() === 'authorization');
        if (!headers['X-ARW-Admin'] && !headers['x-arw-admin']) headers['X-ARW-Admin'] = token;
        if (!hasAuth) headers['Authorization'] = `Bearer ${token}`;
      }
      return headers;
    },
    async fetch(baseOrUrl, pathOrInit, maybeInit){
      let url = baseOrUrl;
      let init = {};
      let tokenBase = null;
      if (typeof pathOrInit === 'string') {
        tokenBase = baseOrUrl;
        url = this._norm(baseOrUrl) + (pathOrInit.startsWith('/') ? pathOrInit : '/' + pathOrInit);
        init = maybeInit || {};
      } else {
        init = pathOrInit || {};
        tokenBase = (()=>{
          try { return new URL(baseOrUrl).origin; } catch { return baseOrUrl; }
        })();
      }
      const opts = Object.assign({}, init);
      opts.headers = await this._headers(tokenBase, init.headers);
      const resp = await fetch(url, opts);
      if (resp && (resp.status === 401 || resp.status === 403)) {
        ARW.auth.notify(tokenBase || url);
      }
      return resp;
    },
    async json(baseOrUrl, pathOrInit, maybeInit){
      const resp = await this.fetch(baseOrUrl, pathOrInit, maybeInit);
      if (!resp.ok) throw new Error('HTTP '+resp.status);
      return resp.json();
    },
    async text(baseOrUrl, pathOrInit, maybeInit){
      const resp = await this.fetch(baseOrUrl, pathOrInit, maybeInit);
      if (!resp.ok) throw new Error('HTTP '+resp.status);
      return resp.text();
    }
  },
  toast(msg) {
    if (!this._toastWrap) {
      const wrap = document.createElement('div');
      wrap.className = 'toast-wrap';
      document.body.appendChild(wrap);
      this._toastWrap = wrap;
    }
    const d = document.createElement('div');
    d.className = 'toast'; d.textContent = msg;
    this._toastWrap.appendChild(d);
    setTimeout(()=>{ try{ this._toastWrap.removeChild(d); }catch(e){} }, 2500);
  },
  toastCaptureError(err, { scope = 'capture', lease = 'io:screenshot' } = {}) {
    const prefix = `Unable to ${scope}`;
    let raw = '';
    if (err && typeof err === 'object' && err.message) raw = String(err.message);
    else if (typeof err === 'string') raw = err;
    else if (err && typeof err.toString === 'function') raw = String(err.toString());
    const msg = raw.trim();
    const lc = msg.toLowerCase();
    let detail = '';
    if (lc.includes('lease') || lc.includes('permission') || lc.includes('denied')) {
      detail = `Grant the ${lease} lease from the sidecar before retrying.`;
    } else if (lc.includes('unauthorized') || lc.includes('401') || lc.includes('403')) {
      detail = 'Authorize this connection with your admin token in Home.';
    } else if (lc.includes('active window') || lc.includes('focus window')) {
      detail = 'Focus the window you want to capture, then try again.';
    } else if (lc.includes('not implemented') || lc.includes('unsupported') || lc.includes('platform')) {
      detail = 'This capture mode is not supported on this platform yet.';
    } else if (lc.includes('timeout')) {
      detail = 'Capture timed out. Retry in a moment.';
    } else if (lc.includes('no displays') || lc.includes('no display') || lc.includes('monitor')) {
      detail = 'No display was detected. Confirm a monitor is available and retry.';
    }
    const finalMsg = detail ? `${prefix}: ${detail}` : `${prefix}. Check service logs for details.`;
    this.toast(finalMsg);
  },
  async getPrefs(ns = 'launcher') {
    try{
      if (this._prefsCache.has(ns)) {
        const v = this._prefsCache.get(ns);
        // return a shallow clone to avoid surprise mutation
        return v && typeof v === 'object' ? { ...v } : v;
      }
      const v = await this.invoke('get_prefs', { namespace: ns });
      if (v && typeof v === 'object') this._prefsCache.set(ns, { ...v }); else this._prefsCache.set(ns, v);
      return v;
    }catch{ return {} }
  },
  async saveToProjectPrompt(path){
    try{
      let initialProject = '';
      try{
        const hubPrefs = await this.getPrefs('ui:hub');
        if (hubPrefs && typeof hubPrefs.lastProject === 'string') {
          initialProject = String(hubPrefs.lastProject).trim();
        }
      }catch{}
      const baseName = (path || '').split(/[\\/]/).pop() || 'capture.png';
      const defaults = {
        project: initialProject,
        dest: `images/${baseName}`,
      };
      const result = await this.modal.form({
        title: 'Save to project',
        description: 'Copy the file into a project workspace and make it available to agents.',
        submitLabel: 'Save',
        focusField: 'project',
        fields: [
          {
            name: 'project',
            label: 'Project',
            value: defaults.project,
            placeholder: 'Enter project name',
            required: true,
            autocomplete: 'off',
            hint: 'Use letters, numbers, spaces, . or -',
          },
          {
            name: 'dest',
            label: 'Destination path',
            value: defaults.dest,
            placeholder: 'images/screenshot.png',
            required: true,
            autocomplete: 'off',
            hint: 'Relative to the project root (no leading /)',
          },
        ],
        validate: (values) => {
          const errors = {};
          let projectResult = this.validateProjectName(values.project);
          let destResult = this.validateProjectRelPath(values.dest);
          if (!projectResult.ok) errors.project = projectResult.error;
          if (!destResult.ok) errors.dest = destResult.error;
          const validValues = {
            project: projectResult.ok ? projectResult.value : values.project,
            dest: destResult.ok ? destResult.value : values.dest,
          };
          return { values: validValues, errors };
        },
      });
      if (!result) return null;
      const proj = result.project;
      const dest = result.dest;
      try{
        await this.invoke('projects_import', {
          proj,
          dest,
          src_path: path,
          mode: 'copy',
          port: this.getPortFromInput('port'),
        });
        this.toast(`Saved to ${proj}: ${dest}`);
        return { proj, dest };
      }catch(err){
        console.error(err);
        const message = err && typeof err === 'string' ? err : err?.message;
        this.toast(message ? `Import failed: ${message}` : 'Import failed');
      }
    }catch(e){
      console.error(e);
      this.toast('Import failed');
    }
    return null;
  },
  _bestAltForPath(path, fallback){
    const record = path ? this._ocrCache.get(path) : null;
    if (record && typeof record.text === 'string'){
      const firstLine = record.text.split(/\r?\n/).find(line => line.trim());
      if (firstLine){
        const trimmed = firstLine.trim();
        if (trimmed.length > 120) return trimmed.slice(0, 117) + '…';
        return trimmed;
      }
    }
    if (fallback && fallback.trim()) return fallback;
    return 'screenshot';
  },
  _updateAltForPath(path){
    if (!path) return;
    const alt = this._bestAltForPath(path, path.split(/[\\/]/).pop() || 'screenshot');
    try{
      const selector = `[data-screenshot-path="${window.CSS?.escape ? CSS.escape(path) : path.replace(/"/g,'\\"')}"]`;
      document.querySelectorAll(selector).forEach(img => { if (img instanceof HTMLImageElement){ img.alt = alt; img.dataset.alt = alt; } });
    }catch{}
  },
  _storeOcrResult(path, payload){
    if (!path) return;
    const record = {
      text: typeof payload?.text === 'string' ? payload.text : '',
      lang: payload?.lang || 'eng',
      generated_at: payload?.generated_at || new Date().toISOString(),
      cached: !!payload?.cached,
    };
    this._ocrCache.set(path, record);
    if (this._ocrCache.size > 200){
      try{
        const firstKey = this._ocrCache.keys().next?.().value;
        if (typeof firstKey === 'string') this._ocrCache.delete(firstKey);
      }catch{}
    }
    this._updateAltForPath(path);
  },
  copyMarkdown(path, alt){
    try{
      const altText = this._bestAltForPath(path, alt);
      const safeAlt = String(altText || '').replace(/[\[\]]/g, ' ');
      const md = `![${safeAlt}](${path})`;
      navigator.clipboard.writeText(md);
      this.toast('Markdown copied');
    }catch{ this.toast('Copy failed'); }
  },
  async appendMarkdownToNotes(proj, relPath, sourcePath){
    try{
      const alt = this._bestAltForPath(sourcePath || relPath, relPath);
      const md = `![${String(alt || '').replace(/[\[\]]/g, ' ')}](${relPath})`;
      await this.invoke('run_tool_admin', {
        id: 'project.notes.append',
        input: {
          project: proj,
          markdown: md,
          timestamp: false
        },
        port: this.toolPort()
      });
      this.toast('Appended to NOTES.md');
    }catch(e){ console.error(e); this.toast('Append failed'); }
  },
  async maybeAppendToNotes(proj, relPath, sourcePath){
    try{
      const prefs = await this.getPrefs('launcher') || {};
      if (prefs.appendToNotes){
        await this.appendMarkdownToNotes(proj, relPath, sourcePath);
      }
    }catch(e){ console.error(e); }
  },
  async setPrefs(ns, value) {
    // Update cache immediately
    try {
      if (value && typeof value === 'object' && !Array.isArray(value)) {
        this._prefsCache.set(ns, { ...value });
      } else {
        this._prefsCache.set(ns, value);
      }
    } catch {}
    const key = ns || 'launcher';
    let entry = this._prefsTimers.get(key);
    if (entry) {
      clearTimeout(entry.timer);
    } else {
      entry = { timer: null, resolvers: [], rejecters: [] };
    }
    return new Promise((resolve, reject) => {
      entry.resolvers.push(resolve);
      entry.rejecters.push(reject);
      entry.timer = setTimeout(async () => {
        const pendingResolvers = entry.resolvers.slice();
        const pendingRejecters = entry.rejecters.slice();
        entry.resolvers.length = 0;
        entry.rejecters.length = 0;
        try {
          let payload = this._prefsCache.get(key);
          if (payload && typeof payload === 'object' && !Array.isArray(payload)) {
            payload = { ...payload };
          }
          await this.invoke('set_prefs', { namespace: key, value: payload ?? {} });
          for (const fn of pendingResolvers) {
            try { fn(); } catch {}
          }
        } catch (err) {
          for (const fn of pendingRejecters) {
            try { fn(err); } catch {}
          }
        } finally {
          this._prefsTimers.delete(key);
        }
      }, 250);
      this._prefsTimers.set(key, entry);
    });
  },
  normalizeBase(base) {
    const raw = (base ?? '').toString().trim();
    if (!raw) return '';
    const stripTrailing = (val) => val.replace(/\/+$/, '');
    const ensureScheme = (input) => {
      const trimmed = stripTrailing(input);
      if (!trimmed) return '';
      return /^[a-zA-Z][a-zA-Z0-9+\-.]*:\/\//.test(trimmed)
        ? trimmed
        : `http://${trimmed}`;
    };
    const candidate = ensureScheme(raw);
    if (!candidate) return '';
    try {
      const url = new URL(candidate);
      if (!url || url.origin === 'null') {
        return ensureScheme(raw);
      }
      const origin = url.origin.toLowerCase();
      let path = stripTrailing(url.pathname || '');
      if (path === '/' || path === '') {
        path = '';
      } else if (!path.startsWith('/')) {
        path = `/${path}`;
      }
      return `${origin}${path}`;
    } catch {
      return ensureScheme(raw);
    }
  },
  isLoopbackHost(host) {
    if (!host) return false;
    let value = String(host).trim().toLowerCase();
    if (!value) return false;
    if (value.includes('://')) {
      try {
        const parsed = new URL(value);
        value = parsed.hostname.toLowerCase();
      } catch {}
    } else {
      if (value.startsWith('[')) {
        const closing = value.indexOf(']');
        if (closing !== -1) {
          value = value.slice(1, closing).toLowerCase();
        }
      }
      if (value.includes(':')) {
        value = value.split(':')[0];
      }
    }
    if (!value) return false;
    if (value === 'localhost') return true;
    if (value === '::1') return true;
    if (value === '0.0.0.0') return true;
    if (value.startsWith('127.')) return true;
    if (value.startsWith('::ffff:127.')) return true;
    return false;
  },
  syncBaseCallout(meta) {
    try {
      const callout = document.getElementById('baseCallout');
      if (!callout) return;
      const info = meta || this.baseMeta(this.getPortFromInput('port'));
      const override = info && info.override;
      const remote = override && info.host && !this.isLoopbackHost(info.host);
      const protocol = info && typeof info.protocol === 'string' ? info.protocol.toLowerCase() : '';
      const insecure = remote && protocol === 'http';
      if (!insecure) {
        callout.hidden = true;
        callout.setAttribute('aria-hidden', 'true');
        return;
      }
      const body = document.getElementById('baseCalloutBody');
      if (body) {
        const origin = info.origin || info.base || 'remote host';
        body.textContent = `Remote base ${origin} is using plain HTTP. Switch to HTTPS or a trusted tunnel before exposing admin surfaces beyond this machine.`;
      }
      const button = document.getElementById('btn-base-callout');
      if (button && button.dataset.bound !== '1') {
        button.addEventListener('click', async () => {
          try {
            await this.invoke('open_url', { url: 'https://t3hw00t.github.io/ARW/guide/network_posture/' });
          } catch {}
        });
        button.dataset.bound = '1';
      }
      callout.hidden = false;
      callout.setAttribute('aria-hidden', 'false');
    } catch {}
  },
  baseMeta(port) {
    const override = this.baseOverride();
    if (override) {
      const normalized = this.normalizeBase(override);
      const info = {
        base: normalized || override,
        origin: normalized || override,
        override: true,
        protocol: null,
        host: override,
        port: null,
      };
      const parseUrl = (value) => {
        if (typeof URL === 'function') {
          try { return new URL(value); }
          catch {}
        }
        return null;
      };
      let url = parseUrl(override);
      if (!url && !override.endsWith('/')) {
        url = parseUrl(`${override}/`);
      }
      if (url) {
        info.protocol = url.protocol ? url.protocol.replace(/:$/, '') : info.protocol;
        info.host = url.host || info.host;
        if (url.port) {
          const parsedPort = Number(url.port);
          info.port = Number.isFinite(parsedPort) ? parsedPort : null;
        } else if (url.protocol === 'https:') {
          info.port = 443;
        } else if (url.protocol === 'http:') {
          info.port = 80;
        }
      } else {
        const match = override.match(/^(https?):\/\/([^\/#?]+)/i);
        if (match) {
          info.protocol = match[1].toLowerCase();
          info.host = match[2].toLowerCase();
          info.origin = `${info.protocol}://${info.host}`;
          const portMatch = info.host.match(/:(\d+)$/);
          if (portMatch) {
            const parsedPort = Number(portMatch[1]);
            if (Number.isFinite(parsedPort)) info.port = parsedPort;
          } else if (info.protocol === 'https') {
            info.port = 443;
          } else if (info.protocol === 'http') {
            info.port = 80;
          }
        }
      }
      if (!info.origin) info.origin = info.base;
      return info;
    }
    const resolved = Number.isFinite(port) && port > 0 ? Number(port) : 8091;
    const baseUrl = `http://127.0.0.1:${resolved}`;
    return {
      base: baseUrl,
      origin: baseUrl,
      override: false,
      protocol: 'http',
      host: `127.0.0.1:${resolved}`,
      port: resolved,
    };
  },
  baseOverride() {
    try {
      const override = typeof window.__ARW_BASE_OVERRIDE === 'string'
        ? window.__ARW_BASE_OVERRIDE.trim()
        : '';
      if (override) return this.normalizeBase(override);
    } catch {
    }
    try {
      const stored = typeof localStorage !== 'undefined'
        ? (localStorage.getItem(this._BASE_OVERRIDE_KEY) || '').trim()
        : '';
      if (stored) return this.normalizeBase(stored);
    } catch {}
    return '';
  },
  baseOverridePort() {
    const override = this.baseOverride();
    if (!override) return null;
    const parsed = (() => {
      try { return new URL(override); }
      catch {
        if (!/^[a-zA-Z][a-zA-Z0-9+\-.]*:\/\//.test(override)) {
          try { return new URL(`http://${override}`); }
          catch { return null; }
        }
        return null;
      }
    })();
    if (!parsed) return null;
    if (parsed.port) {
      const asNum = Number(parsed.port);
      return Number.isFinite(asNum) ? asNum : null;
    }
    if (parsed.protocol === 'https:') return 443;
    if (parsed.protocol === 'http:') return 80;
    return null;
  },
  applyBaseMeta({ portInputId, badgeId, label = 'Base' } = {}) {
    const portInput = portInputId ? document.getElementById(portInputId) : null;
    const currentPort = portInput ? parseInt(portInput.value, 10) : null;
    const meta = this.baseMeta(currentPort);
    if (portInput) {
      if (meta.override) {
        if (meta.port != null) portInput.value = String(meta.port);
        portInput.disabled = true;
        portInput.setAttribute('aria-disabled', 'true');
        portInput.title = 'Port pinned by saved connection base';
      } else {
        portInput.disabled = false;
        portInput.removeAttribute('aria-disabled');
        portInput.removeAttribute('title');
      }
    }
    if (badgeId) {
      const badge = document.getElementById(badgeId);
      if (badge) {
        const text = `${label}: ${meta.origin || meta.base}`;
        badge.textContent = text;
        badge.setAttribute('data-override', meta.override ? 'true' : 'false');
        const remoteOverride = meta.override && meta.host && !this.isLoopbackHost(meta.host);
        const protocol = typeof meta.protocol === 'string' ? meta.protocol.toLowerCase() : '';
        let state = meta.override ? 'remote-https' : 'local';
        if (remoteOverride) {
          state = protocol === 'http' ? 'remote-http' : 'remote-https';
        } else if (meta.override) {
          state = 'loopback';
        }
        badge.setAttribute('data-state', state);
        let title = text;
        if (state === 'remote-http') {
          title = `${text}\nWarning: remote base is using plain HTTP. Prefer HTTPS or a tunnel when sharing admin surfaces.`;
        }
        badge.setAttribute('title', title);
      }
    }
    this.syncBaseCallout(meta);
    return meta;
  },
  base(port) {
    const override = this.baseOverride();
    if (override) return override;
    const p = Number.isFinite(port) && port > 0 ? port : 8091
    return `http://127.0.0.1:${p}`
  },
  toolPort() {
    const meta = this.baseMeta(this.getPortFromInput('port'));
    return meta.port || 8091;
  },
  _BASE_OVERRIDE_KEY: 'arw:base:override',
  _persistBaseOverride(value) {
    try {
      Promise.resolve(this.getPrefs('launcher'))
        .then((prefs) => {
          const next =
            prefs && typeof prefs === 'object' && !Array.isArray(prefs) ? { ...prefs } : {};
          if (value) {
            next.baseOverride = value;
          } else {
            delete next.baseOverride;
          }
          return this.setPrefs('launcher', next);
        })
        .catch(() => {});
    } catch {}
  },
  setBaseOverride(base, options = {}) {
    const { persist = true } = options || {};
    const normalized = this.normalizeBase(base || '');
    if (!normalized) {
      this.clearBaseOverride({ persist });
      return '';
    }
    try { localStorage.setItem(this._BASE_OVERRIDE_KEY, normalized); } catch {}
    try { window.__ARW_BASE_OVERRIDE = normalized; } catch {}
    if (persist) this._persistBaseOverride(normalized);
    this._emitBaseOverride(normalized);
    return normalized;
  },
  clearBaseOverride(options = {}) {
    const { persist = true } = options || {};
    try { localStorage.removeItem(this._BASE_OVERRIDE_KEY); } catch {}
    try { delete window.__ARW_BASE_OVERRIDE; } catch {}
    if (persist) this._persistBaseOverride('');
    this._emitBaseOverride('');
    return '';
  },
  _emitBaseOverride(base) {
    try { window.dispatchEvent(new CustomEvent('arw:base-override-changed', { detail: { base } })); } catch {}
  },
  // Theme override (Auto/Light/Dark) — OS-first when 'auto'
  theme: {
    KEY: 'arw:theme',
    // light/dark neutrals (align to tokens)
    L: { surface:'#ffffff', surfaceMuted:'#fafaf9', ink:'#111827', line:'#e5e7eb' },
    D: { surface:'#0f1115', surfaceMuted:'#0b0d11', ink:'#e5e7eb', line:'#1f232a' },
    apply(val){
      try{
        const root = document.documentElement;
        const body = document.body;
        body?.classList.remove('theme-light','theme-dark');
        // Clear inline overrides first
        const clear = ()=>{
          root.style.removeProperty('--surface');
          root.style.removeProperty('--surface-muted');
          root.style.removeProperty('--color-ink');
          root.style.removeProperty('--color-line');
        };
        if (val === 'light'){
          const v = this.L; body?.classList.add('theme-light');
          root.style.setProperty('--surface', v.surface);
          root.style.setProperty('--surface-muted', v.surfaceMuted);
          root.style.setProperty('--color-ink', v.ink);
          root.style.setProperty('--color-line', v.line);
        } else if (val === 'dark'){
          const v = this.D; body?.classList.add('theme-dark');
          root.style.setProperty('--surface', v.surface);
          root.style.setProperty('--surface-muted', v.surfaceMuted);
          root.style.setProperty('--color-ink', v.ink);
          root.style.setProperty('--color-line', v.line);
        } else { // auto
          clear();
        }
      }catch{}
    },
    set(val){ try{ localStorage.setItem(this.KEY, val); }catch{} this.apply(val); try{ ARW.ui?.badges?.update(); }catch{} ARW.toast('Theme: '+(val||'auto')); },
    init(){ let v='auto'; try{ v = localStorage.getItem(this.KEY)||'auto'; }catch{} this.apply(v); }
  },
  density: {
    KEY: 'arw:density',
    _k(){ return this.KEY + ':' + ARW.util.pageId(); },
    apply(val){ try{ document.body.classList.toggle('compact', val === 'compact'); }catch{} },
    set(val){ try{ localStorage.setItem(this._k(), val); }catch{} this.apply(val); try{ ARW.ui?.badges?.update(); }catch{} ARW.toast('Density: '+(val==='compact'?'compact':'normal')); },
    toggle(){ let v=this.get(); this.set(v==='compact'?'normal':'compact'); },
    get(){ let v='normal'; try{ v = localStorage.getItem(this._k()) || localStorage.getItem(this.KEY) || 'normal'; }catch{} return v; },
    init(){ this.apply(this.get()); }
  },
  layout: {
    KEY: 'arw:focus',
    _k(){ return this.KEY + ':' + ARW.util.pageId(); },
    apply(on){ try{ const root = document.querySelector('.layout'); if (!root) return; root.classList.toggle('full', !!on); }catch{} },
    set(on){ try{ localStorage.setItem(this._k(), on ? '1' : '0'); }catch{} this.apply(!!on); },
    toggle(){ const cur = this.get(); this.set(!cur); ARW.toast('Focus: '+(!cur ? 'on' : 'off')); },
    get(){ let v='0'; try{ v = localStorage.getItem(this._k()) || '0'; }catch{} return v==='1'; },
    init(){ this.apply(this.get()); }
  },
  getPortFromInput(id) {
    const v = parseInt(document.getElementById(id)?.value, 10)
    return Number.isFinite(v) && v > 0 ? v : null
  },
  async applyPortFromPrefs(id, ns = 'launcher') {
    const v = await this.getPrefs(ns)
    if (v && v.port && document.getElementById(id)) document.getElementById(id).value = v.port
  },
  quantReplace(url, q) {
    try {
      if (!url || !/\.gguf$/i.test(url)) return url
      // Replace existing quant token like Q4_K_M, Q5_K_S, Q8_0 etc., else insert before .gguf
      const m = url.match(/(.*?)(Q\d[^/]*?)?(\.gguf)$/i)
      if (!m) return url
      const prefix = m[1]
      const has = !!m[2]
      const ext = m[3]
      if (has) return prefix + q + ext
      // insert with hyphen if the filename part doesn't already end with '-'
      return url.replace(/\.gguf$/i, (prefix.endsWith('-') ? '' : '-') + q + '.gguf')
    } catch { return url }
  },
  // Lightweight SSE store with prefix filters and replay support
  sse: {
    _es: null,
    _subs: new Map(),
    _nextId: 1,
    _lastId: null,
    _connected: false,
    _status: 'idle',
    _statusChangedAt: null,
    _last: null,
    _lastRaw: null,
    _lastKind: null,
    _lastEventAt: null,
    _base: null,
    _opts: null,
    _mode: 'eventsource',
    _retryMs: 500,
    _retryTimer: null,
    _closing: false,
    _abortController: null,
    _maxRetryMs: 5000,
    _updateStatus(status, extra){
      this._status = status;
      this._statusChangedAt = Date.now();
      try{ if (document && document.body) document.body.setAttribute('data-sse-status', status); }catch{}
      const payload = { status, changedAt: this._statusChangedAt, ...(extra||{}) };
      this._emit('*status*', payload);
    },
    _url(baseUrl, opts, afterId){
      const params = new URLSearchParams();
      if (afterId) params.set('after', String(afterId));
      if (!afterId && opts?.replay) params.set('replay', String(opts.replay));
      if (opts?.prefix && Array.isArray(opts.prefix)) {
        for (const p of opts.prefix) params.append('prefix', p);
      } else if (typeof opts?.prefix === 'string' && opts.prefix) {
        params.append('prefix', opts.prefix);
      }
      return baseUrl.replace(/\/$/, '') + '/events' + (params.toString() ? ('?' + params.toString()) : '');
    },
    _clearTimer(){ if (this._retryTimer){ clearTimeout(this._retryTimer); this._retryTimer=null; } },
    _teardownEventSource(){ if (this._es){ try { this._closing = true; this._es.close(); } catch {} this._es = null; this._closing = false; } },
    _teardownFetch(){ if (this._abortController){ try { this._closing = true; this._abortController.abort(); } catch {} } this._abortController = null; this._closing = false; },
    connect(baseUrl, opts = {}, resumeLast = false) {
      this._connectAsync(baseUrl, opts, resumeLast).catch((err)=>{ console.error('SSE connect failed', err); });
    },
    async _connectAsync(baseUrl, opts = {}, resumeLast = false) {
      const prevBase = this._base;
      const baseChanged = typeof prevBase === 'string' && prevBase !== baseUrl;
      this._base = baseUrl;
      this._opts = { ...(opts || {}) };
      const maxRetry = Number(this._opts.maxRetryMs);
      this._maxRetryMs = Number.isFinite(maxRetry) && maxRetry > 0 ? maxRetry : 5000;
      if (baseChanged) {
        this._lastId = null;
      }
      this._clearTimer();
      this._teardownEventSource();
      this._teardownFetch();
      const useAfter = resumeLast && !baseChanged && this._lastId;
      const url = this._url(baseUrl, this._opts, useAfter ? this._lastId : null);
      let token = typeof opts.token === 'function' ? null : opts.token;
      if (token === undefined) {
        try { token = await ARW.connections.tokenFor(baseUrl); }
        catch { token = null; }
      }
      if (typeof opts.token === 'function') {
        try { token = await opts.token(); } catch { token = null; }
      }
      if (token) {
        this._mode = 'fetch';
        await this._connectFetch(url, token);
      } else {
        this._mode = 'eventsource';
        this._connectEventSource(url);
      }
    },
    _connectEventSource(url) {
      this._updateStatus('connecting');
      const es = new EventSource(url, { withCredentials: false });
      es.onopen = () => {
        this._connected = true;
        this._retryMs = 500;
        this._emit('*open*', {});
        this._updateStatus('open');
      };
      es.onerror = () => {
        this._connected = false;
        const cap = this._maxRetryMs;
        const ms = Math.min(this._retryMs, cap);
        const closing = this._closing;
        this._emit('*error*', {});
        this._updateStatus(closing ? 'closed' : 'error', closing ? {} : { retryIn: ms });
        if (!closing) {
          this._scheduleReconnect(ms);
          this._retryMs = Math.min(ms * 2, cap);
        }
      };
      es.onmessage = (ev) => {
        this._lastId = ev.lastEventId || this._lastId;
        let data = null;
        try { data = JSON.parse(ev.data); } catch { data = { raw: ev.data }; }
        const kind = data?.kind || 'unknown';
        this._last = data;
        this._lastRaw = ev.data;
        this._lastKind = kind;
        this._lastEventAt = Date.now();
        this._emit(kind, data);
      };
      this._es = es;
      this._wireOnlineReconnect();
    },
    async _connectFetch(url, token){
      this._updateStatus('connecting');
      const controller = new AbortController();
      this._abortController = controller;
      const headers = { 'Accept': 'text/event-stream', 'X-ARW-Admin': token };
      let response = null;
      try {
        response = await fetch(url, { headers, signal: controller.signal, credentials: 'omit' });
      } catch (err) {
        if (controller.signal.aborted) {
          this._updateStatus('closed');
          return;
        }
        this._handleFetchError(err);
        return;
      }
      if (!response || !response.ok || !response.body) {
        this._handleFetchError(new Error('SSE fetch failed'));
        return;
      }
      this._connected = true;
      this._retryMs = 500;
      this._emit('*open*', {});
      this._updateStatus('open');
      const reader = response.body.getReader();
      const decoder = new TextDecoder('utf-8');
      let buffer = '';
      const readLoop = async () => {
        try {
          while (true) {
            const { done, value } = await reader.read();
            if (done) break;
            buffer += decoder.decode(value, { stream: true });
            buffer = this._processBuffer(buffer);
          }
          // drain remainder
          if (buffer) {
            this._processBuffer(buffer + '\n\n');
          }
        } catch (err) {
          if (!controller.signal.aborted) {
            this._handleFetchError(err);
            return;
          }
        }
        if (!controller.signal.aborted) {
          this._handleFetchError(new Error('SSE stream ended'));
        } else {
          this._updateStatus('closed');
        }
      };
      readLoop();
      this._wireOnlineReconnect();
    },
    _processBuffer(buffer){
      let remaining = buffer;
      let idx = remaining.indexOf('\n\n');
      while (idx >= 0) {
        const chunk = remaining.slice(0, idx);
        remaining = remaining.slice(idx + 2);
        this._handleSseChunk(chunk);
        idx = remaining.indexOf('\n\n');
      }
      return remaining;
    },
    _handleSseChunk(chunk){
      const lines = chunk.split('\n');
      let dataLines = [];
      let eventName = null;
      let lastId = null;
      let retryMs = null;
      for (const rawLine of lines) {
        const line = rawLine.trimEnd();
        if (!line || line.startsWith(':')) continue;
        if (line.startsWith('data:')) {
          dataLines.push(line.slice(5).trimStart());
        } else if (line.startsWith('event:')) {
          eventName = line.slice(6).trimStart();
        } else if (line.startsWith('id:')) {
          lastId = line.slice(3).trimStart();
        } else if (line.startsWith('retry:')) {
          const parsed = Number(line.slice(6).trim());
          if (Number.isFinite(parsed) && parsed >= 0) {
            retryMs = parsed;
          }
        }
      }
      if (lastId) {
        this._lastId = lastId;
      }
      if (retryMs != null) {
        const cap = this._maxRetryMs;
        const clamped = Math.max(250, Math.min(retryMs, cap));
        this._retryMs = clamped;
      }
      const payloadRaw = dataLines.join('\n');
      if (!payloadRaw) return;
      let data = null;
      try { data = JSON.parse(payloadRaw); }
      catch { data = { raw: payloadRaw }; }
      const kind = eventName || data?.kind || 'unknown';
      this._last = data;
      this._lastRaw = payloadRaw;
      this._lastKind = kind;
      this._lastEventAt = Date.now();
      this._emit(kind, data);
    },
    _handleFetchError(err){
      console.warn('SSE fetch error', err?.message || err);
      this._connected = false;
      const cap = this._maxRetryMs;
      const ms = Math.min(this._retryMs, cap);
      const closing = this._closing;
      this._emit('*error*', { error: err });
      this._updateStatus(closing ? 'closed' : 'error', closing ? {} : { retryIn: ms });
      this._abortController = null;
      if (!closing) {
        this._scheduleReconnect(ms);
        const next = Math.min(ms * 2, cap);
        this._retryMs = Math.max(this._retryMs, next);
      }
    },
    _scheduleReconnect(ms){
      this._clearTimer();
      this._retryTimer = setTimeout(() => { try { this.reconnect(); } catch {} }, ms);
    },
    _wireOnlineReconnect(){
      try {
        window.removeEventListener('online', this._onlineOnce);
      } catch {}
      this._onlineOnce = () => { try { this.reconnect(); } catch {} };
      try { window.addEventListener('online', this._onlineOnce, { once: true }); } catch {}
    },
    reconnect(){ if (this._base) this.connect(this._base, this._opts || {}, true); },
    close(){
      this._clearTimer();
      this._teardownEventSource();
      this._teardownFetch();
      this._closing=false;
      this._connected = false;
      this._updateStatus('closed');
    },
    indicator(target, opts = {}){
      const node = typeof target === 'string' ? document.getElementById(target) : target;
      if (!node) return { dispose(){} };
      const self = this;
      try{
        if (!node.dataset.indicator) node.dataset.indicator = 'sse';
        node.classList.add('badge');
        node.classList.add('sse-badge');
        if (typeof node.getAttribute !== 'function' || node.getAttribute('role') == null) {
          node.setAttribute('role', 'status');
        }
        if (typeof node.getAttribute !== 'function' || node.getAttribute('aria-live') == null) {
          node.setAttribute('aria-live', 'polite');
        }
      }catch{}
      const labels = Object.assign({
        open: 'Connected',
        stale: 'Connected',
        connecting: 'Connecting…',
        idle: 'Idle',
        error: 'Retrying…',
        closed: 'Offline',
      }, opts.labels || {});
      const prefix = opts.prefix === undefined ? (node.dataset.ssePrefix ?? 'SSE') : opts.prefix;
      const renderOpt = typeof opts.render === 'function' ? opts.render : null;
      const formatMs = (ms) => {
        if (!Number.isFinite(ms) || ms <= 0) return '';
        if (ms < 1000) return `${Math.round(ms)}ms`;
        if (ms < 2000) return `${(ms / 1000).toFixed(2)}s`;
        if (ms < 5000) return `${(ms / 1000).toFixed(1)}s`;
        if (ms < 60000) return `${Math.round(ms / 1000)}s`;
        const mins = Math.round(ms / 60000);
        return `${mins}m`;
      };
      const relativeTime = (timestamp) => {
        if (!Number.isFinite(timestamp)) return '';
        const diff = Date.now() - timestamp;
        if (diff < 0) return '';
        if (diff < 2000) return 'active now';
        if (diff < 60000) return `${Math.round(diff / 1000)}s ago`;
        if (diff < 3600000) return `${Math.round(diff / 60000)}m ago`;
        return `${Math.round(diff / 3600000)}h ago`;
      };
      const staleMsRaw = Number(opts.staleMs);
      const staleMs = Number.isFinite(staleMsRaw) && staleMsRaw > 0 ? staleMsRaw : 20000;
      const render = (status, info = {}) => {
        const now = Date.now();
        const last = self.lastEventAt();
        const age = Number.isFinite(last) ? now - last : null;
        const isStale = status === 'open' && staleMs && Number.isFinite(age) && age >= staleMs;
        const badgeState = isStale ? 'stale' : status;
        try{ node.dataset.state = badgeState; }catch{}
        if (renderOpt) { renderOpt(node, status, info, { labels, prefix, stale: isStale, age }); return; }
        const labelKey = (badgeState in labels) ? badgeState : status;
        const label = labels[labelKey] ?? labels.default ?? badgeState;
        const parts = [];
        if (prefix) parts.push(`${prefix}: ${label}`);
        else parts.push(label);
        let detail = '';
        if ((status === 'error' || status === 'connecting') && Number.isFinite(info.retryIn)) {
          detail = `retry in ${formatMs(info.retryIn)}`;
        } else if (status === 'open') {
          const rel = relativeTime(last);
          if (rel) detail = `last event ${rel}`;
        } else if (status === 'idle' && info.changedAt) {
          const rel = relativeTime(info.changedAt);
          if (rel) detail = `since ${rel}`;
        }
        if (isStale) {
          detail = detail ? `${detail} (stale)` : 'stale';
        }
        if (detail) parts.push(`· ${detail}`);
        const text = parts.join(' ');
        node.textContent = text;
        node.title = text;
        try { node.setAttribute('aria-label', text); } catch {}
      };
      const refreshMsRaw = Number(opts.refreshMs);
      const refreshMs = Number.isFinite(refreshMsRaw) && refreshMsRaw >= 500 ? refreshMsRaw : 5000;
      let lastStatus = this.status();
      let lastEnv = { status: lastStatus, changedAt: this._statusChangedAt };
      const tick = () => {
        render(lastStatus, lastEnv || {});
      };
      const subId = this.subscribe('*status*', ({ env }) => {
        lastStatus = env?.status || 'idle';
        lastEnv = env || {};
        tick();
      });
      tick();
      const timer = setInterval(() => {
        try { tick(); } catch {}
      }, refreshMs);
      return { dispose(){
        try { clearInterval(timer); } catch {}
        self.unsubscribe(subId);
      } };
    },
    status(){
      try{ if (document && document.body) document.body.setAttribute('data-sse-status', this._status); }catch{}
      return this._status;
    },
    last(){ return { kind: this._lastKind, data: this._last, raw: this._lastRaw, at: this._lastEventAt }; },
    lastEventAt(){ return this._lastEventAt; },
    statusChangedAt(){ return this._statusChangedAt; },
    subscribe(filter, cb) {
      const id = this._nextId++;
      this._subs.set(id, { filter, cb });
      return id;
    },
    unsubscribe(id) { this._subs.delete(id); },
    _emit(kind, env) {
      for (const { filter, cb } of this._subs.values()) {
        try {
          if (filter === '*' || (typeof filter === 'string' && kind.startsWith(filter)) || (typeof filter === 'function' && filter(kind, env))) {
            cb({ kind, env });
          }
        } catch {}
      }
    }
  },
  // SLO preference helper (p95 threshold)
  async slo(){ try{ const p = await this.getPrefs('launcher')||{}; return Number(p.sloP95)||150; }catch{ return 150 } },
  async setSlo(v){ try{ const p = await this.getPrefs('launcher')||{}; p.sloP95 = Number(v)||150; await this.setPrefs('launcher', p); this.toast('SLO set to '+p.sloP95+' ms'); }catch(e){ console.error(e); } },
  // Sidecar UI module is registered via sidecar.js
  sidecar: null,
};

try {
  if (typeof window !== 'undefined' && typeof window.dispatchEvent === 'function') {
    const detail = window.ARW;
    try {
      window.dispatchEvent(new CustomEvent('arw:ready', { detail }));
    } catch {
      window.dispatchEvent(new Event('arw:ready'));
    }
  }
} catch {}

// Read‑model store: maintain local snapshots via RFC6902 patches from SSE
// Payload shape from SSE: { id, patch: [ {op, path, value?} ... ] }
window.ARW.read = {
  _store: new Map(),
  _subs: new Map(),
  _next: 1,
  get(id){ return this._store.get(id); },
  subscribe(id, cb){ const k = this._next++; this._subs.set(k, { id, cb }); return k; },
  unsubscribe(k){ this._subs.delete(k); },
  _emit(id){ for (const {id: iid, cb} of this._subs.values()) if (iid===id) { try{ cb(this._store.get(id)); }catch{} } },
  _applyPointer(obj, path){
    // returns [parent, key] for a JSON pointer path, creating objects/arrays as needed for add
    if (path === '/' || path === '') return [ { '': obj }, '' ];
    const parts = path.split('/').slice(1).map(p=> p.replace(/~1/g,'/').replace(/~0/g,'~'));
    let parent = null, key = null, cur = obj;
    for (let i=0;i<parts.length;i++){
      key = parts[i];
      if (i === parts.length - 1) { parent = cur; break; }
      if (Array.isArray(cur)) {
        const idx = key === '-' ? cur.length : parseInt(key, 10);
        if (!Number.isFinite(idx)) return [null, null];
        if (!cur[idx]) cur[idx] = {};
        cur = cur[idx];
      } else if (cur && typeof cur === 'object') {
        if (!(key in cur)) cur[key] = {};
        cur = cur[key];
      } else {
        return [null, null];
      }
    }
    return [parent, key];
  },
  _applyOp(target, op){
    const { op: kind, path } = op;
    if (!path) return;
    if (kind === 'test') return; // ignored for now
    if (kind === 'copy' || kind === 'move') {
      // basic move/copy support
      const from = op.from;
      const [fp, fk] = this._applyPointer(target, from);
      if (!fp) return;
      let val;
      if (Array.isArray(fp)) val = fp[Number(fk)]; else val = fp[fk];
      if (kind === 'move') {
        if (Array.isArray(fp)) fp.splice(Number(fk),1); else delete fp[fk];
      }
      const [tp, tk] = this._applyPointer(target, path);
      if (!tp) return;
      if (Array.isArray(tp)) {
        const idx = tk === '-' ? tp.length : parseInt(tk,10);
        tp.splice(idx, 0, val);
      } else { tp[tk] = val; }
      return;
    }
    const [p, k] = this._applyPointer(target, path);
    if (!p) return;
    if (kind === 'add') {
      if (Array.isArray(p)) {
        const idx = k === '-' ? p.length : parseInt(k,10);
        p.splice(idx, 0, op.value);
      } else { p[k] = op.value; }
    } else if (kind === 'replace') {
      if (Array.isArray(p)) p[parseInt(k,10)] = op.value; else p[k] = op.value;
    } else if (kind === 'remove') {
      if (Array.isArray(p)) p.splice(parseInt(k,10),1); else delete p[k];
    }
  }
};

// Attach SSE patch listener
window.ARW.sse.subscribe('state.read.model.patch', ({ env }) => {
  try {
    const id = env?.id || env?.payload?.id;
    const patch = env?.patch || env?.payload?.patch;
    if (!id || !Array.isArray(patch)) return;
    const cur = ARW.read._store.get(id) || {};
    for (const op of patch) ARW.read._applyOp(cur, op);
    ARW.read._store.set(id, cur);
    ARW.read._emit(id);
  } catch {}
});

// Command Palette (Ctrl/Cmd-K)
  window.ARW.palette = {
  _wrap: null,
  _input: null,
  _list: null,
  _items: [],
  _actions: [],
  _active: -1,
  _prevFocus: null,
  _render: null,
  _optionSeq: 0,
  _trap: null,
  mount(opts={}){
    if (this._wrap) return; // singleton
    const wrap = document.createElement('div'); wrap.className='palette-wrap';
    wrap.style.display = 'none';
    wrap.setAttribute('aria-hidden','true');
    const pal = document.createElement('div'); pal.className='palette'; pal.setAttribute('role','dialog'); pal.setAttribute('aria-modal','true'); pal.setAttribute('aria-label','Command palette'); wrap.appendChild(pal);
    const header = document.createElement('header');
    const inp = document.createElement('input'); inp.placeholder = 'Search commands…'; inp.setAttribute('aria-label','Search commands'); inp.setAttribute('role','combobox'); header.appendChild(inp);
    pal.appendChild(header);
    const ul = document.createElement('ul'); ul.setAttribute('role','listbox'); const listId = 'arw-palette-listbox'; ul.id = listId; pal.appendChild(ul);
    inp.setAttribute('aria-controls', listId);
    inp.setAttribute('aria-expanded', 'false');
    document.body.appendChild(wrap);
    this._wrap = wrap; this._input = inp; this._list = ul;
    const base = opts.base;
    const emitMascotConfig = async (profile = 'global', overrides = {}) => {
      try {
        const prefs = await ARW.getPrefs('mascot') || {};
        const base = {
          allowInteractions: !(prefs.clickThrough ?? true),
          intensity: prefs.intensity || 'normal',
          snapWindows: prefs.snapWindows !== false,
          quietMode: !!prefs.quietMode,
          compactMode: !!prefs.compactMode,
          character: prefs.character || 'guide',
        };
        const profilePrefs = profile !== 'global'
          && prefs.profiles
          && typeof prefs.profiles === 'object'
          ? prefs.profiles[profile] || {}
          : {};
        if (window.__TAURI__?.event?.emit) {
          await window.__TAURI__.event.emit('mascot:config', {
            profile,
            ...base,
            ...profilePrefs,
            ...overrides,
          });
        }
      } catch (err) {
        console.error(err);
      }
    };
    const characterOrder = ['guide','engineer','researcher','navigator','guardian'];
    this._actions = [
      { id:'open:hub', label:'Open Projects workspace', hint:'window', run:()=> ARW.invoke('open_hub_window') },
      { id:'open:chat', label:'Open Conversations workspace', hint:'window', run:()=> ARW.invoke('open_chat_window') },
      { id:'open:training', label:'Open Training runs', hint:'window', run:()=> ARW.invoke('open_training_window') },
      { id:'open:debug', label:'Open Debug (Window)', hint:'window', run:()=> ARW.invoke('open_debug_window', { port: ARW.getPortFromInput('port') }) },
      { id:'open:events', label:'Open Events Window', hint:'window', run:()=> ARW.invoke('open_events_window') },
      { id:'open:docs', label:'Open Docs Website', hint:'web', run:()=> ARW.invoke('open_url', { url: 'https://t3hw00t.github.io/ARW/' }) },
      { id:'mascot:show', label:'Show Mascot Overlay', hint:'window', run: async ()=> { try { await ARW.invoke('open_mascot_window', { profile: 'global' }); await emitMascotConfig('global'); } catch(e){ console.error(e); } } },
      { id:'mascot:toggle-interactions', label:'Toggle Mascot Interactions (Ctrl/⌘+D)', hint:'action', run: async ()=> { try { document.dispatchEvent(new KeyboardEvent('keydown', { key: 'd', ctrlKey: !navigator.platform.includes('Mac'), metaKey: navigator.platform.includes('Mac') })); } catch(e){ console.error(e); } } },
      { id:'mascot:dock-left', label:'Dock Mascot Left', hint:'window', run:()=> ARW.invoke('position_window', { label:'mascot', anchor:'left', margin: 12 }) },
      { id:'mascot:dock-right', label:'Dock Mascot Right', hint:'window', run:()=> ARW.invoke('position_window', { label:'mascot', anchor:'right', margin: 12 }) },
      { id:'mascot:dock-bottom-right', label:'Dock Mascot Bottom‑Right', hint:'window', run:()=> ARW.invoke('position_window', { label:'mascot', anchor:'bottom-right', margin: 12 }) },
      { id:'mascot:toggle-quiet', label:'Toggle Mascot Quiet Mode', hint:'action', run: async ()=>{
          try {
            const prefs = await ARW.getPrefs('mascot') || {};
            prefs.quietMode = !(prefs.quietMode ?? false);
            await ARW.setPrefs('mascot', prefs);
            await emitMascotConfig();
            ARW.toast(`Mascot quiet mode ${prefs.quietMode ? 'on' : 'off'}`);
          } catch (err) {
            console.error(err);
          }
        }
      },
      { id:'mascot:toggle-compact', label:'Toggle Mascot Compact Mode', hint:'action', run: async ()=>{
          try {
            const prefs = await ARW.getPrefs('mascot') || {};
            prefs.compactMode = !(prefs.compactMode ?? false);
            await ARW.setPrefs('mascot', prefs);
            await emitMascotConfig();
            ARW.toast(`Mascot compact mode ${prefs.compactMode ? 'on' : 'off'}`);
          } catch (err) {
            console.error(err);
          }
        }
      },
      { id:'mascot:cycle-character', label:'Cycle Mascot Character', hint:'action', run: async ()=>{
          try {
            const prefs = await ARW.getPrefs('mascot') || {};
            const current = prefs.character && characterOrder.includes(prefs.character) ? prefs.character : 'guide';
            const next = characterOrder[(characterOrder.indexOf(current) + 1) % characterOrder.length];
            prefs.character = next;
            await ARW.setPrefs('mascot', prefs);
            await emitMascotConfig('global');
            ARW.toast(`Mascot character: ${next}`);
          } catch (err) {
            console.error(err);
          }
        }
      },
      ...characterOrder.map((value) => ({
        id: `mascot:character:${value}`,
        label: `Set Mascot Character: ${value.charAt(0).toUpperCase()}${value.slice(1)}`,
        hint: 'action',
        run: async ()=>{
          try {
            const prefs = await ARW.getPrefs('mascot') || {};
            prefs.character = value;
            await ARW.setPrefs('mascot', prefs);
            await emitMascotConfig('global');
            ARW.toast(`Mascot character: ${value}`);
          } catch (err) {
            console.error(err);
          }
        }
      })),
      { id:'mascot:open-project', label:'Open Mascot for Project…', hint:'window', run: async ()=>{
          try {
            const result = await ARW.modal.form({
              title: 'Open Project Mascot',
              description: 'Spawn a dedicated mascot for a project or workspace.',
              submitLabel: 'Open',
              fields: [
                { name:'name', label:'Project name', placeholder:'Project Alpha', required: true },
                { name:'character', label:'Character', type:'select', value:'guide', options: characterOrder.map((value)=>({ value, label: value.charAt(0).toUpperCase()+value.slice(1) })) },
                { name:'quietMode', label:'Start in quiet mode', type:'checkbox', value:false },
                { name:'compactMode', label:'Start in compact mode', type:'checkbox', value:false },
                { name:'autoOpen', label:'Reopen automatically on launch', type:'checkbox', value:false },
              ],
            });
            if (!result) return;
            const rawName = String(result.name || '').trim();
            if (!rawName) return;
            const slug = rawName
              .toLowerCase()
              .replace(/[^a-z0-9]+/g, '-')
              .replace(/^-+|-+$/g, '')
              || 'project';
            const profile = `project:${slug}`;
            const label = `mascot-${slug}`;
            const overrides = {
              quietMode: !!result.quietMode,
              compactMode: !!result.compactMode,
              character: result.character || 'guide',
              name: rawName,
            };
            const prefs = await ARW.getPrefs('mascot') || {};
            if (typeof prefs.profiles !== 'object' || !prefs.profiles) prefs.profiles = {};
            prefs.profiles[profile] = {
              quietMode: overrides.quietMode,
              compactMode: overrides.compactMode,
              character: overrides.character,
               name: rawName,
               slug,
               autoOpen: !!result.autoOpen,
            };
            await ARW.setPrefs('mascot', prefs);
            await ARW.invoke('open_mascot_window', {
              label,
              profile,
              character: overrides.character,
              quiet: overrides.quietMode,
              compact: overrides.compactMode,
            });
            await emitMascotConfig(profile, overrides);
            ARW.toast(`Opened mascot for ${rawName}`);
          } catch (err) {
            console.error(err);
          }
        }
      },
      { id:'mascot:toggle-auto-open', label:'Toggle Project Mascot Auto-Reopen', hint:'action', run: async ()=>{
          try {
            const prefs = await ARW.getPrefs('mascot') || {};
            const profiles = prefs.profiles && typeof prefs.profiles === 'object' ? prefs.profiles : {};
            const entries = Object.entries(profiles);
            if (!entries.length) {
              ARW.toast('No project mascots saved yet');
              return;
            }
            const options = entries.map(([key, entry]) => ({
              value: key,
              label: entry?.name ? `${entry.name} (${key})` : key,
            }));
            const result = await ARW.modal.form({
              title: 'Toggle Auto Reopen',
              description: 'Select a mascot profile to toggle automatic reopening.',
              submitLabel: 'Toggle',
              fields: [
                { name: 'profile', label: 'Profile', type: 'select', value: options[0].value, options },
              ],
            });
            if (!result || !result.profile) return;
            const target = profiles[result.profile];
            if (!target) {
              ARW.toast('Profile not found');
              return;
            }
            target.autoOpen = !(target.autoOpen ?? false);
            await ARW.setPrefs('mascot', prefs);
            ARW.toast(`Auto reopen ${target.autoOpen ? 'enabled' : 'disabled'} for ${target.name || result.profile}`);
          } catch (err) {
            console.error(err);
          }
        }
      },
      { id:'models:refresh', label:'Refresh Models', hint:'action', run:()=> ARW.invoke('models_refresh', { port: ARW.getPortFromInput('port') }) },
          { id:'sse:replay', label:'Replay SSE (50)', hint:'sse', run:()=> {
              const meta = ARW.baseMeta(ARW.getPortFromInput('port'));
              ARW.sse.connect(meta.base, { replay: 50 });
            }
          },
      { id:'layout:focus', label:'Toggle Focus Mode', hint:'layout', run:()=> ARW.layout.toggle() },
      { id:'layout:density', label:'Toggle Compact Density', hint:'layout', run:()=> ARW.density.toggle() },
      { id:'copy:last', label:'Copy last event JSON', hint:'sse', run:()=> ARW.copy(JSON.stringify(ARW.sse._last||{}, null, 2)) },
      { id:'toggle:auto-ocr', label:'Toggle Auto OCR', hint:'pref', run: async ()=>{
          try{
            const prefs = await ARW.getPrefs('launcher') || {}; prefs.autoOcr = !prefs.autoOcr; await ARW.setPrefs('launcher', prefs);
            ARW.toast('Auto OCR: ' + (prefs.autoOcr? 'on':'off'));
            const el = document.getElementById('autoOcr'); if (el) el.checked = !!prefs.autoOcr;
          }catch(e){ console.error(e); }
        }
      },
      { id:'shot:capture', label:'Capture screen (preview)', hint:'screenshot', run: async ()=>{
          try{
            const port = ARW.toolPort();
            const out = await ARW.invoke('run_tool_admin', { id: 'ui.screenshot.capture', input: { scope:'screen', format:'png', downscale:640 }, port });
            ARW.toast(out && out.path ? ('Saved: ' + out.path) : 'Capture requested');
          }catch(e){ console.error(e); ARW.toastCaptureError(e, { scope: 'capture screen', lease: 'io:screenshot' }); }
        }
      },
      { id:'shot:capture-window', label:'Capture this window (preview)', hint:'screenshot', run: async ()=>{
          try{
            const bounds = await ARW.invoke('active_window_bounds', { label: null });
            const x = bounds?.x ?? 0, y = bounds?.y ?? 0, w = bounds?.w ?? 0, h = bounds?.h ?? 0;
            const scope = `region:${x},${y},${w},${h}`;
            const port = ARW.toolPort();
            const out = await ARW.invoke('run_tool_admin', { id: 'ui.screenshot.capture', input: { scope, format:'png', downscale:640 }, port });
            ARW.toast(out && out.path ? ('Saved: ' + out.path) : 'Capture requested');
          }catch(e){ console.error(e); ARW.toastCaptureError(e, { scope: 'capture window', lease: 'io:screenshot' }); }
        }
      },
      { id:'shot:capture-region', label:'Capture region (drag)', hint:'screenshot', run: async ()=>{ await ARW.region.captureAndSave(); } },
      { id:'gallery:open', label:'Open Screenshots Gallery', hint:'screenshot', run: ()=> ARW.gallery.show() },
      { id:'prefs:set-editor', label:'Set preferred editor…', hint:'pref', run: async ()=>{
          try{
            const cur = ((await ARW.getPrefs('launcher'))||{}).editorCmd || '';
            const result = await ARW.modal.form({
              title: 'Preferred editor command',
              description: 'Provide a shell command. Use {path} where the file path should be inserted.',
              submitLabel: 'Save command',
              focusField: 'command',
              fields: [
                {
                  name: 'command',
                  label: 'Command',
                  value: cur || 'code --goto {path}',
                  autocomplete: 'off',
                  hint: 'Example: code --goto {path}',
                  trim: true,
                },
              ],
            });
            if (!result) return;
            const next = String(result.command || '').trim();
            const prefs = (await ARW.getPrefs('launcher')) || {};
            if (next) {
              prefs.editorCmd = next;
              ARW.toast('Editor set');
            } else {
              delete prefs.editorCmd;
              ARW.toast('Editor cleared');
            }
            await ARW.setPrefs('launcher', prefs);
          }catch(e){ console.error(e); ARW.toast('Failed to save'); }
        }
      },
      { id:'training:show-guide', label:'Show Training Quick Start', hint:'help', run: async ()=>{
          const card = document.querySelector('.training-guide');
          if (!card){
            ARW.toast('Open Training runs to show the quick start.');
            return;
          }
          card.removeAttribute('hidden');
          try{ card.scrollIntoView({ behavior:'smooth', block:'start' }); }catch{}
          try{
            const prefs = (await ARW.getPrefs('launcher')) || {};
            if (prefs.hideTrainingGuide){
              delete prefs.hideTrainingGuide;
              await ARW.setPrefs('launcher', prefs);
            }
          }catch(err){
            console.error('restore training guide failed', err);
          }
          ARW.toast('Training quick start shown.');
        }
      },
      { id:'trial:show-guide', label:'Show Experiment Checklist', hint:'help', run: async ()=>{
          const card = document.querySelector('.trial-guide');
          if (!card){
            ARW.toast('Open Experiment control to show the checklist.');
            return;
          }
          card.removeAttribute('hidden');
          try{ card.scrollIntoView({ behavior:'smooth', block:'start' }); }catch{}
          try{
            const prefs = (await ARW.getPrefs('launcher')) || {};
            if (prefs.hideTrialGuide){
              delete prefs.hideTrialGuide;
              await ARW.setPrefs('launcher', prefs);
            }
          }catch(err){
            console.error('restore trial guide failed', err);
          }
          ARW.toast('Experiment checklist shown.');
        }
      },
      { id:'theme:auto', label:'Theme: Auto (OS)', hint:'theme', run:()=> ARW.theme.set('auto') },
      { id:'theme:light', label:'Theme: Light', hint:'theme', run:()=> ARW.theme.set('light') },
      { id:'theme:dark', label:'Theme: Dark', hint:'theme', run:()=> ARW.theme.set('dark') },
      { id:'ui:reset', label:'Reset UI (Theme/Density/Focus)', hint:'layout', run:()=>{
          try{
            // Theme → auto
            localStorage.removeItem(ARW.theme.KEY); ARW.theme.apply('auto');
            // Density → normal (clear page-specific key)
            localStorage.removeItem(ARW.density._k()); ARW.density.apply('normal');
            // Focus → off
            localStorage.removeItem(ARW.layout._k()); ARW.layout.apply(false);
            ARW.ui?.badges?.update(); ARW.toast('UI reset');
          }catch(e){ ARW.toast('Reset failed'); }
        }
      },
    ];
    const trapFocus = (event)=>{
      if (event.key !== 'Tab' || !this._wrap || this._wrap.style.display !== 'grid') return;
      if (!pal.contains(event.target)) return;
      const focusables = Array.from(pal.querySelectorAll('input, button, [tabindex]:not([tabindex="-1"])')).filter(el => !el.hasAttribute('disabled'));
      if (!focusables.length) {
        event.preventDefault();
        return;
      }
      const first = focusables[0];
      const last = focusables[focusables.length - 1];
      if (event.shiftKey) {
        if (document.activeElement === first) {
          event.preventDefault();
          last.focus();
        }
      } else {
        if (document.activeElement === last) {
          event.preventDefault();
          first.focus();
        }
      }
    };
    wrap.addEventListener('keydown', trapFocus);
    this._trap = trapFocus;
    const render = (q='')=>{
      ul.innerHTML=''; this._items = [];
      const qq = q.toLowerCase();
      for (const a of this._actions) {
        if (!qq || a.label.toLowerCase().includes(qq) || a.id.includes(qq)) {
          const li = document.createElement('li'); li.dataset.id = a.id; li.setAttribute('role','option'); li.setAttribute('aria-selected','false'); li.tabIndex = -1;
          const optId = `palette-opt-${++this._optionSeq}`;
          li.id = optId;
          li.innerHTML = `<span>${a.label}</span><span class="hint">${a.hint}</span>`;
          li.addEventListener('click', ()=>{ this.hide(); try{ a.run(); }catch{} });
          ul.appendChild(li); this._items.push(li);
        }
      }
      this._active = this._items.length ? 0 : -1;
      this._input.setAttribute('aria-expanded', this._items.length ? 'true' : 'false');
      this._highlight();
    };
    this._render = render;
    inp.addEventListener('input', ()=> render(inp.value));
    inp.addEventListener('keydown', (e)=>{
      if (e.key==='ArrowDown'){ this._move(1); e.preventDefault(); }
      else if (e.key==='ArrowUp'){ this._move(-1); e.preventDefault(); }
      else if (e.key==='Enter'){ if (this._active>=0) { const id = this._items[this._active].dataset.id; const act = this._actions.find(a=>a.id===id); this.hide(); try{ act?.run(); }catch{} } }
      else if (e.key==='Escape'){ this.hide(); }
    });
    wrap.addEventListener('click', (e)=>{ if (e.target===wrap) this.hide(); });
    window.addEventListener('keydown', (e)=>{
      const mod = e.metaKey || e.ctrlKey;
      if (mod && e.key.toLowerCase()==='k'){ this.toggle(); e.preventDefault(); }
    });
    render('');
  },
  _move(dir){ if (!this._items.length) return; this._active = (this._active + dir + this._items.length) % this._items.length; this._highlight(); },
  _highlight(){
    let activeId = '';
    this._items.forEach((el,i)=> {
      const on = i===this._active;
      el.classList.toggle('active', on);
      el.setAttribute('aria-selected', on? 'true':'false');
      if (on && el.id) activeId = el.id;
    });
    if (this._list) {
      if (activeId) this._list.setAttribute('aria-activedescendant', activeId);
      else this._list.removeAttribute('aria-activedescendant');
    }
  },
  show(){
    if (!this._wrap) return;
    const activeEl = document.activeElement;
    this._prevFocus = activeEl && typeof activeEl.focus === 'function' ? activeEl : null;
    if (this._render) this._render('');
    this._wrap.style.display='grid';
    this._wrap.removeAttribute('aria-hidden');
    if (this._input){
      this._input.value='';
      this._input.focus({ preventScroll: true });
    }
  },
  hide(){
    if (!this._wrap) return;
    this._wrap.style.display='none';
    this._wrap.setAttribute('aria-hidden','true');
    if (this._input){
      this._input.setAttribute('aria-expanded','false');
      this._input.blur();
    }
    if (this._list) this._list.removeAttribute('aria-activedescendant');
    const prev = this._prevFocus;
    this._prevFocus = null;
    if (prev && document.contains(prev)){
      try{ prev.focus({ preventScroll: true }); }
      catch{ try{ prev.focus(); }catch{} }
    }
  },
  toggle(){ if (!this._wrap) return; const shown = this._wrap.style.display==='grid'; if (shown) this.hide(); else this.show(); }
};

// Screenshots gallery
window.ARW.gallery = {
  _wrap: null,
  _items: [],
  add(ev){
    try{
      const p = ev?.env?.payload || ev?.env || ev;
      const time = ev?.env?.time || new Date().toISOString();
      if (!p || !p.path) return;
      // Deduplicate by path (keep most recent)
      const idx = this._items.findIndex(it => it.path === p.path);
      if (idx >= 0) this._items.splice(idx, 1);
      this._items.unshift({ time, path: p.path, preview_b64: p.preview_b64 || null });
      if (this._items.length > 60) this._items.pop();
    }catch{}
  },
  mount(){
    if (this._wrap) return; const w=document.createElement('div'); w.className='gallery-wrap'; const g=document.createElement('div'); g.className='gallery'; g.setAttribute('role','dialog'); g.setAttribute('aria-modal','true'); g.setAttribute('aria-label','Screenshots gallery');
    const h=document.createElement('header'); const title=document.createElement('strong'); title.id='galleryTitle'; title.textContent='Screenshots'; g.setAttribute('aria-labelledby','galleryTitle'); const close=document.createElement('button'); close.className='ghost'; close.textContent='Close'; close.addEventListener('click', ()=> this.hide()); h.appendChild(title); h.appendChild(close);
    const m=document.createElement('main'); const grid=document.createElement('div'); grid.className='grid-thumbs'; m.appendChild(grid);
    g.appendChild(h); g.appendChild(m); w.appendChild(g); document.body.appendChild(w); this._wrap=w;
    // click-out close
    w.addEventListener('click', (e)=>{ if (e.target===w) this.hide(); });
  },
  render(){ if (!this._wrap) this.mount(); const grid=this._wrap.querySelector('.grid-thumbs'); if (!grid) return; grid.innerHTML='';
    for (const it of this._items){ const d=document.createElement('div'); d.className='thumb'; const img=document.createElement('img'); if (it.preview_b64) img.src=it.preview_b64; img.dataset.screenshotPath = it.path; img.alt=ARW._bestAltForPath(it.path, it.path); const meta=document.createElement('div'); meta.className='dim mono'; meta.textContent = `${it.time} ${it.path}`; const row=document.createElement('div'); row.className='row'; const open=document.createElement('button'); open.className='ghost'; open.textContent='Open'; open.addEventListener('click', async ()=>{ try{ await ARW.invoke('open_path', { path: it.path }); }catch(e){ console.error(e); } }); const copy=document.createElement('button'); copy.className='ghost'; copy.textContent='Copy path'; copy.addEventListener('click', ()=> ARW.copy(it.path)); const md=document.createElement('button'); md.className='ghost'; md.textContent='Copy MD'; md.addEventListener('click', ()=> ARW.copyMarkdown(it.path)); const save=document.createElement('button'); save.className='ghost'; save.textContent='Save to project'; save.addEventListener('click', async ()=>{ const res = await ARW.saveToProjectPrompt(it.path); if (res) await ARW.maybeAppendToNotes(res.proj, res.dest, it.path); }); const ann=document.createElement('button'); ann.className='ghost'; ann.textContent='Annotate'; ann.addEventListener('click', async ()=>{ try{ if (it.preview_b64){ const rects = await ARW.annot.start(it.preview_b64); const res = await ARW.invoke('run_tool_admin', { id: 'ui.screenshot.annotate_burn', input: { path: it.path, annotate: rects, downscale:640 }, port: ARW.toolPort() }); if (res && res.preview_b64){ img.src = res.preview_b64; meta.textContent = `${it.time} ${res.path||''}`; it.path = res.path||it.path; it.preview_b64 = res.preview_b64||it.preview_b64; img.dataset.screenshotPath = it.path; ARW._updateAltForPath(it.path); } } else { ARW.toast('No preview for annotate'); } }catch(e){ console.error(e); } }); row.appendChild(open); row.appendChild(copy); row.appendChild(md); row.appendChild(save); row.appendChild(ann); d.appendChild(img); d.appendChild(meta); d.appendChild(row); grid.appendChild(d); ARW._updateAltForPath(it.path); }
  },
  show(){ if (!this._wrap) this.mount(); this.render(); this._wrap.style.display='grid'; try{ const btn=this._wrap.querySelector('header button'); btn?.focus({ preventScroll:true }); }catch{} },
  hide(){ if (this._wrap) this._wrap.style.display='none'; }
};

// Subscribe gallery to screenshots events
ARW.sse.subscribe((k)=> k.startsWith('screenshots.'), (ev)=> ARW.gallery.add(ev));

// UI badges (Theme/Density)
window.ARW.ui = window.ARW.ui || {};
window.ARW.ui.badges = {
  _el: null,
  mount(){
    if (this._el) return; const el = document.getElementById('statusBadges'); if (!el) return; this._el = el; this.update();
  },
  update(){ if (!this._el) return; const curTheme = (localStorage.getItem(ARW.theme.KEY)||'auto'); const curDen = (localStorage.getItem(ARW.density.KEY)||'normal');
    this._el.innerHTML = '';
    // Theme badge with select
    const b1 = document.createElement('span'); b1.className='badge'; b1.title = 'Theme override (device-wide). Auto follows OS setting.';
    const s1 = document.createElement('select');
    for (const [v,l] of [['auto','Auto (OS)'],['light','Light'],['dark','Dark']]){ const o=document.createElement('option'); o.value=v; o.textContent=l; if (v===curTheme) o.selected=true; s1.appendChild(o); }
    s1.addEventListener('change', ()=> ARW.theme.set(s1.value));
    const t1 = document.createElement('span'); t1.textContent='Theme:'; t1.style.marginRight='6px'; b1.appendChild(t1); b1.appendChild(s1);
    // Density badge with select (per page)
    const b2 = document.createElement('span'); b2.className='badge'; b2.title = 'Density (per page). Compact reduces spacing and radii.';
    const s2 = document.createElement('select');
    const curD = ARW.density.get();
    for (const [v,l] of [['normal','Normal'],['compact','Compact']]){ const o=document.createElement('option'); o.value=v; o.textContent=l; if (v===curD) o.selected=true; s2.appendChild(o); }
    s2.addEventListener('change', ()=> ARW.density.set(s2.value));
    const t2 = document.createElement('span'); t2.textContent='Density:'; t2.style.marginRight='6px'; b2.appendChild(t2); b2.appendChild(s2);
    this._el.appendChild(b1); this._el.appendChild(b2);
  }
};

window.ARW.mode = {
  _modes: ['guided', 'expert'],
  current: 'guided',
  _listeners: new Set(),
  _toggleEl: null,
  _boundToggle: null,
  async init() {
    if (this._initialized) {
      this._apply();
      return;
    }
    this._initialized = true;
    try {
      const prefs = (await ARW.getPrefs('launcher')) || {};
      const stored = typeof prefs.mode === 'string' ? prefs.mode.toLowerCase().trim() : '';
      if (this._modes.includes(stored)) {
        this.current = stored;
      }
    } catch (err) {
      console.error(err);
    }
    this._apply();
  },
  _apply() {
    try {
      const body = document.body;
      if (body) {
        body.dataset.uiMode = this.current;
      }
    } catch (err) {
      console.error(err);
    }
    this._syncToggle();
    this._notify();
  },
  async set(mode) {
    if (!this._modes.includes(mode)) return;
    if (this.current !== mode) {
      this.current = mode;
    }
    this._apply();
    try {
      const prefs = (await ARW.getPrefs('launcher')) || {};
      if (prefs.mode !== this.current) {
        prefs.mode = this.current;
        await ARW.setPrefs('launcher', prefs);
      }
    } catch (err) {
      console.error(err);
    }
  },
  toggle() {
    const next = this.current === 'expert' ? 'guided' : 'expert';
    void this.set(next);
  },
  registerToggle(el) {
    if (!el) return;
    if (!this._boundToggle) {
      this._boundToggle = () => this.toggle();
    }
    if (this._toggleEl && this._toggleEl !== el) {
      this._toggleEl.removeEventListener('click', this._boundToggle);
    }
    this._toggleEl = el;
    el.removeEventListener('click', this._boundToggle);
    el.addEventListener('click', this._boundToggle);
    this._syncToggle();
  },
  _syncToggle() {
    const el = this._toggleEl;
    if (!el) return;
    const isExpert = this.current === 'expert';
    const label = isExpert ? 'Mode: Expert' : 'Mode: Guided';
    const hint = isExpert ? 'Switch to Guided mode (simpler panels)' : 'Switch to Expert mode (show advanced defaults)';
    el.textContent = label;
    el.setAttribute('title', hint);
    el.setAttribute('aria-label', hint);
  },
  subscribe(callback) {
    if (typeof callback !== 'function') return () => {};
    this._listeners.add(callback);
    return () => {
      this._listeners.delete(callback);
    };
  },
  _notify() {
    for (const handler of this._listeners) {
      try {
        handler(this.current);
      } catch (err) {
        console.error(err);
      }
    }
  }
};

window.ARW.nav = {
  groups: [
    {
      label: 'Welcome',
      items: [
        { id: 'index', href: 'index.html', label: 'Home', desc: 'Set up & launch' },
      ],
    },
    {
      label: 'Workspaces',
      items: [
        { id: 'hub', href: 'hub.html', label: 'Projects', desc: 'Organize files & context' },
        { id: 'chat', href: 'chat.html', label: 'Conversations', desc: 'Chat with assistants' },
      ],
    },
    {
      label: 'Automation',
      items: [
        { id: 'training', href: 'training.html', label: 'Training Runs', desc: 'Tune & evaluate models', mode: 'expert' },
        { id: 'trial', href: 'trial.html', label: 'Experiment Control', desc: 'Coordinate staged trials', mode: 'expert' },
      ],
    },
    {
      label: 'Monitoring',
      items: [
        { id: 'events', href: 'events.html', label: 'Live Events', desc: 'Stream telemetry & activity', mode: 'expert' },
        { id: 'logs', href: 'logs.html', label: 'Logs', desc: 'Inspect service output', mode: 'expert' },
        { id: 'models', href: 'models.html', label: 'Model Registry', desc: 'Manage runtimes', mode: 'expert' },
        { id: 'connections', href: 'connections.html', label: 'Connections', desc: 'Remote bases & sharing' },
      ],
    },
  ],
  ensure(){
    try{
      const body = document.body;
      if (!body || body.dataset.globalNav === 'on') return;
      const current = ARW.util?.pageId ? ARW.util.pageId() : 'index';
      const header = document.createElement('header');
      header.className = 'global-bar';
      header.setAttribute('role','banner');
      const inner = document.createElement('div');
      inner.className = 'global-bar__inner';
      header.appendChild(inner);

      const brand = document.createElement('a');
      brand.className = 'global-bar__brand';
      brand.href = 'index.html';
      brand.setAttribute('aria-label', 'Agent Hub launcher');
      brand.innerHTML = '<span class="global-bar__brand-title">Agent Hub</span><span class="global-bar__brand-tag">ARW</span>';
      inner.appendChild(brand);

      const nav = document.createElement('nav');
      nav.className = 'global-bar__nav';
      nav.setAttribute('aria-label', 'Primary navigation');

      const makeLink = (item) => {
        const link = document.createElement('a');
        link.className = 'global-bar__link';
        link.href = item.href;
        link.dataset.page = item.id;
        if (item.mode === 'expert') {
          link.dataset.mode = 'expert-only';
        } else if (item.mode === 'guided') {
          link.dataset.mode = 'guided-only';
        }
        const title = document.createElement('span');
        title.textContent = item.label;
        link.appendChild(title);
        if (item.desc) {
          const hint = document.createElement('small');
          hint.textContent = item.desc;
          link.appendChild(hint);
          link.title = item.desc;
        }
        if (item.id === current) {
          link.classList.add('is-active');
          link.setAttribute('aria-current', 'page');
        }
        return link;
      };

      for (const group of this.groups) {
        const groupWrap = document.createElement('div');
        groupWrap.className = 'global-bar__group';
        const label = document.createElement('span');
        label.className = 'global-bar__group-label';
        label.textContent = group.label;
        groupWrap.appendChild(label);
        const linkWrap = document.createElement('div');
        linkWrap.className = 'global-bar__group-links';
        const modeSet = new Set();
        for (const item of group.items) {
          const link = makeLink(item);
          linkWrap.appendChild(link);
          const itemMode = item.mode === 'expert' || item.mode === 'guided' ? item.mode : 'any';
          modeSet.add(itemMode);
        }
        if (modeSet.size === 1) {
          const only = modeSet.values().next().value;
          if (only === 'expert') {
            groupWrap.dataset.mode = 'expert-only';
          } else if (only === 'guided') {
            groupWrap.dataset.mode = 'guided-only';
          }
        }
        groupWrap.appendChild(linkWrap);
        nav.appendChild(groupWrap);
      }

      inner.appendChild(nav);
      const controls = document.createElement('div');
      controls.className = 'global-bar__controls';
      const modeBtn = document.createElement('button');
      modeBtn.type = 'button';
      modeBtn.className = 'global-bar__mode';
      modeBtn.setAttribute('data-mode-toggle', 'true');
      modeBtn.textContent = 'Mode: Guided';
      controls.appendChild(modeBtn);
      inner.appendChild(controls);
      if (ARW.mode && typeof ARW.mode.registerToggle === 'function') {
        ARW.mode.registerToggle(modeBtn);
      }
      const skip = body.querySelector('.skip-link');
      if (skip && skip.parentElement === body) {
        body.insertBefore(header, skip.nextSibling);
      } else {
        body.insertBefore(header, body.firstChild);
      }
      body.dataset.globalNav = 'on';
      body.classList.add('has-global-bar');
    }catch(e){ console.error(e); }
  }
};

// Apply theme/density on load and mount badges
document.addEventListener('DOMContentLoaded', ()=>{ try{ ARW.mode.init(); }catch{} try{ ARW.nav.ensure(); }catch{} try{ ARW.theme.init(); }catch{} try{ ARW.density.init(); }catch{} try{ ARW.layout.init(); }catch{} try{ ARW.ui.badges.mount(); }catch{} });
// Universal ESC closes overlays (palette/gallery/shortcuts/annot)
window.addEventListener('keydown', (e)=>{
  if (e.key !== 'Escape') return;
  let closed = false;
  try{ if (ARW.palette && ARW.palette._wrap && ARW.palette._wrap.style.display==='grid'){ ARW.palette.hide(); closed = true; } }catch{}
  try{ if (ARW.gallery && ARW.gallery._wrap && ARW.gallery._wrap.style.display && ARW.gallery._wrap.style.display!=='none'){ ARW.gallery.hide(); closed = true; } }catch{}
  try{ if (ARW.shortcuts && ARW.shortcuts._wrap && ARW.shortcuts._wrap.style.display && ARW.shortcuts._wrap.style.display!=='none'){ ARW.shortcuts.hide(); closed = true; } }catch{}
  try{ if (ARW.annot && ARW.annot._wrap && ARW.annot._wrap.style.display && ARW.annot._wrap.style.display!=='none'){ ARW.annot.hide(); closed = true; } }catch{}
  if (closed){ try{ e.preventDefault(); e.stopPropagation(); }catch{} }
});

  // Region capture (drag overlay)
  window.ARW.region = {
  _wrap: null,
  _rect: null,
  _onUp: null,
  mount(){
    if (this._wrap) return;
    const w = document.createElement('div'); w.className='region-wrap';
    const dim = document.createElement('div'); dim.className='region-dim'; w.appendChild(dim);
    const hint = document.createElement('div'); hint.className='region-hint'; hint.textContent='Drag to capture region — Esc to cancel'; w.appendChild(hint);
    const rect = document.createElement('div'); rect.className='region-rect'; w.appendChild(rect);
    document.body.appendChild(w); this._wrap = w; this._rect = rect;
  },
  start(){
    this.mount();
    this._wrap.style.display='block';
    let sx=0, sy=0, ex=0, ey=0; let active=false;
    const rect = this._rect; rect.style.display='none';
    const px = (n)=> Math.floor(n);
    const onMouseDown = (e)=>{ active=true; sx=e.clientX; sy=e.clientY; rect.style.display='block'; update(e); };
    const onMouseMove = (e)=>{ if (!active) return; update(e); };
    const onMouseUp = (e)=>{ if (!active) return; active=false; cleanup(); const r=this._calc(sx,sy,e.clientX,e.clientY); if (r.w>2 && r.h>2) { this._resolve(r); } else { this._reject('empty'); } };
    const onKey = (e)=>{ if (e.key==='Escape'){ cleanup(); this._reject('cancel'); } };
    const update = (e)=>{ ex=e.clientX; ey=e.clientY; const r=this._calc(sx,sy,ex,ey); rect.style.left=r.x+'px'; rect.style.top=r.y+'px'; rect.style.width=r.w+'px'; rect.style.height=r.h+'px'; };
    const cleanup = ()=>{ window.removeEventListener('mousedown', onMouseDown, true); window.removeEventListener('mousemove', onMouseMove, true); window.removeEventListener('mouseup', onMouseUp, true); window.removeEventListener('keydown', onKey, true); this._wrap.style.display='none'; };
    return new Promise((resolve,reject)=>{ this._resolve=resolve; this._reject=reject; window.addEventListener('mousedown', onMouseDown, true); window.addEventListener('mousemove', onMouseMove, true); window.addEventListener('mouseup', onMouseUp, true); window.addEventListener('keydown', onKey, true); });
  },
  _calc(sx,sy,ex,ey){ const x=Math.min(sx,ex), y=Math.min(sy,ey); const w=Math.abs(ex-sx), h=Math.abs(ey-sy); return { x, y, w, h } },
  async captureAndSave(){
    try{
      const win = await ARW.invoke('active_window_bounds', { label: null });
      const r = await this.start();
      const dpr = window.devicePixelRatio || 1;
      // Convert to physical pixels and absolute screen coords
      const X = Math.round((win.x||0) + r.x * dpr);
      const Y = Math.round((win.y||0) + r.y * dpr);
      const W = Math.max(1, Math.round(r.w * dpr));
      const H = Math.max(1, Math.round(r.h * dpr));
      const scope = `region:${X},${Y},${W},${H}`;
      const port = ARW.toolPort();
      const out = await ARW.invoke('run_tool_admin', { id: 'ui.screenshot.capture', input: { scope, format:'png', downscale:640 }, port });
      ARW.toast(out && out.path ? ('Saved: ' + out.path) : 'Capture requested');
      return out;
    }catch(e){ ARW.toast('Region capture canceled'); return null; }
  }
};

// Annotation overlay (draw rectangles on an image)
window.ARW.annot = {
  _wrap: null,
  _panel: null,
  _img: null,
  _rects: [],
  _active: null,
  mount(){ if (this._wrap) return; const w=document.createElement('div'); w.className='annot-wrap'; const dim=document.createElement('div'); dim.className='annot-dim'; const panel=document.createElement('div'); panel.className='annot-panel'; const head=document.createElement('header'); head.innerHTML='<strong>Annotate</strong>'; const main=document.createElement('div'); main.className='annot-canvas'; const img=document.createElement('img'); main.appendChild(img); const foot=document.createElement('footer'); const cancel=document.createElement('button'); cancel.className='ghost'; cancel.textContent='Cancel'; cancel.addEventListener('click', ()=> this.hide()); const apply=document.createElement('button'); apply.className='primary'; apply.textContent='Apply'; apply.addEventListener('click', ()=> this._apply()); foot.appendChild(cancel); foot.appendChild(apply); panel.appendChild(head); panel.appendChild(main); panel.appendChild(foot); w.appendChild(dim); w.appendChild(panel); document.body.appendChild(w); this._wrap=w; this._panel=panel; this._img=img; },
  show(src){ this.mount(); this._wrap.style.display='block'; this._img.src = src; this._rects=[]; this._bind(); },
  hide(){ if (this._wrap) this._wrap.style.display='none'; this._unbind(); },
  _bind(){ const canvas=this._panel.querySelector('.annot-canvas'); let sx=0, sy=0; const onDown=(e)=>{ const r=canvas.getBoundingClientRect(); sx=e.clientX - r.left; sy=e.clientY - r.top; const div=document.createElement('div'); div.className='ann-rect'; canvas.appendChild(div); this._active={ div, sx, sy }; }; const onMove=(e)=>{ if (!this._active) return; const r=canvas.getBoundingClientRect(); const ex=e.clientX - r.left; const ey=e.clientY - r.top; const x=Math.min(this._active.sx, ex), y=Math.min(this._active.sy, ey); const w=Math.abs(ex - this._active.sx), h=Math.abs(ey - this._active.sy); Object.assign(this._active.div.style, { left:x+'px', top:y+'px', width:w+'px', height:h+'px' }); }; const onUp=(e)=>{ if (!this._active) return; const rect=this._active.div.getBoundingClientRect(); const cref=canvas.getBoundingClientRect(); const x=Math.max(0, rect.left - cref.left), y=Math.max(0, rect.top - cref.top), w=rect.width, h=rect.height; this._rects.push({ x, y, w, h, blur:true }); this._active=null; }; this._onDown=onDown; this._onMove=onMove; this._onUp=onUp; canvas.addEventListener('mousedown', onDown); window.addEventListener('mousemove', onMove); window.addEventListener('mouseup', onUp); },
  _unbind(){ const canvas=this._panel?.querySelector('.annot-canvas'); if (!canvas) return; if (this._onDown) canvas.removeEventListener('mousedown', this._onDown); if (this._onMove) window.removeEventListener('mousemove', this._onMove); if (this._onUp) window.removeEventListener('mouseup', this._onUp); this._onDown=this._onMove=this._onUp=null; const rects = Array.from(canvas.querySelectorAll('.ann-rect')); rects.forEach(d=> d.remove()); },
  _apply(){ try{ if (!this._img) return; const imgEl=this._img; const natW=imgEl.naturalWidth||1, natH=imgEl.naturalHeight||1; const disp = imgEl.getBoundingClientRect(); const scaleX = natW / disp.width, scaleY = natH / disp.height; const canvas=this._panel.querySelector('.annot-canvas'); const cref=canvas.getBoundingClientRect(); const rects=Array.from(canvas.querySelectorAll('.ann-rect')).map(el=>{ const r=el.getBoundingClientRect(); const x=Math.max(0, r.left - cref.left) * scaleX; const y=Math.max(0, r.top - cref.top) * scaleY; const w=r.width * scaleX; const h=r.height * scaleY; return { x: Math.round(x), y: Math.round(y), w: Math.round(w), h: Math.round(h), blur:true }; }); this._resolve(rects); this.hide(); }catch(e){ this._reject(e); this.hide(); } },
  start(src){ this.show(src); return new Promise((resolve,reject)=>{ this._resolve=resolve; this._reject=reject; }); }
};

// Keyboard Shortcuts overlay (global)
window.ARW.shortcuts = {
  _wrap: null,
  _panel: null,
  _list: null,
  _prevFocus: null,
  _trap: null,
  _mkRow(k,d){ const tr=document.createElement('tr'); tr.innerHTML=`<td class="mono">${k}</td><td>${d}</td>`; return tr; },
  _content(page){
    const base = [ ['Ctrl/Cmd+K','Command palette'], ['?','Shortcuts help'] ];
    const map = {
      hub: [['Arrows','Navigate files tree'], ['Enter','Open folder / preview file'], ['Left/Right','Collapse/Expand or focus parent/child'], ['/', 'Focus file filter'], ['n','Focus new project'], ['b','Back to previous folder']],
      events: [['p','Pause (checkbox)'], ['c','Clear log']],
      logs: [['r','Refresh'], ['w','Toggle wrap'], ['a','Toggle auto']],
      models: [['R','Refresh'], ['L','Load'], ['S','Save'], ['J','Refresh jobs'], ['A','Toggle jobs auto']],
      chat: [['Enter','Send message'], ['C','Capture (buttons)']],
      training: [['A','Run A/B']],
      index: [['S','Save prefs'], ['T','Start service'], ['X','Stop service'], ['H','Check health'], ['O','Open Debug UI']]
    };
    return base.concat(map[page]||[]);
  },
  mount(){
    if (this._wrap) return;
    const w=document.createElement('div');
    w.className='gallery-wrap';
    w.style.display='none';
    w.setAttribute('aria-hidden','true');
    const p=document.createElement('div');
    p.className='gallery';
    p.setAttribute('role','dialog');
    p.setAttribute('aria-modal','true');
    const h=document.createElement('header');
    const t=document.createElement('strong');
    t.textContent='Keyboard Shortcuts';
    const titleId='shortcutsTitle';
    t.id = titleId;
    p.setAttribute('aria-labelledby', titleId);
    const x=document.createElement('button');
    x.className='ghost';
    x.textContent='Close';
    x.addEventListener('click', ()=> this.hide());
    h.appendChild(t);
    h.appendChild(x);
    const m=document.createElement('main');
    const tbl=document.createElement('table');
    tbl.className='cmp-table';
    const tb=document.createElement('tbody');
    tbl.appendChild(tb);
    m.appendChild(tbl);
    p.appendChild(h);
    p.appendChild(m);
    w.appendChild(p);
    document.body.appendChild(w);
    w.addEventListener('click', (e)=>{ if (e.target===w) this.hide(); });
    const trap = (event)=>{
      if (event.key !== 'Tab' || w.style.display === 'none') return;
      if (!p.contains(event.target)) return;
      const focusables = Array.from(p.querySelectorAll('button, [href], input, select, textarea, [tabindex]:not([tabindex="-1"])'))
        .filter(el => !el.hasAttribute('disabled'));
      if (!focusables.length) { event.preventDefault(); return; }
      const first = focusables[0];
      const last = focusables[focusables.length - 1];
      if (event.shiftKey){
        if (document.activeElement === first){ event.preventDefault(); last.focus(); }
      } else if (document.activeElement === last){ event.preventDefault(); first.focus(); }
    };
    w.addEventListener('keydown', trap);
    this._wrap=w;
    this._panel=p;
    this._list=tb;
    this._trap = trap;
  },
  _render(){
    const tb=this._list;
    if (!tb) return;
    tb.innerHTML='';
    const page = ARW.util.pageId();
    const rows=this._content(page);
    rows.forEach(([k,d])=> tb.appendChild(this._mkRow(k,d)));
  },
  show(){
    this.mount();
    if (!this._wrap) return;
    const activeEl = document.activeElement;
    this._prevFocus = activeEl && typeof activeEl.focus === 'function' ? activeEl : null;
    this._render();
    this._wrap.style.display='grid';
    this._wrap.removeAttribute('aria-hidden');
    try{ this._panel.querySelector('header button')?.focus({ preventScroll:true }); }
    catch{}
  },
  hide(){
    if (!this._wrap) return;
    this._wrap.style.display='none';
    this._wrap.setAttribute('aria-hidden','true');
    const prev = this._prevFocus;
    this._prevFocus = null;
    if (prev && document.contains(prev)){
      try{ prev.focus({ preventScroll:true }); }
      catch{ try{ prev.focus(); }catch{} }
    }
  },
  toggle(){ if (!this._wrap || this._wrap.style.display==='none') this.show(); else this.hide(); }
};

if (!HAS_TAURI) {
  const prefsKey = (ns) => `arw:prefs:${ns || 'launcher'}`;
  const openExternal = (url, target) => {
    if (!url) return null;
    try {
      const win = window.open(url, target || '_blank', target ? '' : 'noopener');
      if (!win && !target) {
        window.location.href = url;
      }
    } catch {
      window.location.href = url;
    }
    return null;
  };
  const openSurface = (page, options = {}) => {
    const opts = options || {};
    const base = typeof opts.base === 'string' && opts.base
      ? opts.base
      : ARW.base();
    let href = page;
    if (base) {
      const joiner = href.includes('?') ? '&' : '?';
      href += `${joiner}base=${encodeURIComponent(base)}`;
    }
    const target = opts.sameTab ? '_self' : '_blank';
    return openExternal(href, target);
  };
  const authHeaders = (token) => {
    const value = typeof token === 'string' ? token.trim() : '';
    if (!value) return {};
    return {
      Authorization: `Bearer ${value}`,
      'X-ARW-Admin': value,
    };
  };
  fallbackHandlers.get_prefs = function({ namespace } = {}) {
    const key = prefsKey(namespace);
    try {
      const raw = localStorage.getItem(key);
      if (!raw) return {};
      return JSON.parse(raw);
    } catch {
      return {};
    }
  };
  fallbackHandlers.set_prefs = function({ namespace, value } = {}) {
    const key = prefsKey(namespace);
    try {
      if (value === null || value === undefined) {
        localStorage.removeItem(key);
      } else {
        localStorage.setItem(key, JSON.stringify(value));
      }
    } catch (err) {
      console.warn('prefs write failed', err);
    }
    return null;
  };
  fallbackHandlers.open_url = function({ url, target } = {}) {
    return openExternal(url, target);
  };
  fallbackHandlers.open_hub_window = function({ base, sameTab } = {}) {
    return openSurface('hub.html', { base, sameTab });
  };
  fallbackHandlers.open_chat_window = function({ base, sameTab } = {}) {
    return openSurface('chat.html', { base, sameTab });
  };
  fallbackHandlers.open_training_window = function({ base, sameTab } = {}) {
    return openSurface('training.html', { base, sameTab });
  };
  fallbackHandlers.open_trial_window = function({ base, sameTab } = {}) {
    return openSurface('trial.html', { base, sameTab });
  };
  fallbackHandlers.open_events_window = function({ base, sameTab } = {}) {
    return openSurface('events.html', { base, sameTab });
  };
  fallbackHandlers.open_logs_window = function({ base, sameTab } = {}) {
    return openSurface('logs.html', { base, sameTab });
  };
  fallbackHandlers.open_models_window = function({ base, sameTab } = {}) {
    return openSurface('models.html', { base, sameTab });
  };
  fallbackHandlers.open_connections_window = function({ base, sameTab } = {}) {
    return openSurface('connections.html', { base, sameTab });
  };
  fallbackHandlers.open_events_window_base = function({ base } = {}) {
    return openSurface('events.html', { base });
  };
  fallbackHandlers.open_logs_window_base = function({ base } = {}) {
    return openSurface('logs.html', { base });
  };
  fallbackHandlers.open_models_window_base = function({ base } = {}) {
    return openSurface('models.html', { base });
  };
  fallbackHandlers.open_debug_window = function({ port } = {}) {
    const base = this.base(port);
    const url = `${String(base || '').replace(/\/$/, '')}/admin/debug`;
    return openExternal(url);
  };
  fallbackHandlers.start_service = function() {
    this.unsupported('Starting the local service');
  };
  fallbackHandlers.stop_service = function() {
    this.unsupported('Stopping the local service');
  };
  fallbackHandlers.launcher_service_log_path = function() {
    return null;
  };
  fallbackHandlers.launcher_recent_service_logs = function() {
    return [];
  };
  fallbackHandlers.launcher_autostart_status = function() {
    return false;
  };
  fallbackHandlers.set_launcher_autostart = function() {
    this.unsupported('Launch at login');
  };
  fallbackHandlers.open_path = function() {
    this.unsupported('Opening local files');
  };
  fallbackHandlers.open_in_editor = function() {
    this.unsupported('Opening an editor');
  };
  fallbackHandlers.active_window_bounds = function() {
    this.unsupported('Window capture');
  };
  fallbackHandlers.run_tool_admin = function() {
    this.unsupported('Tool automation');
  };
  fallbackHandlers.projects_import = function() {
    this.unsupported('Project import');
  };
  fallbackHandlers.check_service_health = async function({ port, base } = {}) {
    let origin = '';
    if (typeof base === 'string' && base.trim()) {
      origin = this.normalizeBase(base) || base.trim();
    } else {
      origin = this.base(port);
    }
    const url = `${String(origin || '').replace(/\/+$/, '')}/healthz`;
    try {
      const resp = await fetch(url, { method: 'GET', cache: 'no-store' });
      return resp.ok;
    } catch {
      return false;
    }
  };
  fallbackHandlers.admin_get_json_base = function({ base, path, token } = {}) {
    const origin = (typeof base === 'string' && base.trim()) || this.base();
    const headers = authHeaders(token);
    return this.http.json(origin, path || '/', { headers });
  };
  fallbackHandlers.admin_post_json_base = async function({ base, path, body, token } = {}) {
    const origin = (typeof base === 'string' && base.trim()) || this.base();
    const headers = Object.assign({ 'Content-Type': 'application/json' }, authHeaders(token));
    const resp = await this.http.fetch(origin, path || '/', {
      method: 'POST',
      headers,
      body: JSON.stringify(body ?? {}),
    });
    if (!resp.ok) throw new Error(`HTTP ${resp.status}`);
    return resp.json();
  };
  fallbackHandlers.run_trials_preflight = function() {
    throw new Error('Trials preflight unavailable in browser mode');
  };
}

// Global shortcuts help wiring
document.addEventListener('DOMContentLoaded', ()=>{
  try{ const b=document.getElementById('btn-shortcuts'); if (b) b.addEventListener('click', ()=> ARW.shortcuts.show()); }catch{}
  try{ const h=document.getElementById('btn-help'); if (h) h.addEventListener('click', ()=> ARW.invoke('open_url', { url: 'https://t3hw00t.github.io/ARW/guide/shortcuts/' })); }catch{}
});
window.addEventListener('keydown', (e)=>{
  const tag=(e.target && e.target.tagName || '').toLowerCase();
  if (tag==='input' || tag==='textarea' || tag==='select') return;
  if (e.ctrlKey || e.metaKey || e.altKey) return;
  if (e.key==='?' || (e.shiftKey && e.key==='/')){ e.preventDefault(); ARW.shortcuts.toggle(); }
});


ARW.personaPanel = (() => {
  const panels = new Set();
  let sseHooked = false;

  const escapeHtml = (value) => String(value ?? '')
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
    .replace(/"/g, '&quot;')
    .replace(/'/g, '&#39;');

  const parseDate = (value) => {
    if (!value) return null;
    try {
      const date = new Date(value);
      if (Number.isNaN(date.getTime())) return null;
      return date;
    } catch {
      return null;
    }
  };

  const formatRelative = (value) => {
    const date = parseDate(value);
    if (!date) return '';
    const diff = Date.now() - date.getTime();
    if (diff < 0) return 'just now';
    if (diff < 5000) return 'just now';
    if (diff < 60000) return `${Math.round(diff / 1000)}s ago`;
    if (diff < 3600000) return `${Math.round(diff / 60000)}m ago`;
    if (diff < 86400000) return `${Math.round(diff / 3600000)}h ago`;
    const days = Math.round(diff / 86400000);
    if (days <= 7) return `${days}d ago`;
    return date.toLocaleString();
  };

  const summarizeMetadata = (value) => {
    if (value == null) return '';
    if (typeof value === 'string') {
      return value.trim();
    }
    if (Array.isArray(value) && value.length === 0) return '';
    if (typeof value === 'object') {
      const keys = Object.keys(value);
      if (!keys.length) return '';
      try {
        const json = JSON.stringify(value);
        if (json.length > 180) return `${json.slice(0, 177)}…`;
        return json;
      } catch {
        return '';
      }
    }
    return String(value);
  };

  function ensureSse() {
    if (sseHooked) return;
    if (!(ARW && ARW.sse && typeof ARW.sse.subscribe === 'function')) return;
    sseHooked = true;
    ARW.sse.subscribe(
      (kind) => kind === 'persona.feedback',
      (event) => {
        try {
          const payload = (event && event.env && event.env.payload) || (event && event.payload) || {};
          const personaId = payload && typeof payload.persona_id === 'string' ? payload.persona_id : null;
          if (!personaId) return;
          panels.forEach((panel) => {
            try { panel.onFeedback(personaId); } catch (err) { console.warn('persona panel refresh failed', err); }
          });
        } catch (err) {
          console.warn('persona feedback event handling failed', err);
        }
      },
    );
  }

  class PersonaPanel {
    constructor(options = {}) {
      this.root = options.root || (options.rootId ? document.getElementById(options.rootId) : null);
      this.select = options.select || (options.selectId ? document.getElementById(options.selectId) : null);
      this.refreshBtn = options.refresh || (options.refreshId ? document.getElementById(options.refreshId) : null);
      this.status = options.status || (options.statusId ? document.getElementById(options.statusId) : null);
      this.scope = options.scope || (options.scopeId ? document.getElementById(options.scopeId) : null);
      this.enable = options.enable || (options.enableId ? document.getElementById(options.enableId) : null);
      this.saveBtn = options.save || (options.saveId ? document.getElementById(options.saveId) : null);
      this.empty = options.empty || (options.emptyId ? document.getElementById(options.emptyId) : null);
      this.metrics = options.metrics || (options.metricsId ? document.getElementById(options.metricsId) : null);
      this.history = options.history || (options.historyId ? document.getElementById(options.historyId) : null);
      this.historyMeta = options.historyMeta || (options.historyMetaId ? document.getElementById(options.historyMetaId) : null);
      this.applyAll = options.applyAll || (options.applyAllId ? document.getElementById(options.applyAllId) : null);
      this.retentionNoteDefault = 'Retention defaults to 50 samples (ARW_PERSONA_VIBE_HISTORY_RETAIN).';
      this.retentionNote = this.retentionNoteDefault;
      this.retainMax = null;
      this._historyMetaPrefix = '';
      this.getBase = typeof options.getBase === 'function' ? options.getBase : () => options.base || '';
      const limitRaw = Number(options.historyLimit);
      this.historyLimit = Number.isFinite(limitRaw) ? Math.max(1, Math.min(100, limitRaw)) : 10;
      this.items = [];
      this.selectedId = null;
      this.refreshTimer = null;
      this.statusTimer = null;
      this.loadingDetails = null;
      this.initialized = false;
      this._disposed = false;
      this.disabled = !this.root || !this.metrics || !this.history;
      this._changeListeners = new Set();
      this._handleSelectChange = this.handleSelectChange.bind(this);
      this._handleRefresh = this.handleRefreshClick.bind(this);
      this._handleSave = this.handleSaveClick.bind(this);
      this._handleApplyAll = this.handleApplyAllClick.bind(this);
    }

    init() {
      if (this.initialized || this.disabled) {
        return Promise.resolve();
      }
      this.initialized = true;
      if (this.select) this.select.addEventListener('change', this._handleSelectChange);
      if (this.refreshBtn) this.refreshBtn.addEventListener('click', this._handleRefresh);
      if (this.saveBtn) this.saveBtn.addEventListener('click', this._handleSave);
      if (this.applyAll) this.applyAll.addEventListener('click', this._handleApplyAll);
      this.clearDetails();
      return this.loadList({ preserveSelection: false });
    }

    dispose() {
      if (this._disposed) return;
      this._disposed = true;
      panels.delete(this);
      if (this.select) this.select.removeEventListener('change', this._handleSelectChange);
      if (this.refreshBtn) this.refreshBtn.removeEventListener('click', this._handleRefresh);
      if (this.saveBtn) this.saveBtn.removeEventListener('click', this._handleSave);
      if (this.applyAll) this.applyAll.removeEventListener('click', this._handleApplyAll);
      if (this._changeListeners) this._changeListeners.clear();
    }

    setBusy(flag) {
      const disable = !!flag;
      if (this.select) this.select.disabled = disable || !this.items.length;
      if (this.refreshBtn) this.refreshBtn.disabled = disable;
      const controlsDisabled = disable || !this.selectedId;
      if (this.scope) this.scope.disabled = controlsDisabled;
      if (this.enable) this.enable.disabled = controlsDisabled;
      if (this.saveBtn) this.saveBtn.disabled = controlsDisabled;
      if (this.applyAll) this.applyAll.disabled = disable || !this.selectedId || !this.items.length;
    }

    setStatus(text, options = {}) {
      if (!this.status) return;
      if (this.statusTimer) {
        clearTimeout(this.statusTimer);
        this.statusTimer = null;
      }
      this.status.textContent = text || '';
      const ttl = options.clearAfter;
      if (text && Number.isFinite(ttl) && ttl > 0) {
        this.statusTimer = setTimeout(() => {
          if (this.status && this.status.textContent === text) {
            this.status.textContent = '';
          }
        }, ttl);
      }
    }

    showEmpty(message) {
      if (!this.empty) return;
      if (message) {
        this.empty.hidden = false;
        this.empty.textContent = message;
      } else {
        this.empty.hidden = true;
      }
    }

    updateHistoryMeta(prefix) {
      if (!this.historyMeta) return;
      if (prefix !== undefined) {
        this._historyMetaPrefix = typeof prefix === 'string' ? prefix.trim() : '';
      }
      const parts = [];
      if (this._historyMetaPrefix) parts.push(this._historyMetaPrefix);
      if (this.retentionNote) parts.push(this.retentionNote);
      this.historyMeta.textContent = parts.join(' · ');
    }

    setRetentionInfo(retainMax) {
      const parsed = Number(retainMax);
      if (Number.isFinite(parsed) && parsed > 0) {
        this.retainMax = parsed;
        this.retentionNote = `Retention capped at ${parsed} samples (ARW_PERSONA_VIBE_HISTORY_RETAIN).`;
      } else {
        this.retainMax = null;
        this.retentionNote = this.retentionNoteDefault;
      }
      this.updateHistoryMeta();
    }

    clearDetails() {
      if (this.metrics) this.metrics.innerHTML = '<p class="dim">Select a persona to view telemetry.</p>';
      if (this.history) this.history.innerHTML = '<li class="dim">No feedback yet.</li>';
      this._historyMetaPrefix = '';
      this.setRetentionInfo(null);
    }

    currentPersonaId() {
      return this.selectedId;
    }

    onPersonaChange(handler) {
      if (typeof handler !== 'function') return () => {};
      this._changeListeners.add(handler);
      return () => this._changeListeners.delete(handler);
    }

    _emitChange(entry) {
      if (!this._changeListeners || this._changeListeners.size === 0) return;
      for (const listener of Array.from(this._changeListeners)) {
        try {
          listener(this.selectedId, entry);
        } catch (err) {
          console.warn('persona change listener failed', err);
        }
      }
    }

    async reload(options = {}) {
      if (this.disabled) return Promise.resolve();
      return this.loadList(options);
    }

    async loadList(options = {}) {
      if (this.disabled) return Promise.resolve();
      const preserveSelection = options && options.preserveSelection !== undefined ? !!options.preserveSelection : true;
      const previousId = preserveSelection ? this.selectedId : null;
      this.setStatus('Loading…');
      this.setBusy(true);
      try {
        const response = await this.fetchJson('/state/persona');
        const items = Array.isArray(response && response.items) ? response.items : [];
        this.items = items;
        if (this.select) {
          const optionsHtml = items
            .map((item) => `<option value="${escapeHtml(item.id)}">${escapeHtml(item.name || item.id)}</option>`)
            .join('');
          this.select.innerHTML = optionsHtml;
          this.select.disabled = !items.length;
        }
        if (!items.length) {
          this.selectedId = null;
        this.showEmpty('No personas found. Enable ARW_PERSONA_ENABLE=1 and seed a persona to begin telemetry.');
        this.clearDetails();
        this.setBusy(true);
        this.setStatus('');
        this._emitChange(null);
        return;
      }
        this.showEmpty('');
        const next = previousId && items.some((item) => item.id === previousId) ? previousId : items[0].id;
        if (this.select) this.select.value = next;
        const entry = items.find((item) => item.id === next) || null;
        this.applyPersonaEntry(entry);
        await this.loadDetails(next);
      } catch (err) {
        const message = String(err && err.message ? err.message : '');
        if (message.includes('HTTP 501')) {
          this.items = [];
          this.selectedId = null;
          if (this.select) {
            this.select.innerHTML = '';
            this.select.disabled = true;
          }
          this.clearDetails();
          this.showEmpty('Persona subsystem disabled. Enable ARW_PERSONA_ENABLE=1.');
          this.setStatus('');
        } else {
          console.warn('persona list fetch failed', err);
          this.setStatus('Failed to load personas', { clearAfter: 4000 });
        }
      } finally {
        this.setBusy(false);
      }
    }

    applyPersonaEntry(entry) {
      this.selectedId = entry ? entry.id : null;
      const telemetry = entry && entry.preferences && entry.preferences.telemetry && entry.preferences.telemetry.vibe;
      const scopeValue = telemetry && typeof telemetry.scope === 'string' && telemetry.scope.trim().length
        ? telemetry.scope.trim()
        : (entry && entry.owner_kind) || '';
      if (this.scope) this.scope.value = scopeValue;
      if (this.enable) this.enable.checked = !!(telemetry && telemetry.enabled === true);
      const controlsDisabled = !this.selectedId;
      if (this.scope) this.scope.disabled = controlsDisabled;
      if (this.enable) this.enable.disabled = controlsDisabled;
      if (this.saveBtn) this.saveBtn.disabled = controlsDisabled;
      if (this.applyAll) this.applyAll.disabled = controlsDisabled || !this.items.length;
      this._emitChange(entry);
    }

    async loadDetails(id, options = {}) {
      if (!id) return;
      const skipLoading = options && options.skipLoading;
      this.loadingDetails = id;
      if (!skipLoading) {
        if (this.metrics) this.metrics.innerHTML = '<p class="dim">Loading telemetry.</p>';
        if (this.history) this.history.innerHTML = '<li class="dim">Loading.</li>';
      }
      let detail = null;
      try {
        const limit = this.historyLimit || 10;
        const detailPromise = this
          .fetchJson(`/state/persona/${encodeURIComponent(id)}`)
          .catch((err) => {
            console.warn('persona detail fetch failed', err);
            return null;
          });
        const metricsPromise = this.fetchJson(`/state/persona/${encodeURIComponent(id)}/vibe_metrics`);
        const historyPromise = this.fetchJson(`/state/persona/${encodeURIComponent(id)}/vibe_history?limit=${limit}`);
        detail = await detailPromise;
        const [metrics, history] = await Promise.all([metricsPromise, historyPromise]);
        if (this.loadingDetails !== id) return;
        this.setStatus('');
        this.renderMetrics(metrics, detail, null);
        this.renderHistory(history);
      } catch (err) {
        if (this.loadingDetails !== id) return;
        const message = String(err && err.message ? err.message : '');
        if (message.includes('HTTP 412')) {
          this.renderMetrics(null, detail, 'Telemetry disabled for this persona (showing last recorded snapshot when available).');
          this.renderHistory(null, 'Enable telemetry to collect new feedback.');
          this.setStatus('Telemetry disabled for this persona', { clearAfter: 4000 });
        } else {
          console.warn('persona telemetry fetch failed', err);
          this.renderMetrics(null, detail, 'Telemetry unavailable.');
          this.renderHistory(null, 'Feedback history unavailable.');
          this.setStatus('Failed to load telemetry', { clearAfter: 4000 });
        }
      } finally {
        if (this.loadingDetails === id) {
          this.loadingDetails = null;
        }
      }
    }\r\n\r\n    renderMetrics(metrics, detail, message) {
      if (!this.metrics) return;
      const fragments = [];
      if (message) {
        fragments.push(`<p class="dim">${escapeHtml(message)}</p>`);
      }
      const previewMetrics = detail && typeof detail === 'object' && detail.vibe_metrics_preview && typeof detail.vibe_metrics_preview === 'object'
        ? detail.vibe_metrics_preview
        : null;
      const source = metrics && typeof metrics === 'object' ? metrics : previewMetrics;
      const biasPreview = detail && typeof detail === 'object' && detail.context_bias_preview && typeof detail.context_bias_preview === 'object'
        ? detail.context_bias_preview
        : null;

      if (source) {
        const retainMax = source.retain_max ?? source.retainMax ?? null;
        this.setRetentionInfo(retainMax);
        const totalRaw = Number(source.total_feedback ?? source.totalFeedback);
        const total = Number.isFinite(totalRaw) ? totalRaw : 0;
        const avgRaw = Number(source.average_strength ?? source.averageStrength);
        const avg = Number.isFinite(avgRaw) ? avgRaw.toFixed(2) : '-';
        const lastSignal = (source.last_signal ?? source.lastSignal ?? 'unspecified') || 'unspecified';
        const lastStrengthRaw = Number(source.last_strength ?? source.lastStrength);
        const lastStrength = Number.isFinite(lastStrengthRaw) ? lastStrengthRaw.toFixed(2) : null;
        const lastUpdated = source.last_updated ?? source.lastUpdated ?? null;
        const lastUpdatedRel = formatRelative(lastUpdated);

        fragments.push(`<div class="metric-card"><span class="metric-label">Total feedback</span><strong>${escapeHtml(total)}</strong></div>`);
        fragments.push(`<div class="metric-card"><span class="metric-label">Average strength</span><strong>${escapeHtml(avg)}</strong></div>`);
        const metaBits = [];
        if (lastStrength) metaBits.push(`strength ${escapeHtml(lastStrength)}`);
        if (lastUpdatedRel) metaBits.push(lastUpdatedRel);
        fragments.push(`<div class="metric-card"><span class="metric-label">Last signal</span><strong>${escapeHtml(lastSignal)}</strong>${metaBits.length ? `<span class="dim">${escapeHtml(metaBits.join(' · '))}</span>` : ''}</div>`);

        const countMap = source.signal_counts && typeof source.signal_counts === 'object' ? source.signal_counts : {};
        const strengthMap = source.signal_strength && typeof source.signal_strength === 'object' ? source.signal_strength : {};
        const weightMap = source.signal_weights && typeof source.signal_weights === 'object' ? source.signal_weights : {};
        const signalEntries = Object.keys(countMap)
          .map((key) => {
            const label = key && key.trim().length ? key : 'unspecified';
            const countVal = Number(countMap[key]);
            const weightVal = Number(weightMap[key]);
            const avgVal = Number(strengthMap[key]);
            return {
              label,
              count: Number.isFinite(countVal) ? countVal : 0,
              weight: Number.isFinite(weightVal) ? weightVal : null,
              avg: Number.isFinite(avgVal) ? avgVal : null,
            };
          })
          .filter((entry) => Number.isFinite(entry.count))
          .sort((a, b) => {
            const aScore = Number.isFinite(a.weight) ? a.weight : a.count;
            const bScore = Number.isFinite(b.weight) ? b.weight : b.count;
            return bScore - aScore;
          });
        if (signalEntries.length) {
          const list = signalEntries.slice(0, 8)
            .map((entry) => {
              const extras = [];
              if (Number.isFinite(entry.avg)) extras.push(`avg ${entry.avg.toFixed(2)}`);
              if (Number.isFinite(entry.weight) && entry.weight !== entry.count) extras.push(`w ${entry.weight.toFixed(2)}`);
              const extrasHtml = extras.length ? `<div class="metric-sub">${escapeHtml(extras.join(' · '))}</div>` : '';
              return `<li><div class="metric-row"><span>${escapeHtml(entry.label)}</span><strong>${escapeHtml(entry.count)}</strong></div>${extrasHtml}</li>`;
            })
            .join('');
          fragments.push(`<div class="metric-card"><span class="metric-label">Signals</span><ul class="metric-list">${list}</ul></div>`);
        }

        const relMeta = formatRelative(lastUpdated);
        this.updateHistoryMeta(relMeta ? `Updated ${relMeta}` : '');
      } else {
        this.setRetentionInfo(null);
        this.updateHistoryMeta('');
      }

      if (biasPreview) {
        const sections = [];
        const lanes = biasPreview.lane_priorities && typeof biasPreview.lane_priorities === 'object' ? biasPreview.lane_priorities : {};
        const laneEntries = Object.keys(lanes)
          .map((lane) => ({ lane, value: Number(lanes[lane]) }))
          .filter((entry) => Number.isFinite(entry.value))
          .sort((a, b) => Math.abs(b.value) - Math.abs(a.value));
        if (laneEntries.length) {
          const laneHtml = laneEntries.slice(0, 6)
            .map((entry) => `<li><div class="metric-row"><span>${escapeHtml(entry.lane)}</span><span class="dim">${entry.value >= 0 ? '+' : ''}${entry.value.toFixed(2)}</span></div></li>`)
            .join('');
          sections.push(`<div class="metric-section"><div class="metric-subtitle">Lane priorities</div><ul class="metric-list">${laneHtml}</ul></div>`);
        }
        const slots = biasPreview.slot_overrides && typeof biasPreview.slot_overrides === 'object' ? biasPreview.slot_overrides : {};
        const slotEntries = Object.keys(slots)
          .map((slot) => ({ slot, limit: Number(slots[slot]) }))
          .filter((entry) => Number.isFinite(entry.limit) && entry.limit > 0)
          .sort((a, b) => b.limit - a.limit);
        if (slotEntries.length) {
          const slotHtml = slotEntries.slice(0, 6)
            .map((entry) => `<li><div class="metric-row"><span>${escapeHtml(entry.slot)}</span><span class="dim">≥ ${entry.limit}</span></div></li>`)
            .join('');
          sections.push(`<div class="metric-section"><div class="metric-subtitle">Slot minimums</div><ul class="metric-list">${slotHtml}</ul></div>`);
        }
        const minScore = Number(biasPreview.min_score_delta ?? biasPreview.minScoreDelta);
        if (Number.isFinite(minScore) && Math.abs(minScore) > Number.EPSILON) {
          sections.push(`<div class="metric-sub">Min score delta: ${minScore >= 0 ? '+' : ''}${minScore.toFixed(2)}</div>`);
        }
        if (!sections.length) {
          sections.push('<div class="metric-sub">No context adjustments recorded yet.</div>');
        }
        fragments.push(`<div class="metric-card metric-card-preview"><span class="metric-label">Context bias</span>${sections.join('')}</div>`);
      }

      if (!source && !biasPreview && fragments.length === 0) {
        fragments.push('<p class="dim">Telemetry unavailable.</p>');
      }

      this.metrics.innerHTML = fragments.join('');
    }    renderHistory(historyData, message) {
      if (!this.history) return;
      if (message) {
        this.history.innerHTML = `<li class="dim">${escapeHtml(message)}</li>`;
        this.setRetentionInfo(null);
        this.updateHistoryMeta('');
        return;
      }
      let retainCandidate = null;
      let items = [];
      if (historyData && typeof historyData === 'object' && !Array.isArray(historyData)) {
        if (historyData.retain_max != null) retainCandidate = historyData.retain_max;
        else if (historyData.retainMax != null) retainCandidate = historyData.retainMax;
        if (Array.isArray(historyData.items)) {
          items = historyData.items;
        }
      } else if (Array.isArray(historyData)) {
        items = historyData;
      }
      this.setRetentionInfo(retainCandidate);
      if (!items.length) {
        this.history.innerHTML = '<li class="dim">No feedback yet.</li>';
        this.updateHistoryMeta('');
        return;
      }
      const limited = items.slice(0, this.historyLimit);
      const html = limited
        .map((item) => {
          const signal = item && typeof item.signal === 'string' && item.signal.trim().length ? item.signal.trim() : 'unspecified';
          const strength = Number.isFinite(item && item.strength) ? item.strength.toFixed(2) : null;
          const recorded = item && (item.recorded_at || item.recordedAt);
          const recordedDate = parseDate(recorded);
          const rel = formatRelative(recorded);
          const timeTitle = recordedDate ? recordedDate.toLocaleString() : (recorded || '');
          const note = item && item.note && String(item.note).trim().length ? String(item.note).trim() : '';
          const meta = summarizeMetadata(item && item.metadata);
          return `<li>
            <div class="meta">
              <strong>${escapeHtml(signal)}</strong>
              ${strength ? `<span class="dim">${escapeHtml(strength)}</span>` : ''}
              ${rel ? `<time datetime="${escapeHtml(recordedDate ? recordedDate.toISOString() : recorded || '')}" title="${escapeHtml(timeTitle)}">${escapeHtml(rel)}</time>` : ''}
            </div>
            ${note ? `<p class="note">${escapeHtml(note)}</p>` : ''}
            ${meta ? `<div class="meta-json">${escapeHtml(meta)}</div>` : ''}
          </li>`;
        })
        .join('');
      this.history.innerHTML = html;
      const latest = limited[0] && (limited[0].recorded_at || limited[0].recordedAt);
      const rel = formatRelative(latest);
      this.updateHistoryMeta(rel ? `Updated ${rel}` : '');
    }

    async saveTelemetry() {
      if (!this.selectedId) return;
      const enabled = this.enable ? !!this.enable.checked : false;
      const scope = this.scope ? this.scope.value.trim() : '';
      const body = { enabled };
      if (scope) body.scope = scope;
      this.setStatus('Saving…');
      try {
        await this.fetchJson(`/admin/persona/${encodeURIComponent(this.selectedId)}/telemetry`, {
          method: 'POST',
          headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify(body),
        });
        this.setStatus('Saved', { clearAfter: 2000 });
        await this.reload({ preserveSelection: true });
      } catch (err) {
        console.error('persona telemetry update failed', err);
        this.setStatus('Update failed', { clearAfter: 4000 });
      }
    }

    handleApplyAllClick(event) {
      if (event && typeof event.preventDefault === 'function') event.preventDefault();
      this.applyTelemetryToAll().catch((err) => console.warn('persona telemetry apply-all failed', err));
    }

    async applyTelemetryToAll() {
      if (!this.selectedId || !this.items.length) return;
      if (!this.enable) return;
      const enabled = !!this.enable.checked;
      const scope = this.scope ? this.scope.value.trim() : '';
      const normalize = (value) => (typeof value === 'string' ? value.trim() : '');
      const scopedInput = normalize(scope);
      const hasExplicitScope = scopedInput.length > 0;
      const items = Array.isArray(this.items) ? [...this.items] : [];
      let skipped = 0;
      const targets = [];
      for (const item of items) {
        if (!item || !item.id) continue;
        const telemetry = item.preferences?.telemetry?.vibe || {};
        const currentEnabled = telemetry.enabled === true;
        const currentScope = normalize(
          typeof telemetry.scope === 'string' && telemetry.scope.trim().length
            ? telemetry.scope
            : ''
        );
        const ownerScope = normalize(item.owner_kind);
        let expectedScope = '';
        if (hasExplicitScope) {
          expectedScope = scopedInput;
        } else if (enabled) {
          expectedScope = ownerScope;
        } else {
          expectedScope = currentScope;
        }
        if (currentEnabled === enabled && currentScope === expectedScope) {
          skipped += 1;
          continue;
        }
        targets.push(item);
      }
      if (!targets.length) {
        const message = skipped
          ? `All ${skipped} persona(s) already match`
          : 'No personas require updates';
        this.setStatus(message, { clearAfter: 4000 });
        return;
      }
      const confirmDescriptionParts = [
        `Propagate the current scope and consent to ${targets.length} persona(s). Existing telemetry preferences will be overwritten for those personas.`,
      ];
      if (skipped) {
        confirmDescriptionParts.push(`${skipped} persona(s) already match and will be skipped.`);
      }
      const confirmed = await ARW.modal.confirm({
        title: 'Apply to all personas?',
        description: confirmDescriptionParts.join(' '),
        submitLabel: 'Apply to all',
        cancelLabel: 'Cancel',
      });
      if (!confirmed) return;
      this.setStatus('Applying…');
      this.setBusy(true);
      const body = { enabled };
      if (scope) body.scope = scope;
      const payload = JSON.stringify(body);
      let failures = 0;
      let updated = 0;
      const failureNotices = [];
      try {
        for (const item of targets) {
          try {
            await this.fetchJson(`/admin/persona/${encodeURIComponent(item.id)}/telemetry`, {
              method: 'POST',
              headers: { 'Content-Type': 'application/json' },
              body: payload,
            });
            updated += 1;
          } catch (err) {
            failures += 1;
            const label =
              (item && typeof item.name === 'string' && item.name.trim()) ? item.name.trim() : item.id;
            if (failureNotices.length < 3) {
              failureNotices.push(label);
            }
            console.warn('persona telemetry bulk update failed', err);
          }
        }
        if (failureNotices.length) {
          failureNotices.forEach((label) => {
            ARW.toast(`Failed to update persona ${label}. Check service logs for details.`);
          });
          if (failures > failureNotices.length) {
            ARW.toast(`${failures - failureNotices.length} additional persona update failure(s).`);
          }
        }
        const summaryParts = [`Applied to ${updated}/${targets.length} persona(s)`];
        if (skipped) summaryParts.push(`skipped ${skipped}`);
        if (failures) summaryParts.push(`${failures} failed`);
        const clearAfter = failures ? 6000 : 4000;
        this.setStatus(summaryParts.join(' · '), { clearAfter });
        await this.reload({ preserveSelection: true });
      } finally {
        this.setBusy(false);
      }
    }

    handleSelectChange() {
      if (!this.select) return;
      const id = this.select.value || '';
      const entry = this.items.find((item) => item.id === id) || null;
      this.applyPersonaEntry(entry);
      if (entry) {
        this.loadDetails(entry.id).catch((err) => console.warn('persona telemetry refresh failed', err));
      } else {
        this.clearDetails();
      }
    }

    handleRefreshClick(event) {
      if (event && typeof event.preventDefault === 'function') event.preventDefault();
      this.reload({ preserveSelection: true }).catch((err) => console.warn('persona panel refresh failed', err));
    }

    handleSaveClick(event) {
      if (event && typeof event.preventDefault === 'function') event.preventDefault();
      this.saveTelemetry().catch((err) => console.warn('persona telemetry save failed', err));
    }

    scheduleRefresh() {
      if (this.refreshTimer) return;
      this.refreshTimer = setTimeout(() => {
        this.refreshTimer = null;
        if (this.selectedId) {
          this.loadDetails(this.selectedId, { skipLoading: true }).catch((err) => console.warn('persona telemetry refresh failed', err));
        }
      }, 400);
    }

    onFeedback(personaId) {
      if (!this.initialized || this.disabled) return;
      if (!personaId || personaId !== this.selectedId) return;
      this.scheduleRefresh();
    }

    fetchJson(path, init) {
      const base = this.getBase();
      if (!base) throw new Error('Base URL unavailable');
      return ARW.http.json(base, path, init);
    }
  }

  return {
    attach(options = {}) {
      const panel = new PersonaPanel(options);
      if (!panel.disabled) {
        panels.add(panel);
        ensureSse();
      }
      return panel;
    },
  };
})();








