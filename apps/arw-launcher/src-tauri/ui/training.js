const TELEMETRY_POLL_MS = 10_000;
const JOBS_POLL_MS = 12_000;
let telemetryTimer = null;
let jobsTimer = null;
let telemetryBase = null;
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

function getCurrentBase() {
  const port = ARW.getPortFromInput('port') || 8091;
  return ARW.base(port);
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

async function startTrainingJob(baseUrl, preset, diversity, compression) {
  const token = await ARW.connections.tokenFor(baseUrl);
  const goal = `Training preset=${preset} diversity=${diversity.toFixed(2)} compression=${compression.toFixed(2)}`;
  const body = {
    goal,
    data: {
      preset,
      diversity,
      compression,
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

function setJobsStatus(message, tone = 'dim') {
  const node = document.getElementById('jobsStatus');
  if (!node) return;
  const cls = tone === 'bad' ? 'bad mono' : tone === 'ok' ? 'ok mono' : 'dim mono';
  node.className = cls;
  node.textContent = message;
}

function escapeText(value) {
  return String(value ?? '').replace(/[\u0000-\u001f\u007f-\u009f]/g, '');
}

function formatPercent(value, digits = 0) {
  if (typeof value !== 'number' || !Number.isFinite(value)) return '—';
  const pct = (value * 100).toFixed(digits);
  return `${pct}%`;
}

function formatSlotName(slot) {
  return escapeText((slot || '').replace(/[_-]/g, ' '));
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

function renderTelemetry(data) {
  lastTelemetry = data;
  const updated = data && data.generated ? new Date(data.generated) : new Date();
  const readable = Number.isNaN(updated.getTime())
    ? escapeText(data && data.generated)
    : updated.toLocaleTimeString([], { hour: '2-digit', minute: '2-digit', second: '2-digit' });
  setTelemetryStatus(`Updated ${readable}`);

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
  const ratio = typeof coverage.needs_more_ratio === 'number' ? coverage.needs_more_ratio : null;
  const ratioEl = document.getElementById('coverageRatio');
  if (ratioEl) ratioEl.textContent = ratio == null ? '—' : formatPercent(ratio, 0);

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

  const avgScore = recall.avg_score;
  const avgScoreEl = document.getElementById('recallAvgScore');
  if (avgScoreEl) avgScoreEl.textContent = avgScore == null ? '—' : formatPercent(avgScore, 0);
  const atRiskRatio = recall.at_risk_ratio;
  const atRiskEl = document.getElementById('recallAtRisk');
  if (atRiskEl) atRiskEl.textContent = atRiskRatio == null ? '—' : formatPercent(atRiskRatio, 0);

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
    const status = (job.status || job.state || '').toLowerCase();
    if (jobStatusFilter === 'running') {
      return status === 'running' || status === 'in_progress';
    }
    if (jobStatusFilter === 'completed') {
      return status === 'complete' || status === 'completed' || status === 'finished';
    }
    if (jobStatusFilter === 'queued') {
      return status === 'queued' || status === 'pending';
    }
    if (jobStatusFilter === 'failed') {
      return status === 'failed' || status === 'error';
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
  table.innerHTML = '<thead><tr><th>Job</th><th>Status</th><th>Progress</th><th>Goal</th><th>Updated</th></tr></thead>';
  const tbody = document.createElement('tbody');

  filtered.forEach((job) => {
    const tr = document.createElement('tr');
    const progress = typeof job.progress === 'number' ? formatPercent(job.progress, 0) : '—';
    const status = job.status || job.state || 'unknown';
    const statusClass = status && status.toLowerCase().includes('fail')
      ? 'bad'
      : status && status.toLowerCase().includes('run')
        ? 'accent'
        : '';
    const updated = job.updated_at || job.updated || job.finished_at || job.created_at || job.created || '';
    let updatedLabel = '—';
    if (updated) {
      const parsed = new Date(updated);
      updatedLabel = Number.isNaN(parsed.getTime())
        ? escapeText(String(updated))
        : parsed.toLocaleTimeString([], { hour: '2-digit', minute: '2-digit', second: '2-digit' });
    }
    tr.innerHTML = `
      <td class="mono">${escapeText(job.id || '')}</td>
      <td class="${statusClass}">${escapeText(status)}</td>
      <td>${progress}</td>
      <td>${escapeText(job.goal || '')}</td>
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
    detailCell.colSpan = 5;
    const details = document.createElement('details');
    const summary = document.createElement('summary');
    summary.textContent = 'Details';
    details.appendChild(summary);

    const metaList = document.createElement('ul');
    metaList.className = 'metric-list';
    const createdItem = document.createElement('li');
    createdItem.textContent = `Created: ${escapeText(job.created_at || job.created || '—')}`;
    metaList.appendChild(createdItem);
    const stateItem = document.createElement('li');
    stateItem.textContent = `State: ${escapeText(job.state || 'n/a')}`;
    metaList.appendChild(stateItem);
    const progressItem = document.createElement('li');
    progressItem.textContent = `Progress: ${progress}`;
    metaList.appendChild(progressItem);
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
    ARW.setPrefs('launcher', prefs);
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
  } catch {}
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
  const historyEntry = {
    ...entry,
    ts: entry.ts || now,
    jobId,
    logicUnitId,
    via,
    rawKind,
  };
  logicUnitHistory.unshift(historyEntry);
  if (logicUnitHistory.length > 10) logicUnitHistory.length = 10;
  const payload = entry.payload && typeof entry.payload === 'object' ? entry.payload : {};
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
    ARW.setPrefs('launcher', prefs);
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
  const manual = Boolean(options.manual);
  const base = telemetryBase || getCurrentBase();
  telemetryBase = base;
  if (!manual) {
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
  telemetryBase = getCurrentBase();
  const base = telemetryBase;
  ARW.sidecar.mount('sidecar', ['timeline','context','policy','metrics','models'], { base });
  ARW.sse.indicator('sseStat', { prefix: 'SSE' });
  const sseFilters = ['state.', 'models.', 'logic.unit.', 'config.patch.'];
  ARW.sse.connect(base, { replay: 10, prefix: sseFilters });

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
    refreshTelemetry().catch(() => {});
  });

  const diversitySlider = document.getElementById('diversitySlider');
  const diversityValue = document.getElementById('diversityValue');
  const compressionSlider = document.getElementById('compressionSlider');
  const compressionValue = document.getElementById('compressionValue');
  const presetSelect = document.getElementById('presetSelect');
  const refreshJobsButton = document.getElementById('refreshJobs');
  const runButton = document.getElementById('abRun');
  const statusFilterSelect = document.getElementById('jobStatusFilter');
  const resetDismissedButton = document.getElementById('resetDismissedJobs');
  const exportButton = document.getElementById('exportHistory');

  loadDismissedPrefs();

  const updateSliderLabel = (slider, label) => {
    if (!slider || !label) return;
    const value = Number.parseFloat(slider.value || '0');
    label.textContent = value.toFixed(2);
  };

  if (diversitySlider && diversityValue) {
    diversitySlider.addEventListener('input', () => updateSliderLabel(diversitySlider, diversityValue));
    updateSliderLabel(diversitySlider, diversityValue);
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
        const compression = compressionSlider ? parseFloat(compressionSlider.value || '0') : 0;
        setJobsStatus('Submitting job…');
        await startTrainingJob(telemetryBase || getCurrentBase(), preset, diversity, compression);
        ARW.toast('Training job submitted');
        refreshJobs().catch(() => {});
      } catch (err) {
        const message = err && err.message ? err.message : 'Start failed';
        ARW.toast(message);
        setJobsStatus(`Failed to submit job (${message})`, 'bad');
      }
    });
  }

  if (portInput) {
    portInput.addEventListener('change', async () => {
      const p = ARW.getPortFromInput('port') || 8091;
      await ARW.setPrefs('launcher', { ...(await ARW.getPrefs('launcher')), port: p });
      telemetryBase = ARW.base(p);
      ARW.sse.connect(telemetryBase, { replay: 5 });
      refreshTelemetry().catch(() => {});
      refreshJobs().catch(() => {});
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
  });
});
