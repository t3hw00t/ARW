const TELEMETRY_POLL_MS = 10_000;
const JOBS_POLL_MS = 12_000;
let telemetryTimer = null;
let jobsTimer = null;
let telemetryBase = null;
let trainingSidecar = null;
let baseMeta = null;
let lastTelemetry = null;
let lastJobs = null;
let jobStatusFilter = 'all';
let logicUnitIndex = new Map();
const dismissedJobs = new Set();
const logicUnitHistory = [];
let logicUnitHistoryRaw = [];
let lastFetchedHistorySignature = '';
let historyHydrated = false;
let pendingHistoryFetch = null;
let telemetryEventTimer = null;
let jobsStatusTimer = null;
const GOVERNOR_HISTORY_LIMIT = 6;
let governorHistory = [];
let lastGovernorToast = { key: null, time: 0 };
let cascadeSummaries = [];
let cascadeMeta = { generated_ms: null };
let cascadeTargetProject = null;
let cascadeHydrated = false;
let cascadeFetchAbort = null;
const TRAINING_LANES_EXPERT = ['timeline', 'approvals', 'context', 'provenance', 'policy', 'metrics', 'models'];
const TRAINING_LANES_GUIDED = ['timeline', 'context', 'metrics'];
const dedupeLanes = (lanes = []) => Array.from(new Set(lanes));
const arraysEqual = (a = [], b = []) => a.length === b.length && a.every((value, index) => value === b[index]);
const lanesForMode = (mode) => (mode === 'expert' ? dedupeLanes(TRAINING_LANES_EXPERT) : dedupeLanes(TRAINING_LANES_GUIDED));
let activeMode = (window.ARW?.mode?.current === 'expert') ? 'expert' : 'guided';
let currentLaneProfile = lanesForMode(activeMode);
let currentSidecarBase = null;
let personaPanelInstance = null;
let personaChangeUnsub = null;

const updateBaseMeta = () => ARW.applyBaseMeta({ portInputId: 'port', badgeId: 'baseBadge', label: 'Base' });

function clearTelemetryTimer() {
  if (telemetryTimer) {
    clearTimeout(telemetryTimer);
    telemetryTimer = null;
  }
}

function scheduleTelemetryRefresh(delay = TELEMETRY_POLL_MS) {
  clearTelemetryTimer();
  telemetryTimer = setTimeout(() => {
    refreshTelemetry().catch(() => {});
  }, delay);
}

function queueTelemetryRefresh(options = {}) {
  const delay = Number.isFinite(options.delay) ? Math.max(0, options.delay) : 400;
  const quiet = options.quiet !== undefined ? Boolean(options.quiet) : true;
  if (document?.hidden) {
    scheduleTelemetryRefresh(delay || 500);
    return;
  }
  if (telemetryEventTimer) {
    clearTimeout(telemetryEventTimer);
    telemetryEventTimer = null;
  }
  clearTelemetryTimer();
  telemetryEventTimer = setTimeout(() => {
    telemetryEventTimer = null;
    refreshTelemetry({ quiet }).catch(() => {});
  }, delay);
}

function clearJobsTimer() {
  if (jobsTimer) {
    clearTimeout(jobsTimer);
    jobsTimer = null;
  }
}

function scheduleJobsRefresh(delay = JOBS_POLL_MS) {
  clearJobsTimer();
  jobsTimer = setTimeout(() => {
    refreshJobs().catch(() => {});
  }, delay);
}

function mountTrainingSidecar({ lanes, force = false } = {}) {
  const profile = dedupeLanes(Array.isArray(lanes) && lanes.length ? lanes : lanesForMode(activeMode));
  const base = telemetryBase || getCurrentBase();
  const profileChanged = !arraysEqual(profile, currentLaneProfile);
  const baseChanged = base !== currentSidecarBase;
  if (!force && !profileChanged && !baseChanged) {
    return;
  }
  try {
    trainingSidecar?.dispose?.();
  } catch {}
  trainingSidecar = ARW.sidecar.mount('sidecar', profile, { base });
  currentLaneProfile = profile;
  currentSidecarBase = base;
}

function updateModeUi(mode, { force = false } = {}) {
  const normalized = mode === 'expert' ? 'expert' : 'guided';
  const shouldRemount = force || normalized !== activeMode;
  activeMode = normalized;
  const hideExpert = normalized !== 'expert';
  document.querySelectorAll('[data-mode="expert-only"]').forEach((el) => {
    if (!(el instanceof HTMLElement)) return;
    if (hideExpert) {
      el.setAttribute('aria-hidden', 'true');
    } else {
      el.removeAttribute('aria-hidden');
    }
  });
  document.querySelectorAll('[data-mode="guided-only"]').forEach((el) => {
    if (!(el instanceof HTMLElement)) return;
    if (hideExpert) {
      el.removeAttribute('aria-hidden');
    } else {
      el.setAttribute('aria-hidden', 'true');
    }
  });
  if (shouldRemount) {
    mountTrainingSidecar({ lanes: lanesForMode(activeMode), force: true });
  }
}

function getCurrentBase() {
  const port = ARW.getPortFromInput('port') || 8091;
  return ARW.base(port);
}

function resolveJobPersona(job) {
  if (!job || typeof job !== 'object') return null;
  const direct = typeof job.persona_id === 'string' && job.persona_id.trim() ? job.persona_id.trim() : null;
  const data = job.data && typeof job.data === 'object' ? job.data : null;
  const dataPersona = data && typeof data.persona_id === 'string' && data.persona_id.trim() ? data.persona_id.trim() : null;
  const trainingPersona = data && data.training && typeof data.training === 'object' && typeof data.training.persona_id === 'string'
    ? data.training.persona_id.trim()
    : null;
  return dataPersona || trainingPersona || direct || null;
}

async function fetchTrainingTelemetry(baseUrl) {
  const token = await ARW.connections.tokenFor(baseUrl);
  const payload = await ARW.invoke('admin_get_json_base', {
    base: baseUrl,
    path: 'state/training/telemetry',
    token,
  });
  return payload;
}

async function fetchLogicUnitHistorySnapshot(baseUrl, { limit = 100, offset = 0 } = {}) {
  const token = await ARW.connections.tokenFor(baseUrl);
  const params = new URLSearchParams();
  if (limit) params.set('limit', String(limit));
  if (offset) params.set('offset', String(offset));
  const path = params.toString()
    ? `state/training/actions?${params.toString()}`
    : 'state/training/actions';
  const payload = await ARW.invoke('admin_get_json_base', {
    base: baseUrl,
    path,
    token,
  });
  if (payload && typeof payload === 'object') {
    const items = Array.isArray(payload.items) ? payload.items : [];
    return {
      items,
      total: typeof payload.total === 'number' ? payload.total : items.length,
      limit: typeof payload.limit === 'number' ? payload.limit : limit,
      offset: typeof payload.offset === 'number' ? payload.offset : offset,
    };
  }
  return { items: [], total: 0, limit, offset };
}

async function fetchOrchestratorJobs(baseUrl) {
  const token = await ARW.connections.tokenFor(baseUrl);
  const payload = await ARW.invoke('admin_get_json_base', {
    base: baseUrl,
    path: 'state/orchestrator/jobs?limit=10',
    token,
  });
  return payload && typeof payload === 'object' ? payload.items || [] : [];
}

async function startTrainingJob(baseUrl, preset, diversity, recency, compression, personaId) {
  const token = await ARW.connections.tokenFor(baseUrl);
  const personaSegment = personaId ? ` persona=${personaId}` : '';
  const goal = `Training${personaSegment} preset=${preset} diversity=${diversity.toFixed(2)} recency=${recency.toFixed(2)} compression=${compression.toFixed(2)}`;
  const body = {
    goal,
    persona_id: personaId || undefined,
    data: {
      preset,
      diversity,
      recency,
      compression,
      persona_id: personaId || undefined,
    },
  };
  return ARW.invoke('admin_post_json_base', {
    base: baseUrl,
    path: 'orchestrator/mini_agents/start_training',
    body,
    token,
  });
}

async function fetchLogicUnits(baseUrl) {
  const token = await ARW.connections.tokenFor(baseUrl);
  const payload = await ARW.invoke('admin_get_json_base', {
    base: baseUrl,
    path: 'logic-units?limit=50',
    token,
  });
  const items = payload && typeof payload === 'object' ? payload.items || [] : [];
  const map = new Map();
  items.forEach((item) => {
    if (item && item.id) {
      map.set(String(item.id), item);
    }
  });
  logicUnitIndex = map;
  return items;
}

async function applyLogicUnit(baseUrl, logicUnit, { dryRun = false } = {}) {
  const token = await ARW.connections.tokenFor(baseUrl);
  if (!logicUnit) throw new Error('logic unit unavailable');
  const manifest = logicUnit.manifest || logicUnit;
  const patches = manifest.patches || logicUnit.patches || [];
  if (!patches.length) throw new Error('logic unit has no patches');
  const body = {
    id: manifest.id || logicUnit.id,
    dry_run: dryRun,
    patches,
  };
  if (manifest.schema_ref) body.schema_ref = manifest.schema_ref;
  if (manifest.schema_pointer) body.schema_pointer = manifest.schema_pointer;
  const result = await ARW.invoke('admin_post_json_base', {
    base: baseUrl,
    path: 'logic-units/apply',
    body,
    token,
  });
  return result;
}

function resolveLogicUnit(job) {
  const candidates = [];
  if (job.logic_unit_id) candidates.push(String(job.logic_unit_id));
  if (job.manifest && job.manifest.id) candidates.push(String(job.manifest.id));
  if (job.id) candidates.push(`lu-${job.id}`);
  for (const id of candidates) {
    if (logicUnitIndex.has(id)) {
      return logicUnitIndex.get(id);
    }
  }
  return null;
}

function setTelemetryStatus(message, tone = 'dim') {
  const node = document.getElementById('contextTelemetryStatus');
  if (!node) return;
  const cls = tone === 'bad' ? 'bad mono' : tone === 'ok' ? 'ok mono' : 'dim mono';
  node.className = cls;
  node.textContent = message;
}

function showTelemetryBody(show) {
  const body = document.getElementById('contextTelemetryBody');
  if (body) body.hidden = !show;
}

function setJobsStatus(message, tone = 'dim', options = {}) {
  const node = document.getElementById('jobsStatus');
  if (!node) return;
  if (jobsStatusTimer) {
    clearTimeout(jobsStatusTimer);
    jobsStatusTimer = null;
  }
  const cls = tone === 'bad' ? 'bad mono' : tone === 'ok' ? 'ok mono' : 'dim mono';
  node.className = cls;
  node.textContent = message;
  const clearAfter = Number.isFinite(options.clearAfter) ? Math.max(0, options.clearAfter) : null;
  if (clearAfter) {
    jobsStatusTimer = setTimeout(() => {
      const defaultClass = 'dim mono';
      node.textContent = '';
      node.className = defaultClass;
      jobsStatusTimer = null;
    }, clearAfter);
  }
}

function escapeText(value) {
  return String(value ?? '').replace(/[\u0000-\u001f\u007f-\u009f]/g, '');
}

function formatRelativeTraining(ms) {
  if (!Number.isFinite(ms)) return 'recently';
  const diff = Date.now() - ms;
  if (!Number.isFinite(diff)) return 'recently';
  if (Math.abs(diff) < 1000) return 'just now';
  const abs = Math.abs(diff);
  const sign = diff >= 0 ? 'ago' : 'from now';
  const minutes = abs / 60000;
  if (minutes < 1) return `${Math.round(abs / 1000)} s ${sign}`;
  if (minutes < 60) return `${Math.round(minutes)} m ${sign}`;
  const hours = minutes / 60;
  if (hours < 24) return `${Math.round(hours)} h ${sign}`;
  const days = hours / 24;
  if (days < 7) return `${Math.round(days)} d ${sign}`;
  const weeks = days / 7;
  if (weeks < 4) return `${Math.round(weeks)} w ${sign}`;
  const months = days / 30;
  if (months < 12) return `${Math.round(months)} mo ${sign}`;
  const years = days / 365;
  return `${Math.round(years)} yr ${sign}`;
}

function jobStatusMeta(job) {
  const raw = job && (job.status_slug || job.status || job.state || 'unknown');
  const slug = (() => {
    if (typeof raw !== 'string') return 'unknown';
    const trimmed = raw.trim().toLowerCase();
    return trimmed || 'unknown';
  })();
  const label = (() => {
    if (typeof job?.status_label === 'string' && job.status_label.trim()) {
      return job.status_label.trim();
    }
    if (slug === 'unknown') {
      return typeof raw === 'string' && raw.trim() ? raw.trim() : 'Unknown';
    }
    return slug
      .split('_')
      .map((part) => (part ? part[0].toUpperCase() + part.slice(1) : ''))
      .join(' ') || 'Unknown';
  })();
  return { slug, label };
}

function formatRelativeAbsTraining(ms) {
  if (!Number.isFinite(ms)) return new Date().toLocaleString();
  const date = new Date(ms);
  return `${date.toLocaleDateString()} ${date.toLocaleTimeString([], { hour: '2-digit', minute: '2-digit', second: '2-digit' })}`;
}

function formatPercent(value, digits = 0) {
  if (typeof value !== 'number' || !Number.isFinite(value)) return '—';
  const pct = (value * 100).toFixed(digits);
  return `${pct}%`;
}

function formatSlotName(slot) {
  return escapeText((slot || '').replace(/[_-]/g, ' '));
}

function formatStatusLabel(status) {
  if (!status) return 'unknown';
  const map = {
    active: 'Active',
    renew_due: 'Renew window',
    expiring: 'Expiring soon',
    expired: 'Expired',
    unbounded: 'No lease',
  };
  return map[status] || status.replace(/_/g, ' ');
}

function normaliseHintNumber(value) {
  if (typeof value === 'number' && Number.isFinite(value)) {
    return Math.min(Math.max(value, 0), 1);
  }
  if (typeof value === 'string' && value.trim()) {
    const parsed = Number.parseFloat(value.trim());
    if (Number.isFinite(parsed)) {
      return Math.min(Math.max(parsed, 0), 1);
    }
  }
  return null;
}

function extractTrainingHints(job) {
  if (!job || typeof job !== 'object') return null;
  const raw = job.data;
  if (!raw || typeof raw !== 'object') return null;
  const source = raw.training && typeof raw.training === 'object' ? raw.training : raw;
  if (!source || typeof source !== 'object') return null;
  const presetRaw = typeof source.preset === 'string' ? source.preset.trim() : '';
  const preset = presetRaw ? presetRaw : null;
  const diversity = normaliseHintNumber(source.diversity);
  const recency = normaliseHintNumber(source.recency);
  const compression = normaliseHintNumber(source.compression);
  const modeRaw = typeof source.mode === 'string' ? source.mode.trim() : '';
  const mode = modeRaw ? modeRaw : null;
  if (!preset && diversity == null && recency == null && compression == null && !mode) return null;
  return { preset, diversity, recency, compression, mode };
}

function formatTrainingHintsSummary(job) {
  const hints = extractTrainingHints(job);
  if (!hints) return null;
  return {
    text: formatGovernorHints(hints),
    hints,
  };
}

function normalizeGovernorHints(raw) {
  if (!raw || typeof raw !== 'object' || Array.isArray(raw)) return null;
  const source = raw;
  const normalizeNumber = (value) => {
    if (typeof value === 'number' && Number.isFinite(value)) {
      return Math.min(Math.max(value, 0), 1);
    }
    if (typeof value === 'string') {
      const trimmed = value.trim();
      if (trimmed) {
        const parsed = Number.parseFloat(trimmed);
        if (Number.isFinite(parsed)) {
          return Math.min(Math.max(parsed, 0), 1);
        }
      }
    }
    return null;
  };
  const pickNumber = (...keys) => {
    for (const key of keys) {
      if (key in source) {
        const normalized = normalizeNumber(source[key]);
        if (normalized != null) return normalized;
      }
    }
    return null;
  };
  const presetRaw = typeof source.preset === 'string' ? source.preset.trim() : '';
  const modeRaw = typeof source.mode === 'string' ? source.mode.trim().toLowerCase() : '';
  const normalized = {
    mode: modeRaw || null,
    preset: presetRaw || null,
    diversity: pickNumber('diversity', 'retrieval_div'),
    recency: pickNumber('recency', 'mmr_lambda'),
    compression: pickNumber('compression', 'compression_aggr'),
  };
  if (
    !normalized.mode &&
    !normalized.preset &&
    normalized.diversity == null &&
    normalized.recency == null &&
    normalized.compression == null
  ) {
    return null;
  }
  return normalized;
}

function formatJobCategory(job) {
  if (!job) return '—';
  const category = (job.data && job.data.category) || job.category;
  if (!category) return '—';
  const label = typeof category === 'string' ? category : String(category);
  return label.replace(/_/g, ' ');
}

function governorHistoryKey(hints) {
  const keyObj = {
    mode: hints.mode || null,
    diversity: Number.isFinite(hints.diversity) ? Number(hints.diversity.toFixed(4)) : null,
    recency: Number.isFinite(hints.recency) ? Number(hints.recency.toFixed(4)) : null,
    compression: Number.isFinite(hints.compression) ? Number(hints.compression.toFixed(4)) : null,
  };
  return JSON.stringify(keyObj);
}

function formatGovernorHints(hints) {
  if (!hints || typeof hints !== 'object') return '';
  const parts = [];
  if (hints.mode) {
    const label = hints.mode.charAt(0).toUpperCase() + hints.mode.slice(1);
    parts.push(`Mode ${label}`);
  }
  if (hints.preset) parts.push(`Preset ${hints.preset}`);
  if (Number.isFinite(hints.diversity)) parts.push(`Diversity ${formatPercent(hints.diversity, 0)}`);
  if (Number.isFinite(hints.recency)) parts.push(`Recency ${formatPercent(hints.recency, 0)}`);
  if (Number.isFinite(hints.compression)) parts.push(`Compression ${formatPercent(hints.compression, 0)}`);
  return parts.join(' · ');
}

function recordGovernorProfile(rawHints, options = {}) {
  const normalized = normalizeGovernorHints(rawHints);
  if (!normalized) return null;
  const key = governorHistoryKey(normalized);
  let finalHints = normalized;
  let finalSummary = formatGovernorHints(normalized);
  const existingIndex = governorHistory.findIndex((entry) => entry.key === key);
  if (existingIndex !== -1) {
    const existing = governorHistory.splice(existingIndex, 1)[0];
    const mergedHints = { ...existing.hints };
    if (normalized.mode) mergedHints.mode = normalized.mode;
    if (normalized.preset) mergedHints.preset = normalized.preset;
    if (normalized.diversity != null) mergedHints.diversity = normalized.diversity;
    if (normalized.recency != null) mergedHints.recency = normalized.recency;
    if (normalized.compression != null) mergedHints.compression = normalized.compression;
    const mergedSummary = formatGovernorHints(mergedHints);
    governorHistory.unshift({ key, hints: mergedHints, summary: mergedSummary });
    finalHints = mergedHints;
    finalSummary = mergedSummary;
  } else {
    const summaryText = formatGovernorHints(normalized);
    governorHistory.unshift({ key, hints: normalized, summary: summaryText });
    if (governorHistory.length > GOVERNOR_HISTORY_LIMIT) {
      governorHistory.length = GOVERNOR_HISTORY_LIMIT;
    }
    finalSummary = summaryText;
    finalHints = normalized;
  }
  persistDismissed();
  renderGovernorHistory();
  return { key, summary: finalSummary, hints: finalHints };
}

function renderGovernorHistory() {
  const list = document.getElementById('governorHistoryList');
  const clearBtn = document.getElementById('clearGovernorHistory');
  if (!list) return;
  list.innerHTML = '';
  if (!governorHistory.length) {
    const li = document.createElement('li');
    li.className = 'dim';
    li.textContent = 'No profiles yet.';
    list.appendChild(li);
    if (clearBtn) clearBtn.disabled = true;
    return;
  }
  if (clearBtn) clearBtn.disabled = false;
  governorHistory.forEach((entry) => {
    const li = document.createElement('li');
    const btn = document.createElement('button');
    btn.className = 'ghost';
    btn.type = 'button';
    const summaryText = entry.summary || 'Governor profile';
    const personaLabel = entry.personaId ? escapeText(entry.personaId) : null;
    btn.textContent = personaLabel ? `${summaryText} (Persona ${personaLabel})` : summaryText;
    btn.addEventListener('click', async () => {
      btn.disabled = true;
      try {
        const base = telemetryBase || getCurrentBase();
        await applyTrainingHints(base, entry.hints, { source: 'history' });
      } catch (err) {
        const message = err && err.message ? err.message : 'Apply failed';
        ARW.toast(message);
      } finally {
        btn.disabled = false;
      }
    });
    li.appendChild(btn);
    list.appendChild(li);
  });
}

function friendlyHintSource(source) {
  switch ((source || '').toLowerCase()) {
    case 'orchestrator':
      return 'training run';
    case 'admin':
      return 'admin';
    case 'feedback':
      return 'feedback';
    case 'history':
      return 'history';
    case 'manual':
      return 'launcher';
    default:
      return source || '';
  }
}

function maybeToastGovernorSummary(summary, key, options = {}) {
  const now = Date.now();
  if (key && lastGovernorToast.key === key && now - lastGovernorToast.time < 1200) {
    return;
  }
  const sourceLabel = friendlyHintSource(options.source);
  const prefix = sourceLabel ? `Governor hints (${sourceLabel})` : 'Governor hints';
  const message = summary ? `${prefix}: ${summary}` : `${prefix} applied`;
  ARW.toast(message);
  if (key) {
    lastGovernorToast = { key, time: now };
  }
}

function clearGovernorHistory() {
  governorHistory = [];
  persistDismissed();
  renderGovernorHistory();
  ARW.toast('Governor history cleared');
}

async function applyTrainingHints(baseUrl, hints, { source = 'manual', silent = false } = {}) {
  const normalized = normalizeGovernorHints(hints);
  if (!normalized) throw new Error('No training hints available');
  const body = {};
  if (normalized.mode) body.mode = normalized.mode.toLowerCase();
  if (normalized.diversity != null) body.retrieval_div = normalized.diversity;
  if (normalized.recency != null) body.mmr_lambda = normalized.recency;
  if (normalized.compression != null) body.compression_aggr = normalized.compression;
  if (Object.keys(body).length === 0) throw new Error('Training hints missing governor parameters');
  const token = await ARW.connections.tokenFor(baseUrl);
  await ARW.invoke('admin_post_json_base', {
    base: baseUrl,
    path: 'governor/hints',
    body,
    token,
  });
  const recorded = recordGovernorProfile(normalized, { silent: true });
  const summary = recorded?.summary || formatGovernorHints(normalized);
  const key = recorded?.key || governorHistoryKey(normalized);
  if (!silent) {
    maybeToastGovernorSummary(summary, key, { source });
  }
  return { summary, key };
}

function renderList(targetId, items, formatter, emptyText) {
  const el = typeof targetId === 'string' ? document.getElementById(targetId) : targetId;
  if (!el) return;
  el.innerHTML = '';
  if (!Array.isArray(items) || items.length === 0) {
    const li = document.createElement('li');
    li.className = 'dim';
    li.textContent = emptyText || 'No data';
    el.appendChild(li);
    return;
  }
  for (const item of items) {
    const entry = formatter(item);
    if (!entry) continue;
    const li = document.createElement('li');
    li.textContent = entry;
    el.appendChild(li);
  }
  if (!el.childElementCount) {
    const li = document.createElement('li');
    li.className = 'dim';
    li.textContent = emptyText || 'No data';
    el.appendChild(li);
  }
}

function setCascadeStatus(text, level = 'info') {
  const node = document.getElementById('cascadeStatus');
  if (!node) return;
  node.classList.remove('bad', 'dim', 'ok');
  if (level === 'bad') node.classList.add('bad');
  else if (level === 'ok') node.classList.add('ok');
  else node.classList.add('dim');
  node.textContent = text;
}

function renderCascadeSummariesList() {
  const list = document.getElementById('cascadeSummaries');
  if (!list) return;
  list.innerHTML = '';
  if (!Array.isArray(cascadeSummaries) || cascadeSummaries.length === 0) {
    const li = document.createElement('li');
    li.className = 'dim';
    li.textContent = 'No cascade summaries yet';
    list.appendChild(li);
    return;
  }
  cascadeSummaries.slice(0, 20).forEach((record) => {
    const value = record && typeof record === 'object' && record.value && typeof record.value === 'object'
      ? record.value
      : {};
    const stats = value.stats && typeof value.stats === 'object' ? value.stats : {};
    const episodeId = value.episode_id || record.key || 'episode';
    const abstract = value.abstract && typeof value.abstract === 'object' ? value.abstract : {};
    const outline = Array.isArray(value.outline) ? value.outline : [];
    const events = Number(stats.events || 0);
    const errors = Number(stats.errors || 0);
    let summaryText = abstract.text || record.text || '';
    if (!summaryText && outline.length) {
      summaryText = outline.slice(0, 2).map((item) => String(item || '')).filter(Boolean).join(' | ');
    }
    if (!summaryText) summaryText = 'summary unavailable';
    let label = `${episodeId}: ${summaryText}`;
    if (events > 0) {
      label += ` · ${events} event${events === 1 ? '' : 's'}`;
    }
    if (errors > 0) {
      label += ` (${errors} error${errors === 1 ? '' : 's'})`;
    }
    const li = document.createElement('li');
    li.textContent = label;
    list.appendChild(li);
  });
}

async function refreshCascadeSummaries(options = {}) {
  const base = telemetryBase || getCurrentBase();
  if (!base) return;
  if (cascadeFetchAbort) {
    try { cascadeFetchAbort.abort(); } catch {}
  }
  const controller = new AbortController();
  cascadeFetchAbort = controller;
  const project = options.project !== undefined ? options.project : cascadeTargetProject;
  const quiet = Boolean(options.quiet);
  if (!quiet) {
    setCascadeStatus('Loading…');
  }
  try {
    const params = new URLSearchParams();
    params.set('limit', '40');
    if (project) params.set('project', project);
    const response = await ARW.http.json(base, `/state/context/cascade?${params.toString()}`, { signal: controller.signal });
    cascadeSummaries = Array.isArray(response?.items) ? response.items : [];
    cascadeMeta = {
      generated_ms: Number.isFinite(Number(response?.generated_ms)) ? Number(response.generated_ms) : null,
      generated: response?.generated || null,
      project: project || null,
    };
    cascadeHydrated = true;
    renderCascadeSummariesList();
    if (cascadeMeta.generated_ms != null) {
      const rel = formatRelativeTraining(cascadeMeta.generated_ms);
      const abs = formatRelativeAbsTraining(cascadeMeta.generated_ms);
      const scope = cascadeMeta.project ? ` · project ${cascadeMeta.project}` : '';
      setCascadeStatus(`Updated ${rel} (${abs})${scope}`, 'ok');
    } else {
      const scope = cascadeMeta.project ? `project ${cascadeMeta.project}` : 'cascade';
      setCascadeStatus(`Updated ${new Date().toLocaleTimeString()} · ${scope}`, 'ok');
    }
  } catch (err) {
    if (err?.name === 'AbortError') return;
    console.error('cascade summaries fetch failed', err);
    if (!cascadeHydrated) {
      renderCascadeSummariesList();
    }
    setCascadeStatus('Cascade refresh failed', 'bad');
  } finally {
    if (cascadeFetchAbort === controller) cascadeFetchAbort = null;
  }
}

function renderTelemetry(data) {
  lastTelemetry = data;
  let updatedMs = Number(data?.generated_ms ?? data?.generatedMs);
  if (!Number.isFinite(updatedMs)) {
    const parsed = Date.parse(data && data.generated ? data.generated : '');
    updatedMs = Number.isNaN(parsed) ? null : parsed;
  }
  if (Number.isFinite(updatedMs)) {
    setTelemetryStatus(`Updated ${formatRelativeTraining(updatedMs)} (${formatRelativeAbsTraining(updatedMs)})`);
  } else if (data && data.generated) {
    setTelemetryStatus(`Updated ${escapeText(data.generated)}`);
  } else {
    setTelemetryStatus('Updated (time unknown)');
  }

  const context = data && data.context ? data.context : {};
  const coverage = context.coverage || {};
  const recall = context.recall_risk || {};
  const latestCoverage = coverage.latest || {};
  const coverageNeedsMore = Boolean(latestCoverage.needs_more);
  const coverageBadge = document.getElementById('coverageNeedsMore');
  if (coverageBadge) {
    coverageBadge.className = `metric-pill ${coverageNeedsMore ? 'bad' : 'ok'}`;
    coverageBadge.textContent = coverageNeedsMore ? 'Needs more coverage' : 'Coverage satisfied';
  }
  const coverageRatioRaw = coverage.needs_more_ratio;
  const coverageRatio = Number.isFinite(coverageRatioRaw) ? coverageRatioRaw : null;
  const ratioEl = document.getElementById('coverageRatio');
  if (ratioEl) ratioEl.textContent = coverageRatio == null ? '—' : formatPercent(coverageRatio, 0);
  ARW.ui.updateRatioBar('coverageRatioBar', coverageRatio, {
    preferLow: true,
    warn: 0.2,
    bad: 0.4,
    formatText: (_value, pct) => `${pct}% of assemblies needing more coverage`,
  });

  const reasons = Array.isArray(latestCoverage.reasons) ? latestCoverage.reasons : [];
  renderList('coverageReasons', reasons, (reason) => {
    if (!reason) return null;
    if (typeof reason === 'string' && reason.startsWith('slot_underfilled:')) {
      const slot = reason.split(':')[1] || '';
      return `Slot underfilled — ${formatSlotName(slot)}`;
    }
    return escapeText(reason);
  }, 'No recent coverage gaps');

  const topCoverageSlots = Array.isArray(coverage.top_slots) ? coverage.top_slots : [];
  renderList(
    'coverageTopSlots',
    topCoverageSlots,
    (item) => {
      if (!item || typeof item !== 'object') return null;
      const slot = formatSlotName(item.slot || '');
      const count = item.count ?? 0;
      return `${slot} · ${count} event${count === 1 ? '' : 's'}`;
    },
    'No slot gaps observed'
  );

  const avgScoreRaw = recall.avg_score;
  const avgScore = Number.isFinite(avgScoreRaw) ? avgScoreRaw : null;
  const avgScoreEl = document.getElementById('recallAvgScore');
  if (avgScoreEl) avgScoreEl.textContent = avgScore == null ? '—' : formatPercent(avgScore, 0);
  ARW.ui.updateRatioBar('recallAvgScoreBar', avgScore, {
    preferLow: true,
    warn: 0.45,
    bad: 0.7,
    formatText: (_value, pct) => `Risk score ${pct}%`,
  });
  const atRiskRatioRaw = recall.at_risk_ratio;
  const atRiskRatio = Number.isFinite(atRiskRatioRaw) ? atRiskRatioRaw : null;
  const atRiskEl = document.getElementById('recallAtRisk');
  if (atRiskEl) atRiskEl.textContent = atRiskRatio == null ? '—' : formatPercent(atRiskRatio, 0);
  ARW.ui.updateRatioBar('recallAtRiskBar', atRiskRatio, {
    preferLow: true,
    warn: 0.2,
    bad: 0.4,
    formatText: (_value, pct) => `${pct}% of assemblies flagged at risk`,
  });

  const recallSlots = Array.isArray(recall.top_slots) ? recall.top_slots : [];
  renderList(
    'recallTopSlots',
    recallSlots,
    (item) => {
      if (!item || typeof item !== 'object') return null;
      const slot = formatSlotName(item.slot || '');
      const avgGap = item.avg_gap;
      const maxGap = item.max_gap;
      const samples = item.samples ?? 0;
      const avgLabel = avgGap == null ? '—' : formatPercent(avgGap, 0);
      const maxLabel = maxGap == null ? '—' : formatPercent(maxGap, 0);
      return `${slot} · avg gap ${avgLabel} · max ${maxLabel} · ${samples} sample${samples === 1 ? '' : 's'}`;
    },
    'No slot gaps recorded'
  );

  const assembled = context.assembled || {};
  const workingSet = assembled.working_set || {};
  const counts = workingSet.counts || {};
  const countsEl = document.getElementById('contextSummaryCounts');
  if (countsEl) {
    const items = counts.items ?? 0;
    const seeds = counts.seeds ?? 0;
    const expanded = counts.expanded ?? 0;
    countsEl.textContent = `Items ${items} · Seeds ${seeds} · Expanded ${expanded}`;
  }
  const spec = workingSet.final_spec || assembled.spec || {};
  const parts = [];
  if (Array.isArray(spec.lanes) && spec.lanes.length) {
    parts.push(`Lanes: ${spec.lanes.map((lane) => formatSlotName(lane)).join(', ')}`);
  }
  if (spec.project) {
    parts.push(`Project: ${spec.project}`);
  }
  const slotBudgets = spec.slot_budgets || {};
  const budgetKeys = Object.keys(slotBudgets);
  if (budgetKeys.length) {
    const budgets = budgetKeys
      .sort()
      .map((slot) => `${formatSlotName(slot)}=${slotBudgets[slot]}`)
      .join(', ');
    parts.push(`Slot budgets: ${budgets}`);
  }
  if (spec.query_provided) {
    parts.push('Query provided');
  }
  const specEl = document.getElementById('contextSpecSummary');
  if (specEl) {
    specEl.textContent = parts.length ? parts.join(' · ') : '—';
  }

  let nextCascadeProject = null;
  if (typeof spec.project === 'string' && spec.project.trim()) {
    nextCascadeProject = spec.project.trim();
  } else if (Array.isArray(workingSet.projects) && workingSet.projects.length) {
    const first = workingSet.projects.find((proj) => typeof proj === 'string' && proj.trim());
    if (first) nextCascadeProject = first.trim();
  }
  const prevProjectKey = cascadeTargetProject ? cascadeTargetProject.toLowerCase() : null;
  const nextProjectKey = nextCascadeProject ? nextCascadeProject.toLowerCase() : null;
  if (nextProjectKey !== prevProjectKey) {
    cascadeTargetProject = nextCascadeProject || null;
    cascadeHydrated = false;
    refreshCascadeSummaries({ project: cascadeTargetProject, quiet: true });
  } else if (!cascadeHydrated && !cascadeFetchAbort) {
    refreshCascadeSummaries({ project: cascadeTargetProject, quiet: true });
  }

  const capsules = data && typeof data.capsules === 'object' ? data.capsules : {};
  const capsuleSummaryEl = document.getElementById('capsuleAccessibleSummary');
  if (capsuleSummaryEl) {
    capsuleSummaryEl.textContent = capsules.accessible_summary || 'No policy capsules active.';
  }
  const nextExpiryEl = document.getElementById('capsuleNextExpiry');
  if (nextExpiryEl) {
    const nextExpiryMs = Number(capsules.next_expiry_ms);
    if (Number.isFinite(nextExpiryMs)) {
      const rel = formatRelativeTraining(nextExpiryMs);
      const abs = formatRelativeAbsTraining(nextExpiryMs);
      const label = capsules.next_expiry_label ? `${capsules.next_expiry_label} · ` : '';
      nextExpiryEl.textContent = `${label}${rel} (${abs})`;
    } else {
      nextExpiryEl.textContent = '—';
    }
  }
  const statusCounts = capsules.status_counts && typeof capsules.status_counts === 'object'
    ? Object.entries(capsules.status_counts)
    : [];
  renderList(
    'capsuleStatusCounts',
    statusCounts,
    ([status, total]) => {
      const count = Number(total || 0);
      return `${formatStatusLabel(status)} · ${count}`;
    },
    capsules.count ? 'No recent status changes' : 'No policy capsules active'
  );
  const capsuleSample = Array.isArray(capsules.sample) ? capsules.sample : [];
  renderList(
    'capsuleSample',
    capsuleSample,
    (item) => {
      if (!item || typeof item !== 'object') return null;
      const id = item.id || 'capsule';
      const label = item.status_label || formatStatusLabel(item.status);
      const leaseUntil = Number(item.lease_until_ms);
      const renewStarted = item.renew_window_started === true;
      let suffix = '';
      if (Number.isFinite(leaseUntil)) {
        suffix = ` · ${formatRelativeTraining(leaseUntil)} (${formatRelativeAbsTraining(leaseUntil)})`;
      } else if (Number.isFinite(Number(item.expires_in_ms))) {
        suffix = ` · expires ${formatRelativeTraining(Date.now() + Number(item.expires_in_ms))}`;
      } else if (renewStarted) {
        suffix = ' · renewal window open';
      }
      return `${id} · ${label}${suffix}`;
    },
    capsules.count ? 'No sample available' : 'No policy capsules active'
  );

  const toolInvocations = data && typeof data.tool_invocations === 'object' ? data.tool_invocations : {};
  const overallTools = toolInvocations && typeof toolInvocations.overall === 'object' ? toolInvocations.overall : {};
  const toolOverallEl = document.getElementById('toolSuccessOverall');
  if (toolOverallEl) {
    const total = Number(overallTools.total ?? 0);
    if (total > 0) {
      const success = Number(overallTools.success ?? 0);
      const failed = Number(overallTools.failed ?? 0);
      const rate = overallTools.success_rate;
      const rateLabel = Number.isFinite(rate) ? formatPercent(rate, 0) : '—';
      toolOverallEl.textContent = `${success}/${total} success (${rateLabel}) · ${failed} failed`;
    } else {
      toolOverallEl.textContent = 'No recent tool invocations.';
    }
  }

  const toolSummary = Array.isArray(toolInvocations.summary) ? toolInvocations.summary : [];
  renderList(
    'toolSuccessList',
    toolSummary,
    (entry) => {
      if (!entry || typeof entry !== 'object') return null;
      const toolId = entry.tool_id || 'tool';
      const total = Number(entry.total ?? 0);
      const success = Number(entry.success ?? 0);
      const failed = Number(entry.failed ?? 0);
      const rate = entry.success_rate;
      const rateLabel = Number.isFinite(rate) ? formatPercent(rate, 0) : '—';
      let lastNote = '';
      const lastMs = Number(entry.last_ms ?? 0);
      if (Number.isFinite(lastMs) && lastMs > 0) {
        lastNote = ` · last ${formatRelativeTraining(lastMs)}`;
      }
      const status = typeof entry.last_status === 'string' && entry.last_status.trim()
        ? entry.last_status.trim()
        : '';
      if (status) {
        lastNote += lastNote ? ` (${status})` : ` (${status})`;
      }
      return `${toolId} · ${success}/${total} success (${rateLabel}) · ${failed} failed${lastNote}`;
    },
    'No tool activity recorded'
  );

  const toolTotals = Array.isArray(data?.tools?.totals_by_tool) ? data.tools.totals_by_tool : [];
  renderList(
    'toolUsageTotals',
    toolTotals,
    (entry) => {
      if (!entry || typeof entry !== 'object') return null;
      const toolId = entry.tool_id || 'tool';
      const count = Number(entry.count ?? 0);
      return `${toolId} · ${count}`;
    },
    'No usage recorded'
  );

  showTelemetryBody(true);
}

function renderJobs(items) {
  lastJobs = Array.isArray(items) ? items : [];
  const container = document.getElementById('results');
  if (!container) return;
  container.innerHTML = '';

  const filtered = lastJobs.filter((job) => {
    if (dismissedJobs.has(job.id || job.job_id)) {
      return false;
    }
    if (jobStatusFilter === 'all') return true;
    const { slug } = jobStatusMeta(job);
    if (jobStatusFilter === 'running') {
      return slug === 'running';
    }
    if (jobStatusFilter === 'completed') {
      return slug === 'completed';
    }
    if (jobStatusFilter === 'queued') {
      return slug === 'queued';
    }
    if (jobStatusFilter === 'failed') {
      return slug === 'failed';
    }
    return true;
  });

  if (!filtered.length) {
    const p = document.createElement('p');
    p.className = 'dim';
    p.textContent = 'No jobs match the current filters.';
    container.appendChild(p);
    return;
  }

  const table = document.createElement('table');
  table.className = 'jobs-table';
  table.innerHTML = '<thead><tr><th>Job</th><th>Category</th><th>Persona</th><th>Status</th><th>Progress</th><th>Goal</th><th>Updated</th></tr></thead>';
  const tbody = document.createElement('tbody');

  filtered.forEach((job) => {
    const tr = document.createElement('tr');
    const progress = typeof job.progress === 'number' ? formatPercent(job.progress, 0) : '—';
    const statusMeta = jobStatusMeta(job);
    const statusSlug = statusMeta.slug;
    const statusLabel = statusMeta.label;
    const safeLabel = escapeText(statusLabel);
    const safeSlug = escapeText(statusSlug);
    const statusDisplay = statusSlug === 'unknown'
      ? safeLabel
      : `${safeLabel} (${safeSlug})`;
    const statusClass = statusSlug === 'failed'
      ? 'bad'
      : statusSlug === 'running'
        ? 'accent'
        : statusSlug === 'cancelled'
          ? 'dim'
        : statusSlug === 'completed'
          ? 'ok'
          : '';
    const trainingSummary = formatTrainingHintsSummary(job);
    const personaId = resolveJobPersona(job);
    const personaLabel = personaId ? escapeText(personaId) : '—';
    const updated = job.updated_at || job.updated || job.finished_at || job.created_at || job.created || '';
    let updatedLabel = '—';
    if (updated) {
      const parsed = new Date(updated);
      updatedLabel = Number.isNaN(parsed.getTime())
        ? escapeText(String(updated))
        : parsed.toLocaleTimeString([], { hour: '2-digit', minute: '2-digit', second: '2-digit' });
    }
    const goalText = escapeText(job.goal || '');
    const categoryName = formatJobCategory(job);
    const categoryLabel = escapeText(categoryName);
    const goalHints = [];
    if (trainingSummary && trainingSummary.text) goalHints.push(trainingSummary.text);
    if (personaId) goalHints.push(`Persona ${personaId}`);
    const goalHintMarkup = goalHints.length ? `<span class="hint">${escapeText(goalHints.join(' · '))}</span>` : '';
    const goalCell = `${goalText}${goalHintMarkup}`;
    tr.innerHTML = `
      <td class="mono">${escapeText(job.id || '')}</td>
      <td class="mono">${categoryLabel}</td>
      <td class="mono">${personaLabel}</td>
      <td class="${statusClass}">${statusDisplay}</td>
      <td>${progress}</td>
      <td>${goalCell}</td>
      <td class="mono">${updatedLabel}</td>
    `;
    tr.dataset.jobId = job.id || '';
    tr.tabIndex = 0;
    tr.addEventListener('focus', () => tr.classList.add('focused-row'));
    tr.addEventListener('blur', () => tr.classList.remove('focused-row'));
    tr.addEventListener('keydown', (event) => {
      const key = event.key;
      if (key === 'ArrowDown' || key === 'ArrowUp') {
        event.preventDefault();
        const rows = Array.from(container.querySelectorAll('tr[data-job-id]'));
        const idx = rows.indexOf(tr);
        if (idx === -1) return;
        const nextIdx = key === 'ArrowDown' ? Math.min(idx + 1, rows.length - 1) : Math.max(idx - 1, 0);
        const nextRow = rows[nextIdx];
        if (nextRow) {
          nextRow.focus();
          nextRow.scrollIntoView({ block: 'nearest' });
        }
        return;
      }
      if (key === 'Enter' || key === ' ') {
        const detailRow = tr.nextElementSibling;
        const details = detailRow?.querySelector?.('details');
        if (details) {
          event.preventDefault();
          details.open = !details.open;
          details.scrollIntoView({ block: 'nearest' });
        }
      }
    });
    tbody.appendChild(tr);

    const detailRow = document.createElement('tr');
    detailRow.className = 'job-detail';
    const detailCell = document.createElement('td');
    detailCell.colSpan = 7;
    const details = document.createElement('details');
    const summary = document.createElement('summary');
    summary.textContent = 'Details';
    details.appendChild(summary);

    const metaList = document.createElement('ul');
    metaList.className = 'metric-list';
    const createdItem = document.createElement('li');
    createdItem.textContent = `Created: ${escapeText(job.created_at || job.created || '—')}`;
    metaList.appendChild(createdItem);
    const categoryItem = document.createElement('li');
    categoryItem.textContent = `Category: ${categoryName || '—'}`;
    metaList.appendChild(categoryItem);
    const personaItem = document.createElement('li');
    personaItem.textContent = `Persona: ${personaId ? personaLabel : '—'}`;
    metaList.appendChild(personaItem);
    const tags = Array.isArray(job?.data?.tags) ? job.data.tags.filter((tag) => typeof tag === 'string' && tag.trim()) : [];
    if (tags.length) {
      const tagsItem = document.createElement('li');
      tagsItem.textContent = 'Tags: ';
      const frag = document.createDocumentFragment();
      tags.forEach((tag, index) => {
        const span = document.createElement('span');
        span.className = 'badge';
        span.textContent = tag;
        frag.appendChild(span);
        if (index < tags.length - 1) {
          frag.appendChild(document.createTextNode(' '));
        }
      });
      tagsItem.appendChild(frag);
      metaList.appendChild(tagsItem);
    }
    const topics = Array.isArray(job?.data?.topics) ? job.data.topics.filter((topic) => typeof topic === 'string' && topic.trim()) : [];
    if (topics.length) {
      const topicsItem = document.createElement('li');
      topicsItem.textContent = `Topics: ${topics.join(', ')}`;
      metaList.appendChild(topicsItem);
    }
    const storyThreads = Array.isArray(job?.data?.story_threads)
      ? job.data.story_threads.filter((thread) => thread && typeof thread === 'object')
      : [];
    if (storyThreads.length) {
      const threadsItem = document.createElement('li');
      threadsItem.className = 'story-thread-summary';
      const heading = document.createElement('span');
      heading.textContent = 'Story threads:';
      threadsItem.appendChild(heading);
      const list = document.createElement('ul');
      list.className = 'compact-list';
      storyThreads.slice(0, 3).forEach((thread) => {
        const entry = document.createElement('li');
        const topic = typeof thread.topic === 'string' && thread.topic.trim()
          ? thread.topic.trim()
          : typeof thread.topic_key === 'string' && thread.topic_key.trim()
            ? thread.topic_key.trim()
            : typeof thread.id === 'string'
              ? thread.id
              : 'thread';
        const summary = typeof thread.summary === 'string' && thread.summary.trim()
          ? thread.summary.trim()
          : '';
        entry.innerHTML = `<span class="mono">${escapeText(topic)}</span>${summary ? ` — ${escapeText(summary)}` : ''}`;
        list.appendChild(entry);
      });
      if (storyThreads.length > 3) {
        const remaining = storyThreads.length - 3;
        const more = document.createElement('li');
        more.className = 'dim';
        more.textContent = `+${remaining} more`; 
        list.appendChild(more);
      }
      threadsItem.appendChild(list);
      metaList.appendChild(threadsItem);
    }
    const stateItem = document.createElement('li');
    const detailMeta = jobStatusMeta(job);
    const detailLabel = escapeText(detailMeta.label || job.state || job.status || 'n/a');
    const detailSlug = escapeText(detailMeta.slug);
    const detailText = detailMeta.slug === 'unknown'
      ? detailLabel
      : `${detailLabel} (${detailSlug})`;
    stateItem.textContent = `State: ${detailText}`;
    metaList.appendChild(stateItem);
    const progressItem = document.createElement('li');
    progressItem.textContent = `Progress: ${progress}`;
    metaList.appendChild(progressItem);
    if (trainingSummary && trainingSummary.text) {
      const hintsItem = document.createElement('li');
      hintsItem.textContent = `Training hints: ${trainingSummary.text}`;
      metaList.appendChild(hintsItem);
    }
    details.appendChild(metaList);

      const logicUnit = resolveLogicUnit(job);
      if (logicUnit) {
        const luBlock = document.createElement('div');
        luBlock.className = 'metric-block';
        const header = document.createElement('p');
      header.className = 'metric-inline';
      header.textContent = `Suggested logic unit: ${logicUnit.id}`;
      luBlock.appendChild(header);
      if (logicUnit.kind) {
        const kindLine = document.createElement('p');
        kindLine.className = 'metric-inline';
        kindLine.textContent = `Kind: ${logicUnit.kind}`;
        luBlock.appendChild(kindLine);
      }
      if (trainingSummary && trainingSummary.text) {
        const hintLine = document.createElement('p');
        hintLine.className = 'metric-inline dim';
        hintLine.textContent = `Training hints: ${trainingSummary.text}`;
        luBlock.appendChild(hintLine);
      }
      if (personaId) {
        const personaLine = document.createElement('p');
        personaLine.className = 'metric-inline';
        personaLine.textContent = `Persona: ${personaLabel}`;
        luBlock.appendChild(personaLine);
      }
      const actions = document.createElement('div');
      actions.className = 'row';
      const applyBtn = document.createElement('button');
      applyBtn.className = 'primary';
      applyBtn.textContent = 'Apply suggestion';
      applyBtn.addEventListener('click', async () => {
        applyBtn.disabled = true;
        try {
          const res = await applyLogicUnit(telemetryBase || getCurrentBase(), logicUnit, { dryRun: false });
          const msg = res && res.ok ? 'Logic unit applied' : 'Apply returned without ok flag';
          ARW.toast(msg);
          appendDiffSummary(details, res, { dryRun: false });
          pushHistory({ type: 'Applied', jobId: job.id || '', logicUnitId: logicUnit.id });
          refreshTelemetry().catch(() => {});
        } catch (err) {
          const msg = err && err.message ? err.message : 'Apply failed';
          appendError(details, msg);
          ARW.toast(msg);
        } finally {
          applyBtn.disabled = false;
        }
      });
      const dryBtn = document.createElement('button');
      dryBtn.className = 'ghost';
      dryBtn.textContent = 'Dry-run';
      dryBtn.addEventListener('click', async () => {
        dryBtn.disabled = true;
        try {
          const res = await applyLogicUnit(telemetryBase || getCurrentBase(), logicUnit, { dryRun: true });
          ARW.toast('Dry-run complete');
          appendDiffSummary(details, res, { dryRun: true });
          pushHistory({ type: 'Dry-run', jobId: job.id || '', logicUnitId: logicUnit.id });
        } catch (err) {
          const msg = err && err.message ? err.message : 'Dry-run failed';
          appendError(details, msg);
          ARW.toast(msg);
        } finally {
          dryBtn.disabled = false;
        }
      });
      const dismissBtn = document.createElement('button');
      dismissBtn.className = 'ghost';
      dismissBtn.textContent = 'Hide job';
      dismissBtn.addEventListener('click', () => {
        dismissedJobs.add(job.id || job.job_id);
        persistDismissed();
        renderJobs(lastJobs);
      });
      actions.appendChild(applyBtn);
      actions.appendChild(dryBtn);
      actions.appendChild(dismissBtn);
      luBlock.appendChild(actions);
      details.appendChild(luBlock);
    }

    if (trainingSummary && trainingSummary.hints) {
      const hintsBlock = document.createElement('div');
      hintsBlock.className = 'metric-block';
      const hintsHeader = document.createElement('p');
      hintsHeader.className = 'metric-inline';
      hintsHeader.textContent = 'Governor hints';
      hintsBlock.appendChild(hintsHeader);
      const reapplyBtn = document.createElement('button');
      reapplyBtn.className = 'ghost';
      reapplyBtn.textContent = 'Apply hints to governor';
      reapplyBtn.addEventListener('click', async () => {
        reapplyBtn.disabled = true;
        try {
          await applyTrainingHints(telemetryBase || getCurrentBase(), trainingSummary.hints, { source: 'manual' });
        } catch (err) {
          const msg = err && err.message ? err.message : 'Hints apply failed';
          ARW.toast(msg);
        } finally {
          reapplyBtn.disabled = false;
        }
      });
      hintsBlock.appendChild(reapplyBtn);
      details.appendChild(hintsBlock);
    }

    if (job.data) {
      const pre = document.createElement('pre');
      pre.textContent = JSON.stringify(job.data, null, 2);
      details.appendChild(pre);
    }
    if (job.manifest) {
      const manifestPre = document.createElement('pre');
      manifestPre.textContent = JSON.stringify(job.manifest, null, 2);
      details.appendChild(manifestPre);
    }
    if (job.manifest_patch) {
      const patchPre = document.createElement('pre');
      patchPre.textContent = JSON.stringify(job.manifest_patch, null, 2);
      details.appendChild(patchPre);
    }

    detailCell.appendChild(details);
    detailRow.appendChild(detailCell);
    tbody.appendChild(detailRow);
  });

  table.appendChild(tbody);
  container.appendChild(table);
}

function renderCompactDiff(container, diffSummary) {
  if (!Array.isArray(diffSummary) || !diffSummary.length) return;
  const wrap = document.createElement('div');
  wrap.className = 'metric-block';
  const title = document.createElement('p');
  title.className = 'metric-inline';
  title.textContent = 'Diff summary';
  wrap.appendChild(title);
  const list = document.createElement('ul');
  list.className = 'metric-list';
  diffSummary.forEach((entry) => {
    const target = entry.target || entry.pointer || 'unknown';
    const beforeExists = entry.before !== undefined;
    const afterExists = entry.after !== undefined;
    let tone = '';
    if (!beforeExists && afterExists) tone = 'ok';
    else if (beforeExists && !afterExists) tone = 'bad';
    const before = summariseValue(entry.before);
    const after = summariseValue(entry.after);
    const labelBefore = beforeExists ? before : '∅';
    const labelAfter = afterExists ? after : '∅';
    const li = document.createElement('li');
    li.className = tone ? tone : '';
    li.textContent = `${target}: ${labelBefore} → ${labelAfter}`;
    if (tone === 'bad') {
      li.textContent += ' (removed)';
    } else if (tone === 'ok' && !beforeExists) {
      li.textContent += ' (added)';
    }
    list.appendChild(li);
  });
  wrap.appendChild(list);
  container.appendChild(wrap);
}

function summariseValue(value) {
  if (value == null) return 'null';
  if (typeof value === 'string') {
    return value.length > 32 ? `${value.slice(0, 32)}…` : value;
  }
  if (typeof value === 'number' || typeof value === 'boolean') {
    return String(value);
  }
  if (Array.isArray(value)) {
    return `[${value.length} items]`;
  }
  if (typeof value === 'object') {
    const keys = Object.keys(value);
    return `{${keys.slice(0, 3).join(', ')}${keys.length > 3 ? ', …' : ''}}`;
  }
  return String(value);
}

function appendDiffSummary(container, response, { dryRun }) {
  if (!response) return;
  const wrap = document.createElement('div');
  wrap.className = 'metric-block';
  const title = document.createElement('p');
  title.className = dryRun ? 'metric-inline dim' : 'metric-inline ok';
  const typeLabel = dryRun ? 'Dry-run diff' : 'Applied diff';
  title.textContent = `${typeLabel} (${new Date().toLocaleTimeString([], { hour: '2-digit', minute: '2-digit', second: '2-digit' })})`;
  wrap.appendChild(title);
  if (Array.isArray(response.diff_summary) && response.diff_summary.length) {
    const pre = document.createElement('pre');
    pre.textContent = JSON.stringify(response.diff_summary, null, 2);
    wrap.appendChild(pre);
    renderCompactDiff(container, response.diff_summary);
  } else {
    const p = document.createElement('p');
    p.className = 'dim';
    p.textContent = 'No diff summary returned.';
    wrap.appendChild(p);
  }
  if (Array.isArray(response.safety_issues) && response.safety_issues.length) {
    const issues = document.createElement('ul');
    issues.className = 'metric-list';
    response.safety_issues.forEach((issue) => {
      const li = document.createElement('li');
      li.className = 'bad';
      li.textContent = typeof issue === 'string' ? issue : JSON.stringify(issue);
      issues.appendChild(li);
    });
    wrap.appendChild(issues);
  }
  container.appendChild(wrap);
}

function appendError(container, message) {
  const wrap = document.createElement('div');
  wrap.className = 'metric-block';
  const title = document.createElement('p');
  title.className = 'metric-inline bad';
  title.textContent = `Error: ${message}`;
  wrap.appendChild(title);
  const actions = document.createElement('div');
  actions.className = 'row';
  const copyBtn = document.createElement('button');
  copyBtn.className = 'ghost';
  copyBtn.textContent = 'Copy error';
  copyBtn.addEventListener('click', () => {
    try {
      navigator.clipboard.writeText(message);
      ARW.toast('Error copied');
    } catch {
      ARW.toast('Copy failed');
    }
  });
  actions.appendChild(copyBtn);
  wrap.appendChild(actions);
  container.appendChild(wrap);
}

function persistDismissed() {
  try {
    const ids = Array.from(dismissedJobs);
    const prefs = { ...(ARW._prefsCache.get('launcher') || {}) };
    prefs.dismissedJobs = ids;
    prefs.logicUnitHistory = logicUnitHistory;
    prefs.governorHistory = governorHistory.map((entry) => entry.hints);
    void ARW.setPrefs('launcher', prefs).catch((err) => {
      console.error('persist dismissed jobs failed', err);
    });
  } catch {}
}

async function loadDismissedPrefs() {
  try {
    const prefs = (await ARW.getPrefs('launcher')) || {};
    const ids = Array.isArray(prefs.dismissedJobs) ? prefs.dismissedJobs : [];
    ids.forEach((id) => dismissedJobs.add(String(id)));
    if (Array.isArray(prefs.logicUnitHistory)) {
      logicUnitHistory.length = 0;
      logicUnitHistoryRaw = [];
      prefs.logicUnitHistory.forEach((entry) => {
        if (!entry || !entry.ts) return;
        const normalized = {
          ts: entry.ts,
          type: entry.type || entry.rawKind || 'logic.unit',
          jobId: entry.jobId ? String(entry.jobId) : '',
          logicUnitId: entry.logicUnitId ? String(entry.logicUnitId) : '',
          via: entry.via || 'local',
          rawKind: entry.rawKind || entry.kind || entry.type || 'logic.unit',
        };
        logicUnitHistory.push(normalized);
        logicUnitHistoryRaw.push({
          time: normalized.ts,
          kind: normalized.rawKind,
          payload: {
            job_id: normalized.jobId,
            logic_unit_id: normalized.logicUnitId,
            via: normalized.via,
          },
        });
      });
      if (logicUnitHistoryRaw.length > 200) logicUnitHistoryRaw.length = 200;
      historyHydrated = logicUnitHistoryRaw.length > 0;
      lastFetchedHistorySignature = signatureFromEntries(logicUnitHistoryRaw);
      renderHistory();
    }
    const savedGovernor = Array.isArray(prefs.governorHistory) ? prefs.governorHistory : [];
    governorHistory = savedGovernor
      .map((item) => normalizeGovernorHints(item))
      .filter(Boolean)
      .map((hints) => ({
        key: governorHistoryKey(hints),
        hints,
        summary: formatGovernorHints(hints),
      }));
  } catch {}
  renderGovernorHistory();
}

function findJobElement(jobId) {
  const rows = document.querySelectorAll('#results tr');
  for (const row of rows) {
    if (row.dataset.jobId === jobId) {
      return row;
    }
  }
  return null;
}

function pushHistory(entry) {
  const now = new Date().toISOString();
  const rawKind = entry.rawKind || entry.kind || (entry.type ? `logic.unit.${entry.type.toLowerCase()}` : 'logic.unit.local');
  const via = entry.via || 'local';
  const jobId = entry.jobId ? String(entry.jobId) : '';
  const logicUnitId = entry.logicUnitId ? String(entry.logicUnitId) : '';
  const relatedJob = (lastJobs || []).find((item) => String(item.id || item.job_id || '') === jobId);
  const personaForHistory = relatedJob ? resolveJobPersona(relatedJob) : null;
  const historyEntry = {
    ...entry,
    ts: entry.ts || now,
    jobId,
    logicUnitId,
    via,
    rawKind,
    personaId: personaForHistory || entry.personaId || null,
  };
  logicUnitHistory.unshift(historyEntry);
  if (logicUnitHistory.length > 10) logicUnitHistory.length = 10;
  const payload = entry.payload && typeof entry.payload === 'object' ? { ...entry.payload } : {};
  if (personaForHistory && !payload.persona_id) {
    payload.persona_id = personaForHistory;
  }
  logicUnitHistoryRaw.unshift({
    time: historyEntry.ts,
    kind: rawKind,
    payload: {
      ...payload,
      job_id: jobId,
      logic_unit_id: logicUnitId,
      via,
    },
  });
  if (logicUnitHistoryRaw.length > 200) logicUnitHistoryRaw.length = 200;
  persistHistory();
  renderHistory();
}

function exportHistory() {
  if (logicUnitHistoryRaw.length) {
    return logicUnitHistoryRaw.map((entry) => ({ ...entry }));
  }
  return logicUnitHistory.map((item) => ({
    time: item.ts,
    kind: item.rawKind || item.type || 'logic.unit',
    payload: {
      job_id: item.jobId,
      logic_unit_id: item.logicUnitId,
      via: item.via || 'local',
    },
  }));
}

function persistHistory() {
  try {
    const prefs = { ...(ARW._prefsCache.get('launcher') || {}) };
    prefs.logicUnitHistory = logicUnitHistory;
    prefs.governorHistory = governorHistory.map((entry) => entry.hints);
    void ARW.setPrefs('launcher', prefs).catch((err) => {
      console.error('persist training history failed', err);
    });
  } catch {}
}

function renderHistory() {
  const container = document.getElementById('logicUnitHistory');
  if (!container) return;
  container.innerHTML = '';
  const title = document.createElement('h4');
  title.textContent = 'Recent logic unit actions';
  container.appendChild(title);
  if (!logicUnitHistory.length) {
    const empty = document.createElement('p');
    empty.className = 'dim';
    empty.textContent = 'No actions yet.';
    container.appendChild(empty);
    return;
  }
  const list = document.createElement('ul');
  list.className = 'metric-list';
  logicUnitHistory.forEach((item) => {
    const li = document.createElement('li');
    const time = new Date(item.ts).toLocaleTimeString([], { hour: '2-digit', minute: '2-digit', second: '2-digit' });
    li.textContent = `${time} · ${item.type} · job ${item.jobId || 'n/a'} (${item.logicUnitId || 'unknown'})`;
    const badge = document.createElement('button');
    badge.className = 'ghost';
    badge.type = 'button';
    badge.textContent = 'Focus job';
    badge.addEventListener('click', () => {
      if (!item.jobId) return;
      const row = findJobElement(String(item.jobId));
      if (row) {
        row.focus();
        row.scrollIntoView({ block: 'nearest' });
      }
    });
    li.appendChild(document.createTextNode(' '));
    li.appendChild(badge);
    list.appendChild(li);
  });
  container.appendChild(list);
}

function signatureFromEntries(entries) {
  if (!Array.isArray(entries) || !entries.length) return '';
  const first = entries[0] || {};
  const time = String(first.time || first.ts || '');
  const kind = String(first.kind || first.rawKind || first.type || '');
  const payload = first.payload || {};
  const id = payload.id || payload.logic_unit_id || payload.logicUnitId || payload.job_id || '';
  return `${time}|${kind}|${id}`;
}

function adoptServerHistory(entries, options = {}) {
  const arr = Array.isArray(entries) ? entries : [];
  const { persistRaw = false, signature } = options;
  if (persistRaw) {
    logicUnitHistoryRaw = arr.slice(0, 500);
  }
  const mapped = arr
    .map((entry) => {
      if (!entry || typeof entry !== 'object') return null;
      const rawKind = String(entry.kind || '');
      const normalizedKind = rawKind.toLowerCase();
      const payload = entry.payload || {};
      const typeMap = {
        'logic.unit.suggested': 'Suggested',
        'logic.unit.applied': 'Applied',
        'logic.unit.installed': 'Installed',
        'logic.unit.reverted': 'Reverted',
      };
      const type = typeMap[normalizedKind] || rawKind || 'logic.unit';
      const jobCandidate = payload.job_id || payload.jobId || payload.goal || payload.snapshot_id || '';
      const luCandidate = payload.id || payload.logic_unit_id || payload.logicUnitId || '';
      return {
        ts: entry.time || new Date().toISOString(),
        type,
        jobId: jobCandidate ? String(jobCandidate) : '',
        logicUnitId: luCandidate ? String(luCandidate) : '',
        via: entry.via || entry.source || 'server',
        rawKind: rawKind || type,
      };
    })
    .filter(Boolean);

  mapped.sort((a, b) => (a.ts < b.ts ? 1 : -1));
  logicUnitHistory.length = 0;
  mapped.slice(0, 10).forEach((item) => logicUnitHistory.push(item));
  persistHistory();
  renderHistory();
  if (typeof signature === 'string') {
    lastFetchedHistorySignature = signature;
  } else if (persistRaw) {
    lastFetchedHistorySignature = signatureFromEntries(logicUnitHistoryRaw);
  }
}

async function refreshTelemetry(options = {}) {
  if (telemetryEventTimer) {
    clearTimeout(telemetryEventTimer);
    telemetryEventTimer = null;
  }
  const manual = Boolean(options.manual);
  const quiet = Boolean(options.quiet);
  const base = telemetryBase || getCurrentBase();
  telemetryBase = base;
  if (!manual && !quiet) {
    setTelemetryStatus('Loading…');
  }
  try {
    const snapshot = await fetchTrainingTelemetry(base);
    if (snapshot && typeof snapshot === 'object' && snapshot.status >= 400) {
      const title = snapshot.title || `HTTP ${snapshot.status}`;
      throw new Error(title);
    }
    let telemetrySignature = '';
    if (snapshot && Array.isArray(snapshot.logic_history)) {
      telemetrySignature = signatureFromEntries(snapshot.logic_history);
      adoptServerHistory(snapshot.logic_history);
    } else {
      adoptServerHistory([]);
    }
    if (
      !historyHydrated ||
      (telemetrySignature && telemetrySignature !== lastFetchedHistorySignature)
    ) {
      refreshLogicHistory().catch(() => {});
    }
    renderTelemetry(snapshot);
    scheduleTelemetryRefresh();
  } catch (err) {
    const message = err && err.message ? err.message : 'Unknown error';
    showTelemetryBody(false);
    setTelemetryStatus(`Failed to load telemetry (${message})`, 'bad');
    scheduleTelemetryRefresh(manual ? TELEMETRY_POLL_MS : TELEMETRY_POLL_MS * 1.5);
    if (manual) ARW.toast('Telemetry refresh failed');
  }
}

async function refreshLogicHistory(options = {}) {
  if (pendingHistoryFetch) return pendingHistoryFetch;
  const { limit = 200 } = options;
  const base = telemetryBase || getCurrentBase();
  pendingHistoryFetch = (async () => {
    try {
      const snapshot = await fetchLogicUnitHistorySnapshot(base, { limit, offset: 0 });
      if (snapshot && Array.isArray(snapshot.items)) {
        const items = snapshot.items.slice(0, limit);
        historyHydrated = true;
        adoptServerHistory(items, {
          persistRaw: true,
          signature: signatureFromEntries(items),
        });
      }
    } catch (err) {
      console.debug('logic history fetch failed', err);
    } finally {
      pendingHistoryFetch = null;
    }
  })();
  return pendingHistoryFetch;
}

async function refreshJobs(options = {}) {
  const manual = Boolean(options.manual);
  const base = telemetryBase || getCurrentBase();
  if (!manual) {
    setJobsStatus('Loading…');
  }
  try {
    await fetchLogicUnits(base);
    const jobs = await fetchOrchestratorJobs(base);
    renderJobs(jobs);
    const updated = new Date();
    setJobsStatus(`Updated ${updated.toLocaleTimeString([], { hour: '2-digit', minute: '2-digit', second: '2-digit' })}`);
    scheduleJobsRefresh();
  } catch (err) {
    const message = err && err.message ? err.message : 'Unknown error';
    setJobsStatus(`Failed to load jobs (${message})`, 'bad');
    renderJobs([]);
    scheduleJobsRefresh(manual ? JOBS_POLL_MS : JOBS_POLL_MS * 1.5);
    if (manual) ARW.toast('Jobs refresh failed');
  }
}

document.addEventListener('visibilitychange', () => {
  if (document.hidden) {
    clearTelemetryTimer();
    clearJobsTimer();
  } else {
    scheduleTelemetryRefresh(500);
    scheduleJobsRefresh(500);
  }
});

document.addEventListener('DOMContentLoaded', async () => {
  await ARW.applyPortFromPrefs('port');
  const portInput = document.getElementById('port');
  baseMeta = updateBaseMeta();
  telemetryBase = baseMeta.base || getCurrentBase();
  const base = telemetryBase;
  const personaOptions = {
    root: document.getElementById('trainingPersonaPanel'),
    select: document.getElementById('trainingPersonaSelect'),
    refresh: document.getElementById('trainingPersonaRefresh'),
    status: document.getElementById('trainingPersonaStatus'),
    scope: document.getElementById('trainingPersonaScope'),
    enable: document.getElementById('trainingPersonaTelemetryEnable'),
    save: document.getElementById('trainingPersonaTelemetrySave'),
    applyAll: document.getElementById('trainingPersonaTelemetryApplyAll'),
    empty: document.getElementById('trainingPersonaEmpty'),
    metrics: document.getElementById('trainingPersonaMetrics'),
    history: document.getElementById('trainingPersonaHistory'),
    historyMeta: document.getElementById('trainingPersonaHistoryMeta'),
    historyLimit: 6,
    getBase: () => telemetryBase || getCurrentBase(),
  };
  if (ARW.personaPanel && personaOptions.root) {
    personaPanelInstance = ARW.personaPanel.attach(personaOptions);
    if (personaPanelInstance && personaPanelInstance.disabled) {
      personaPanelInstance = null;
    }
    try {
      await personaPanelInstance?.init?.();
    } catch (err) {
      console.warn('persona panel init failed', err);
    }
  }
  const prefs = (await ARW.getPrefs('launcher')) || {};
  const guideCard = document.querySelector('.training-guide');
  if (guideCard && prefs.hideTrainingGuide) {
    guideCard.setAttribute('hidden', 'true');
  }
  updateModeUi(window.ARW?.mode?.current || activeMode, { force: true });
  if (ARW.mode && typeof ARW.mode.subscribe === 'function') {
    ARW.mode.subscribe((modeValue) => {
      updateModeUi(modeValue, { force: true });
    });
  }
  const applyBaseChange = async () => {
    baseMeta = updateBaseMeta();
    const p = ARW.getPortFromInput('port') || baseMeta.port || 8091;
    try {
      const prefs = (await ARW.getPrefs('launcher')) || {};
      if (prefs.port !== p) {
        prefs.port = p;
        await ARW.setPrefs('launcher', prefs);
      }
    } catch {}
    telemetryBase = ARW.base(p);
    mountTrainingSidecar({ force: true });
    ARW.sse.connect(telemetryBase, { replay: 5 });
    await Promise.allSettled([
      refreshTelemetry(),
      refreshJobs(),
      refreshCascadeSummaries({ project: cascadeTargetProject, quiet: true }),
      personaPanelInstance && !personaPanelInstance.disabled
        ? personaPanelInstance.reload({ preserveSelection: true })
        : Promise.resolve(),
    ]);
  };
  ARW.sse.indicator('sseStat', { prefix: 'SSE' });
  const sseFilters = ['state.', 'models.', 'logic.unit.', 'config.patch.', 'context.cascade.'];
  ARW.sse.connect(base, { replay: 10, prefix: sseFilters });
  const guideBtn = document.getElementById('trainingGuideDocs');
  if (guideBtn) {
    guideBtn.addEventListener('click', async () => {
      try {
        await ARW.invoke('open_url', { url: 'https://t3hw00t.github.io/ARW/guide/training_park/' });
      } catch (err) {
        console.error('open training guide failed', err);
        ARW.toast('Unable to open guide');
      }
    });
  }
  const dismissGuideBtn = document.getElementById('trainingGuideDismiss');
  if (dismissGuideBtn) {
    dismissGuideBtn.addEventListener('click', async () => {
      const card = document.querySelector('.training-guide');
      if (card) card.setAttribute('hidden', 'true');
      try {
        const next = (await ARW.getPrefs('launcher')) || {};
        if (!next.hideTrainingGuide) {
          next.hideTrainingGuide = true;
          await ARW.setPrefs('launcher', next);
        }
      } catch (err) {
        console.error('store training guide pref failed', err);
      }
      ARW.toast('Training quick start hidden (Command Palette → Show Training Quick Start to restore).');
    });
  }

  const logicUnitEvents = ([kind, env]) => {
    const match = kind.startsWith('logic.unit.') || kind.startsWith('config.patch.');
    if (match && kind === 'logic.unit.suggested') {
      const id = env?.payload?.id || env?.id;
      const goal = env?.payload?.job_id || env?.job_id || '';
      const label = id ? `New logic unit ${id}` : 'New logic unit suggested';
      ARW.toast(goal ? `${label} (job ${goal})` : label);
      pushHistory({ type: 'Suggested', jobId: goal, logicUnitId: id || '', via: 'event' });
    }
    return match;
  };
  const logicUnitSub = ARW.sse.subscribe(logicUnitEvents, () => {
    refreshJobs().catch(() => {});
    queueTelemetryRefresh({ delay: 250, quiet: true });
  });

  const contextEvents = (kind) => kind === 'context.coverage' || kind === 'context.recall.risk';
  const contextSub = ARW.sse.subscribe(contextEvents, () => {
    queueTelemetryRefresh({ delay: 200, quiet: true });
  });

  const cascadeSub = ARW.sse.subscribe(
    (kind) => kind === 'context.cascade.updated',
    () => refreshCascadeSummaries({ quiet: true })
  );

  const governorHintSub = ARW.sse.subscribe('actions.hint.applied', ({ env }) => {
    const payload = env?.payload || env;
    if (!payload || payload.action !== 'governor.hints') return;
    const params = payload.params && typeof payload.params === 'object' ? payload.params : null;
    if (!params) return;
    const normalized = normalizeGovernorHints(params);
    if (!normalized) return;
    const recorded = recordGovernorProfile(normalized, { silent: true });
    const summary = recorded?.summary || formatGovernorHints(normalized);
    const key = recorded?.key || governorHistoryKey(normalized);
    maybeToastGovernorSummary(summary, key, { source: payload.source || 'server' });
  });

  const diversitySlider = document.getElementById('diversitySlider');
  const diversityValue = document.getElementById('diversityValue');
  const recencySlider = document.getElementById('recencySlider');
  const recencyValue = document.getElementById('recencyValue');
  const compressionSlider = document.getElementById('compressionSlider');
  const compressionValue = document.getElementById('compressionValue');
  const presetSelect = document.getElementById('presetSelect');
  const refreshJobsButton = document.getElementById('refreshJobs');
  const runButton = document.getElementById('abRun');
  const statusFilterSelect = document.getElementById('jobStatusFilter');
  const resetDismissedButton = document.getElementById('resetDismissedJobs');
  const exportButton = document.getElementById('exportHistory');
  const clearGovernorHistoryButton = document.getElementById('clearGovernorHistory');

  loadDismissedPrefs();

  const updateSliderLabel = (slider, label) => {
    if (!slider || !label) return;
    const value = Number.parseFloat(slider.value || '0');
    label.textContent = value.toFixed(2);
    slider.setAttribute('aria-valuenow', value.toFixed(2));
    const percent = Math.round(value * 100);
    slider.setAttribute('aria-valuetext', `${percent}%`);
  };

  if (diversitySlider && diversityValue) {
    diversitySlider.addEventListener('input', () => updateSliderLabel(diversitySlider, diversityValue));
    updateSliderLabel(diversitySlider, diversityValue);
  }
  if (recencySlider && recencyValue) {
    recencySlider.addEventListener('input', () => updateSliderLabel(recencySlider, recencyValue));
    updateSliderLabel(recencySlider, recencyValue);
  }
  if (compressionSlider && compressionValue) {
    compressionSlider.addEventListener('input', () => updateSliderLabel(compressionSlider, compressionValue));
    updateSliderLabel(compressionSlider, compressionValue);
  }

  if (refreshJobsButton) {
    refreshJobsButton.addEventListener('click', () => {
      refreshJobs({ manual: true }).catch(() => {});
    });
  }

  if (statusFilterSelect) {
    statusFilterSelect.addEventListener('change', () => {
      jobStatusFilter = String(statusFilterSelect.value || 'all').toLowerCase();
      renderJobs(lastJobs || []);
    });
  }

  if (resetDismissedButton) {
    resetDismissedButton.addEventListener('click', () => {
      dismissedJobs.clear();
      persistDismissed();
      renderJobs(lastJobs || []);
    });
  }

  if (exportButton) {
    exportButton.addEventListener('click', async () => {
      try {
        const base = telemetryBase || getCurrentBase();
        let snapshot;
        try {
          snapshot = await fetchLogicUnitHistorySnapshot(base, { limit: 500, offset: 0 });
        } catch (err) {
          console.debug('history export fetch failed', err);
        }
        let items = [];
        if (snapshot && Array.isArray(snapshot.items) && snapshot.items.length) {
          items = snapshot.items;
          adoptServerHistory(items, {
            persistRaw: true,
            signature: signatureFromEntries(items),
          });
        } else {
          items = exportHistory();
        }
        const payload = {
          generated: new Date().toISOString(),
          total: snapshot && typeof snapshot.total === 'number' ? snapshot.total : items.length,
          items,
        };
        const blob = new Blob([JSON.stringify(payload, null, 2)], {
          type: 'application/json',
        });
        const url = URL.createObjectURL(blob);
        const link = document.createElement('a');
        link.href = url;
        link.download = `logic_unit_history_${Date.now()}.json`;
        document.body.appendChild(link);
        link.click();
        document.body.removeChild(link);
        setTimeout(() => URL.revokeObjectURL(url), 1000);
        ARW.toast('History exported');
      } catch (err) {
        ARW.toast(err && err.message ? err.message : 'Export failed');
      }
    });
  }

  if (clearGovernorHistoryButton) {
    clearGovernorHistoryButton.addEventListener('click', () => {
      clearGovernorHistory();
    });
  }

  window.addEventListener('keydown', async (event) => {
    if (!event.shiftKey) return;
    const key = event.key.toLowerCase();
    if (key !== 'a' && key !== 'd') return;
    const focusedRow = document.activeElement?.closest?.('tr[data-job-id]');
    if (!focusedRow) return;
    const jobId = focusedRow.dataset.jobId;
    if (!jobId) return;
    const job = (lastJobs || []).find((item) => String(item.id || '') === jobId);
    if (!job) return;
    const logicUnit = resolveLogicUnit(job);
    if (!logicUnit) {
      ARW.toast('No logic unit for this job');
      return;
    }
    event.preventDefault();
    const details = focusedRow.nextElementSibling?.querySelector?.('details');
    if (details && !details.open) details.open = true;
    if (key === 'a') {
      try {
        const res = await applyLogicUnit(telemetryBase || getCurrentBase(), logicUnit, { dryRun: false });
        ARW.toast('Applied via hotkey');
        if (details) appendDiffSummary(details, res, { dryRun: false });
        refreshTelemetry().catch(() => {});
      } catch (err) {
        const msg = err && err.message ? err.message : 'Apply failed';
        if (details) appendError(details, msg);
        ARW.toast(msg);
      }
    } else if (key === 'd') {
      try {
        const res = await applyLogicUnit(telemetryBase || getCurrentBase(), logicUnit, { dryRun: true });
        ARW.toast('Dry-run via hotkey');
        if (details) appendDiffSummary(details, res, { dryRun: true });
        pushHistory({ type: 'Dry-run', jobId, logicUnitId: logicUnit.id, via: 'hotkey' });
      } catch (err) {
        const msg = err && err.message ? err.message : 'Dry-run failed';
        if (details) appendError(details, msg);
        ARW.toast(msg);
      }
    }
  });

  if (runButton) {
    runButton.addEventListener('click', async () => {
      try {
        const preset = presetSelect ? String(presetSelect.value || 'balanced') : 'balanced';
        const diversity = diversitySlider ? parseFloat(diversitySlider.value || '0') : 0;
        const recency = recencySlider ? parseFloat(recencySlider.value || '0') : 0;
        const compression = compressionSlider ? parseFloat(compressionSlider.value || '0') : 0;
        const personaId = personaPanelInstance
          ? (typeof personaPanelInstance.currentPersonaId === 'function'
              ? personaPanelInstance.currentPersonaId()
              : personaPanelInstance.selectedId || null)
          : null;
        setJobsStatus('Submitting job…');
        await startTrainingJob(telemetryBase || getCurrentBase(), preset, diversity, recency, compression, personaId);
        ARW.toast('Training job submitted');
        refreshJobs().catch(() => {});
        if (!personaId && personaPanelInstance && !personaPanelInstance.disabled) {
          setJobsStatus('Submitted without persona tag (no persona selected)', 'dim', { clearAfter: 5000 });
        }
      } catch (err) {
        const message = err && err.message ? err.message : 'Start failed';
        ARW.toast(message);
        setJobsStatus(`Failed to submit job (${message})`, 'bad');
      }
    });
  }

  if (portInput) {
    portInput.addEventListener('change', () => {
      applyBaseChange().catch(() => {});
    });
  }

  const refreshBtn = document.getElementById('refreshTelemetry');
  if (refreshBtn) {
    refreshBtn.addEventListener('click', () => {
      refreshTelemetry({ manual: true }).catch(() => {});
    });
  }

  ARW.palette.mount({ base });
  refreshTelemetry().catch(() => {});
  refreshJobs().catch(() => {});
  window.addEventListener('beforeunload', () => {
    ARW.sse.unsubscribe(logicUnitSub);
    ARW.sse.unsubscribe(contextSub);
    ARW.sse.unsubscribe(governorHintSub);
    personaPanelInstance?.dispose?.();
    personaPanelInstance = null;
  });
  window.addEventListener('arw:base-override-changed', () => {
    applyBaseChange().catch(() => {});
  });
});
