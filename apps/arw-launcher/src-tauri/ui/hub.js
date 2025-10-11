const RUNTIME_ACCEL_LABELS = {
  cpu: 'CPU',
  gpu_cuda: 'GPU (CUDA)',
  gpu_rocm: 'GPU (ROCm)',
  gpu_metal: 'GPU (Metal)',
  gpu_vulkan: 'GPU (Vulkan)',
  npu_directml: 'NPU (DirectML)',
  npu_coreml: 'NPU (CoreML)',
  npu_other: 'NPU',
  other: 'Other',
  unspecified: 'Unspecified',
};

const RUNTIME_MODALITY_LABELS = {
  audio: 'Audio',
  text: 'Text',
  vision: 'Vision',
};

const CONSENT_MODALITY_MESSAGES = {
  audio: 'Capture microphone or audio streams only after participants acknowledge the consent overlay.',
  vision: 'Screen or camera capture must present the consent banner before streaming begins.',
};

const HARDWARE_TIPS = {
  cpu: 'Keep at least four performance cores available and avoid CPU throttling during long runs.',
  gpu_cuda: 'Install current NVIDIA drivers with matching CUDA libraries and ensure sufficient VRAM headroom.',
  gpu_rocm: 'Confirm the ROCm stack is installed and that the GPU model is on the supported compatibility list.',
  gpu_metal: 'Requires macOS with a Metal-capable GPU; keep the display awake to maintain capture consent surfaces.',
  gpu_vulkan: 'Ensure Vulkan runtimes and drivers are present; restart after driver updates to refresh detection.',
  npu_directml: 'Verify DirectML is enabled and GPU drivers are up to date on Windows before starting these bundles.',
  npu_coreml: 'Use Apple Silicon devices with Core ML support and connect to AC power for sustained workloads.',
  npu_other: 'Confirm the vendor NPU runtime stack matches the bundle manifest before promotion.',
  other: 'Review the runtime manifest for vendor-specific accelerator requirements.',
  unspecified: 'Declare accelerator requirements in the runtime manifest so operators can stage the right hardware.',
};

const CONSENT_TAG_REQUIRED = 'consent.required';
const CONSENT_TAG_MODALITIES = 'consent.modalities';
const CONSENT_TAG_MODALITIES_FLAT = 'consent.modalities_flat';
const CONSENT_TAG_NOTE = 'consent.note';

const CAN_OPEN_LOCAL = !!(ARW.env && ARW.env.isTauri);

const updateBaseMeta = () => ARW.applyBaseMeta({ portInputId: 'port', badgeId: 'baseBadge', label: 'Base' });

document.addEventListener('DOMContentLoaded', async () => {
  await ARW.applyPortFromPrefs('port');
  let hubPrefs = {};
  try {
    hubPrefs = (await ARW.getPrefs('ui:hub')) || {};
  } catch {
    hubPrefs = {};
  }
  let meta = updateBaseMeta();
  let port = ARW.getPortFromInput('port') || meta.port || 8091;
  const ensureLane = (list, lane, opts = {}) => {
    const lanes = Array.isArray(list) ? [...list] : [];
    if (lanes.includes(lane)) return lanes;
    if (opts.after && lanes.includes(opts.after)) {
      const idx = lanes.indexOf(opts.after);
      lanes.splice(idx + 1, 0, lane);
      return lanes;
    }
    lanes.unshift(lane);
    return lanes;
  };
  let base = ARW.base(port);
  let curProj = typeof hubPrefs.lastProject === 'string' ? hubPrefs.lastProject : null;
  const baseLanes = ensureLane(['timeline','context','provenance','policy','metrics','models','activity'], 'provenance', { after: 'context' });
  const expertLaneProfileBase = ensureLane(baseLanes, 'approvals', { after: 'timeline' });
  const guidedLaneProfileBase = ['timeline', 'context', 'activity'];
  const lanesForMode = (mode) => {
    const normalized = mode === 'expert' ? expertLaneProfileBase : guidedLaneProfileBase;
    return Array.from(new Set(normalized));
  };
  const arraysEqual = (a = [], b = []) => a.length === b.length && a.every((value, index) => value === b[index]);
  let activeMode = (ARW.mode && ARW.mode.current === 'expert') ? 'expert' : 'guided';
  let currentLaneProfile = lanesForMode(activeMode);
  let sidecarSource = 'initial';
  let sc = null;
  const mountSidecar = (lanes, options = {}) => {
    const profile = Array.isArray(lanes) && lanes.length ? Array.from(new Set(lanes)) : lanesForMode(activeMode);
    const force = options.force === true;
    if (!force && arraysEqual(profile, currentLaneProfile)) {
      return;
    }
    try {
      sc?.dispose?.();
    } catch {}
    const node = document.getElementById('sidecar');
    if (node) {
      node.innerHTML = '';
    }
    sc = ARW.sidecar.mount('sidecar', profile, { base, getProject: () => curProj });
    currentLaneProfile = profile;
    sidecarSource = options.source || sidecarSource;
  };
  mountSidecar(currentLaneProfile, { source: 'initial', force: true });
  const elRuntimeBadge = document.getElementById('runtimeBadge');
  const elRuntimeTable = document.getElementById('runtimeTbl');
  const elRuntimeEmpty = document.getElementById('runtimeEmpty');
  const elRuntimeRefreshBtn = document.getElementById('runtimeRefresh');
  const elRuntimeCopyBtn = document.getElementById('runtimeCopyAll');
  const elRuntimeFocusBtn = document.getElementById('runtimeFocusTable');
  const elRuntimeAuto = document.getElementById('runtimeAuto');
  const elRuntimeStat = document.getElementById('runtimeStat');
  const elRuntimeRunbook = document.getElementById('runtimeRunbook');
  const elRuntimeMatrix = document.getElementById('runtimeMatrix');
  const elRuntimeBundles = document.getElementById('runtimeBundles');
  const elRuntimeConsentHint = document.getElementById('runtimeConsentHint');
  const elRuntimeHardwareHint = document.getElementById('runtimeHardwareHint');
  const elRuntimeHints = document.getElementById('runtimeHints');
  const elRuntimeAnnounce = document.getElementById('runtimeAnnounce');
  if (elRuntimeFocusBtn && elRuntimeTable) {
    elRuntimeFocusBtn.setAttribute('aria-label', 'Focus runtime table');
    elRuntimeFocusBtn.addEventListener('click', () => {
      try {
        elRuntimeTable.focus();
      } catch {}
      if (elRuntimeTable && typeof elRuntimeTable.scrollIntoView === 'function') {
        try {
          elRuntimeTable.scrollIntoView({ behavior: 'smooth', block: 'center' });
        } catch {
          elRuntimeTable.scrollIntoView();
        }
      }
    });
  }
  const runtimePending = new Set();
  const updateModeUi = (mode, { remount = false } = {}) => {
    const normalized = mode === 'expert' ? 'expert' : 'guided';
    const shouldRemount = remount || normalized !== activeMode;
    activeMode = normalized;
    if (shouldRemount) {
      mountSidecar(lanesForMode(activeMode), { force: true, source: 'mode' });
    }
    if (elRuntimeRunbook) {
      if (normalized === 'expert') {
        elRuntimeRunbook.open = true;
      } else {
        elRuntimeRunbook.open = false;
      }
    }
  };
  updateModeUi(activeMode);
  if (ARW.mode && typeof ARW.mode.subscribe === 'function') {
    ARW.mode.subscribe((modeValue) => {
      updateModeUi(modeValue, { remount: true });
    });
  }

  function setRuntimeBadge(text, level = 'neutral', hint = '') {
    if (!elRuntimeBadge) return;
    const cls = level === 'warn' ? 'badge warn' : 'badge';
    elRuntimeBadge.className = cls;
    elRuntimeBadge.textContent = text;
    elRuntimeBadge.title = hint || text;
    elRuntimeBadge.setAttribute('aria-label', hint ? `${text}. ${hint}` : text);
  }

  function setRuntimeStat(text, sticky = false) {
    if (!elRuntimeStat) return;
    elRuntimeStat.textContent = text || '';
    if (!text || sticky) return;
    setTimeout(() => {
      if (elRuntimeStat.textContent === text) {
        elRuntimeStat.textContent = '';
      }
    }, 2000);
  }

  function joinList(values = []) {
    const unique = Array.from(new Set(values.filter(Boolean)));
    if (!unique.length) return '';
    if (unique.length === 1) return unique[0];
    if (unique.length === 2) return `${unique[0]} and ${unique[1]}`;
    return `${unique.slice(0, -1).join(', ')}, and ${unique.at(-1)}`;
  }

  function runtimeDisplayName(descriptor = {}) {
    return descriptor.name || descriptor.id || 'runtime';
  }

  function setRuntimeHint(element, message, tone = 'info') {
    if (!element) return;
    element.textContent = message || '';
    element.title = message || '';
    element.classList.remove('warn');
  if (tone === 'warn') {
    element.classList.add('warn');
  }
}

  function parseBoolean(value) {
    if (value === true) return true;
    if (value === false) return false;
    if (value == null) return null;
    const normalized = String(value).trim().toLowerCase();
    if (!normalized) return null;
    if (['1', 'true', 'yes', 'required', 'on'].includes(normalized)) return true;
    if (['0', 'false', 'no', 'optional', 'off'].includes(normalized)) return false;
    return null;
  }

  function extractConsentInfo(tags) {
    const info = {
      required: null,
      modalities: [],
      note: '',
      annotated: false,
    };
    if (!tags || typeof tags !== 'object') {
      return info;
    }
    const requiredValue = tags[CONSENT_TAG_REQUIRED];
    const parsedRequired = parseBoolean(requiredValue);
    if (parsedRequired !== null) {
      info.required = parsedRequired;
      info.annotated = true;
    } else if (typeof requiredValue === 'string' && requiredValue.trim()) {
      info.annotated = true;
    }

    let modalitiesRaw = [];
    const modalJson = tags[CONSENT_TAG_MODALITIES];
    if (typeof modalJson === 'string' && modalJson.trim()) {
      try {
        const parsed = JSON.parse(modalJson);
        if (Array.isArray(parsed)) {
          modalitiesRaw = parsed;
        }
      } catch {
        // fall back to flat key below
      }
    }
    if (!modalitiesRaw.length) {
      const flat = tags[CONSENT_TAG_MODALITIES_FLAT];
      if (typeof flat === 'string' && flat.trim()) {
        modalitiesRaw = flat
          .split(',')
          .map((entry) => entry.trim())
          .filter(Boolean);
      }
    }
    if (modalitiesRaw.length) {
      info.modalities = Array.from(
        new Set(
          modalitiesRaw
            .map((mode) => String(mode || '').trim().toLowerCase())
            .filter(Boolean),
        ),
      );
      info.annotated = true;
    }

    const note = tags[CONSENT_TAG_NOTE];
    if (typeof note === 'string' && note.trim()) {
      info.note = note.trim();
      info.annotated = true;
    }
    return info;
  }

  function buildConsentHint(runtimes) {
    const requiredList = [];
    const missingList = [];
    const optionalList = [];
    for (const entry of runtimes) {
      const descriptor = entry?.descriptor || {};
      const name = runtimeDisplayName(descriptor);
      const modalities = Array.isArray(descriptor.modalities)
        ? descriptor.modalities.map((mode) => String(mode || '').toLowerCase())
        : [];
      const tags =
        descriptor.tags && typeof descriptor.tags === 'object' ? descriptor.tags : null;
      const consentInfo = extractConsentInfo(tags);
      const consentModalities = consentInfo.modalities.length
        ? consentInfo.modalities
        : modalities.filter((mode) => mode === 'audio' || mode === 'vision');
      const needsConsentByModality = modalities.some(
        (mode) => mode === 'audio' || mode === 'vision',
      );
      if (consentInfo.required === true && consentModalities.length) {
        requiredList.push({
          name,
          modalities: consentModalities,
          note: consentInfo.note,
        });
      } else if (needsConsentByModality && consentInfo.required === null && !consentInfo.annotated) {
        missingList.push({
          name,
          modalities: modalities.filter((mode) => mode === 'audio' || mode === 'vision'),
        });
      } else if (consentInfo.required === false && consentModalities.length) {
        optionalList.push({
          name,
          modalities: consentModalities,
          note: consentInfo.note,
        });
      }
    }
    if (!requiredList.length && !missingList.length && !optionalList.length) {
      return {
        message: 'Consent: Text-only runtimes detected; no additional consent prompt required before start.',
        tone: 'info',
      };
    }
    const statements = [];
    if (requiredList.length) {
      const segments = requiredList.map((item) => {
        const label =
          runtimeModalitiesLabel(item.modalities) || joinList(item.modalities);
        const segment = label ? `${item.name} (${label})` : item.name;
        return item.note ? `${segment} – ${item.note}` : segment;
      });
      const requirements = joinList(segments);
      const guidance = [];
      if (requiredList.some((item) => item.modalities.includes('audio'))) {
        guidance.push(CONSENT_MODALITY_MESSAGES.audio);
      }
      if (requiredList.some((item) => item.modalities.includes('vision'))) {
        guidance.push(CONSENT_MODALITY_MESSAGES.vision);
      }
      const guidanceText = guidance.join(' ');
      statements.push(
        `Consent required: ${requirements}. ${guidanceText ||
          'Confirm the overlay acknowledgement before restoring these runtimes.'}`,
      );
    }
    if (optionalList.length && !requiredList.length) {
      const segments = optionalList.map((item) => {
        const label =
          runtimeModalitiesLabel(item.modalities) || joinList(item.modalities);
        const segment = label ? `${item.name} (${label})` : item.name;
        return item.note ? `${segment} – ${item.note}` : segment;
      });
      statements.push(
        `Consent overlays optional: ${joinList(segments)}.`,
      );
    }
    if (missingList.length) {
      const segments = missingList.map((item) => {
        const label =
          runtimeModalitiesLabel(item.modalities) || joinList(item.modalities);
        return label ? `${item.name} (${label})` : item.name;
      });
      statements.push(
        `Add consent metadata for ${joinList(segments)} so the launcher can verify overlays before activation.`,
      );
    }
    const tone = requiredList.length || missingList.length ? 'warn' : 'info';
    return { message: statements.join(' ').trim(), tone };
  }

  function buildHardwareHint(runtimes) {
    const groups = new Map();
    const issues = [];
    for (const entry of runtimes) {
      const descriptor = entry?.descriptor || {};
      const status = entry?.status || {};
      const name = runtimeDisplayName(descriptor);
      let accelerator = descriptor.accelerator;
      accelerator = accelerator ? String(accelerator).toLowerCase() : '';
      if (!accelerator) accelerator = 'unspecified';
      if (!groups.has(accelerator)) {
        groups.set(accelerator, []);
      }
      groups.get(accelerator).push(name);
      const summary = String(status.summary || '');
      const detailLines = Array.isArray(status.detail) ? status.detail : [];
      const combined = `${summary} ${detailLines.join(' ')}`.toLowerCase();
      const hardwareWarning =
        (combined.includes('gpu') || combined.includes('accelerator')) &&
        (combined.includes('not available') ||
          combined.includes('not detected') ||
          combined.includes('missing') ||
          combined.includes('driver') ||
          combined.includes('simulated'));
      if (hardwareWarning) {
        issues.push(`${name}: ${summary || 'hardware attention required.'}`);
      }
    }
    const groupParts = [];
    for (const [accel, names] of groups.entries()) {
      const label = runtimeAcceleratorLabel(accel);
      const list = joinList(names);
      if (!list) continue;
      groupParts.push(`${label}: ${list}`);
    }
    groupParts.sort();
    let message = '';
    if (groupParts.length) {
      message = `Hardware: ${groupParts.join('; ')}.`;
    } else {
      message = 'Hardware: Runtime manifests did not expose accelerator metadata.';
    }
    const tips = new Set();
    for (const accel of groups.keys()) {
      const tipKey = accel || 'unspecified';
      const tip = HARDWARE_TIPS[tipKey] || HARDWARE_TIPS.unspecified;
      if (tip) tips.add(tip);
    }
    if (tips.size) {
      message += ` Tips: ${Array.from(tips).join(' ')}`;
    }
    if (issues.length) {
      message += ` Warnings: ${issues.join(' ')}`;
    }
    const tone = issues.length ? 'warn' : 'info';
    return { message: message.trim(), tone };
  }

  function updateRuntimeHints(runtimes) {
    if (!elRuntimeHints || !elRuntimeConsentHint || !elRuntimeHardwareHint) return;
    if (!Array.isArray(runtimes) || runtimes.length === 0) {
      elRuntimeHints.hidden = true;
      setRuntimeHint(elRuntimeConsentHint, '');
      setRuntimeHint(elRuntimeHardwareHint, '');
      return;
    }
    elRuntimeHints.hidden = false;
    const consent = buildConsentHint(runtimes);
    const hardware = buildHardwareHint(runtimes);
    setRuntimeHint(elRuntimeConsentHint, consent.message, consent.tone);
    setRuntimeHint(elRuntimeHardwareHint, hardware.message, hardware.tone);
  }

  let lastRuntimeAnnouncement = '';
  function setRuntimeAnnouncement(message) {
    if (!elRuntimeAnnounce) return;
    if (message === lastRuntimeAnnouncement) return;
    lastRuntimeAnnouncement = message;
    elRuntimeAnnounce.textContent = message || '';
  }

  setRuntimeBadge('Runtime: loading…');
  setRuntimeAnnouncement('Runtime snapshot loading.');

  function formatReset(ts) {
    try {
      const dt = new Date(ts);
      if (!Number.isFinite(dt.getTime())) return String(ts || '');
      const opts = { hour: '2-digit', minute: '2-digit' };
      return `${dt.toLocaleDateString(undefined, { month: 'short', day: 'numeric' })} ${dt.toLocaleTimeString(undefined, opts)}`;
    } catch {
      return String(ts || '');
    }
  }

  let runtimeModel = null;
  function renderRuntimeSupervisor(model) {
    runtimeModel = model || runtimeModel;
    const snapshot = runtimeModel;
    const runtimes = Array.isArray(snapshot?.runtimes) ? snapshot.runtimes : [];
    if (elRuntimeTable) {
      renderRuntimeTable(runtimes);
    } else if (elRuntimeEmpty) {
      elRuntimeEmpty.style.display = runtimes.length ? 'none' : '';
    }
    updateRuntimeHints(runtimes);
    if (elRuntimeFocusBtn) {
      elRuntimeFocusBtn.disabled = !runtimes.length;
    }
    if (!runtimes.length) {
      setRuntimeAnnouncement('Runtime snapshot cleared. No managed runtimes registered.');
    }
    if (!elRuntimeBadge) return;
    if (!runtimes.length) {
      setRuntimeBadge('Runtime: none', 'neutral', 'No managed runtimes registered');
      return;
    }
    let readyCount = 0;
    let warn = false;
    let err = false;
    let minRemaining = null;
    let resetAt = null;
    const details = [];
    for (const entry of runtimes) {
      const descriptor = entry?.descriptor || {};
      const status = entry?.status || {};
      const name = descriptor.name || descriptor.id || 'runtime';
      const stateInfo = ARW.runtime.state(status.state);
      const severityInfo = ARW.runtime.severity(status.severity);
      const state = stateInfo.slug;
      const severity = severityInfo.slug;
      if (state === 'ready') readyCount += 1;
      if (state === 'error' || severity === 'error' || state === 'offline') {
        err = true;
      } else if (state === 'degraded' || severity === 'warn') {
        warn = true;
      }
      const budget = status.restart_budget;
      if (budget && typeof budget === 'object') {
        const remaining = Number(budget.remaining ?? NaN);
        if (Number.isFinite(remaining)) {
          if (minRemaining == null || remaining < minRemaining) {
            minRemaining = remaining;
            resetAt = budget.reset_at || resetAt;
          } else if (remaining === minRemaining && !resetAt && budget.reset_at) {
            resetAt = budget.reset_at;
          }
        }
        if (remaining === 0) warn = true;
      }
      const summary = status.summary || stateInfo.label;
      let detail = `${name}: ${summary} (${stateInfo.label} / ${state})`;
      if (budget && typeof budget === 'object') {
        const used = Number(budget.used ?? NaN);
        const max = Number(budget.max_restarts ?? NaN);
        const remaining = Number(budget.remaining ?? NaN);
        const parts = [];
        if (Number.isFinite(remaining) && Number.isFinite(max)) {
          parts.push(`${remaining}/${max} restarts left`);
        }
        if (budget.reset_at) {
          parts.push(`reset ${formatReset(budget.reset_at)}`);
        }
        if (parts.length) {
          detail += ` — ${parts.join(', ')}`;
        }
      }
      details.push(detail);
    }
    const total = runtimes.length;
    const badgeParts = [`${readyCount}/${total} ready`];
    if (minRemaining != null) {
      const plural = minRemaining === 1 ? '' : 's';
      badgeParts.push(`${minRemaining} restart${plural} left`);
    }
    if (resetAt) {
      badgeParts.push(`resets ${formatReset(resetAt)}`);
    }
    const title = details.join('\n');
    const level = err ? 'warn' : warn ? 'warn' : 'neutral';
    const announceParts = [`Runtime snapshot updated: ${readyCount} of ${total} ready`];
    if (minRemaining != null) {
      const plural = minRemaining === 1 ? '' : 's';
      announceParts.push(`${minRemaining} restart${plural} remaining`);
    }
    if (resetAt) {
      announceParts.push(`next reset ${formatReset(resetAt)}`);
    }
    setRuntimeAnnouncement(announceParts.join('. '));
    setRuntimeBadge(`Runtime: ${badgeParts.join(' · ')}`, level, title);
  }

  let runtimeMatrixModel = null;
  function renderRuntimeMatrix(model) {
    runtimeMatrixModel = model || runtimeMatrixModel;
    if (!elRuntimeMatrix) return;
    const items = runtimeMatrixModel && runtimeMatrixModel.items ? runtimeMatrixModel.items : {};
    const keys = Object.keys(items || {});
    if (!keys.length) {
      elRuntimeMatrix.innerHTML = '<div class="dim">No matrix data reported.</div>';
      return;
    }
    const cards = document.createElement('div');
    cards.className = 'matrix-grid';
    for (const key of keys.sort()) {
      const entry = items[key];
      if (!entry || typeof entry !== 'object') continue;
      const card = document.createElement('div');
      card.className = 'matrix-card';
      const status = entry.status || {};
      const severity = String(status.severity || '').toLowerCase();
      if (severity === 'error') card.classList.add('bad');
      else if (severity === 'warn') card.classList.add('warn');

      const title = document.createElement('div');
      title.className = 'matrix-title';
      const heading = document.createElement('span');
      heading.textContent = entry.target || key;
      const statusLabel = document.createElement('span');
      statusLabel.className = 'dim';
      statusLabel.textContent = status.label || status.severity_label || status.severity || 'unknown';
      title.appendChild(heading);
      title.appendChild(statusLabel);
      card.appendChild(title);

      const meta = document.createElement('div');
      meta.className = 'matrix-meta';
      const detailLines = Array.isArray(status.detail) ? status.detail.filter(Boolean) : [];
      if (detailLines.length) {
        const detail = document.createElement('div');
        detail.textContent = detailLines[0];
        meta.appendChild(detail);
      }
      if (entry.runtime && entry.runtime.restart_pressure && entry.runtime.restart_pressure.length) {
        const pressure = document.createElement('div');
        pressure.textContent = entry.runtime.restart_pressure.join('; ');
        meta.appendChild(pressure);
      }
      const updated = entry.generated || entry.runtime?.updated;
      if (updated) {
        const ts = document.createElement('div');
        ts.textContent = `updated ${formatRelativeIso(updated) || updated}`;
        ts.title = updated;
        meta.appendChild(ts);
      }
      if (entry.http) {
        const http = entry.http;
        const parts = [];
        if (Number.isFinite(http.avg_ewma_ms)) parts.push(`ewma ${Math.round(http.avg_ewma_ms)} ms`);
        if (Number.isFinite(http.error_rate)) parts.push(`errors ${(http.error_rate * 100).toFixed(1)}%`);
        if (http.slow_routes && http.slow_routes.length) parts.push(`slow ${http.slow_routes.slice(0,2).join(', ')}`);
        if (parts.length) {
          const httpDiv = document.createElement('div');
          httpDiv.textContent = parts.join(' · ');
          meta.appendChild(httpDiv);
        }
      }
      if (entry.bus) {
        const bus = document.createElement('div');
        bus.textContent = `bus published ${entry.bus.published}, lagged ${entry.bus.lagged}`;
        meta.appendChild(bus);
      }
      if (entry.events) {
        const events = document.createElement('div');
        events.textContent = `events ${entry.events.total} kinds ${entry.events.kinds}`;
        meta.appendChild(events);
      }
      if (entry.kernel && entry.kernel.enabled === false) {
        const kernel = document.createElement('div');
        kernel.textContent = 'Kernel disabled';
        meta.appendChild(kernel);
      }
      if (entry.runtime && entry.runtime.severity) {
        const severityMap = entry.runtime.severity;
        const render = Object.entries(severityMap)
          .map(([sev, val]) => `${sev}: ${val}`)
          .join(', ');
        if (render) {
          const sevEl = document.createElement('div');
          sevEl.textContent = `runtime ${render}`;
          meta.appendChild(sevEl);
        }
      }
      if (meta.children.length) {
        card.appendChild(meta);
      }
      cards.appendChild(card);
    }
    elRuntimeMatrix.innerHTML = '';
    elRuntimeMatrix.appendChild(cards);
  }

  let runtimeBundlesModel = null;
  function renderRuntimeBundles(model) {
    runtimeBundlesModel = model || runtimeBundlesModel;
    if (!elRuntimeBundles) return;

    const frag = document.createDocumentFragment();
    const installs = Array.isArray(runtimeBundlesModel?.installations)
      ? [...runtimeBundlesModel.installations]
      : [];
    installs.sort((a, b) => {
      const lhs = (a?.name || a?.id || '').toString().toLowerCase();
      const rhs = (b?.name || b?.id || '').toString().toLowerCase();
      return lhs.localeCompare(rhs);
    });

    const summary = document.createElement('div');
    summary.className = 'runtime-bundle-summary';
    if (installs.length) {
      const label = installs.length === 1 ? 'bundle installed' : 'bundles installed';
      summary.textContent = `${installs.length} ${label}`;
    } else {
      summary.textContent = 'No bundles installed yet.';
    }
    frag.appendChild(summary);

    const columns = ['Bundle', 'Adapter', 'Accelerator', 'Profiles', 'Modalities', 'Channel', 'Location', 'Installed'];

    if (installs.length) {
      const table = document.createElement('table');
      table.className = 'runtime-bundle-table';
      const thead = document.createElement('thead');
      const headRow = document.createElement('tr');
      for (const label of columns) {
        const th = document.createElement('th');
        th.scope = 'col';
        th.textContent = label;
        headRow.appendChild(th);
      }
      thead.appendChild(headRow);
      table.appendChild(thead);

      const tbody = document.createElement('tbody');
      for (const inst of installs) {
        const runtimeId = inst?.id || '';
        const displayName = inst?.name || runtimeId || 'Bundle';
        const adapter = inst?.adapter || '—';
        const accelerator = runtimeAcceleratorLabel(inst?.accelerator || '') || '—';
        const profiles = Array.isArray(inst?.profiles) && inst.profiles.length
          ? inst.profiles.join(', ')
          : '—';
        const modalities = runtimeModalitiesLabel(inst?.modalities) || '—';
        const channel = inst?.channel || '—';
        const location = inst?.root || inst?.metadata_path || '—';
        const installedIso = inst?.installed_at || inst?.imported_at || '';
        const installedHuman = formatRelativeIso(installedIso) || installedIso || '—';

        const row = document.createElement('tr');

        const cellName = document.createElement('td');
        const strongName = document.createElement('strong');
        strongName.textContent = displayName;
        cellName.appendChild(strongName);
        if (runtimeId && runtimeId !== displayName) {
          const idLine = document.createElement('div');
          idLine.className = 'mono dim';
          idLine.textContent = runtimeId;
          cellName.appendChild(idLine);
        }
        row.appendChild(cellName);

        const cellAdapter = document.createElement('td');
        cellAdapter.textContent = adapter;
        row.appendChild(cellAdapter);

        const cellAccel = document.createElement('td');
        cellAccel.textContent = accelerator;
        row.appendChild(cellAccel);

        const cellProfiles = document.createElement('td');
        cellProfiles.textContent = profiles;
        row.appendChild(cellProfiles);

        const cellModalities = document.createElement('td');
        cellModalities.textContent = modalities || '—';
        row.appendChild(cellModalities);

        const cellChannel = document.createElement('td');
        cellChannel.textContent = channel || '—';
        row.appendChild(cellChannel);

        const cellLocation = document.createElement('td');
        cellLocation.textContent = location;
        if (location && location !== '—') {
          cellLocation.classList.add('mono');
          cellLocation.title = location;
        }
        row.appendChild(cellLocation);

        const cellInstalled = document.createElement('td');
        cellInstalled.textContent = installedHuman;
        if (installedIso && installedIso !== installedHuman) {
          cellInstalled.title = installedIso;
        }
        row.appendChild(cellInstalled);

        tbody.appendChild(row);

        const detailParts = [];
        if (inst?.artifacts && Array.isArray(inst.artifacts) && inst.artifacts.length) {
          const artifactLabels = inst.artifacts.map((artifact) => {
            const name = artifact?.name || 'artifact';
            if (typeof artifact?.bytes === 'number' && Number.isFinite(artifact.bytes)) {
              return `${name} (${formatBytes(artifact.bytes)})`;
            }
            return name;
          });
          if (artifactLabels.length) {
            detailParts.push(`artifacts ${artifactLabels.join(', ')}`);
          }
        }
        const sourceLabel = describeBundleSource(inst?.source);
        if (sourceLabel) {
          detailParts.push(sourceLabel);
        }
        if (inst?.metadata_path) {
          detailParts.push(`metadata ${inst.metadata_path}`);
        }
        if (inst?.root && inst.root !== location) {
          detailParts.push(`root ${inst.root}`);
        }
        if (detailParts.length) {
          const metaRow = document.createElement('tr');
          metaRow.className = 'runtime-bundle-meta';
          const metaCell = document.createElement('td');
          metaCell.colSpan = columns.length;
          metaCell.textContent = detailParts.join(' · ');
          metaRow.appendChild(metaCell);
          tbody.appendChild(metaRow);
        }
      }

      table.appendChild(tbody);
      frag.appendChild(table);
    } else {
      const empty = document.createElement('div');
      empty.className = 'dim';
      empty.textContent = 'Install a bundle or import artifacts to stage managed runtimes.';
      frag.appendChild(empty);
    }

    const catalogs = Array.isArray(runtimeBundlesModel?.catalogs)
      ? runtimeBundlesModel.catalogs
      : [];
    const catalogBlock = document.createElement('div');
    catalogBlock.className = 'runtime-bundle-catalogs';
    if (catalogs.length) {
      const list = document.createElement('ul');
      list.className = 'runtime-bundle-catalog-list';
      for (const cat of catalogs) {
        const item = document.createElement('li');
        const label = document.createElement('strong');
        label.textContent = cat?.path || 'Catalog';
        item.appendChild(label);
        const metaParts = [];
        if (typeof cat?.version !== 'undefined') metaParts.push(`version ${cat.version}`);
        if (cat?.channel) metaParts.push(`channel ${cat.channel}`);
        if (cat?.notes) metaParts.push(cat.notes);
        if (metaParts.length) {
          const span = document.createElement('span');
          span.className = 'dim';
          span.textContent = metaParts.join(' · ');
          item.appendChild(span);
        }
        list.appendChild(item);
      }
      catalogBlock.appendChild(list);
    } else {
      const none = document.createElement('div');
      none.className = 'dim';
      none.textContent = 'No bundle catalogs discovered yet.';
      catalogBlock.appendChild(none);
    }
    frag.appendChild(catalogBlock);

    const roots = Array.isArray(runtimeBundlesModel?.roots)
      ? runtimeBundlesModel.roots.filter(Boolean)
      : [];
    if (roots.length) {
      const rootsDiv = document.createElement('div');
      rootsDiv.className = 'dim mono';
      rootsDiv.textContent = `Roots: ${roots.join(', ')}`;
      frag.appendChild(rootsDiv);
    }

    elRuntimeBundles.innerHTML = '';
    elRuntimeBundles.appendChild(frag);

    function describeBundleSource(source) {
      if (!source) return '';
      if (typeof source === 'string') {
        return `source ${source}`;
      }
      if (typeof source !== 'object') {
        return '';
      }
      if (Array.isArray(source)) {
        return '';
      }
      const obj = source;
      const parts = [];
      if (typeof obj.kind === 'string') parts.push(obj.kind);
      if (typeof obj.channel === 'string') parts.push(`channel ${obj.channel}`);
      if (parts.length) return parts.join(' ');
      try {
        return `source ${JSON.stringify(obj)}`;
      } catch {
        return '';
      }
    }
  }

  function runtimeAcceleratorLabel(slug) {
    if (!slug) return '';
    const key = String(slug || '').toLowerCase();
    return RUNTIME_ACCEL_LABELS[key] || String(slug);
  }

  function runtimeModalitiesLabel(modalities) {
    if (!Array.isArray(modalities) || modalities.length === 0) return '';
    return modalities
      .map((mode) => {
        const key = String(mode || '').toLowerCase();
        return RUNTIME_MODALITY_LABELS[key] || String(mode || '');
      })
      .filter(Boolean)
      .join(', ');
  }

  function renderRuntimeTable(runtimes) {
    if (!elRuntimeTable) return;
    elRuntimeTable.innerHTML = '';
    if (!Array.isArray(runtimes) || runtimes.length === 0) {
      if (elRuntimeEmpty) {
        elRuntimeEmpty.style.display = '';
      }
      return;
    }
    if (elRuntimeEmpty) {
      elRuntimeEmpty.style.display = 'none';
    }
    for (const entry of runtimes) {
      const descriptor = entry?.descriptor || {};
      const status = entry?.status || {};
      const runtimeId = descriptor.id || status.id;
      if (!runtimeId) continue;
      const prettyName = descriptor.name || runtimeId;
      const tr = document.createElement('tr');
      if (runtimePending.has(runtimeId)) {
        tr.setAttribute('data-runtime-pending', 'true');
      }

      const nameCell = document.createElement('td');
      const nameLabel = document.createElement('strong');
      nameLabel.textContent = prettyName;
      nameCell.appendChild(nameLabel);
      const idEl = document.createElement('div');
      idEl.className = 'dim mono';
      idEl.textContent = runtimeId;
      nameCell.appendChild(idEl);
      if (status.summary) {
        const summary = document.createElement('div');
        summary.className = 'dim';
        summary.textContent = status.summary;
        nameCell.appendChild(summary);
      }
      const detailLines = Array.isArray(status.detail)
        ? status.detail.filter((line) => typeof line === 'string' && line.trim())
        : [];
      if (detailLines.length) {
        const detailsEl = document.createElement('details');
        detailsEl.className = 'runtime-detail';
        const summaryEl = document.createElement('summary');
        summaryEl.textContent = 'Details';
        detailsEl.appendChild(summaryEl);
        const list = document.createElement('ul');
        for (const line of detailLines) {
          const li = document.createElement('li');
          li.textContent = line;
          list.appendChild(li);
        }
        detailsEl.appendChild(list);
        nameCell.appendChild(detailsEl);
      }
      tr.appendChild(nameCell);

      const statusCell = document.createElement('td');
      const stateInfo = ARW.runtime.state(status.state);
      const severityInfo = ARW.runtime.severity(status.severity);
      statusCell.textContent = `${stateInfo.label} · ${severityInfo.label}`;
      if (
        severityInfo.slug === 'error' ||
        stateInfo.slug === 'error' ||
        stateInfo.slug === 'offline'
      ) {
        statusCell.classList.add('bad');
      } else if (severityInfo.slug === 'warn' || stateInfo.slug === 'degraded') {
        statusCell.classList.add('dim');
      }
      tr.appendChild(statusCell);

      const profileCell = document.createElement('td');
      const profileParts = [];
      if (descriptor.profile) profileParts.push(descriptor.profile);
      if (descriptor.adapter) profileParts.push(descriptor.adapter);
      const modalitiesLabel = runtimeModalitiesLabel(descriptor.modalities);
      if (modalitiesLabel) profileParts.push(modalitiesLabel);
      const accel = runtimeAcceleratorLabel(descriptor.accelerator);
      if (accel) profileParts.push(accel);
      profileCell.textContent = profileParts.length ? profileParts.join(' · ') : '–';
      tr.appendChild(profileCell);

      const budgetCell = document.createElement('td');
      let budgetRemaining = null;
      if (status.restart_budget && typeof status.restart_budget === 'object') {
        const budget = status.restart_budget;
        const remaining = Number(budget.remaining ?? NaN);
        const max = Number(budget.max_restarts ?? NaN);
        const used = Number(budget.used ?? NaN);
        const windowSeconds = Number(budget.window_seconds ?? NaN);
        budgetRemaining = Number.isFinite(remaining) ? remaining : null;
        if (Number.isFinite(remaining) && Number.isFinite(max)) {
          budgetCell.textContent = `${remaining}/${max} left`;
        } else {
          budgetCell.textContent = '—';
        }
        const hints = [];
        if (Number.isFinite(used) && Number.isFinite(max)) {
          hints.push(`used ${used} of ${max}`);
        }
        if (Number.isFinite(windowSeconds)) {
          hints.push(`window ${windowSeconds}s`);
        }
        if (budget.reset_at) {
          const rel = formatRelativeIso(budget.reset_at);
          hints.push(rel ? `resets ${rel}` : `resets ${budget.reset_at}`);
        }
        if (hints.length) {
          budgetCell.title = hints.join(' · ');
        }
      } else {
        budgetCell.textContent = '—';
      }
      tr.appendChild(budgetCell);

      const updatedCell = document.createElement('td');
      const updatedAt = status.updated_at;
      updatedCell.textContent = formatIsoWithRelative(updatedAt);
      if (updatedAt) {
        updatedCell.title = updatedAt;
      }
      tr.appendChild(updatedCell);

      const actionsCell = document.createElement('td');
      const actionsWrap = document.createElement('div');
      actionsWrap.className = 'runtime-actions';
      const isPending = runtimePending.has(runtimeId);

      const stateSlug = stateInfo.slug;
      if (stateSlug === 'offline' || stateSlug === 'unknown') {
        const startBtn = document.createElement('button');
        startBtn.className = 'ghost btn-small';
        let startLabel = `Start runtime ${prettyName}`;
        if (isPending) {
          startBtn.textContent = 'Starting…';
          startBtn.disabled = true;
          startLabel = 'Start request in flight';
        } else {
          startBtn.textContent = 'Start';
        }
        startBtn.title = startLabel;
        startBtn.setAttribute('aria-label', startLabel);
        startBtn.addEventListener('click', () => handleRuntimeStart(entry));
        actionsWrap.appendChild(startBtn);
      } else {
        const stopBtn = document.createElement('button');
        stopBtn.className = 'ghost btn-small';
        let stopLabel = `Stop runtime ${prettyName}`;
        if (isPending) {
          stopBtn.textContent = 'Stopping…';
          stopBtn.disabled = true;
          stopLabel = 'Stop request in flight';
        } else {
          stopBtn.textContent = 'Stop';
        }
        stopBtn.title = stopLabel;
        stopBtn.setAttribute('aria-label', stopLabel);
        stopBtn.addEventListener('click', () => handleRuntimeStop(entry));
        actionsWrap.appendChild(stopBtn);
      }

      const restartBtn = document.createElement('button');
      restartBtn.className = 'ghost btn-small';
      let restartLabel = `Request restart for ${prettyName}`;
      if (isPending) {
        restartBtn.textContent = 'Requesting…';
        restartBtn.disabled = true;
        restartLabel = 'Restart request in flight';
      } else {
        restartBtn.textContent = 'Restart';
      }
      let disableReason = '';
      if (budgetRemaining != null && budgetRemaining <= 0) {
        restartBtn.disabled = true;
        disableReason = 'Restart budget exhausted';
      }
      if (isPending) {
        restartBtn.title = restartLabel;
      } else if (disableReason) {
        restartBtn.title = disableReason;
      } else {
        restartBtn.title = restartLabel;
      }
      const ariaRestart = disableReason ? `${disableReason} for ${prettyName}` : restartLabel;
      restartBtn.setAttribute('aria-label', ariaRestart);
      restartBtn.addEventListener('click', () => handleRuntimeRestart(entry));
      actionsWrap.appendChild(restartBtn);

      actionsCell.appendChild(actionsWrap);
      tr.appendChild(actionsCell);

      elRuntimeTable.appendChild(tr);
    }
  }

  const runtimePresetCache = new Map();

async function handleRuntimeRestart(entry) {
    const descriptor = entry?.descriptor || {};
    const status = entry?.status || {};
    const runtimeId = descriptor.id || status.id;
    if (!runtimeId) {
      return;
    }
    const name = descriptor.name || runtimeId;
    try {
      const budget = status.restart_budget || {};
      const remaining = Number(budget.remaining ?? NaN);
      const max = Number(budget.max_restarts ?? NaN);
      const resetHint = budget.reset_at ? formatRelativeIso(budget.reset_at) : '';
      const summary = status.summary || ARW.runtime.state(status.state).label;
      const budgetLine = Number.isFinite(remaining) && Number.isFinite(max)
        ? `${remaining}/${max} restarts left${resetHint ? ` (resets ${resetHint})` : ''}`
        : 'Restart budget not reported';
      const defaultPreset = runtimePresetCache.get(runtimeId) || descriptor.profile || '';
      const presetResult = await ARW.modal.form({
        title: `Restart ${name}`,
        body: `${summary}\n${budgetLine}`,
        submitLabel: 'Continue',
        cancelLabel: 'Cancel',
        focusField: 'preset',
        fields: [
          {
            name: 'preset',
            label: 'Preset label (optional)',
            value: defaultPreset,
            placeholder: 'e.g. high-throughput',
            hint: 'Leave blank to reuse current behavior.',
            autocomplete: 'off',
            trim: true,
          },
        ],
      });
      if (!presetResult) return;
      const presetValue = String(presetResult.preset || '').trim();
      if (presetValue) {
        runtimePresetCache.set(runtimeId, presetValue);
      } else {
        runtimePresetCache.delete(runtimeId);
      }
      const presetLine = presetValue ? `Preset: ${presetValue}` : '';
      const confirmMsg = [`Request a restart for ${name}?`, summary, budgetLine, presetLine]
        .filter(Boolean)
        .join('\n');
      const confirmed = await ARW.modal.confirm({
        title: 'Confirm restart',
        body: confirmMsg,
        submitLabel: 'Restart runtime',
        cancelLabel: 'Cancel',
      });
      if (!confirmed) return;

      runtimePending.add(runtimeId);
      renderRuntimeSupervisor();

      const resp = await ARW.http.fetch(base, `/orchestrator/runtimes/${encodeURIComponent(runtimeId)}/restore`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify(
          presetValue
            ? { restart: true, preset: presetValue }
            : { restart: true }
        ),
      });

      if (resp.status === 429) {
        const denied = await resp.json().catch(() => ({}));
        const deniedBudget = denied?.restart_budget || {};
        const rem = Number(deniedBudget.remaining ?? NaN);
        const maxRestarts = Number(deniedBudget.max_restarts ?? NaN);
        const reason = denied?.reason || 'Restart budget exhausted';
        const note = Number.isFinite(rem) && Number.isFinite(maxRestarts)
          ? `${reason} (${rem}/${maxRestarts} remaining)`
          : reason;
        setRuntimeStat(`${name}: ${note}`, true);
        ARW.toast('Restart denied');
        return;
      }

      if (!resp.ok) {
        throw new Error(`restart failed (${resp.status})`);
      }

      const accepted = await resp.json().catch(() => ({}));
      const acceptedBudget = accepted?.restart_budget || {};
      const rem = Number(acceptedBudget.remaining ?? NaN);
      const maxRestarts = Number(acceptedBudget.max_restarts ?? NaN);
      const note = Number.isFinite(rem) && Number.isFinite(maxRestarts)
        ? `${rem}/${maxRestarts} remaining`
        : 'requested';
      const presetNote = presetValue ? ` preset ${presetValue}` : '';
      setRuntimeStat(`${name}: restart ${note}${presetNote}`, true);
      ARW.toast('Restart requested');
      scheduleRuntimeRefresh(true);
    } catch (err) {
      console.error(err);
      setRuntimeStat(`${name}: restart failed`, true);
      ARW.toast('Restart failed');
    } finally {
      runtimePending.delete(runtimeId);
      renderRuntimeSupervisor();
    }
  }

  let runtimeAbort = null;
  async function refreshRuntimeSupervisor() {
    try {
      if (runtimeAbort) {
        try {
          runtimeAbort.abort();
        } catch {}
      }
      runtimeAbort = new AbortController();
      await fetchReadModel('runtime_supervisor', '/state/runtime_supervisor', {
        signal: runtimeAbort.signal,
        transform(raw) {
          if (raw && typeof raw === 'object' && !Array.isArray(raw)) {
            const out = { ...raw };
            if (!Array.isArray(out.runtimes)) out.runtimes = [];
            return out;
          }
          return { runtimes: [] };
        },
      });
    } catch (err) {
      if (!(err && err.name === 'AbortError')) {
        console.warn('runtime supervisor fetch failed', err);
        setRuntimeBadge('Runtime: unavailable', 'warn', 'Failed to load runtime supervisor snapshot');
      }
    }
  }

let runtimeMatrixAbort = null;
async function refreshRuntimeMatrix() {
  if (!elRuntimeMatrix) return;
    let controller = null;
    try {
      if (runtimeMatrixAbort) {
        try { runtimeMatrixAbort.abort(); } catch {}
      }
      controller = new AbortController();
      runtimeMatrixAbort = controller;
      await fetchReadModel('runtime_matrix', '/state/runtime_matrix', {
        signal: controller.signal,
        transform(raw) {
          if (raw && typeof raw === 'object') {
            const items = raw.items && typeof raw.items === 'object' ? raw.items : {};
            return { items };
          }
          return { items: {} };
        },
      });
    } catch (err) {
      if (!(err && err.name === 'AbortError')) {
        console.warn('runtime matrix fetch failed', err);
      }
    } finally {
      if (runtimeMatrixAbort === controller) {
        runtimeMatrixAbort = null;
      }
    }
  }
}

async function handleRuntimeStart(entry) {
  const descriptor = entry?.descriptor || {};
  const status = entry?.status || {};
  const runtimeId = descriptor.id || status.id;
  if (!runtimeId) return;
  const name = descriptor.name || runtimeId;
  try {
    const confirmed = await ARW.modal.confirm({
      title: 'Start runtime',
      body: `Start ${name}? The managed supervisor will launch the bundle if it has been staged.`,
      submitLabel: 'Start runtime',
      cancelLabel: 'Cancel',
    });
    if (!confirmed) return;

    runtimePending.add(runtimeId);
    renderRuntimeSupervisor();

    const resp = await ARW.http.fetch(
      base,
      `/orchestrator/runtimes/${encodeURIComponent(runtimeId)}/restore`,
      {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ restart: false }),
      }
    );

    if (!resp.ok) {
      throw new Error(`start failed (${resp.status})`);
    }

    setRuntimeStat(`${name}: start requested`, true);
    ARW.toast('Start requested');
    scheduleRuntimeRefresh(true);
  } catch (err) {
    console.error(err);
    setRuntimeStat(`${name}: start failed`, true);
    ARW.toast('Start failed');
  } finally {
    runtimePending.delete(runtimeId);
    renderRuntimeSupervisor();
  }
}

async function handleRuntimeStop(entry) {
  const descriptor = entry?.descriptor || {};
  const status = entry?.status || {};
  const runtimeId = descriptor.id || status.id;
  if (!runtimeId) return;
  const name = descriptor.name || runtimeId;
  try {
    const confirmed = await ARW.modal.confirm({
      title: 'Stop runtime',
      body: `Stop ${name}? Active requests will be interrupted.`,
      submitLabel: 'Stop runtime',
      cancelLabel: 'Cancel',
    });
    if (!confirmed) return;

    runtimePending.add(runtimeId);
    renderRuntimeSupervisor();

    const resp = await ARW.http.fetch(
      base,
      `/orchestrator/runtimes/${encodeURIComponent(runtimeId)}/shutdown`,
      {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
      }
    );

    if (!resp.ok) {
      throw new Error(`stop failed (${resp.status})`);
    }

    setRuntimeStat(`${name}: stop requested`, true);
    ARW.toast('Stop requested');
    scheduleRuntimeRefresh(true);
  } catch (err) {
    console.error(err);
    setRuntimeStat(`${name}: stop failed`, true);
    ARW.toast('Stop failed');
  } finally {
    runtimePending.delete(runtimeId);
    renderRuntimeSupervisor();
  }
}

let runtimeBundlesAbort = null;
async function refreshRuntimeBundles() {
  if (!elRuntimeBundles) return;
  let controller = null;
  try {
    if (runtimeBundlesAbort) {
      try {
        runtimeBundlesAbort.abort();
      } catch {}
    }
    controller = new AbortController();
    runtimeBundlesAbort = controller;
    await fetchReadModel('runtime_bundles', '/state/runtime/bundles', {
      signal: controller.signal,
      transform(raw) {
        if (raw && typeof raw === 'object') {
          const installations = Array.isArray(raw.installations) ? raw.installations : [];
          const catalogs = Array.isArray(raw.catalogs) ? raw.catalogs : [];
          const roots = Array.isArray(raw.roots) ? raw.roots : [];
          return { installations, catalogs, roots };
        }
        return { installations: [], catalogs: [], roots: [] };
      },
    });
  } catch (err) {
    if (!(err && err.name === 'AbortError')) {
      console.warn('runtime bundles fetch failed', err);
    }
  } finally {
    if (runtimeBundlesAbort === controller) {
      runtimeBundlesAbort = null;
    }
  }
}
  }

  let runtimeRefreshScheduled = false;
  function scheduleRuntimeRefresh(force = false) {
    if (!force && elRuntimeAuto && !elRuntimeAuto.checked) return;
    if (runtimeRefreshScheduled) return;
    runtimeRefreshScheduled = true;
    setTimeout(() => {
      runtimeRefreshScheduled = false;
      refreshRuntimeSupervisor();
    }, 400);
  }
  const applyBaseChange = async () => {
    meta = updateBaseMeta();
    port = ARW.getPortFromInput('port') || meta.port || 8091;
    base = ARW.base(port);
    try {
      const prefs = (await ARW.getPrefs('launcher')) || {};
      if (prefs.port !== port) {
        prefs.port = port;
        await ARW.setPrefs('launcher', prefs);
      }
    } catch {}
    mountSidecar(currentLaneProfile, { force: true, source: sidecarSource || 'base-change' });
    ARW.sse.connect(base, { replay: 25 });
    await Promise.allSettled([
      refreshEpisodesSnapshot(),
      refreshProjectsSnapshot(),
      refreshRuntimeSupervisor(),
      refreshRuntimeMatrix(),
      refreshRuntimeBundles(),
      refreshContextSnapshot(),
      refreshContextCascade({ quiet: true }),
      refreshContextMetrics(),
    ]);
  };
  ARW.sse.indicator('sseStat', { prefix: 'SSE' });
  ARW.sse.connect(base, { replay: 25 });
  await refreshRuntimeSupervisor();
  await refreshRuntimeMatrix();
  await refreshRuntimeBundles();
  await refreshContextMetrics();
  if (elRuntimeRefreshBtn) {
    elRuntimeRefreshBtn.addEventListener('click', async () => {
      try {
        setRuntimeStat('Refreshing…');
        await refreshRuntimeSupervisor();
        setRuntimeStat('Snapshot updated', true);
      } catch {
        setRuntimeStat('Refresh failed', true);
      }
    });
  }
  if (elRuntimeCopyBtn) {
    elRuntimeCopyBtn.addEventListener('click', async () => {
      if (!runtimeModel || !Array.isArray(runtimeModel.runtimes)) {
        setRuntimeStat('No snapshot yet', true);
        return;
      }
      try {
        const payload = {
          copied_at: new Date().toISOString(),
          runtimes: runtimeModel.runtimes,
          matrix: runtimeMatrixModel?.items || {},
        };
        await ARW.copy(JSON.stringify(payload, null, 2));
      } catch (err) {
        console.error(err);
        ARW.toast('Copy failed');
      }
    });
  }
  if (elRuntimeAuto) {
    elRuntimeAuto.addEventListener('change', () => {
      if (elRuntimeAuto.checked) {
        scheduleRuntimeRefresh(true);
        setRuntimeStat('Auto refresh on', true);
      } else {
        setRuntimeStat('Auto refresh paused', true);
      }
    });
  }
  // ---------- Runs: episodes list + snapshot ----------
  let runsCache = [];
  let episodesPrimed = false;
  let runSnapshot = null;
  let projectsModel = null;
  const projectsIndex = new Map();
  let projPrefs = {};
  const elRunsTbl = document.getElementById('runsTbl');
  const elRunsStat = document.getElementById('runsStat');
  const elRunFilter = document.getElementById('runFilter');
  const elRunErrOnly = document.getElementById('runErrOnly');
  const elRunActor = document.getElementById('runActorFilter');
  const elRunKind = document.getElementById('runKindFilter');
  const elRunSnap = document.getElementById('runSnap');
  const elRunSnapMeta = document.getElementById('runSnapMeta');
  const elArtifactsTbl = document.getElementById('artifactsTbl');
  const btnRunCopy = document.getElementById('btnRunCopy');
  const btnRunPinA = document.getElementById('btnRunPinA');
  const btnRunPinB = document.getElementById('btnRunPinB');
  const elContextPreview = document.getElementById('contextPreview');
  const elContextSummary = document.getElementById('contextSummary');
  const elContextMeta = document.getElementById('contextMeta');
  const elContextMetricsWrap = document.getElementById('contextMetrics');
  const elContextMetricsUpdated = document.getElementById('contextMetricsUpdated');
  const elContextCoverageStatus = document.getElementById('contextCoverageStatus');
  const elContextCoverageRatio = document.getElementById('contextCoverageRatio');
  const elContextCoverageReasons = document.getElementById('contextCoverageReasons');
  const elContextRecallAvg = document.getElementById('contextRecallAvg');
  const elContextRecallAtRisk = document.getElementById('contextRecallAtRisk');
  const elContextRecallTop = document.getElementById('contextRecallTop');
  const elContextItems = document.getElementById('contextItems');
  const elContextCascade = document.getElementById('contextCascade');
  const elContextStat = document.getElementById('contextStat');
  const elContextIncludeSources = document.getElementById('contextIncludeSources');
  const btnContextRefresh = document.getElementById('btnContextRefresh');
  const btnContextCopy = document.getElementById('btnContextCopy');
  const elContextRehydrateMeta = document.getElementById('contextRehydrateMeta');
  const elContextRehydrateOut = document.getElementById('contextRehydrateOut');
  const elContextRehydrateWrap = document.getElementById('contextRehydrateWrap');
  let contextLastResultText = '';
  let contextModel = null;
  let contextAbort = null;
  let contextPrimed = false;
  let contextMetricsModel = null;
  let contextMetricsAbort = null;
  function formatPercent(value, digits = 0) {
    if (typeof value !== 'number' || !Number.isFinite(value)) return '—';
    return `${(value * 100).toFixed(digits)}%`;
  }
  function formatSlotLabel(value) {
    if (typeof value !== 'string') return 'unknown';
    const trimmed = value.trim();
    if (!trimmed) return 'unknown';
    const replaced = trimmed.replace(/[_-]/g, ' ').replace(/\s+/g, ' ').trim();
    return replaced || trimmed;
  }
  function renderMetricList(target, items, formatter, emptyLabel) {
    const node = typeof target === 'string' ? document.getElementById(target) : target;
    if (!node) return;
    node.innerHTML = '';
    const collection = Array.isArray(items) ? items : [];
    let appended = 0;
    for (const entry of collection) {
      const text = formatter ? formatter(entry) : entry;
      if (!text) continue;
      const li = document.createElement('li');
      li.textContent = text;
      node.appendChild(li);
      appended += 1;
    }
    if (!appended && emptyLabel) {
      const li = document.createElement('li');
      li.className = 'dim';
      li.textContent = emptyLabel;
      node.appendChild(li);
    }
  }
  function normalizeProject(value) {
    return typeof value === 'string' && value.trim() ? value.trim().toLowerCase() : null;
  }
  function filterByProject(records, project) {
    const list = Array.isArray(records) ? records : [];
    const normalized = normalizeProject(project);
    if (!normalized) return list;
    const filtered = list.filter((entry) => normalizeProject(entry && entry.project) === normalized);
    return filtered.length ? filtered : list;
  }
  function selectLatest(records, fallback) {
    if (Array.isArray(records) && records.length) return records[0];
    if (fallback && typeof fallback === 'object') return fallback;
    return null;
  }
  function deriveSlotStats(records) {
    const list = Array.isArray(records) ? records : [];
    const store = new Map();
    for (const entry of list) {
      const slots = entry && entry.components && entry.components.slots;
      if (!slots || typeof slots !== 'object') continue;
      for (const [slot, raw] of Object.entries(slots)) {
        const gap = Number(raw);
        if (!Number.isFinite(gap)) continue;
        const key = typeof slot === 'string' ? slot : '';
        let stats = store.get(key);
        if (!stats) {
          stats = { sum: 0, count: 0, max: 0 };
          store.set(key, stats);
        }
        stats.sum += gap;
        stats.count += 1;
        if (gap > stats.max) stats.max = gap;
      }
    }
    const entries = Array.from(store.entries())
      .filter(([, stats]) => stats.count > 0)
      .map(([slot, stats]) => ({
        slot,
        avg_gap: stats.sum / stats.count,
        max_gap: stats.max,
        samples: stats.count,
      }));
    entries.sort((a, b) => {
      const diff = (b.avg_gap ?? 0) - (a.avg_gap ?? 0);
      if (Math.abs(diff) > 1e-6) return diff;
      return (a.slot || '').localeCompare(b.slot || '');
    });
    return entries.slice(0, 5);
  }
  function applyContextMetrics(model) {
    if (model && typeof model === 'object') {
      contextMetricsModel = model;
    }
    const metrics = contextMetricsModel || {};
    if (!elContextMetricsWrap) return;

    const coverage = metrics.coverage || {};
    const recall = metrics.recall_risk || {};
    const projectKey = normalizeProject(curProj);

    const coverageRecent = filterByProject(coverage.recent, projectKey);
    const recallRecent = filterByProject(recall.recent, projectKey);

    const coverageLatest = selectLatest(coverageRecent, coverage.latest);
    const recallLatest = selectLatest(recallRecent, recall.latest);

    const coverageRatio = (() => {
      if (coverageRecent.length) {
        const flagged = coverageRecent.filter((entry) => entry && entry.needs_more === true).length;
        return coverageRecent.length ? flagged / coverageRecent.length : null;
      }
      return typeof coverage.needs_more_ratio === 'number' ? coverage.needs_more_ratio : null;
    })();

    const coverageStatus = coverageLatest && coverageLatest.needs_more === true;
    if (elContextCoverageStatus) {
      const cls = coverageStatus ? 'metric-pill bad' : 'metric-pill ok';
      elContextCoverageStatus.className = coverageLatest ? cls : 'metric-pill';
      elContextCoverageStatus.textContent = coverageLatest
        ? (coverageStatus ? 'Needs more coverage' : 'Coverage satisfied')
        : 'Awaiting signal';
    }
    if (elContextCoverageRatio) {
      elContextCoverageRatio.textContent = coverageRatio == null ? '—' : formatPercent(coverageRatio, 0);
    }
    ARW.ui.updateRatioBar('contextCoverageRatioBar', coverageRatio, {
      preferLow: true,
      warn: 0.2,
      bad: 0.4,
      formatText: (_v, pct) => `${pct}% of assemblies needing more coverage`,
    });
    const coverageReasons = Array.isArray(coverageLatest?.reasons) ? coverageLatest.reasons.slice(0, 5) : [];
    renderMetricList(
      elContextCoverageReasons,
      coverageReasons,
      (reason) => {
        if (!reason) return null;
        if (typeof reason === 'string' && reason.startsWith('slot_underfilled:')) {
          const slot = reason.split(':')[1] || '';
          return `Slot underfilled — ${formatSlotLabel(slot)}`;
        }
        return String(reason);
      },
      'No recent coverage gaps'
    );

    const recallAvgScore = (() => {
      if (recallRecent.length) {
        let sum = 0;
        let count = 0;
        for (const entry of recallRecent) {
          const score = Number(entry && entry.score);
          if (Number.isFinite(score)) {
            sum += score;
            count += 1;
          }
        }
        return count ? sum / count : null;
      }
      return typeof recall.avg_score === 'number' ? recall.avg_score : null;
    })();
    const recallAtRiskRatio = (() => {
      if (recallRecent.length) {
        const flagged = recallRecent.filter((entry) => entry && entry.at_risk === true).length;
        return recallRecent.length ? flagged / recallRecent.length : null;
      }
      return typeof recall.at_risk_ratio === 'number' ? recall.at_risk_ratio : null;
    })();

    if (elContextRecallAvg) {
      elContextRecallAvg.textContent = recallAvgScore == null ? '—' : formatPercent(recallAvgScore, 0);
    }
    ARW.ui.updateRatioBar('contextRecallAvgBar', recallAvgScore, {
      preferLow: true,
      warn: 0.45,
      bad: 0.7,
      formatText: (_v, pct) => `Risk score ${pct}%`,
    });
    if (elContextRecallAtRisk) {
      elContextRecallAtRisk.textContent = recallAtRiskRatio == null ? '—' : formatPercent(recallAtRiskRatio, 0);
    }
    ARW.ui.updateRatioBar('contextRecallAtRiskBar', recallAtRiskRatio, {
      preferLow: true,
      warn: 0.2,
      bad: 0.4,
      formatText: (_v, pct) => `${pct}% of assemblies flagged at risk`,
    });

    const slotStats = deriveSlotStats(recallRecent);
    const slotItems = slotStats.length ? slotStats : (Array.isArray(recall.top_slots) ? recall.top_slots : []);
    renderMetricList(
      elContextRecallTop,
      slotItems,
      (item) => {
        if (!item || typeof item !== 'object') return null;
        const slotName = formatSlotLabel(item.slot || item.reason || '');
        const avg = typeof item.avg_gap === 'number' ? formatPercent(item.avg_gap, 0) : null;
        const max = typeof item.max_gap === 'number' ? formatPercent(item.max_gap, 0) : null;
        const samples = Number(item.samples ?? item.count ?? 0);
        const pieces = [slotName];
        if (avg) pieces.push(`avg gap ${avg}`);
        if (max) pieces.push(`max ${max}`);
        if (Number.isFinite(samples) && samples > 0) {
          pieces.push(`${samples} sample${samples === 1 ? '' : 's'}`);
        }
        return pieces.join(' · ');
      },
      'No slot gaps recorded'
    );

    if (elContextMetricsUpdated) {
      const iso = coverageLatest?.time || recallLatest?.time || coverage.latest?.time || recall.latest?.time || null;
      const project = coverageLatest?.project || recallLatest?.project || (curProj || null);
      if (iso) {
        const rel = formatRelativeIso(iso) || iso;
        elContextMetricsUpdated.textContent = project
          ? `Metrics updated ${rel} · project ${project}`
          : `Metrics updated ${rel}`;
      } else {
        elContextMetricsUpdated.textContent = project
          ? `Metrics scope: project ${project}`
          : 'Metrics awaiting context activity';
      }
    }
  }
  let cascadeAbort = null;
  let cascadeItems = [];
  if (elContextIncludeSources) {
    elContextIncludeSources.checked = hubPrefs.contextIncludeSources === true;
  }
  const runDetailsOpen = new Set();
  function setRunsStat(txt, sticky=false){
    if (!elRunsStat) return;
    elRunsStat.textContent = txt || '';
    if (!txt || sticky) return;
    setTimeout(()=>{ if (elRunsStat.textContent===txt) elRunsStat.textContent=''; }, 1200);
  }
  function describeRunsStat(count){
    const parts = [`Episodes: ${count}`];
    if (curProj) parts.push(`project=${curProj}`);
    const actorValue = elRunActor && elRunActor.value ? elRunActor.value : '';
    if (actorValue) parts.push(`actor=${actorValue}`);
    if (elRunErrOnly && elRunErrOnly.checked) parts.push('errors only');
    return parts.join(' · ');
  }
  function normArr(v){ return Array.isArray(v) ? v : []; }
  function parseMillis(ts){ try{ const ms = Date.parse(ts); return Number.isFinite(ms) ? ms : null; }catch{return null;} }
  function isErrorEvent(ev){
    try {
      const kind = String(ev?.kind || '').toLowerCase();
      if (kind.includes('error') || kind.includes('failed') || kind.includes('denied')) return true;
      const payload = ev?.payload;
      if (payload && typeof payload === 'object') {
        if (payload.error != null || payload.err != null) return true;
        if (payload.ok === false) return true;
        const status = String(payload.status || '').toLowerCase();
        if (status === 'error' || status === 'failed' || status === 'denied') return true;
      }
    } catch {}
    return false;
  }
  function hydrateEpisode(ep){
    const serverItems = normArr(ep?.items);
    const eventsRaw = normArr(ep?.events);
    const events = serverItems.length ? serverItems : eventsRaw;
    const startTs = ep?.start || events[0]?.time || null;
    const endTs = ep?.end || events[events.length - 1]?.time || null;
    const startMs = parseMillis(startTs);
    const endMs = parseMillis(endTs);
    const durationServer = Number(ep?.duration_ms ?? ep?.durationMs);
    const durationComputed =
      startMs != null && endMs != null && endMs >= startMs ? Math.round(endMs - startMs) : null;
    const duration = Number.isFinite(durationServer) ? durationServer : durationComputed;
    const errorsServer = Number(ep?.errors ?? ep?.error_count ?? ep?.errorCount);
    const errorsComputed = events.reduce((acc, ev) => {
      const flagged = ev?.error === true || isErrorEvent(ev);
      return acc + (flagged ? 1 : 0);
    }, 0);
    const errors = Number.isFinite(errorsServer) ? errorsServer : errorsComputed;
    const countServer = Number(ep?.count);
    const count = Number.isFinite(countServer) ? countServer : events.length;
    const lastTs = ep?.last || endTs || '';
    const firstKind = ep?.first_kind || ep?.firstKind || events[0]?.kind || '';
    const lastKind = ep?.last_kind || ep?.lastKind || events[events.length - 1]?.kind || '';
    const projects = Array.isArray(ep?.projects) ? ep.projects : [];
    const actors = Array.isArray(ep?.actors) ? ep.actors : [];
    const kinds = Array.isArray(ep?.kinds)
      ? ep.kinds
      : Array.from(new Set(events.map((ev) => ev?.kind).filter(Boolean)));
    return {
      ...(ep && typeof ep === 'object' ? ep : {}),
      id: ep?.id || '',
      start: startTs,
      end: endTs,
      duration_ms: duration != null ? duration : null,
      last: lastTs,
      count,
      errors,
      first_kind: firstKind,
      last_kind: lastKind,
      projects,
      actors,
      kinds,
      items: events,
      events,
    };
  }

  async function fetchReadModel(id, path, options = {}) {
    const { signal, transform } = options;
    const fetchOpts = {};
    if (signal) fetchOpts.signal = signal;
    const resp = await ARW.http.fetch(base, path, fetchOpts);
    if (!resp.ok) {
      const err = new Error(`failed to fetch read model: ${id}`);
      err.status = resp.status;
      throw err;
    }
    const raw = await resp.json();
    const model = transform ? transform(raw) : raw;
    ARW.read._store.set(id, model);
    ARW.read._emit(id);
    return model;
  }
  const fetchJson = (path, init) => ARW.http.json(base, path, init);
  const fetchText = (path, init) => ARW.http.text(base, path, init);
  const fetchRaw = (path, init) => ARW.http.fetch(base, path, init);
  function formatDuration(n){
    if (!Number.isFinite(n) || n < 0) return '–';
    if (n >= 1000) return `${(n/1000).toFixed(1)} s`;
    return `${n} ms`;
  }
  function formatBytes(bytes){
    const n = Number(bytes);
    if (!Number.isFinite(n) || n < 0) return '';
    if (n === 0) return '0 B';
    const units = ['B','KiB','MiB','GiB','TiB'];
    let value = n;
    let unit = 0;
    while (value >= 1024 && unit < units.length - 1){
      value /= 1024;
      unit += 1;
    }
    const rounded = unit === 0 || value >= 10 ? Math.round(value) : Math.round(value * 10) / 10;
    const str = unit === 0 || value >= 10 ? String(rounded) : rounded.toFixed(1);
    return `${str} ${units[unit]}`;
  }
  function formatRelativeIso(iso){
    if (!iso) return '';
    try{
      const dt = new Date(iso);
      if (Number.isNaN(dt.getTime())) return '';
      const diffMs = Date.now() - dt.getTime();
      const absSec = Math.round(Math.abs(diffMs) / 1000);
      const units = [
        { limit: 60, div: 1, label: 's' },
        { limit: 3600, div: 60, label: 'm' },
        { limit: 86400, div: 3600, label: 'h' },
        { limit: 2592000, div: 86400, label: 'd' },
        { limit: 31536000, div: 2592000, label: 'mo' },
      ];
      for (const unit of units){
        if (absSec < unit.limit){
          const value = Math.max(1, Math.floor(absSec / unit.div));
          return diffMs >= 0 ? `${value}${unit.label} ago` : `in ${value}${unit.label}`;
        }
      }
      const years = Math.max(1, Math.floor(absSec / 31536000));
      return diffMs >= 0 ? `${years}y ago` : `in ${years}y`;
    }catch{
      return '';
    }
  }
  function formatIsoWithRelative(iso){
    if (!iso) return '–';
    const rel = formatRelativeIso(iso);
    return rel ? `${iso} (${rel})` : iso;
  }
  function summarizeList(values, fallback = '–'){
    const seen = Array.from(new Set((values || []).filter(Boolean)));
    return seen.length ? seen.join(', ') : fallback;
  }
  function buildRunDetails(run){
    const details = document.createElement('details');
    details.className = 'run-details';
    if (run && runDetailsOpen.has(run.id)) {
      details.open = true;
    }
    details.addEventListener('toggle', () => {
      if (!run || !run.id) return;
      if (details.open) {
        runDetailsOpen.add(run.id);
      } else {
        runDetailsOpen.delete(run.id);
      }
    });

    const summary = document.createElement('summary');
    summary.textContent = 'Details';
    if (run?.id) {
      const sr = document.createElement('span');
      sr.className = 'sr-only';
      sr.textContent = ` for run ${run.id}`;
      summary.appendChild(sr);
      summary.setAttribute('aria-label', `Toggle details for run ${run.id}`);
    }
    details.appendChild(summary);

    const list = document.createElement('ul');
    list.className = 'run-meta';

    const addItem = (label, value) => {
      if (!label) return;
      const text = value ? String(value) : null;
      if (!text || !text.trim()) return;
      const li = document.createElement('li');
      const spanLabel = document.createElement('span');
      spanLabel.className = 'run-meta-label';
      spanLabel.textContent = label;
      const spanValue = document.createElement('span');
      spanValue.className = 'run-meta-value';
      spanValue.textContent = text;
      li.appendChild(spanLabel);
      li.appendChild(spanValue);
      list.appendChild(li);
    };

    addItem('Start', formatIsoWithRelative(run?.start));
    addItem('Last', formatIsoWithRelative(run?.last));
    if (run?.duration_ms != null) {
      addItem('Duration', formatDuration(run.duration_ms));
    }
    addItem('Projects', summarizeList(run?.projects));
    addItem('Actors', summarizeList(run?.actors));
    addItem('Kinds', summarizeList(run?.kinds));
    if (run?.first_kind || run?.last_kind) {
      const first = run?.first_kind ? String(run.first_kind) : '–';
      const last = run?.last_kind ? String(run.last_kind) : '–';
      addItem('First → Last', `${first} → ${last}`);
    }

    if (!list.childElementCount) {
      const li = document.createElement('li');
      li.className = 'run-meta-empty';
      li.textContent = 'No additional details available.';
      list.appendChild(li);
    }

    details.appendChild(list);
    return details;
  }
  function updateRunFilterOptions(){
    const actorPrev = elRunActor ? elRunActor.value : '';
    const kindPrev = elRunKind ? elRunKind.value : '';
    if (elRunActor){
      const actors = new Set();
      for (const run of runsCache){
        for (const actor of run.actors || []){
          if (actor) actors.add(actor);
        }
      }
      const sorted = Array.from(actors).sort((a, b) => a.localeCompare(b));
      elRunActor.innerHTML = '';
      const defaultOpt = document.createElement('option');
      defaultOpt.value = '';
      defaultOpt.textContent = 'all actors';
      elRunActor.appendChild(defaultOpt);
      for (const actor of sorted){
        const opt = document.createElement('option');
        opt.value = actor;
        opt.textContent = actor;
        elRunActor.appendChild(opt);
      }
      if (actorPrev && sorted.includes(actorPrev)) {
        elRunActor.value = actorPrev;
      }
    }
    if (elRunKind){
      const kinds = new Set();
      for (const run of runsCache){
        for (const kind of run.kinds || []){
          if (kind) kinds.add(kind);
        }
      }
      const sortedKinds = Array.from(kinds).sort((a, b) => a.localeCompare(b));
      elRunKind.innerHTML = '';
      const defaultOpt = document.createElement('option');
      defaultOpt.value = '';
      defaultOpt.textContent = 'all kinds';
      elRunKind.appendChild(defaultOpt);
      for (const kind of sortedKinds){
        const opt = document.createElement('option');
        opt.value = kind;
        opt.textContent = kind;
        elRunKind.appendChild(opt);
      }
      if (kindPrev && sortedKinds.includes(kindPrev)) {
        elRunKind.value = kindPrev;
      }
    }
  }
  function renderRuns(){
    if (!elRunsTbl) return;
    const q = (elRunFilter?.value||'').toLowerCase();
    const errOnly = !!(elRunErrOnly && elRunErrOnly.checked);
    const actorFilter = (elRunActor?.value || '').toLowerCase();
    const kindFilter = (elRunKind?.value || '').toLowerCase();
    const rows = runsCache.filter(r => {
      if (errOnly && (r.errors|0) === 0) return false;
      if (actorFilter){
        const actors = (r.actors || []).map(a => String(a||'').toLowerCase());
        if (!actors.includes(actorFilter)) return false;
      }
      if (kindFilter){
        const kinds = (r.kinds || []).map(k => String(k||'').toLowerCase());
        if (!kinds.includes(kindFilter)) return false;
      }
      if (!q) return true;
      const haystackParts = [];
      haystackParts.push(String(r.id||''));
      if (Array.isArray(r.projects)) haystackParts.push(r.projects.join(' '));
      if (Array.isArray(r.actors)) haystackParts.push(r.actors.join(' '));
      if (Array.isArray(r.kinds)) haystackParts.push(r.kinds.join(' '));
      if (r.first_kind) haystackParts.push(String(r.first_kind));
      if (r.last_kind) haystackParts.push(String(r.last_kind));
      const haystack = haystackParts.join(' ').toLowerCase();
      if (!haystack.trim()) return false;
      return haystack.includes(q);
    });
    setRunsStat(describeRunsStat(rows.length), true);
    elRunsTbl.innerHTML='';
    for (const r of rows){
      const tr = document.createElement('tr');
      if (r.id) tr.dataset.runId = r.id;
      const id = document.createElement('td'); id.className='mono'; id.textContent = r.id||'';
      const count = document.createElement('td'); count.textContent = r.count||0;
      const dur = document.createElement('td'); dur.textContent = formatDuration(r.duration_ms);
      const err = document.createElement('td'); const errVal = r.errors|0; err.textContent = errVal; if (errVal>0) err.className='bad';
      const info = document.createElement('td');
      info.appendChild(buildRunDetails(r));
      const act = document.createElement('td');
      const view = document.createElement('button');
      view.className='ghost';
      view.textContent='View';
      view.title='View snapshot';
      if (r.id) {
        view.setAttribute('aria-label', `View snapshot for run ${r.id}`);
      }
      view.addEventListener('click', ()=> viewRun(r.id));
      act.appendChild(view);
      tr.appendChild(id); tr.appendChild(count); tr.appendChild(dur); tr.appendChild(err); tr.appendChild(info); tr.appendChild(act);
      elRunsTbl.appendChild(tr);
    }
  }
  function setContextStat(text, sticky = false) {
    if (!elContextStat) return;
    elContextStat.textContent = text || '';
    if (!text || sticky) return;
    setTimeout(() => {
      if (elContextStat.textContent === text) {
        elContextStat.textContent = '';
      }
    }, 1500);
  }
  function truncateText(value, max = 320) {
    const str = String(value ?? '');
    if (str.length <= max) return str;
    return `${str.slice(0, Math.max(0, max - 1))}…`;
  }
  function pickFirstString(obj, keys = []) {
    if (!obj || typeof obj !== 'object') return null;
    for (const key of keys) {
      const value = obj[key];
      if (typeof value === 'string' && value.trim()) {
        return value.trim();
      }
    }
    return null;
  }
  function contextLabel(item, index = 0) {
    if (typeof item === 'string' && item.trim()) {
      return truncateText(item.trim(), 80);
    }
    if (!item || typeof item !== 'object') {
      return `Item ${index + 1}`;
    }
    const label =
      pickFirstString(item, ['title', 'name', 'heading']) ||
      pickFirstString(item, ['summary', 'text']) ||
      (typeof item.id === 'string' && item.id.trim() ? item.id.trim() : '') ||
      (typeof item.key === 'string' && item.key.trim() ? item.key.trim() : '');
    if (label) return truncateText(label, 80);
    if (typeof item.value === 'string' && item.value.trim()) {
      return truncateText(item.value.trim(), 80);
    }
    return `Item ${index + 1}`;
  }
  function contextSnippet(item) {
    if (!item) return '';
    if (typeof item === 'string') {
      return truncateText(item, 320);
    }
    if (typeof item === 'object') {
      const primary =
        pickFirstString(item, ['summary', 'text', 'content', 'preview', 'value', 'body']) ||
        (typeof item.value === 'string' && item.value.trim() ? item.value.trim() : null);
      if (primary) {
        return truncateText(primary, 320);
      }
      if (item.value && typeof item.value === 'object') {
        const nested = pickFirstString(item.value, ['summary', 'text', 'content', 'preview']);
        if (nested) {
          return truncateText(nested, 320);
        }
      }
      try {
        return truncateText(JSON.stringify(item, null, 2), 320);
      } catch {
        return '';
      }
    }
    return '';
  }
  function gatherContextTags(item) {
    if (!item || typeof item !== 'object') return [];
    const out = new Set();
    const raw = item.tags ?? item.labels;
    if (Array.isArray(raw)) {
      for (const tag of raw) {
        if (typeof tag === 'string' && tag.trim()) {
          out.add(tag.trim());
        }
      }
    } else if (typeof raw === 'string') {
      for (const tag of raw.split(/[;,]/)) {
        const trimmed = tag.trim();
        if (trimmed) out.add(trimmed);
      }
    }
    return Array.from(out).slice(0, 12);
  }
  function formatContextScore(value) {
    const num = Number(value);
    if (!Number.isFinite(num)) return null;
    if (num >= 0 && num <= 1) return `${Math.round(num * 100)}%`;
    if (num >= 1 && num <= 100) return `${Math.round(num)}%`;
    return num.toFixed(2);
  }
  function extractContextPointer(obj) {
    const seen = new Set();
    const queue = [];
    if (obj && typeof obj === 'object') queue.push(obj);
    while (queue.length) {
      const current = queue.shift();
      if (!current || typeof current !== 'object') continue;
      if (seen.has(current)) continue;
      seen.add(current);
      if (!Array.isArray(current)) {
        const pointer = current.ptr || current.pointer;
        if (pointer && typeof pointer === 'object' && !Array.isArray(pointer) && typeof pointer.kind === 'string') {
          return pointer;
        }
      }
      const values = Array.isArray(current) ? current : Object.values(current);
      for (const value of values) {
        if (value && typeof value === 'object' && !seen.has(value)) {
          queue.push(value);
        }
      }
    }
    return null;
  }
  function pointerLabel(ptr) {
    if (!ptr || typeof ptr !== 'object') return '';
    const kind = String(ptr.kind || '').toLowerCase();
    if (kind === 'memory') {
      return ptr.id ? `memory:${ptr.id}` : 'memory pointer';
    }
    if (kind === 'file') {
      return ptr.path ? `file:${ptr.path}` : 'file pointer';
    }
    if (ptr.uri) return ptr.uri;
    return kind || 'pointer';
  }
  function pointerDetail(ptr) {
    if (!ptr || typeof ptr !== 'object') return '';
    if (typeof ptr.path === 'string' && ptr.path) return ptr.path;
    if (typeof ptr.id === 'string' && ptr.id) return ptr.id;
    if (typeof ptr.uri === 'string' && ptr.uri) return ptr.uri;
    return '';
  }
  function clearContextPanel(message) {
    const msg = message || 'No context assembled yet.';
    contextModel = null;
    contextPrimed = false;
    if (elContextPreview) {
      elContextPreview.textContent = msg;
    }
    if (elContextSummary) {
      elContextSummary.textContent = 'Items 0 · Seeds 0 · Expanded 0';
    }
    if (elContextMeta) {
      elContextMeta.textContent = 'No context specification recorded yet.';
    }
    if (elContextItems) {
      elContextItems.innerHTML = '';
      const empty = document.createElement('div');
      empty.className = 'context-empty';
      empty.textContent = msg;
      elContextItems.appendChild(empty);
    }
    if (elContextRehydrateOut) {
      elContextRehydrateOut.textContent = '';
    }
    if (elContextRehydrateMeta) {
      elContextRehydrateMeta.textContent = '';
    }
    if (elContextRehydrateWrap) {
      elContextRehydrateWrap.classList.remove('active');
    }
    if (btnContextCopy) {
      btnContextCopy.disabled = true;
    }
    contextLastResultText = '';
    clearCascadePanel();
  }
  function clearCascadePanel(message) {
    cascadeItems = [];
    if (!elContextCascade) return;
    const text = message || 'Select a project to view cascade summaries.';
    elContextCascade.innerHTML = '';
    const empty = document.createElement('div');
    empty.className = 'context-empty';
    empty.textContent = text;
    elContextCascade.appendChild(empty);
  }
  function buildContextItem(item, index) {
    const wrap = document.createElement('div');
    wrap.className = 'context-item';
    wrap.setAttribute('role', 'listitem');

    const header = document.createElement('div');
    header.className = 'context-item-header';
    const title = document.createElement('span');
    title.textContent = contextLabel(item, index);
    header.appendChild(title);

    const lane = item && typeof item === 'object'
      ? (item.slot || item.lane || item.kind || '')
      : '';
    if (lane) {
      const laneSpan = document.createElement('span');
      laneSpan.className = 'context-lane';
      laneSpan.textContent = String(lane);
      header.appendChild(laneSpan);
    }

    const scoreValue = item && typeof item === 'object'
      ? (item.cscore ?? item.score ?? item.sim)
      : null;
    const scoreLabel = formatContextScore(scoreValue);
    if (scoreLabel) {
      const scoreSpan = document.createElement('span');
      scoreSpan.className = 'context-item-score';
      scoreSpan.textContent = `Score ${scoreLabel}`;
      header.appendChild(scoreSpan);
    }

    const iteration = item && typeof item === 'object' ? item.iteration : null;
    if (Number.isFinite(iteration)) {
      const iterBadge = document.createElement('span');
      iterBadge.className = 'badge';
      iterBadge.textContent = `iter ${iteration}`;
      header.appendChild(iterBadge);
    }

    wrap.appendChild(header);

    const snippet = contextSnippet(item);
    if (snippet) {
      const body = document.createElement('div');
      body.className = 'context-item-body';
      body.textContent = snippet;
      wrap.appendChild(body);
    }

    const tags = gatherContextTags(item);
    if (tags.length) {
      const tagRow = document.createElement('div');
      tagRow.className = 'context-tags';
      tagRow.textContent = tags.join(' · ');
      wrap.appendChild(tagRow);
    }

    const pointer = extractContextPointer(item);
    if (pointer) {
      const pointerRow = document.createElement('div');
      pointerRow.className = 'context-pointer';

      const pointerInfo = document.createElement('span');
      pointerInfo.className = 'mono';
      const label = pointerLabel(pointer) || 'pointer';
      pointerInfo.textContent = label;
      pointerRow.appendChild(pointerInfo);

      const detailText = pointerDetail(pointer);
      if (detailText && detailText !== label) {
        const detail = document.createElement('code');
        detail.className = 'mono';
        detail.textContent = truncateText(detailText, 160);
        pointerRow.appendChild(detail);
      }

      const actions = document.createElement('div');
      actions.className = 'context-actions';
      const btn = document.createElement('button');
      btn.className = 'ghost';
      btn.textContent = 'Rehydrate';
      btn.addEventListener('click', () => {
        const payload = JSON.parse(JSON.stringify(pointer));
        rehydratePointer(payload, label, btn);
      });
      actions.appendChild(btn);
      pointerRow.appendChild(actions);
      wrap.appendChild(pointerRow);
    }

    const stamp = item && typeof item === 'object'
      ? (item.updated || item.last || item.modified || item.time)
      : null;
    if (stamp) {
      const metaRow = document.createElement('div');
      metaRow.className = 'context-meta';
      const timeSpan = document.createElement('span');
      timeSpan.textContent = `Updated ${formatIsoWithRelative(stamp)}`;
      metaRow.appendChild(timeSpan);
      wrap.appendChild(metaRow);
    }

    return wrap;
  }
  function renderContext(model) {
    if (!model || typeof model !== 'object') {
      clearContextPanel();
      return;
    }
    contextModel = model;
    contextPrimed = true;
    const preview = typeof model.context_preview === 'string' && model.context_preview.trim()
      ? model.context_preview.trim()
      : 'Context preview not available.';
    if (elContextPreview) {
      elContextPreview.textContent = truncateText(preview, 1200);
    }
    const working = model.working_set && typeof model.working_set === 'object' ? model.working_set : {};
    const counts = working.counts || {};
    if (elContextSummary) {
      const items = Number.isFinite(Number(counts.items)) ? Number(counts.items) : (Array.isArray(working.items) ? working.items.length : 0);
      const seeds = Number.isFinite(Number(counts.seeds)) ? Number(counts.seeds) : 0;
      const expanded = Number.isFinite(Number(counts.expanded)) ? Number(counts.expanded) : 0;
      elContextSummary.textContent = `Items ${items} · Seeds ${seeds} · Expanded ${expanded}`;
    }
    if (elContextMeta) {
      const spec =
        (working.final_spec && typeof working.final_spec === 'object' && working.final_spec) ||
        (model.final_spec && typeof model.final_spec === 'object' && model.final_spec) ||
        (model.requested_spec && typeof model.requested_spec === 'object' && model.requested_spec) ||
        null;
      const metaParts = [];
      if (spec) {
        if (Array.isArray(spec.lanes) && spec.lanes.length) {
          metaParts.push(`Lanes: ${spec.lanes.join(', ')}`);
        }
        if (spec.limit != null) metaParts.push(`Limit: ${spec.limit}`);
        if (spec.project) metaParts.push(`Project: ${spec.project}`);
        if (spec.expand_query) metaParts.push('Expand query');
        if (spec.scorer) metaParts.push(`Scorer: ${spec.scorer}`);
        if (spec.slot_budgets && typeof spec.slot_budgets === 'object') {
          const entries = Object.entries(spec.slot_budgets).map(([slot, limit]) => `${slot}=${limit}`);
          if (entries.length) metaParts.push(`Slot budgets: ${entries.join(', ')}`);
        }
      }
      if (typeof model.query === 'string' && model.query.trim()) {
        metaParts.push(`Query: ${truncateText(model.query.trim(), 160)}`);
      }
      elContextMeta.textContent = metaParts.length
        ? metaParts.join(' · ')
        : 'No context specification recorded yet.';
    }
    if (elContextItems) {
      elContextItems.innerHTML = '';
      const items = Array.isArray(working.items) ? working.items : [];
      if (!items.length) {
        const empty = document.createElement('div');
        empty.className = 'context-empty';
        empty.textContent = 'No context assembled yet.';
        elContextItems.appendChild(empty);
      } else {
        const maxItems = 60;
        items.slice(0, maxItems).forEach((entry, idx) => {
          elContextItems.appendChild(buildContextItem(entry, idx));
        });
        if (items.length > maxItems) {
          const more = document.createElement('div');
          more.className = 'context-empty';
          more.textContent = `Showing ${maxItems} of ${items.length} items. Adjust the spec to view more.`;
          elContextItems.appendChild(more);
        }
      }
    }
    if (elContextRehydrateWrap) {
      elContextRehydrateWrap.classList.remove('active');
    }
    if (elContextRehydrateOut) {
      elContextRehydrateOut.textContent = '';
    }
    if (elContextRehydrateMeta) {
      elContextRehydrateMeta.textContent = '';
    }
    if (btnContextCopy) {
      btnContextCopy.disabled = true;
    }
    contextLastResultText = '';
  }
  function renderCascadeItems() {
    if (!elContextCascade) return;
    elContextCascade.innerHTML = '';
    if (!cascadeItems.length) {
      const empty = document.createElement('div');
      empty.className = 'context-empty';
      empty.textContent = 'No cascade summaries yet.';
      elContextCascade.appendChild(empty);
      return;
    }
    for (const record of cascadeItems) {
      elContextCascade.appendChild(buildCascadeCard(record));
    }
  }
  function buildCascadeCard(record) {
    const card = document.createElement('div');
    card.className = 'context-item';
    card.setAttribute('role', 'listitem');
    const value = record && typeof record === 'object' && record.value && typeof record.value === 'object' ? record.value : {};
    const abstract = value.abstract && typeof value.abstract === 'object' ? value.abstract : {};
    const stats = value.stats && typeof value.stats === 'object' ? value.stats : {};
    const outline = Array.isArray(value.outline) ? value.outline : [];
    const extract = Array.isArray(value.extract) ? value.extract : [];

    const header = document.createElement('div');
    header.className = 'context-item-header';
    const title = document.createElement('span');
    title.textContent = abstract.text || record.text || record.key || 'Summary';
    header.appendChild(title);

    const laneBadge = document.createElement('span');
    laneBadge.className = 'context-lane';
    laneBadge.textContent = 'episodic summary';
    header.appendChild(laneBadge);

    if (stats.events != null) {
      const score = document.createElement('span');
      score.className = 'context-item-score';
      const eventsCount = Number(stats.events) || 0;
      const errorsCount = Number(stats.errors) || 0;
      let label = `${eventsCount} event${eventsCount === 1 ? '' : 's'}`;
      if (errorsCount > 0) {
        label += ` · ${errorsCount} error${errorsCount === 1 ? '' : 's'}`;
      }
      score.textContent = label;
      header.appendChild(score);
    }
    card.appendChild(header);

    if (abstract.text) {
      const body = document.createElement('div');
      body.className = 'context-item-body';
      body.textContent = abstract.text;
      card.appendChild(body);
    }

    if (outline.length) {
      const outlineRow = document.createElement('div');
      outlineRow.className = 'context-meta';
      outlineRow.textContent = outline
        .slice(0, 3)
        .map((item) => String(item || ''))
        .filter(Boolean)
        .join(' | ');
      if (outlineRow.textContent) {
        card.appendChild(outlineRow);
      }
    }

    if (extract.length) {
      const list = document.createElement('ul');
      list.className = 'context-tags';
      extract.slice(0, 4).forEach((item) => {
        const text = item && typeof item === 'object'
          ? (item.summary || item.kind || '')
          : item;
        if (!text) return;
        const li = document.createElement('li');
        li.textContent = String(text);
        list.appendChild(li);
      });
      if (list.childElementCount) {
        card.appendChild(list);
      }
    }

    const actions = document.createElement('div');
    actions.className = 'context-pointer';

    const episodeId = value.episode_id || (typeof record.key === 'string' ? record.key.replace(/^episode:/, '') : '');
    if (episodeId) {
      const btnEpisode = document.createElement('button');
      btnEpisode.className = 'ghost';
      btnEpisode.textContent = 'View episode';
      btnEpisode.addEventListener('click', () => viewRun(episodeId));
      actions.appendChild(btnEpisode);
    }

    const btnCopy = document.createElement('button');
    btnCopy.className = 'ghost';
    btnCopy.textContent = 'Copy summary';
    btnCopy.addEventListener('click', () => {
      try {
        ARW.copy(JSON.stringify(record, null, 2));
        setContextStat('Cascade summary copied', true);
      } catch {}
    });
    actions.appendChild(btnCopy);

    card.appendChild(actions);
    return card;
  }
  async function refreshContextCascade(opts = {}) {
    if (!elContextCascade) return;
    if (!curProj) {
      clearCascadePanel('Select a project to view cascade summaries.');
      return;
    }
    if (cascadeAbort) {
      try { cascadeAbort.abort(); } catch {}
    }
    const controller = new AbortController();
    cascadeAbort = controller;
    try {
      const params = new URLSearchParams();
      params.set('limit', '80');
      if (curProj) params.set('project', curProj);
      const response = await ARW.http.json(base, `/state/context/cascade?${params.toString()}`, { signal: controller.signal });
      const items = Array.isArray(response?.items) ? response.items : [];
      const currentProject = String(curProj || '').toLowerCase();
      const filtered = items.filter((entry) => {
        if (!currentProject) return true;
        try {
          const value = entry && typeof entry === 'object' && entry.value && typeof entry.value === 'object' ? entry.value : {};
          const projects = Array.isArray(entry.projects)
            ? entry.projects
            : Array.isArray(value.projects)
              ? value.projects
              : [];
          if (!projects.length) return entry.project_id && String(entry.project_id).toLowerCase() === currentProject;
          return projects.some((p) => String(p || '').toLowerCase() === currentProject);
        } catch {
          return false;
        }
      });
      cascadeItems = filtered.slice(0, 40);
      renderCascadeItems();
      if (!opts.quiet) setContextStat('Cascade refreshed', true);
    } catch (err) {
      if (err?.name !== 'AbortError') {
        console.error('context cascade fetch failed', err);
        if (!cascadeItems.length) clearCascadePanel('Unable to load cascade summaries.');
      }
    } finally {
      if (cascadeAbort === controller) cascadeAbort = null;
    }
  }
  async function refreshContextMetrics() {
    if (!elContextMetricsWrap) return;
    if (contextMetricsAbort) {
      try { contextMetricsAbort.abort(); } catch {}
    }
    const controller = new AbortController();
    contextMetricsAbort = controller;
    try {
      await fetchReadModel('context_metrics', '/state/context_metrics', { signal: controller.signal });
    } catch (err) {
      if (!(err && err.name === 'AbortError')) {
        console.warn('context metrics fetch failed', err);
      }
    } finally {
      if (contextMetricsAbort === controller) {
        contextMetricsAbort = null;
      }
    }
  }
  async function refreshContextSnapshot() {
    if (!curProj) {
      clearContextPanel('Select a project to assemble context.');
      return null;
    }
    if (contextAbort) {
      try { contextAbort.abort(); } catch {}
    }
    const controller = new AbortController();
    contextAbort = controller;
    const includeSources = !!(elContextIncludeSources && elContextIncludeSources.checked);
    const payload = { proj: curProj };
    if (includeSources) payload.include_sources = true;
    setContextStat('Loading…');
    try {
      const resp = await fetchRaw('/context/assemble', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify(payload),
        signal: controller.signal,
      });
      if (resp.status === 501) {
        clearContextPanel('Context kernel disabled for this deployment.');
        setContextStat('Kernel disabled', true);
        return null;
      }
      if (!resp.ok) {
        const text = await resp.text().catch(() => '');
        console.warn('context assemble failed', resp.status, text);
        setContextStat(`Context assemble failed (${resp.status})`, true);
        if (!contextPrimed) clearContextPanel('Context assemble failed.');
        return null;
      }
      const json = await resp.json().catch(() => null);
      const body = json && typeof json === 'object' && json.data && typeof json.data === 'object'
        ? json.data
        : json;
      if (body && typeof body === 'object') {
        renderContext(body);
        try { await refreshContextMetrics(); } catch {}
        setContextStat('Context assembled', false);
        return body;
      }
      setContextStat('Context response empty', true);
      return null;
    } catch (err) {
      if (err?.name !== 'AbortError') {
        console.error('context assemble error', err);
        setContextStat('Context assemble failed', true);
        if (!contextPrimed) clearContextPanel('Context assemble failed.');
      }
      return null;
    } finally {
      if (contextAbort === controller) {
        contextAbort = null;
      }
    }
  }
  async function rehydratePointer(ptr, label, button) {
    if (!ptr || typeof ptr !== 'object') return;
    const btn = button || null;
    if (btn) {
      btn.disabled = true;
      btn.textContent = 'Rehydrating…';
    }
    setContextStat('Rehydrating…');
    try {
      const resp = await fetchRaw('/context/rehydrate', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ ptr }),
      });
      if (resp.status === 403) {
        const text = await resp.text().catch(() => '');
        setContextStat(text || 'Lease required to rehydrate context', true);
        return;
      }
      if (!resp.ok) {
        const text = await resp.text().catch(() => '');
        console.warn('context rehydrate failed', resp.status, text);
        setContextStat(`Rehydrate failed (${resp.status})`, true);
        return;
      }
      const data = await resp.json().catch(() => null);
      renderContextRehydrate(data, label, ptr);
      setContextStat('Rehydrate complete', false);
    } catch (err) {
      console.error('context rehydrate error', err);
      setContextStat('Rehydrate failed', true);
    } finally {
      if (btn) {
        btn.disabled = false;
        btn.textContent = 'Rehydrate';
      }
    }
  }
  function renderContextRehydrate(result, label, ptr) {
    const bits = [];
    if (label) bits.push(label);
    if (ptr && typeof ptr === 'object') {
      const detail = pointerDetail(ptr);
      if (detail && detail !== label) bits.push(detail);
    }
    if (elContextRehydrateMeta) {
      elContextRehydrateMeta.textContent = bits.join(' · ');
    }
    let output = '';
    if (result && typeof result === 'object') {
      if (typeof result.content === 'string' && result.content.trim()) {
        output = result.content;
      } else {
        try { output = JSON.stringify(result, null, 2); }
        catch { output = String(result); }
      }
    } else if (result != null) {
      output = String(result);
    }
    if (elContextRehydrateOut) {
      elContextRehydrateOut.textContent = output;
    }
    contextLastResultText = output;
    if (btnContextCopy) {
      btnContextCopy.disabled = !output;
    }
    if (elContextRehydrateWrap) {
      if (output) elContextRehydrateWrap.classList.add('active');
      else elContextRehydrateWrap.classList.remove('active');
    }
  }
  clearContextPanel('Context preview will appear here after the next assembly.');
  if (btnContextRefresh) {
    btnContextRefresh.addEventListener('click', () => {
      refreshContextSnapshot();
      refreshContextCascade();
    });
  }
  if (btnContextCopy) {
    btnContextCopy.addEventListener('click', () => {
      if (contextLastResultText) {
        ARW.copy(contextLastResultText);
        setContextStat('Copied rehydrate result', true);
      } else {
        setContextStat('Nothing to copy', true);
      }
    });
  }
  if (elContextIncludeSources) {
    elContextIncludeSources.addEventListener('change', async () => {
      hubPrefs.contextIncludeSources = !!elContextIncludeSources.checked;
      try { await ARW.setPrefs('ui:hub', hubPrefs); } catch {}
      refreshContextSnapshot();
    });
  }
  let runsAbort = null;
  async function refreshEpisodesSnapshot(){
    try{
      if (runsAbort) { try{ runsAbort.abort(); }catch{} }
      runsAbort = new AbortController();
      const params = new URLSearchParams();
      params.set('limit', '200');
      if (curProj) params.set('project', curProj);
      const actorValue = elRunActor && typeof elRunActor.value === 'string' ? elRunActor.value.trim() : '';
      if (actorValue) params.set('actor', actorValue);
      if (elRunErrOnly && elRunErrOnly.checked) params.set('errors_only', 'true');
      const path = params.toString() ? `/state/episodes?${params.toString()}` : '/state/episodes';
      await fetchReadModel('episodes', path, {
        signal: runsAbort.signal,
        transform(raw){
          const isObj = raw && typeof raw === 'object' && !Array.isArray(raw);
          const out = isObj ? { ...raw } : {};
          const items = isObj && Array.isArray(raw.items)
            ? raw.items
            : Array.isArray(raw)
              ? raw
              : [];
          out.items = items;
          return out;
        }
      });
    }catch(e){
      if (!(e && e.name === 'AbortError')) console.error(e);
    }
  }
  let snapAbort = null;
  async function viewRun(id){
    try{
      if (snapAbort) { try{ snapAbort.abort(); }catch{} }
      snapAbort = new AbortController();
      let snap = null;
      try {
      const resp = await fetchRaw(`/state/episode/${encodeURIComponent(id)}/snapshot`, { signal: snapAbort.signal });
        if (resp.ok) {
          const payload = await resp.json();
          if (payload && typeof payload === 'object') {
            snap = payload.episode || payload;
          }
        }
      } catch (err) {
        if (!(err && err.name === 'AbortError')) console.warn('snapshot fetch failed', err);
      }
      if (!snap) {
        const fromCache = runsCache.find(r => r.id === id);
        if (fromCache) {
          snap = {
            id: fromCache.id,
            start: fromCache.start,
            end: fromCache.end,
            duration_ms: fromCache.duration_ms,
            items: normArr(fromCache.items),
          };
        }
      }
      runSnapshot = snap;
      if (elRunSnap) elRunSnap.textContent = snap ? JSON.stringify(snap, null, 2) : '';
      if (elRunSnapMeta) elRunSnapMeta.textContent = snap ? 'episode: ' + id : '';
      updateRunActionLabels(runSnapshot?.id || '');
      renderArtifacts();
    }catch(e){ console.error(e); runSnapshot=null; if (elRunSnap) elRunSnap.textContent=''; if (elRunSnapMeta) elRunSnapMeta.textContent=''; updateRunActionLabels(''); renderArtifacts(); }
  }
  document.getElementById('btnRunsRefresh')?.addEventListener('click', ()=>{ refreshEpisodesSnapshot(); });
  // Do not persist filters; just render on change
  elRunFilter?.addEventListener('input', ()=>{ renderRuns(); });
  elRunActor?.addEventListener('change', ()=>{
    renderRuns();
    refreshEpisodesSnapshot();
  });
  elRunKind?.addEventListener('change', ()=>{ renderRuns(); });
  elRunErrOnly?.addEventListener('change', ()=>{ refreshEpisodesSnapshot(); });
  btnRunCopy?.addEventListener('click', ()=>{ if (runSnapshot) ARW.copy(JSON.stringify(runSnapshot, null, 2)); });
  btnRunPinA?.addEventListener('click', ()=>{ if (runSnapshot){ const ta=document.getElementById('cmpA'); if (ta){ ta.value = JSON.stringify(runSnapshot, null, 2); updateCompareLink('text'); } } });
  btnRunPinB?.addEventListener('click', ()=>{ if (runSnapshot){ const tb=document.getElementById('cmpB'); if (tb){ tb.value = JSON.stringify(runSnapshot, null, 2); updateCompareLink('text'); } } });
  const applyEpisodesModel = (model) => {
    if (!model) return;
    episodesPrimed = true;
    const items = Array.isArray(model.items) ? model.items : [];
    runsCache = items.map(hydrateEpisode);
    for (const id of Array.from(runDetailsOpen)) {
      if (!runsCache.some(run => run.id === id)) {
        runDetailsOpen.delete(id);
      }
    }
    setRunsStat(describeRunsStat(runsCache.length), true);
    updateRunFilterOptions();
    renderRuns();
  };
  const idEpisodesRead = ARW.read.subscribe('episodes', applyEpisodesModel);
  const idRuntimeRead = ARW.read.subscribe('runtime_supervisor', renderRuntimeSupervisor);
  const idRuntimeMatrixRead = ARW.read.subscribe('runtime_matrix', renderRuntimeMatrix);
  const idRuntimeBundlesRead = ARW.read.subscribe('runtime_bundles', renderRuntimeBundles);
  const idContextMetricsRead = ARW.read.subscribe('context_metrics', applyContextMetrics);
  await refreshEpisodesSnapshot();
  // Throttle SSE-driven refresh on episode-related activity
  let _lastRunsAt = 0;
  const runsTick = ()=>{
    if (episodesPrimed) return;
    const now = Date.now();
    if (now - _lastRunsAt > 1200) {
      _lastRunsAt = now;
      refreshEpisodesSnapshot();
    }
  };
  ARW.sse.subscribe((k, e) => {
    try{
      const p = e?.env?.payload || {};
      return !!p.corr_id || /^intents\.|^actions\.|^feedback\./.test(k||'');
    }catch{ return false }
  }, runsTick);
  ARW.sse.subscribe(
    (kind) => kind === 'runtime.state.changed' || kind === 'runtime.restore.completed',
    () => scheduleRuntimeRefresh()
  );
  ARW.sse.subscribe(
    (kind) => kind === 'runtime.health',
    () => refreshRuntimeMatrix()
  );
  ARW.sse.subscribe(
    (kind) => kind === 'context.assembled',
    ({ env }) => {
      try {
        const payload = env && typeof env.payload === 'object' ? env.payload : (env && typeof env === 'object' ? env : null);
        if (!payload) return;
        const project = typeof payload.project === 'string' ? payload.project : null;
        if (!curProj) {
          return;
        }
        if (project && project !== curProj) {
          return;
        }
        if (!project && curProj) {
          return;
        }
        renderContext(payload);
        setContextStat('Context updated', false);
      } catch (err) {
        console.error('context assembled event handling failed', err);
      }
    }
  );
  ARW.sse.subscribe(
    (kind) => kind === 'context.cascade.updated',
    () => refreshContextCascade({ quiet: true })
  );
  // ---------- Projects: list/create/tree/notes ----------
  // Simple file metadata cache to avoid repeated GETs (5s TTL)
  const fileCache = new Map(); // rel -> { data, t }
  const fileTTL = 5000;
  async function getFileMeta(rel, opts = {}){
    const key = String(rel || '');
    const cached = fileCache.get(key);
    const now = Date.now();
    if (!opts.force && cached && (now - cached.t) < fileTTL) {
      return cached.data;
    }
    const data = await fetchJson(`/state/projects/${encodeURIComponent(curProj)}/file?path=${encodeURIComponent(key)}`);
    fileCache.set(key, { data, t: now });
    return data;
  }
  function clearNotesMeta(){
    if (!elNotesMeta) return;
    elNotesMeta.textContent = '';
    elNotesMeta.style.display = 'none';
    elNotesMeta.classList.remove('warn');
    elNotesMeta.removeAttribute('title');
  }
  function renderNotesMeta(info){
    if (!elNotesMeta) return;
    const notes = info && info.notes && typeof info.notes === 'object' ? info.notes : null;
    if (!notes){
      clearNotesMeta();
      return;
    }
    const parts = [];
    const details = [];
    if (notes.modified){
      const rel = formatRelativeIso(notes.modified);
      if (rel) parts.push(`modified ${rel}`);
      details.push(`modified ${notes.modified}`);
    }
    if (Number.isFinite(Number(notes.bytes))){
      const bytes = Number(notes.bytes);
      parts.push(formatBytes(bytes));
      details.push(`${bytes} bytes`);
    }
    if (notes.sha256){
      const sha = String(notes.sha256);
      parts.push(`sha ${sha.slice(0, 12)}`);
      details.push(`sha ${sha}`);
    }
    const truncated = notes.truncated === true;
    if (truncated){
      parts.push('preview truncated for UI');
      details.push('preview truncated in launcher (full file on disk)');
    }
    if (!parts.length){
      clearNotesMeta();
      return;
    }
    elNotesMeta.textContent = parts.join(' · ');
    elNotesMeta.style.display = '';
    if (details.length) {
      elNotesMeta.title = details.join(' · ');
    } else {
      elNotesMeta.removeAttribute('title');
    }
    elNotesMeta.classList.toggle('warn', truncated);
  }
  function clearTreeMeta(){
    if (!elTreeMeta) return;
    elTreeMeta.textContent = '';
    elTreeMeta.style.display = 'none';
    elTreeMeta.classList.remove('warn');
    elTreeMeta.removeAttribute('title');
  }
  function renderTreeMeta(info){
    if (!elTreeMeta) return;
    const tree = info && info.tree && typeof info.tree === 'object' ? info.tree : null;
    if (!tree){
      clearTreeMeta();
      return;
    }
    const parts = [];
    const details = [];
    if (typeof tree.digest === 'string' && tree.digest){
      parts.push(`digest ${tree.digest.slice(0, 12)}`);
      details.push(`digest ${tree.digest}`);
    }
    const truncated = tree.truncated && typeof tree.truncated === 'object' && !Array.isArray(tree.truncated)
      ? tree.truncated
      : null;
    let warn = false;
    if (truncated){
      const key = currentPath || '';
      const currentVal = Number(truncated[key]);
      if (Number.isFinite(currentVal) && currentVal > 0){
        warn = true;
        const label = currentVal === 1 ? 'entry' : 'entries';
        parts.push(`+${currentVal} hidden ${label} here`);
        details.push(`${currentVal} entries trimmed in this folder`);
      }
      const others = Object.entries(truncated)
        .filter(([k, v]) => (k || '') !== key && Number(v) > 0);
      if (others.length){
        warn = true;
        const summary = others
          .slice(0, 2)
          .map(([k, v]) => `${k || 'root'} (+${v})`)
          .join(', ');
        const extra = others.length > 2 ? '…' : '';
        parts.push(`others truncated: ${summary}${extra}`);
        const detailItems = others.map(([k, v]) => `${k || 'root'} (+${v})`);
        details.push(`other folders trimmed: ${detailItems.join(', ')}`);
      }
    }
    if (!parts.length){
      clearTreeMeta();
      return;
    }
    elTreeMeta.textContent = parts.join(' · ');
    elTreeMeta.style.display = '';
    if (details.length) {
      elTreeMeta.title = details.join(' · ');
    } else {
      elTreeMeta.removeAttribute('title');
    }
    elTreeMeta.classList.toggle('warn', warn);
  }
  // Nested tree cache and expansion state
  const treeCache = new Map(); // path -> items
  let expanded = new Set(); // rel paths that are expanded (persisted)
  let searchExpanded = new Set(); // ephemeral expansions from filter
  const elProjSel = document.getElementById('projSel');
  const elProjName = document.getElementById('projName');
  const elProjTree = document.getElementById('projTree');
  const elProjStat = document.getElementById('projStat');
  const elNotes = document.getElementById('notesArea');
  const elNotesMeta = document.getElementById('notesMeta');
  const elCurProj = document.getElementById('curProj');
  const elNotesAutosave = document.getElementById('notesAutosave');
  const elProjPrefsBadge = document.getElementById('projPrefsBadge');
  const elFileFilter = document.getElementById('fileFilter');
  const elTreeMeta = document.getElementById('treeMeta');
  const hubLayout = document.getElementById('hubLayout');
  const hubOnboarding = document.getElementById('hubOnboarding');
  const btnOnboardCreate = document.getElementById('btnOnboardCreate');
  const btnOnboardDocs = document.getElementById('btnOnboardDocs');
  let isFirstRun = false;
  function toggleFirstRun(state){
    isFirstRun = !!state;
    if (hubLayout) hubLayout.hidden = isFirstRun;
    if (hubOnboarding) hubOnboarding.hidden = !isFirstRun;
    if (isFirstRun) {
      if (elCurProj) elCurProj.textContent = '–';
      if (elNotes) elNotes.value = '';
      clearNotesMeta();
      clearTreeMeta();
      clearContextPanel('Create or link a project to hydrate context.');
      clearCascadePanel('Create or link a project to view cascade summaries.');
    }
  }
  if (btnOnboardCreate) {
    btnOnboardCreate.addEventListener('click', ()=>{
      toggleFirstRun(false);
      if (hubLayout) hubLayout.hidden = false;
      const filesPanel = document.getElementById('files');
      if (filesPanel && typeof filesPanel.scrollIntoView === 'function') {
        filesPanel.scrollIntoView({ behavior: 'smooth', block: 'start' });
      }
      if (elProjName) {
        requestAnimationFrame(()=> elProjName.focus());
      }
    });
  }
  if (btnOnboardDocs) {
    btnOnboardDocs.addEventListener('click', async ()=>{
      try{
        await ARW.invoke('open_url', { url: 'https://t3hw00t.github.io/ARW/guide/workflow_views/' });
      }catch(e){
        console.error(e);
      }
    });
  }
  let currentPath = '';
  const pathStack = [];
  function setStat(txt){ if (elProjStat) { elProjStat.textContent = txt||''; if (txt) setTimeout(()=>{ if (elProjStat.textContent===txt) elProjStat.textContent=''; }, 1500); } }
  async function applyProjectsModel(model){
    projectsModel = model || {};
    projectsIndex.clear();
    const items = Array.isArray(projectsModel.items) ? projectsModel.items : [];
    for (const raw of items) {
      if (!raw || typeof raw.name !== 'string') continue;
      projectsIndex.set(raw.name, raw);
    }
    const names = Array.from(projectsIndex.keys()).sort();
    if (elProjSel) {
      const currentOptions = Array.from(elProjSel.options || []).map((o) => o.value);
      const changed =
        currentOptions.length !== names.length ||
        currentOptions.some((v, idx) => v !== names[idx]);
      if (changed) {
        elProjSel.innerHTML = '';
        for (const name of names) {
          const opt = document.createElement('option');
          opt.value = name;
          opt.textContent = name;
          elProjSel.appendChild(opt);
        }
      }
    }
    if (!names.length) {
      toggleFirstRun(true);
      if (elProjSel) {
        elProjSel.innerHTML = '';
      }
      curProj = null;
      return;
    }
    toggleFirstRun(false);
    let targetProj = curProj;
    if (!targetProj || !projectsIndex.has(targetProj)) {
      targetProj = names[0] || null;
    }
    if (targetProj && targetProj !== curProj) {
      try {
        await setProj(targetProj);
      } catch (err) {
        console.error(err);
      }
    }
    if (elProjSel) {
      elProjSel.value = targetProj || '';
    }
    if (!targetProj || !projectsIndex.has(targetProj)) {
      clearNotesMeta();
      clearTreeMeta();
      return;
    }
    await loadNotes(true);
    await loadTree(currentPath);
  }
  async function setProj(name){
    curProj = name||null;
    if (elCurProj) elCurProj.textContent = curProj||'–';
    if (curProj) {
      const info = projectsIndex.get(curProj);
      renderNotesMeta(info);
      renderTreeMeta(info);
    } else {
      clearNotesMeta();
      clearTreeMeta();
      clearContextPanel('Select a project to assemble context.');
    }
    hubPrefs.lastProject = curProj;
    try { await ARW.setPrefs('ui:hub', hubPrefs); } catch {}
    try { projPrefs = await ARW.getPrefs('ui:proj:'+curProj) || {}; } catch { projPrefs = {}; }
    try { const arr = Array.isArray(projPrefs.expanded)? projPrefs.expanded : []; expanded = new Set(arr.map(String)); } catch { expanded = new Set(); }
    const as = (projPrefs && projPrefs.notesAutoSave !== false);
    if (elNotesAutosave) elNotesAutosave.checked = as;
    const hasEditor = !!(projPrefs && projPrefs.editorCmd);
    if (elProjPrefsBadge) elProjPrefsBadge.style.display = hasEditor? 'inline-flex':'none';
    treeCache.clear();
    if (sc && typeof sc.refresh === 'function') {
      sc.refresh({ immediate: true, reason: 'project-change' });
    }
    applyContextMetrics();
    // restore last folder if present
    if (curProj){
      loadNotes();
      try{ const lp = projPrefs && projPrefs.lastPath ? String(projPrefs.lastPath) : ''; currentPath=''; pathStack.length=0; loadTree(lp); }
      catch{ loadTree(''); }
      clearContextPanel('Loading context…');
      clearCascadePanel('Loading cascade summaries…');
    }
    refreshEpisodesSnapshot();
    refreshContextSnapshot();
    refreshContextCascade({ quiet: true });
  }
  async function refreshProjectsSnapshot(){
    try{
      await fetchReadModel('projects', '/state/projects');
    }catch(e){
      if (e && e.status === 401) {
        setStat('Set an admin token (Home → Connection & alerts) to load projects');
      } else {
        console.error(e);
      }
    }
  }
  async function createProj(){
    const n = (elProjName?.value||'').trim(); if (!n) return;
    try{
      const resp = await fetchRaw('/projects', { method:'POST', headers:{'Content-Type':'application/json'}, body: JSON.stringify({ name:n }) });
      if (!resp.ok) {
        let body = '';
        try {
          body = await resp.text();
        } catch {}
        const trimmed = (body || '').trim();
        let extracted = '';
        if (trimmed) {
          try {
            const parsed = JSON.parse(trimmed);
            if (parsed && typeof parsed === 'object') {
              extracted = parsed.error || parsed.message || parsed.detail || '';
            } else if (typeof parsed === 'string') {
              extracted = parsed;
            }
          } catch {
            extracted = trimmed;
          }
        }
        if (extracted && extracted.length > 180) {
          extracted = `${extracted.slice(0, 177)}…`;
        }
        let message;
        if (resp.status === 401 || resp.status === 403) {
          message = extracted ? `Create failed: ${extracted}` : 'Create failed: admin token required';
        } else if (extracted) {
          message = `Create failed: ${extracted}`;
        } else {
          message = `Create failed (${resp.status})`;
        }
        const err = new Error(message);
        err.status = resp.status;
        err.detail = trimmed;
        throw err;
      }
      await refreshProjectsSnapshot();
      await setProj(n);
      if (elProjName) elProjName.value = '';
      setStat('Project created');
      ARW.toast(`Project created: ${n}`);
    }catch(e){
      console.error(e);
      const message = e && e.message ? e.message : 'Create failed';
      setStat(message);
      ARW.toast(message);
    }
  }
  async function loadNotes(skipFetch=false){
    if (!curProj || !elNotes){
      clearNotesMeta();
      return;
    }
    const info = projectsIndex.get(curProj);
    const content = info?.notes?.content;
    if (typeof content === 'string') {
      elNotes.value = content;
      renderNotesMeta(info);
      return;
    }
    const shouldSkip = skipFetch === true;
    if (shouldSkip) {
      renderNotesMeta(info);
      return;
    }
    try{
      const t = await fetchText(`/state/projects/${encodeURIComponent(curProj)}/notes`);
      elNotes.value = t;
      renderNotesMeta(info);
    }catch(e){
      console.error(e);
      elNotes.value='';
      renderNotesMeta(info);
    }
  }
  async function saveNotes(quiet=false){ if (!curProj||!elNotes) return; try{ const t = elNotes.value||''; await fetchRaw(`/projects/${encodeURIComponent(curProj)}/notes`, { method:'PUT', headers:{'Content-Type':'text/plain'}, body: t + '\n' }); const ns=document.getElementById('notesStat'); if (ns){ ns.textContent='Saved'; setTimeout(()=>{ if (ns.textContent==='Saved') ns.textContent=''; }, 1200); } if (!quiet) { /* optional toast removed for quieter UX */ } }catch(e){ console.error(e); const ns=document.getElementById('notesStat'); if (ns){ ns.textContent='Error'; setTimeout(()=>{ if (ns.textContent==='Error') ns.textContent=''; }, 1500); } }
  async function loadTree(rel){
    if (!curProj||!elProjTree){
      clearTreeMeta();
      return;
    }
    const next = String(rel||'');
    if (next !== currentPath && currentPath !== '') { pathStack.push(currentPath); }
    currentPath = next;
    try{ const ns='ui:proj:'+curProj; const p=await ARW.getPrefs(ns)||{}; p.lastPath = currentPath; await ARW.setPrefs(ns, p); }catch{}
    renderCrumbs(currentPath);
    const key = String(currentPath||'');
    let fromModel = false;
    const info = projectsIndex.get(curProj);
    const treePaths = info?.tree?.paths;
    if (treePaths && Object.prototype.hasOwnProperty.call(treePaths, key)) {
      const entries = Array.isArray(treePaths[key]) ? treePaths[key] : [];
      treeCache.set(key, entries);
      fromModel = true;
    }
    if (!fromModel) {
      try{
        const j = await fetchJson(`/state/projects/${encodeURIComponent(curProj)}/tree?path=${encodeURIComponent(currentPath)}`).catch(()=>({items:[]}));
        treeCache.set(String(currentPath||''), j.items||[]);
      }catch(e){
        console.error(e);
        elProjTree.textContent = 'Error';
        renderTreeMeta(info);
        return;
      }
    }
    await expandOnSearch((elFileFilter?.value||'').trim());
    renderTree(treeCache.get(String(currentPath||''))||[]);
    renderTreeMeta(info);
  }
  async function ensureChildren(path){
    const key = String(path||'');
    if (!treeCache.has(key)){
      const info = projectsIndex.get(curProj);
      const treePaths = info?.tree?.paths;
      if (treePaths && Object.prototype.hasOwnProperty.call(treePaths, key)) {
        const entries = Array.isArray(treePaths[key]) ? treePaths[key] : [];
        treeCache.set(key, entries);
      } else {
        try{ const j=await fetchJson(`/state/projects/${encodeURIComponent(curProj)}/tree?path=${encodeURIComponent(key)}`).catch(()=>({items:[]})); treeCache.set(key, (j.items||[])); }
        catch{ treeCache.set(key, []); }
      }
    }
    return treeCache.get(key)||[];
  }
  function prefixesOf(rel){
    const parts = String(rel||'').split('/').filter(Boolean);
    const out = [];
    for (let i=1;i<parts.length;i++){ out.push(parts.slice(0,i).join('/')); }
    return out;
  }
  async function expandOnSearch(q){
    searchExpanded = new Set();
    const needle = String(q||'').trim().toLowerCase();
    if (!needle){ return; }
    const MAX_NODES = 1000;
    let scanned = 0;
    const queue = [ String(currentPath||'') ];
    const seen = new Set(queue);
    while (queue.length && scanned < MAX_NODES){
      const dir = queue.shift();
      const items = await ensureChildren(dir);
      scanned += items.length;
      for (const it of items){
        const name = String(it.name||'').toLowerCase();
        const rel = String(it.rel||'');
        if (name.includes(needle)){
          prefixesOf(rel).forEach(p => searchExpanded.add(p));
        }
        if (it.dir && !seen.has(rel)) { seen.add(rel); queue.push(rel); }
      }
    }
  }
  function renderCrumbs(path){
    const bar = document.getElementById('crumbs'); if (!bar) return; bar.innerHTML='';
    const parts = (String(path||'').split('/').filter(Boolean));
    const rootBtn = document.createElement('button'); rootBtn.className='ghost'; rootBtn.textContent='root'; rootBtn.addEventListener('click', ()=> loadTree('')); bar.appendChild(rootBtn);
    let acc = '';
    for (const seg of parts){ const sep=document.createElement('span'); sep.className='dim'; sep.textContent=' / '; bar.appendChild(sep); acc = acc ? (acc + '/' + seg) : seg; const btn=document.createElement('button'); btn.className='ghost'; btn.textContent=seg; btn.addEventListener('click', ()=> loadTree(acc)); bar.appendChild(btn); }
  }
  async function renderInlineChildren(parentRel, host, depth){
    try{
      let items = treeCache.get(parentRel);
      if (!Array.isArray(items)){
        const j = await fetchJson(`/state/projects/${encodeURIComponent(curProj)}/tree?path=${encodeURIComponent(parentRel||'')}`).catch(()=>({items:[]}));
        items = (j.items||[]);
        treeCache.set(parentRel, items);
      }
      // Clear and render
      host.innerHTML = '';
      const q = (elFileFilter?.value||'').toLowerCase().trim();
      for (const it of items){
        if (q){
          const match = String(it.name||'').toLowerCase().includes(q);
          const isDir = !!it.dir;
          const isExpandedBySearch = searchExpanded.has(String(it.rel||''));
          if (!match && !(isDir && isExpandedBySearch)) continue;
        }
        const row = document.createElement('div'); row.className='row'; row.style.justifyContent='space-between'; row.style.borderBottom='1px dashed #eee'; row.style.padding='4px 2px'; row.style.paddingLeft = (depth*12)+'px';
        row.setAttribute('data-row','1'); row.setAttribute('data-rel', String(it.rel||'')); row.setAttribute('data-dir', it.dir? '1':'0'); row.tabIndex = -1;
        row.setAttribute('role','treeitem'); row.setAttribute('aria-level', String((depth||0)+1));
        const nameWrap = document.createElement('div'); nameWrap.style.display='flex'; nameWrap.style.alignItems='center'; nameWrap.style.gap='6px';
        if (it.dir){ const btn=document.createElement('button'); btn.className='ghost'; btn.style.width='22px'; btn.style.padding='2px 4px'; const open=expanded.has(it.rel||'') || searchExpanded.has(String(it.rel||'')); btn.textContent = open?'▾':'▸'; btn.setAttribute('aria-label', (open? 'Collapse ':'Expand ') + (String(it.name||''))); row.setAttribute('aria-expanded', open? 'true':'false'); btn.addEventListener('click', async (e)=>{ e.stopPropagation(); if (expanded.has(it.rel||'')) expanded.delete(it.rel||''); else expanded.add(it.rel||''); try{ const ns='ui:proj:'+curProj; const p=await ARW.getPrefs(ns)||{}; p.expanded = Array.from(expanded); await ARW.setPrefs(ns, p);}catch{} await renderInlineChildren(parentRel, host, depth); }); nameWrap.appendChild(btn); } else { row.removeAttribute('aria-expanded'); }
        else { const sp=document.createElement('span'); sp.style.display='inline-block'; sp.style.width='22px'; nameWrap.appendChild(sp); }
        const name=document.createElement('div'); name.style.cursor='pointer';
        const icon = (it.dir?'📁 ':'📄 ');
        const label = String(it.name||'');
        const filterText = (elFileFilter?.value||'').trim();
        if (filterText){
          const esc = (s)=> s.replace(/&/g,'&amp;').replace(/</g,'&lt;');
          const re = new RegExp(filterText.replace(/[-/\\^$*+?.()|[\]{}]/g,'\\$&'), 'ig');
          const marked = esc(label).replace(re, (m)=> '<span class="hl-match">'+esc(m)+'</span>');
          name.innerHTML = icon + marked;
        } else {
          name.textContent = icon + label;
        }
        name.addEventListener('click', ()=>{ if (it.dir) loadTree(it.rel||''); else filePreview(it.rel||''); }); nameWrap.appendChild(name);
        const actions=document.createElement('div'); actions.className='row';
        if (!it.dir){
          const copyBtn=document.createElement('button'); copyBtn.className='ghost'; copyBtn.textContent='Copy'; copyBtn.title='Copy file contents to clipboard'; copyBtn.addEventListener('click', async ()=>{ try{ const rel=it.rel||''; const data=await getFileMeta(rel); ARW.copy(String(data?.content||'')); }catch(e){ console.error(e); ARW.toast('Copy failed'); } }); actions.appendChild(copyBtn);
          if (CAN_OPEN_LOCAL) {
            const openBtn=document.createElement('button'); openBtn.className='ghost'; openBtn.textContent='Open'; openBtn.title='Open with system default'; openBtn.addEventListener('click', async ()=>{ try{ const rel=it.rel||''; const data=await getFileMeta(rel); if(data&&data.abs_path){ await ARW.invoke('open_path',{path:data.abs_path}); } else { ARW.toast('Path unavailable'); } }catch(e){ console.error(e); ARW.toast('Open failed'); } }); actions.appendChild(openBtn);
            const editOpen=document.createElement('button'); editOpen.className='ghost'; editOpen.textContent='Open in Editor'; editOpen.title='Open in configured editor'; editOpen.addEventListener('click', async ()=>{ try{ const rel=it.rel||''; const data=await getFileMeta(rel); if(data&&data.abs_path){ const eff=(projPrefs&&projPrefs.editorCmd)||((await ARW.getPrefs('launcher'))||{}).editorCmd||null; await ARW.invoke('open_in_editor',{path:data.abs_path, editor_cmd: eff}); } else { ARW.toast('Path unavailable'); } }catch(e){ console.error(e); ARW.toast('Open in Editor failed'); } }); actions.appendChild(editOpen);
          }
        }
        const left=document.createElement('div'); left.style.display='flex'; left.style.alignItems='center'; left.style.gap='6px'; left.appendChild(nameWrap); row.appendChild(left); row.appendChild(actions); host.appendChild(row);
        if (it.dir && (expanded.has(it.rel||'') || searchExpanded.has(String(it.rel||'')))){
          const childHost=document.createElement('div'); childHost.setAttribute('role','group'); host.appendChild(childHost);
          await renderInlineChildren(it.rel||'', childHost, depth+1);
        }
      }
    }catch(e){ console.error(e); }
  }
  // Keyboard navigation for the tree
  function collectRows(){ return Array.from(elProjTree.querySelectorAll('[data-row="1"]')); }
  function parentOf(rel){ const s=String(rel||''); const i=s.lastIndexOf('/'); return i>=0? s.slice(0,i): ''; }
  function focusRowByIndex(idx){ const rows=collectRows(); if(!rows.length) return; idx=Math.max(0,Math.min(idx,rows.length-1)); rows.forEach(r=> { r.tabIndex=-1; r.setAttribute('aria-selected','false'); }); const row=rows[idx]; row.tabIndex=0; row.setAttribute('aria-selected','true'); row.focus({ preventScroll:true }); }
  function focusRowByRel(rel){ const rows=collectRows(); const i=rows.findIndex(r=> (r.getAttribute('data-rel')||'')===String(rel||'')); if (i>=0) focusRowByIndex(i); }
  elProjTree.addEventListener('keydown', async (e)=>{
    const rows = collectRows(); if (!rows.length) return;
    const active = document.activeElement; const curIdx = rows.findIndex(r=> r===active || r.contains(active));
    const cur = curIdx>=0? rows[curIdx]: rows[0]; const rel=cur.getAttribute('data-rel')||''; const isDir = cur.getAttribute('data-dir')==='1';
    if (e.key==='ArrowDown'){ e.preventDefault(); focusRowByIndex((curIdx>=0?curIdx:0)+1); }
    else if (e.key==='ArrowUp'){ e.preventDefault(); focusRowByIndex((curIdx>=0?curIdx:0)-1); }
    else if (e.key==='Home'){ e.preventDefault(); focusRowByIndex(0); }
    else if (e.key==='End'){ e.preventDefault(); focusRowByIndex(rows.length-1); }
    else if (e.key==='ArrowRight'){
      if (!isDir) return; e.preventDefault();
      const open = expanded.has(rel) || searchExpanded.has(rel);
      if (!open){ expanded.add(rel); try{ const ns='ui:proj:'+curProj; const p=await ARW.getPrefs(ns)||{}; p.expanded = Array.from(expanded); await ARW.setPrefs(ns, p);}catch{} await renderInlineChildren(parentOf(rel), cur.parentElement, (cur.style.paddingLeft? (parseInt(cur.style.paddingLeft)/12):0)); focusRowByRel(rel); }
      else { const kids = await ensureChildren(rel); if (kids && kids.length){ focusRowByRel(kids[0].rel||''); } }
    }
    else if (e.key==='ArrowLeft'){
      e.preventDefault();
      if (isDir && expanded.has(rel)){ expanded.delete(rel); try{ const ns='ui:proj:'+curProj; const p=await ARW.getPrefs(ns)||{}; p.expanded = Array.from(expanded); await ARW.setPrefs(ns, p);}catch{} await renderInlineChildren(parentOf(rel), cur.parentElement, (cur.style.paddingLeft? (parseInt(cur.style.paddingLeft)/12):0)); focusRowByRel(rel); }
      else { focusRowByRel(parentOf(rel)); }
    }
    else if (e.key==='Enter'){
      e.preventDefault(); if (isDir) loadTree(rel); else filePreview(rel);
    }
  });
  // Global shortcuts (avoid when typing in inputs)
  window.addEventListener('keydown', (e)=>{
    const tag = (e.target && e.target.tagName || '').toLowerCase();
    const typing = tag==='input' || tag==='textarea' || tag==='select' || e.metaKey || e.ctrlKey || e.altKey;
    if (typing) return;
    if (e.key === '/') { const ff = document.getElementById('fileFilter'); if (ff) { e.preventDefault(); ff.focus({ preventScroll:true }); } }
    else if (e.key.toLowerCase() === 'n') { const np = document.getElementById('projName'); if (np) { e.preventDefault(); np.focus({ preventScroll:true }); } }
    else if (e.key.toLowerCase() === 'b') { const bb = document.getElementById('btnTreeBack'); if (bb) { e.preventDefault(); bb.click(); } }
  });
  function renderTree(items){
    elProjTree.innerHTML='';
    const wrap = document.createElement('div');
    // Top-level rows
    const topHost = document.createElement('div'); wrap.appendChild(topHost);
    // Render top-level and any expanded children beneath
    (async ()=>{ await renderInlineChildren(String(currentPath||''), topHost, 0); })();
    elProjTree.appendChild(wrap);
    const prev = document.createElement('div'); prev.id='treePrev'; elProjTree.appendChild(prev);
    elProjTree.innerHTML='';
    const wrap = document.createElement('div');
    const q = (elFileFilter?.value||'').toLowerCase().trim();
    (items||[]).filter(it=>{ if (!q) return true; const n=(it.name||'').toLowerCase(); return n.includes(q); }).forEach(it=>{
      const row = document.createElement('div'); row.className='row'; row.style.justifyContent='space-between'; row.style.borderBottom='1px dashed #eee'; row.style.padding='4px 2px';
      const name = document.createElement('div'); name.style.cursor='pointer'; name.textContent = (it.dir?'📁 ':'📄 ') + (it.name||'');
      name.addEventListener('click', ()=>{ if (it.dir) loadTree(it.rel||''); else filePreview(it.rel||''); });
      const actions = document.createElement('div'); actions.className='row';
      if (!it.dir){
        const copyBtn=document.createElement('button'); copyBtn.className='ghost'; copyBtn.textContent='Copy'; copyBtn.addEventListener('click', async ()=>{
          try{
            const rel = it.rel||'';
            const data = await getFileMeta(rel);
            ARW.copy(String(data?.content||''));
          }catch(e){ console.error(e); ARW.toast('Copy failed'); }
        }); actions.appendChild(copyBtn);
        if (CAN_OPEN_LOCAL) {
          const openBtn=document.createElement('button'); openBtn.className='ghost'; openBtn.textContent='Open'; openBtn.addEventListener('click', async ()=>{
            try{
              const rel = it.rel||'';
              const data = await getFileMeta(rel);
              if (data && data.abs_path) { await ARW.invoke('open_path', { path: data.abs_path }); } else { ARW.toast('Path unavailable'); }
            }catch(e){ console.error(e); ARW.toast('Open failed'); }
          }); actions.appendChild(openBtn);
          const editOpen=document.createElement('button'); editOpen.className='ghost'; editOpen.textContent='Open in Editor'; editOpen.addEventListener('click', async ()=>{
            try{
              const rel = it.rel||'';
              const data = await getFileMeta(rel);
              if (data && data.abs_path) { const eff = (projPrefs&&projPrefs.editorCmd) || ((await ARW.getPrefs('launcher'))||{}).editorCmd || null; await ARW.invoke('open_in_editor', { path: data.abs_path, editor_cmd: eff }); } else { ARW.toast('Path unavailable'); }
            }catch(e){ console.error(e); ARW.toast('Open in Editor failed'); }
          }); actions.appendChild(editOpen);
        }
      }
      row.appendChild(name); row.appendChild(actions); wrap.appendChild(row);
    });
    elProjTree.appendChild(wrap);
    const prev = document.createElement('div'); prev.id='treePrev'; elProjTree.appendChild(prev);
    // Drag & drop upload onto tree area
    elProjTree.addEventListener('dragover', (e)=>{ e.preventDefault(); elProjTree.style.outline='2px dashed #b87333'; });
    elProjTree.addEventListener('dragleave', ()=>{ elProjTree.style.outline=''; });
    elProjTree.addEventListener('drop', async (e)=>{
      e.preventDefault(); elProjTree.style.outline='';
      const list = e.dataTransfer?.files || [];
      if (!curProj || !list.length) return;
      for (const f of list){
        try{
          let dest = currentPath ? (currentPath.replace(/\/$/, '') + '/' + f.name) : f.name;
          let exists = false;
          try{ const r = await fetchRaw(`/state/projects/${encodeURIComponent(curProj)}/file?path=${encodeURIComponent(dest)}`); exists = r.ok; }catch{}
          if (exists){
            const overwrite = await ARW.modal.confirm({
              title: 'Overwrite file?',
              body: `${dest} already exists. Overwrite it?`,
              submitLabel: 'Overwrite',
              cancelLabel: 'Create copy',
            });
            if (!overwrite){
              const m=f.name.match(/^(.*?)(\.[^.]*)?$/);
              const baseN=m?m[1]:f.name;
              const ext=m?m[2]||'':'';
              dest = (currentPath? currentPath+'/' : '') + baseN + ' (copy)' + ext;
            }
          }
          if (f.size > 10*1024*1024){ ARW.toast('File too large (max 10 MiB)'); continue; }
          let body = {};
          if ((f.type||'').startsWith('text/') || f.size < 256*1024){ const t = await f.text(); body = { content: t, prev_sha256: null }; }
          else {
            const ab = await f.arrayBuffer();
            const b64 = (function(u8){ let bin=''; const CHUNK=0x8000; for(let i=0;i<u8.length;i+=CHUNK){ bin += String.fromCharCode.apply(null, u8.subarray(i,i+CHUNK)); } return btoa(bin); })(new Uint8Array(ab));
            body = { content_b64: b64, prev_sha256: null };
          }
          const resp = await fetchRaw(`/projects/${encodeURIComponent(curProj)}/file?path=${encodeURIComponent(dest)}`, { method:'PUT', headers:{'Content-Type':'application/json'}, body: JSON.stringify(body) });
          if (!resp.ok){ console.warn('Upload failed', f.name, resp.status); continue; }
        }catch(err){ console.error('Upload error', err); }
      }
      await loadTree(currentPath);
    });
  }
  elFileFilter?.addEventListener('input', ()=> renderTree(treeCache.get(String(currentPath||''))||[]));
  const btnBack = document.getElementById('btnTreeBack'); if (btnBack) btnBack.addEventListener('click', ()=>{ const prev = pathStack.pop(); loadTree(prev||''); });
  async function filePreview(rel){
    try{
      const j=await getFileMeta(rel||'', { force: true });
      const prev=document.getElementById('treePrev'); if (!prev) return;
      prev.innerHTML='';
      // Header line
      const cap=document.createElement('div'); cap.className='dim mono'; cap.textContent = String(rel||''); prev.appendChild(cap);
      // Action row
      const row=document.createElement('div'); row.className='row';
      const editBtn=document.createElement('button'); editBtn.className='ghost'; editBtn.textContent='Edit'; editBtn.title='Edit this file inline';
      const saveBtn=document.createElement('button'); saveBtn.className='primary'; saveBtn.textContent='Save'; saveBtn.title='Save changes'; saveBtn.style.display='none';
      const revertBtn=document.createElement('button'); revertBtn.className='ghost'; revertBtn.textContent='Revert'; revertBtn.title='Revert to last loaded content'; revertBtn.style.display='none';
      row.appendChild(editBtn); row.appendChild(saveBtn); row.appendChild(revertBtn);
      if (CAN_OPEN_LOCAL) {
        const openEditor=document.createElement('button'); openEditor.className='ghost'; openEditor.textContent='Open in Editor';
        openEditor.addEventListener('click', async ()=>{ try{ const rel = pathRel||''; const data = await getFileMeta(rel); if (data && data.abs_path) { const eff = (projPrefs&&projPrefs.editorCmd) || ((await ARW.getPrefs('launcher'))||{}).editorCmd || null; await ARW.invoke('open_in_editor', { path: data.abs_path, editor_cmd: eff }); } else { ARW.toast('Path unavailable'); } }catch(e){ console.error(e); ARW.toast('Open in Editor failed'); } });
        row.appendChild(openEditor);
      }
      prev.appendChild(row);
      // Preview and editor
      const pre=document.createElement('pre'); pre.className='mono'; pre.style.maxHeight='140px'; pre.style.overflow='auto'; pre.textContent = String(j.content||'');
      const ta=document.createElement('textarea'); ta.style.width='100%'; ta.style.minHeight='140px'; ta.style.display='none'; ta.value = String(j.content||'');
      prev.appendChild(pre); prev.appendChild(ta);
      // State for sha
      let sha = j.sha256 || null;
      const pathRel = rel;
      function toggleEditing(on){ pre.style.display = on? 'none':'block'; ta.style.display = on? 'block':'none'; editBtn.style.display = on? 'none':'inline-block'; saveBtn.style.display = on? 'inline-block':'none'; revertBtn.style.display = on? 'inline-block':'none'; }
      editBtn.addEventListener('click', ()=> toggleEditing(true));
      revertBtn.addEventListener('click', ()=>{ ta.value = String(j.content||''); toggleEditing(false); });
      saveBtn.addEventListener('click', async ()=>{
        try{
          const body = { content: ta.value||'', prev_sha256: sha };
          const resp = await fetchRaw(`/projects/${encodeURIComponent(curProj)}/file?path=${encodeURIComponent(pathRel||'')}`, { method:'PUT', headers:{'Content-Type':'application/json'}, body: JSON.stringify(body) });
          if (!resp.ok){
            if (resp.status === 409){
              // Conflict: fetch latest and present a simple merge panel
              try{
                const j3=await fetchJson(`/state/projects/${encodeURIComponent(curProj)}/file?path=${encodeURIComponent(pathRel||'')}`);
                const merge = document.createElement('div'); merge.className='card'; merge.style.marginTop='6px';
                const h=document.createElement('div'); h.className='row'; h.innerHTML = '<strong>Merge needed</strong><span class="dim">Server changed since you loaded this file</span>';
                const help=document.createElement('p'); help.className='sr-only'; help.id='mergeHelp'; help.textContent='Conflict detected. You can replace the editor with the server version, or save your version by overwriting the server. Use Diff to inspect line differences.';
                const controls=document.createElement('div'); controls.className='row';
                const useServer=document.createElement('button'); useServer.className='ghost'; useServer.textContent='Replace with server'; useServer.title='Load server version into editor'; useServer.setAttribute('aria-describedby','mergeHelp');
                const saveMine=document.createElement('button'); saveMine.className='primary'; saveMine.textContent='Save my version'; saveMine.title='Overwrite server version with your changes'; saveMine.setAttribute('aria-describedby','mergeHelp');
                const copyServer=document.createElement('button'); copyServer.className='ghost'; copyServer.textContent='Copy server'; copyServer.title='Copy server content to clipboard'; copyServer.setAttribute('aria-describedby','mergeHelp');
                const showDiff=document.createElement('button'); showDiff.className='ghost'; showDiff.textContent='Diff'; showDiff.title='Show differences between your version and server'; showDiff.setAttribute('aria-describedby','mergeHelp');
                controls.appendChild(useServer); controls.appendChild(saveMine); controls.appendChild(copyServer); controls.appendChild(showDiff);
                const grid=document.createElement('div'); grid.className='grid cols-2';
                const left=document.createElement('div'); const lt=document.createElement('textarea'); lt.style.width='100%'; lt.style.minHeight='140px'; lt.value = ta.value||''; left.appendChild(lt);
                const right=document.createElement('div'); const rp=document.createElement('pre'); rp.className='mono'; rp.style.maxHeight='140px'; rp.style.overflow='auto'; rp.style.whiteSpace='pre'; rp.textContent = String(j3.content||''); right.appendChild(rp);
                grid.appendChild(left); grid.appendChild(right);
                merge.appendChild(h); merge.appendChild(help); merge.appendChild(controls); merge.appendChild(grid);
                const diffOut=document.createElement('div'); diffOut.className='cmp-code'; diffOut.style.marginTop='6px'; merge.appendChild(diffOut);
                prev.appendChild(merge);
                // Wire actions
                copyServer.addEventListener('click', ()=> ARW.copy(String(j3.content||'')));
                useServer.addEventListener('click', ()=>{ ta.value = String(j3.content||''); lt.value = String(j3.content||''); merge.remove(); ARW.toast('Server version loaded'); });
                saveMine.addEventListener('click', async ()=>{
                  try{
                    const body2 = { content: lt.value||'', prev_sha256: j3.sha256||null };
                    const resp2 = await fetchRaw(`/projects/${encodeURIComponent(curProj)}/file?path=${encodeURIComponent(pathRel||'')}`, { method:'PUT', headers:{'Content-Type':'application/json'}, body: JSON.stringify(body2) });
                    if (!resp2.ok){ ARW.toast('Save failed'); return; }
                    const j4=await fetchJson(`/state/projects/${encodeURIComponent(curProj)}/file?path=${encodeURIComponent(pathRel||'')}`);
                    sha=j4.sha256||null; j.content=j4.content||''; pre.textContent=String(j.content||''); ta.value=j4.content||''; toggleEditing(false); merge.remove(); ARW.toast('Saved');
                  }catch(e){ console.error(e); ARW.toast('Save failed'); }
                });
                // Scroll sync
                let syncing = false;
                const sync = (src, dst)=>{
                  if (syncing) return; syncing = true;
                  try{
                    const ratio = src.scrollTop / Math.max(1, (src.scrollHeight - src.clientHeight));
                    dst.scrollTop = ratio * (dst.scrollHeight - dst.clientHeight);
                  }finally{ syncing = false; }
                };
                lt.addEventListener('scroll', ()=> sync(lt, rp));
                rp.addEventListener('scroll', ()=> sync(rp, lt));

                showDiff.addEventListener('click', ()=>{
                  try{
                    // Use existing diff/highlight helpers
                    const a = lt.value || '';
                    const b = String(j3.content||'');
                    diffOut.innerHTML='';
                    const wrap = document.createDocumentFragment();
                    const frag = (function(){
                      const aa = String(a).split(/\r?\n/);
                      const bb = String(b).split(/\r?\n/);
                      const n = Math.max(aa.length, bb.length);
                      const f = document.createDocumentFragment();
                      const hl = (s)=>{
                        const esc = (t)=> t.replace(/&/g,'&amp;').replace(/</g,'&lt;');
                        let txt = s; try{ txt = JSON.stringify(JSON.parse(s), null, 2) }catch{}
                        return esc(txt)
                          .replace(/\b(true|false)\b/g, '<span class="hl-bool">$1<\/span>')
                          .replace(/\b(null)\b/g, '<span class="hl-null">$1<\/span>')
                          .replace(/\"([^\"]+)\"\s*\:/g, '<span class="hl-key">"$1"<\/span>:')
                          .replace(/:\s*\"([^\"]*)\"/g, ': <span class="hl-str">"$1"<\/span>')
                          .replace(/:\s*(-?\d+(?:\.\d+)?)/g, ': <span class="hl-num">$1<\/span>');
                      };
                      for(let i=0;i<n;i++){
                        const la = aa[i] ?? '';
                        const lb = bb[i] ?? '';
                        if(la === lb){ const div = document.createElement('div'); div.className='cmp-line'; div.textContent=la; f.appendChild(div); }
                        else { const di = document.createElement('div'); di.className='cmp-line del'; di.innerHTML = hl(la); f.appendChild(di); const da = document.createElement('div'); da.className='cmp-line add'; da.innerHTML = hl(lb); f.appendChild(da); }
                      }
                      return f;
                    })();
                    diffOut.appendChild(frag);
                  }catch(e){ console.error(e); }
                });
              }catch(e){ console.error(e); ARW.toast('Conflict: reload file'); }
              return;
            }
            ARW.toast('Save failed'); return;
          }
          // reload to get new sha/content
          const j2=await fetchJson(`/state/projects/${encodeURIComponent(curProj)}/file?path=${encodeURIComponent(pathRel||'')}`);
          sha = j2.sha256||null; j.content = j2.content||''; pre.textContent = String(j.content||''); toggleEditing(false); ARW.toast('Saved');
        }catch(e){ console.error(e); ARW.toast('Save failed'); }
      });
    }catch(e){ console.error(e); }
  }
  // Wire events
  if (elProjSel) elProjSel.addEventListener('change', ()=> setProj(elProjSel.value||''));
  const btnCreate = document.getElementById('btnCreateProj'); if (btnCreate) btnCreate.addEventListener('click', createProj);
  const btnRefresh = document.getElementById('btnRefreshProj'); if (btnRefresh) btnRefresh.addEventListener('click', ()=>{ refreshProjectsSnapshot(); if (curProj) loadTree(''); });
  const btnSaveNotes = document.getElementById('btnSaveNotes'); if (btnSaveNotes) btnSaveNotes.addEventListener('click', saveNotes);
  const btnReloadNotes = document.getElementById('btnReloadNotes'); if (btnReloadNotes) btnReloadNotes.addEventListener('click', loadNotes);
  if (elFileFilter){ elFileFilter.addEventListener('input', async ()=>{ try{ await expandOnSearch((elFileFilter.value||'').trim()); }catch{} renderTree(treeCache.get(String(currentPath||''))||[]); }); }
  // Debounced autosave for notes
  let notesTimer = null;
  if (elNotes){ elNotes.addEventListener('input', async ()=>{ try{ const on = elNotesAutosave ? !!elNotesAutosave.checked : true; if (!on) return; if (notesTimer) clearTimeout(notesTimer); notesTimer = setTimeout(()=> saveNotes(true), 1200); }catch{} }); }
  if (elNotesAutosave){ elNotesAutosave.addEventListener('change', async ()=>{ try{ if (!curProj) return; const ns='ui:proj:'+curProj; const p=await ARW.getPrefs(ns)||{}; p.notesAutoSave = !!elNotesAutosave.checked; await ARW.setPrefs(ns, p); }catch{} }); }
  // Project prefs (per-project overrides like editor)
  const btnProjPrefs = document.getElementById('btnProjPrefs');
  if (btnProjPrefs) btnProjPrefs.addEventListener('click', async ()=>{
    if (!curProj) return;
    try{
      const ns='ui:proj:'+curProj;
      const cur=await ARW.getPrefs(ns)||{};
      const prev=cur.editorCmd||'';
      const result = await ARW.modal.form({
        title: `Editor command for ${curProj}`,
        description: 'Override the command used when opening files from this project. Use {path} as a placeholder.',
        submitLabel: 'Save command',
        cancelLabel: 'Cancel',
        focusField: 'command',
        fields: [
          {
            name: 'command',
            label: 'Command',
            value: prev || '',
            placeholder: 'code --goto {path}',
            hint: 'Leave blank to inherit the global editor preference.',
            autocomplete: 'off',
            trim: true,
          },
        ],
      });
      if (!result) return;
      const next = String(result.command || '').trim();
      if (next) {
        cur.editorCmd = next;
      } else {
        delete cur.editorCmd;
      }
      await ARW.setPrefs(ns, cur);
      projPrefs = cur;
      if (elProjPrefsBadge) elProjPrefsBadge.style.display = (cur.editorCmd? 'inline-flex':'none');
      ARW.toast(next ? 'Project editor set' : 'Project editor cleared');
    }
    catch(e){ console.error(e); ARW.toast('Save failed'); }
  });
  const idProjectsRead = ARW.read.subscribe('projects', (model) => {
    applyProjectsModel(model).catch((err) => console.error(err));
  });
  await refreshProjectsSnapshot();
  // Quick state probe (models count)
  try {
    const j = await fetchJson('/state/models');
    const sec = document.querySelector('#agents');
    if (sec) {
      const b = document.createElement('div');
      b.className = 'badge';
      b.innerHTML = `<span class="dot"></span> models: ${(Array.isArray(j)?j.length:(j?.data?.length||0))}`;
      sec.querySelector('h3')?.appendChild(b);
    }
  } catch {}

  // Compare: JSON-highlighting line diff
  function highlightJSON(s){
    const esc = (t)=> t.replace(/&/g,'&amp;').replace(/</g,'&lt;');
    let txt = s; try{ txt = JSON.stringify(JSON.parse(s), null, 2) }catch{}
    return esc(txt)
      .replace(/\b(true|false)\b/g, '<span class="hl-bool">$1<\/span>')
      .replace(/\b(null)\b/g, '<span class="hl-null">$1<\/span>')
      .replace(/\"([^\"]+)\"\s*\:/g, '<span class="hl-key">"$1"<\/span>:')
      .replace(/:\s*\"([^\"]*)\"/g, ': <span class="hl-str">"$1"<\/span>')
      .replace(/:\s*(-?\d+(?:\.\d+)?)/g, ': <span class="hl-num">$1<\/span>');
  }
  function diffLines(a, b){
    const aa = String(a||'').split(/\r?\n/);
    const bb = String(b||'').split(/\r?\n/);
    const n = Math.max(aa.length, bb.length);
    const frag = document.createDocumentFragment();
    for(let i=0;i<n;i++){
      const la = aa[i] ?? '';
      const lb = bb[i] ?? '';
      if(la === lb){ const div = document.createElement('div'); div.className='cmp-line'; div.textContent=la; frag.appendChild(div); }
      else {
        const di = document.createElement('div'); di.className='cmp-line del'; di.innerHTML = highlightJSON(la); frag.appendChild(di);
        const da = document.createElement('div'); da.className='cmp-line add'; da.innerHTML = highlightJSON(lb); frag.appendChild(da);
      }
    }
    return frag;
  }
  document.getElementById('btn-diff').addEventListener('click', ()=>{
    const a = document.getElementById('cmpA').value;
    const b = document.getElementById('cmpB').value;
    const onlyChanges = document.getElementById('txtOnlyChanges').checked;
    const wrap = document.getElementById('txtWrap').checked;
    const out = document.getElementById('cmpOut'); out.innerHTML='';
    out.style.whiteSpace = wrap ? 'pre-wrap' : 'pre';
    const frag = diffLines(a,b);
    if (onlyChanges){
      [...frag.childNodes].forEach(node=>{ if (node.classList && !/add|del/.test(node.className)) node.remove(); });
    }
    out.appendChild(frag);
  });
  document.getElementById('btn-copy-diff').addEventListener('click', ()=>{
    const txt = document.getElementById('cmpOut').innerText || '';
    ARW.copy(txt);
  });
  function wireDrop(id){
    const el = document.getElementById(id);
    el.addEventListener('dragover', (e)=>{ e.preventDefault(); el.style.outline='2px dashed #b87333' });
    el.addEventListener('dragleave', ()=> el.style.outline='');
    el.addEventListener('drop', async (e)=>{
      e.preventDefault(); el.style.outline='';
      const f = e.dataTransfer?.files?.[0]; if(!f) return;
      try{
        const text = await f.text(); el.value = text;
      }catch{}
    });
  }
  wireDrop('cmpA'); wireDrop('cmpB');
  // Image compare
  function loadImg(input, imgEl){ input.addEventListener('change', ()=>{ const f = input.files?.[0]; if(!f) return; const url = URL.createObjectURL(f); imgEl.src = url; }); }
  loadImg(document.getElementById('imgA'), document.getElementById('imgOver'));
  loadImg(document.getElementById('imgB'), document.getElementById('imgUnder'));
  document.getElementById('imgSlider').addEventListener('input', (e)=>{
    const v = parseInt(e.target.value,10); const clip = 100 - v;
    document.getElementById('imgOver').style.clipPath = `inset(0 ${clip}% 0 0)`;
  });
  // Tabs
  const tabText = document.getElementById('tab-text'); const tabImg = document.getElementById('tab-image');
  const tabCsv = document.getElementById('tab-csv');
  function showTab(which){
    const map = { text:'cmpText', image:'cmpImage', csv:'cmpCSV' };
    const pText = document.getElementById('cmpText'); const pImg = document.getElementById('cmpImage'); const pCsv = document.getElementById('cmpCSV');
    // visibility
    pText.style.display = which==='text' ? 'block':'none'; pText.toggleAttribute('hidden', which!=='text');
    pImg.style.display = which==='image' ? 'block':'none'; pImg.toggleAttribute('hidden', which!=='image');
    pCsv.style.display = which==='csv' ? 'block':'none'; pCsv.toggleAttribute('hidden', which!=='csv');
    // tabs active + aria-selected/tabindex
    const setTab = (el, on)=>{ el.classList.toggle('active', on); el.setAttribute('aria-selected', on? 'true':'false'); el.tabIndex = on? 0 : -1; };
    setTab(tabText, which==='text'); setTab(tabImg, which==='image'); setTab(tabCsv, which==='csv');
  }
  tabText.addEventListener('click', ()=> showTab('text'));
  tabImg.addEventListener('click', ()=> showTab('image'));
  tabCsv.addEventListener('click', ()=> showTab('csv'));
  // Tabs keyboard navigation (roving tabindex)
  const tabList = document.querySelector('.cmp-tabs[role="tablist"]');
  tabList?.addEventListener('keydown', (e)=>{
    const tabs = [tabText, tabImg, tabCsv];
    const cur = document.activeElement;
    let idx = tabs.indexOf(cur);
    if (e.key==='ArrowRight'){ e.preventDefault(); idx = (idx+1+tabs.length)%tabs.length; tabs[idx].focus({ preventScroll:true }); tabs[idx].click(); }
    else if (e.key==='ArrowLeft'){ e.preventDefault(); idx = (idx-1+tabs.length)%tabs.length; tabs[idx].focus({ preventScroll:true }); tabs[idx].click(); }
    else if (e.key==='Home'){ e.preventDefault(); tabs[0].focus({ preventScroll:true }); tabs[0].click(); }
    else if (e.key==='End'){ e.preventDefault(); tabs[tabs.length-1].focus({ preventScroll:true }); tabs[tabs.length-1].click(); }
  });

  // CSV/Table compare
  function detectDelim(s){ const tc=(s.match(/\t/g)||[]).length; const cc=(s.match(/,/g)||[]).length; return tc>cc?'\t':',' }
  function parseCSV(text, hasHeader){
    const delim = detectDelim(text);
    const rows = []; let row = []; let cur=''; let q=false; for(let i=0;i<text.length;i++){
      const ch = text[i];
      if (q){ if (ch==='"'){ if (text[i+1]==='"'){ cur+='"'; i++; } else { q=false; } } else { cur+=ch; } continue; }
      if (ch==='"'){ q=true; continue; }
      if (ch===delim){ row.push(cur); cur=''; continue; }
      if (ch==='\n'){ row.push(cur); rows.push(row); row=[]; cur=''; continue; }
      if (ch==='\r'){ continue; }
      cur += ch;
    }
    if (cur.length>0 || row.length>0) { row.push(cur); rows.push(row); }
    if (!rows.length) return { headers: [], rows: [] };
    let headers = [];
    if (hasHeader){ headers = rows.shift(); }
    else { const maxLen = Math.max(...rows.map(r=>r.length)); headers = Array.from({length:maxLen}, (_,i)=> 'col'+(i+1)); }
    const out = rows.map(r=>{ const o={}; headers.forEach((h,idx)=> o[h]=r[idx]??''); return o; });
    return { headers, rows: out };
  }
  function diffTables(aText, bText, keyColsStr, hasHeader){
    const A = parseCSV(aText, hasHeader); const B = parseCSV(bText, hasHeader);
    const headers = Array.from(new Set([...(A.headers||[]), ...(B.headers||[])]));
    const keys = (keyColsStr||'').split(',').map(s=>s.trim()).filter(Boolean);
    const ks = keys.length? keys : [headers[0]].filter(Boolean);
    const kfun = (o)=> ks.map(k=> (o[k]??'')).join('||');
    const mapA = new Map(A.rows.map(r=> [kfun(r), r]));
    const mapB = new Map(B.rows.map(r=> [kfun(r), r]));
    const added=[], removed=[], changed=[];
    // added/changed
    for (const [k, rb] of mapB.entries()){
      const ra = mapA.get(k);
      if (!ra) { added.push({k, r: rb}); continue; }
      let diff=false; for (const h of headers){ if ((ra[h]||'') !== (rb[h]||'')) { diff=true; break; } }
      if (diff) changed.push({k, a: ra, b: rb});
    }
    for (const [k, ra] of mapA.entries()) if (!mapB.has(k)) removed.push({k, r: ra});
    return { headers, ks, added, removed, changed };
  }
  function renderTableDiff(res){
    const out = document.getElementById('csvOut'); const sum = document.getElementById('csvSummary'); out.innerHTML=''; sum.textContent='';
    window.__lastCsvDiff = res;
    sum.textContent = `+${res.added.length} −${res.removed.length} Δ${res.changed.length}`;
    const tbl = document.createElement('table'); tbl.className='cmp-table';
    const thead = document.createElement('thead'); thead.innerHTML = '<tr>' + ['key', ...res.headers].map(h=>`<th>${h}</th>`).join('') + '</tr>'; tbl.appendChild(thead);
    const tb = document.createElement('tbody');
    const addRows = res.added.map(x=>({ type:'add', key:x.k, row:x.r }));
    const delRows = res.removed.map(x=>({ type:'del', key:x.k, row:x.r }));
    const chRows = res.changed.map(x=>({ type:'chg', key:x.k, a:x.a, b:x.b }));
    const all = [...addRows, ...delRows, ...chRows].sort((x,y)=> String(x.key).localeCompare(String(y.key)));
    for (const e of all){
      const tr = document.createElement('tr'); tr.className = 'cmp-row ' + (e.type==='add'?'add': e.type==='del'?'del':'');
      const keytd = document.createElement('td'); keytd.textContent = e.key; keytd.className='mono'; tr.appendChild(keytd);
      for (const h of res.headers){
        const td = document.createElement('td');
        if (e.type==='chg'){
          const av = e.a[h]||''; const bv = e.b[h]||'';
          if (av !== bv){ td.className='cmp-cell diff'; td.title = `was: ${av}`; td.textContent = bv; }
          else { td.textContent = bv; }
        } else {
          td.textContent = e.row[h]||'';
        }
        tr.appendChild(td);
      }
      tb.appendChild(tr);
    }
    tbl.appendChild(tb); out.appendChild(tbl);
  }
  document.getElementById('btn-csv-diff').addEventListener('click', ()=>{
    const a = document.getElementById('csvA').value; const b = document.getElementById('csvB').value;
    const key = document.getElementById('csvKey').value; const hasHeader = document.getElementById('csvHeader').checked;
    const res = diffTables(a,b,key,hasHeader);
    if (document.getElementById('csvOnlyChanges').checked){
      res.added = res.added; res.removed = res.removed; res.changed = res.changed; // no-op; already only changed
    }
    renderTableDiff(res);
  });
  function downloadCsv(filename, rows){
    const csv = rows.map(r => r.map(v => /[",\n]/.test(String(v)) ? '"'+String(v).replace(/"/g,'""')+'"' : v).join(',')).join('\n');
    const blob = new Blob([csv], { type: 'text/csv;charset=utf-8;' });
    const link = document.createElement('a'); link.href = URL.createObjectURL(blob); link.download = filename; document.body.appendChild(link); link.click(); document.body.removeChild(link);
  }
  document.getElementById('btn-csv-export').addEventListener('click', ()=>{
    const d = window.__lastCsvDiff; if (!d) { ARW.toast('No diff to export'); return; }
    const twoRow = !!document.getElementById('csvTwoRow')?.checked;
    let rows = [['type','key', ...d.headers]];
    d.added.forEach(x=> rows.push(['add', x.k, ...d.headers.map(h=> x.r[h]||'')]));
    d.removed.forEach(x=> rows.push(['del', x.k, ...d.headers.map(h=> x.r[h]||'')]));
    if (twoRow) {
      d.changed.forEach(x=> {
        rows.push(['chg-before', x.k, ...d.headers.map(h=> x.a[h]||'')]);
        rows.push(['chg-after',  x.k, ...d.headers.map(h=> x.b[h]||'')]);
      });
    } else {
      // Wide format: after values only
      d.changed.forEach(x=> rows.push(['chg', x.k, ...d.headers.map(h=> x.b[h]||'')]));
    }
    downloadCsv('table_diff.csv', rows);
  });
  document.getElementById('btn-csv-copy').addEventListener('click', ()=>{
    const sum = document.getElementById('csvSummary').textContent.trim(); if (sum) ARW.copy(sum);
  });
  document.getElementById('port').addEventListener('change', () => {
    applyBaseChange().catch(() => {});
  });
  window.addEventListener('arw:base-override-changed', () => {
    applyBaseChange().catch(() => {});
  });
  document.getElementById('btn-save').addEventListener('click', async ()=>{
    const layout = {
      lanes: ensureLane(
        ensureLane(['timeline','context','provenance','policy','metrics','models','activity'], 'provenance', { after: 'context' }),
        'approvals',
        { after: 'timeline' },
      ),
      grid: 'cols-2',
      focused: document.querySelector('.layout').classList.contains('full')
    };
    await ARW.templates.save('hub', layout);
    const perProj = !!document.getElementById('tplPerProj')?.checked;
    if (perProj && curProj) {
      try { const ns = 'ui:proj:'+curProj; const cur = await ARW.getPrefs(ns)||{}; cur.template = layout; await ARW.setPrefs(ns, cur); } catch {}
    }
  })
  // Apply layout (grid/focused/lanes) from per-project or global template
  function applyTemplate(tpl){
    if (!tpl || typeof tpl !== 'object') return;
    try{
      // Focused mode
      const root = document.querySelector('.layout');
      if (tpl.focused){ root.classList.add('full'); } else { root.classList.remove('full'); }
      // Grid columns
      const main = document.getElementById('main');
      if (main && typeof tpl.grid === 'string'){
        main.classList.remove('cols-1','cols-2','cols-3');
        main.classList.add(tpl.grid);
      }
      // Lanes: re-mount sidecar if lanes differ
      if (Array.isArray(tpl.lanes) && tpl.lanes.length){
        const lanes = ensureLane(tpl.lanes, 'approvals', { after: 'timeline' });
        mountSidecar(lanes, { force: true, source: 'template' });
      }
      ARW.toast('Layout applied');
    }catch(e){ console.error(e); ARW.toast('Apply failed'); }
  }
  document.getElementById('btn-apply').addEventListener('click', async ()=>{
    try{
      let tpl = null;
      try { if (curProj) { const p = await ARW.getPrefs('ui:proj:'+curProj)||{}; tpl = p.template || null; } } catch {}
      if (!tpl) { tpl = await ARW.templates.load('hub'); }
      applyTemplate(tpl);
    }catch(e){ console.error(e); ARW.toast('Apply failed'); }
  });
  // Apply saved template (focused toggle)
  try{
    // Per-project template overrides the global hub template
    let tpl = null;
    try { if (curProj) { const p = await ARW.getPrefs('ui:proj:'+curProj)||{}; tpl = p.template || null; } } catch {}
    if (!tpl) { tpl = await ARW.templates.load('hub'); }
    if (tpl) applyTemplate(tpl);
  }catch{}
  // ---------- Agents: role and state ----------
  async function loadAgentState(){
    try{ const t=await fetchText('/hierarchy/state'); const el=document.getElementById('agentState'); if (el) el.textContent = t; }catch(e){ console.error(e); }
  }
  async function applyRole(){
    try{ const role = (document.getElementById('roleSel')?.value||'edge'); await fetchRaw('/hierarchy/role', { method:'POST', headers:{'Content-Type':'application/json'}, body: JSON.stringify({ role }) }); await loadAgentState(); ARW.toast('Role applied'); }catch(e){ console.error(e); ARW.toast('Apply failed'); }
  }
  document.getElementById('btnRoleApply')?.addEventListener('click', applyRole);
  document.getElementById('btnRoleRefresh')?.addEventListener('click', loadAgentState);
  loadAgentState();
  // Focused layout toggle
  document.getElementById('btn-focus').addEventListener('click', ()=>{
    const root = document.querySelector('.layout');
    root.classList.toggle('full');
  });
  document.getElementById('btn-gallery')?.addEventListener('click', ()=> ARW.gallery.show());
  // Gallery badge: show recent screenshots count
  function updateGalleryBadge(){ try{ const b=document.getElementById('galleryBadge'); if (!b) return; const n = (ARW.gallery && Array.isArray(ARW.gallery._items)) ? ARW.gallery._items.length : 0; b.innerHTML = '<span class="dot"></span> ' + n; b.className = 'badge ' + (n>0 ? 'ok' : ''); }catch{} }
  updateGalleryBadge();
  ARW.sse.subscribe((k)=> k.startsWith('screenshots.'), ()=> updateGalleryBadge());
  // Command palette
  ARW.palette.mount({ base });
});
  // ---------- Compare deep-link (Text/JSON) ----------
  function enc64(s){ try{ return btoa(unescape(encodeURIComponent(String(s||'')))); }catch{ return '' } }
  function dec64(s){ try{ return decodeURIComponent(escape(atob(String(s||'')))); }catch{ return '' } }
  function updateCompareLink(tab){
    try{
      const a = document.getElementById('cmpA')?.value||'';
      const b = document.getElementById('cmpB')?.value||'';
      const t = tab || (document.getElementById('tab-image')?.getAttribute('aria-selected')==='true'? 'image' : (document.getElementById('tab-csv')?.getAttribute('aria-selected')==='true'? 'csv' : 'text'));
      const params = new URLSearchParams();
      if (a) params.set('cmpA', enc64(a));
      if (b) params.set('cmpB', enc64(b));
      if (t) params.set('tab', t);
      const hash = params.toString();
      if (hash) { history.replaceState(null, '', '#' + hash); }
      else { history.replaceState(null, '', window.location.pathname); }
    }catch{}
  }
  function applyCompareFromLink(){
    try{
      const h = window.location.hash.replace(/^#/, ''); if (!h) return;
      const p = new URLSearchParams(h);
      const a = dec64(p.get('cmpA')||''); const b = dec64(p.get('cmpB')||'');
      const tab = p.get('tab')||'text';
      if (a){ const ta=document.getElementById('cmpA'); if (ta) ta.value=a; }
      if (b){ const tb=document.getElementById('cmpB'); if (tb) tb.value=b; }
      // Switch tabs only for text compare (safe default)
      if (tab==='text'){ document.getElementById('tab-text')?.click?.(); }
    }catch{}
  }
  // Update link when user clicks Diff or types in compare boxes
  document.getElementById('btn-diff')?.addEventListener('click', ()=> updateCompareLink('text'));
  document.getElementById('cmpA')?.addEventListener('input', ()=> updateCompareLink('text'));
  document.getElementById('cmpB')?.addEventListener('input', ()=> updateCompareLink('text'));
  applyCompareFromLink();

  window.addEventListener('beforeunload', () => {
    try { ARW.read.unsubscribe(idEpisodesRead); } catch {}
    try { ARW.read.unsubscribe(idProjectsRead); } catch {}
    try { ARW.read.unsubscribe(idRuntimeRead); } catch {}
    try { ARW.read.unsubscribe(idRuntimeMatrixRead); } catch {}
    try { ARW.read.unsubscribe(idRuntimeBundlesRead); } catch {}
    try { ARW.read.unsubscribe(idContextMetricsRead); } catch {}
  });

  // ---------- Artifacts rendering from run snapshot ----------
  function summarizeValue(v){
    if (v == null) return 'null';
    if (typeof v === 'string') return v.length <= 60 ? v : (v.slice(0,57) + '…');
    if (typeof v === 'number' || typeof v === 'boolean') return String(v);
    try{ const s = JSON.stringify(v); return s.length <= 60 ? s : (s.slice(0,57) + '…'); }catch{ return '[object]' }
  }
  function collectArtifactsFromSnapshot(snap){
    const out = [];
    try{
      const items = Array.isArray(snap?.items) ? snap.items : [];
      for (const it of items){
        const kind = it?.kind || 'event';
        const p = it?.payload || {};
        if (p && typeof p === 'object'){
          if (p.output !== undefined){ out.push({ kind, text: JSON.stringify(p.output, null, 2), summary: summarizeValue(p.output) }); }
          else { out.push({ kind, text: JSON.stringify(p, null, 2), summary: summarizeValue(p) }); }
        } else if (typeof p === 'string') {
          out.push({ kind, text: p, summary: summarizeValue(p) });
        }
      }
    }catch{}
    return out;
  }
  function renderArtifacts(){
    if (!elArtifactsTbl) return;
    elArtifactsTbl.innerHTML='';
    const activeRunId = runSnapshot?.id || '';
    const arts = collectArtifactsFromSnapshot(runSnapshot).slice(0, 50);
    for (const a of arts){
      const tr = document.createElement('tr');
      const tdK = document.createElement('td'); tdK.className='mono'; tdK.textContent = a.kind || '';
      const tdS = document.createElement('td'); tdS.className='mono'; tdS.textContent = a.summary || '';
      if (a.summary) tdS.title = a.summary;
      const tdA = document.createElement('td');
      const pa = document.createElement('button');
      pa.className='ghost';
      pa.textContent='Pin A';
      pa.title='Pin to compare slot A';
      const labelA = activeRunId
        ? (a.summary ? `Pin artifact ${a.summary} from run ${activeRunId} to compare slot A` : `Pin artifact from run ${activeRunId} to compare slot A`)
        : (a.summary ? `Pin artifact ${a.summary} to compare slot A` : 'Pin artifact to compare slot A');
      pa.setAttribute('aria-label', labelA);
      pa.addEventListener('click', ()=>{ const ta=document.getElementById('cmpA'); if (ta){ ta.value = a.text||''; updateCompareLink('text'); } });
      const pb = document.createElement('button');
      pb.className='ghost';
      pb.textContent='Pin B';
      pb.title='Pin to compare slot B';
      const labelB = activeRunId
        ? (a.summary ? `Pin artifact ${a.summary} from run ${activeRunId} to compare slot B` : `Pin artifact from run ${activeRunId} to compare slot B`)
        : (a.summary ? `Pin artifact ${a.summary} to compare slot B` : 'Pin artifact to compare slot B');
      pb.setAttribute('aria-label', labelB);
      pb.addEventListener('click', ()=>{ const tb=document.getElementById('cmpB'); if (tb){ tb.value = a.text||''; updateCompareLink('text'); } });
      tdA.appendChild(pa); tdA.appendChild(pb);
      tr.appendChild(tdK); tr.appendChild(tdS); tr.appendChild(tdA);
      elArtifactsTbl.appendChild(tr);
    }
  }
  function updateRunActionLabels(runId){
    const suffix = runId ? ` for run ${runId}` : '';
    if (btnRunCopy){
      btnRunCopy.setAttribute('aria-label', `Copy snapshot${suffix}`);
      btnRunCopy.title = btnRunCopy.title || 'Copy snapshot';
    }
    if (btnRunPinA){
      btnRunPinA.setAttribute('aria-label', `Pin snapshot${suffix} to compare slot A`);
      btnRunPinA.title = btnRunPinA.title || 'Pin snapshot to compare slot A';
    }
    if (btnRunPinB){
      btnRunPinB.setAttribute('aria-label', `Pin snapshot${suffix} to compare slot B`);
      btnRunPinB.title = btnRunPinB.title || 'Pin snapshot to compare slot B';
    }
  }
  updateRunActionLabels('');
