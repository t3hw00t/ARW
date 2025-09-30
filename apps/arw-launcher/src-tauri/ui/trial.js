(function(){
  const STORAGE_KEY = 'arw:trial:last-preflight';
  const STATUS_LABELS = { ok: 'All good', warn: 'Check soon', bad: 'Action needed', unknown: 'Unknown' };
  const AUTONOMY_OPERATOR_KEY = 'arw:trial:autonomy-operator';

  const STATE = {
    systems: { level: 'unknown', summary: 'Loading...', meta: [] },
    memory: { level: 'unknown', summary: 'Loading...', meta: [] },
    approvals: { level: 'unknown', summary: 'Loading...', meta: [] },
    safety: { level: 'unknown', summary: 'Loading...', meta: [] },
    autonomy: { level: 'unknown', summary: 'Loading...', meta: [], lane: null, snapshot: null, line: 'Autonomy status loading...', operator: null, alerts: [], updatedMs: null, lastEvent: null, lastReason: null },
    overview: [],
    workflows: [],
    safeguards: [],
    focus: [],
    focusUpdatedMs: null,
    errors: [],
    unauthorized: false,
    base: null,
  };

  document.addEventListener('DOMContentLoaded', init); // eslint-disable-line no-undef

  async function init(){
    try{ await ARW.applyPortFromPrefs('port'); }catch{}
    loadStoredPreflight();
    bindEvents();
    setTab('overview');
    refresh();
  }

  function bindEvents(){
    const refreshBtn = document.getElementById('btn-refresh');
    if (refreshBtn) refreshBtn.addEventListener('click', refresh);

    const runbookBtn = document.getElementById('btn-open-runbook');
    if (runbookBtn) runbookBtn.addEventListener('click', openRunbook);

    const preflightBtn = document.getElementById('btn-preflight');
    if (preflightBtn) preflightBtn.addEventListener('click', runPreflight);

    const focusSourcesBtn = document.getElementById('btn-focus-sources');
    if (focusSourcesBtn) focusSourcesBtn.addEventListener('click', openFocusSources);

    const autoPauseBtn = document.getElementById('btn-autonomy-pause');
    if (autoPauseBtn) autoPauseBtn.addEventListener('click', pauseAutonomy);

    const autoResumeBtn = document.getElementById('btn-autonomy-resume');
    if (autoResumeBtn) autoResumeBtn.addEventListener('click', resumeAutonomy);

    const autoFlushBtn = document.getElementById('btn-autonomy-flush');
    if (autoFlushBtn) autoFlushBtn.addEventListener('click', flushAutonomy);

    const tabs = document.querySelectorAll('.tab-buttons [role="tab"]');
    tabs.forEach(btn => {
      btn.addEventListener('click', () => setTab(btn.id.replace('tabbtn-','')));
      btn.addEventListener('keydown', (evt) => {
        if (evt.key === 'Enter' || evt.key === ' ') {
          evt.preventDefault();
          setTab(btn.id.replace('tabbtn-',''));
        }
      });
    });
  }

  function loadStoredPreflight(){
    try{
      const raw = localStorage.getItem(STORAGE_KEY);
      if (!raw) return;
      const parsed = JSON.parse(raw);
      if (!parsed || typeof parsed !== 'object') return;
      const ts = typeof parsed.time === 'number' ? parsed.time : null;
      const log = typeof parsed.log === 'string' ? parsed.log : '';
      if (ts) updatePreflightStatus(new Date(ts), log, false);
    }catch{}
  }

  function storePreflight(time, log){
    try{
      localStorage.setItem(STORAGE_KEY, JSON.stringify({ time, log }));
    }catch{}
  }

  function setTab(id){
    const tabs = document.querySelectorAll('.tab-buttons [role="tab"]');
    const panels = document.querySelectorAll('.tab-panel');
    tabs.forEach(btn => {
      const active = btn.id === `tabbtn-${id}`;
      btn.classList.toggle('active', active);
      btn.setAttribute('aria-selected', active ? 'true' : 'false');
      const panel = document.getElementById(`tab-${btn.id.replace('tabbtn-','')}`);
      if (panel) panel.classList.toggle('active', active);
    });
    panels.forEach(panel => {
      panel.classList.toggle('active', panel.id === `tab-${id}`);
    });
  }

  function setLoading(){
    setStatus('systems', 'unknown', 'Loading...');
    setStatus('memory', 'unknown', 'Loading...');
    setStatus('approvals', 'unknown', 'Loading...');
    setStatus('safety', 'unknown', 'Loading...');
    setStatus('autonomy', 'unknown', 'Loading...');
    setTile('systems', { level: 'unknown', summary: 'Loading...', meta: [] });
    setTile('memory', { level: 'unknown', summary: 'Loading...', meta: [] });
    setTile('approvals', { level: 'unknown', summary: 'Loading...', meta: [] });
    setTile('safety', { level: 'unknown', summary: 'Loading...', meta: [] });
    setTile('autonomy', { level: 'unknown', summary: 'Loading...', meta: [] });
    STATE.autonomy = {
      level: 'unknown',
      summary: 'Loading...',
      meta: [],
      lane: null,
      snapshot: null,
      line: 'Autonomy status loading...',
      operator: STATE.autonomy?.operator || null,
      alerts: [],
      updatedMs: null,
      lastEvent: null,
      lastReason: null,
    };
    syncAutonomyControlsFromState();
    setFocus([]);
    STATE.focusUpdatedMs = null;
    setFocusUpdated(null);
    renderLists();
  }

  async function refresh(){
    const refreshBtn = document.getElementById('btn-refresh');
    if (refreshBtn) refreshBtn.disabled = true;
    setLoading();
    const notice = document.getElementById('dataNotice');
    if (notice) notice.classList.add('hidden');
    try {
      const port = ARW.getPortFromInput('port');
      STATE.base = ARW.base(port);
      const payload = await fetchSnapshots(STATE.base);
      STATE.errors = payload.errors;
      STATE.unauthorized = payload.unauthorized;
      updateSystems(payload.serviceStatus, payload.routeStats);
      updateMemory(payload.telemetry, payload.memoryRecent);
      updateApprovals(payload.staging);
      updateSafety(payload.serviceStatus, payload.guardrails);
      updateFocus(payload.memoryRecent);
      updateLists(payload.routeStats);
      renderLists();
      if (STATE.unauthorized) {
        showNotice('Add an admin token in Launcher -> Preferences to see live metrics.');
      } else if (STATE.errors.length) {
        showNotice(`Partial data: ${STATE.errors.join('; ')}`);
      }
    } catch (err) {
      console.error('Refresh failed', err);
      showNotice('Failed to refresh trial data. Ensure the server is running and the admin token is set.');
      ARW.toast('Refresh failed');
    } finally {
      if (refreshBtn) refreshBtn.disabled = false;
    }
  }

  async function fetchSnapshots(base){
    const errors = [];
    let unauthorized = false;

    async function safeJson(path){
      try {
        return await ARW.http.json(base, path);
      } catch (err) {
        const msg = err && err.message ? String(err.message) : 'unknown error';
        errors.push(`${path}: ${msg}`);
        if (/401/.test(msg)) unauthorized = true;
        return null;
      }
    }

    const [serviceStatus, routeStats, staging, telemetry, guardrails, memoryRecent] = await Promise.all([
      safeJson('/state/service_status'),
      safeJson('/state/route_stats'),
      safeJson('/state/staging/actions?status=pending&limit=50'),
      safeJson('/state/training/telemetry'),
      safeJson('/state/guardrails_metrics'),
      safeJson('/state/memory/recent?limit=5')
    ]);

    return { serviceStatus, routeStats, staging, telemetry, guardrails, memoryRecent, errors, unauthorized };
  }

  function updateSystems(status, routeStats){
    if (!status) {
      const summary = STATE.unauthorized ? 'Authorize to read service status' : 'Service status unavailable';
      STATE.systems = { level: 'unknown', summary, meta: [] };
      setStatus('systems', 'unknown', summary);
      setTile('systems', STATE.systems);
      return;
    }
    const safe = status?.safe_mode?.active === true;
    const safeUntil = toNumber(status?.safe_mode?.until_ms || status?.safe_mode?.untilMs);
    const lastHealth = status?.last_health || {};
    const lastCrash = status?.last_crash || {};

    let level = 'ok';
    let summary = 'All systems nominal';

    const healthStatus = String(lastHealth.status || lastHealth.state || '').toLowerCase();
    if (safe) {
      level = 'bad';
      summary = 'Safe mode engaged';
    } else if (healthStatus && healthStatus !== 'ok' && healthStatus !== 'healthy') {
      level = healthStatus === 'degraded' ? 'warn' : 'bad';
      summary = `Health ${healthStatus}`;
    }

    const crashTime = parseTimestamp(lastCrash.time_ms || lastCrash.ts_ms || lastCrash.time);
    if (crashTime) {
      const mins = minutesAgo(crashTime);
      if (mins < 60 && level !== 'bad') {
        level = 'warn';
        summary = 'Recent crash recovered';
      }
    }

    const meta = [];
    const healthTime = parseTimestamp(lastHealth.time_ms || lastHealth.ts_ms || lastHealth.time);
    if (healthTime) {
      meta.push(['Last signal', formatRelative(healthTime)]);
    }
    if (safe) {
      meta.push(['Safe mode', safeUntil ? `until ${formatRelativeAbs(safeUntil)}` : 'active']);
    } else {
      meta.push(['Safe mode', 'off']);
    }
    if (crashTime) {
      meta.push(['Last crash', formatRelative(crashTime)]);
    }

    if (Array.isArray(routeStats?.routes)) {
      const worst = [...routeStats.routes]
        .filter(r => typeof r?.path === 'string')
        .sort((a, b) => (Number(b?.p95_ms) || 0) - (Number(a?.p95_ms) || 0))
        .slice(0, 1)[0];
      if (worst) {
        meta.push(['Slowest route', `${worst.path} | p95 ${(Number(worst.p95_ms) || 0).toFixed(0)} ms`]);
      }
    }

    STATE.systems = { level, summary, meta };
    setStatus('systems', level, summary);
    setTile('systems', STATE.systems);
  }

  function updateMemory(telemetry, memoryRecent){
    if (!telemetry) {
      const summary = STATE.unauthorized ? 'Authorize to read context telemetry' : 'Context telemetry unavailable';
      STATE.memory = { level: 'unknown', summary, meta: [] };
      setStatus('memory', 'unknown', summary);
      setTile('memory', STATE.memory);
      setFocus([]);
      return;
    }
    const context = telemetry?.context || {};
    const coverage = context.coverage || {};
    const recall = context.recall_risk || context.recallRisk || {};
    const assembled = context.assembled || {};
    let generatedMs = toNumber(
      telemetry.generated_ms ??
      telemetry.generatedMs ??
      context.generated_ms ??
      context.generatedMs
    );
    if (!Number.isFinite(generatedMs)) {
      generatedMs = parseTimestamp(telemetry.generated || context.generated);
    }
    const coverageLatest = coverage?.latest || {};
    const recallLatest = recall?.latest || {};
    const coverageSummary = summaryFromPayload(coverageLatest.summary);
    const recallSummary = summaryFromPayload(recallLatest.summary);

    const needsMore = Number(coverage.needs_more_ratio ?? coverage.needsMoreRatio ?? 0);
    const riskRatio = Number(recall.at_risk_ratio ?? recall.atRiskRatio ?? 0);
    const avgScore = Number(recall.avg_score ?? recall.avgScore ?? NaN);
    const latestScore = Number(recallLatest.score ?? NaN);
    const latestLevel = typeof recallLatest.level === 'string' ? recallLatest.level : '';

    let level = 'ok';
    let summary = 'Context coverage steady';
    if (needsMore > 0.25 || riskRatio > 0.25) {
      level = 'bad';
      summary = 'Context underfilled';
    } else if (needsMore > 0 || riskRatio > 0) {
      level = 'warn';
      summary = 'Context needs widening';
    } else if (coverageSummary) {
      summary = coverageSummary;
    } else if (recallSummary) {
      summary = recallSummary;
    }

    const meta = [];
    if (Number.isFinite(generatedMs)) {
      meta.push(['Telemetry updated', `${formatRelative(generatedMs)} (${formatRelativeAbs(generatedMs)})`]);
    }
    if (Number.isFinite(needsMore)) meta.push(['Needs more ratio', (needsMore * 100).toFixed(0) + '%']);
    if (Number.isFinite(riskRatio)) meta.push(['Recall risk', (riskRatio * 100).toFixed(0) + '%']);
    if (Number.isFinite(avgScore)) meta.push(['Avg recall score', avgScore.toFixed(2)]);
    if (Number.isFinite(latestScore)) meta.push(['Latest recall score', latestScore.toFixed(2)]);
    if (latestLevel) meta.push(['Latest recall level', latestLevel]);
    if (Array.isArray(assembled?.working_set?.counts)) {
      const counts = assembled.working_set.counts;
      if (typeof counts === 'object') {
        const total = Object.values(counts).reduce((acc, v) => acc + Number(v || 0), 0);
        meta.push(['Working set size', String(total)]);
      }
    }
    if (coverageSummary && summary !== coverageSummary) {
      meta.push(['Coverage summary', coverageSummary]);
    }
    if (recallSummary && summary !== recallSummary) {
      meta.push(['Recall summary', recallSummary]);
    }

    STATE.memory = { level, summary, meta };
    if (Number.isFinite(generatedMs)) {
      STATE.memory.generatedMs = generatedMs;
    } else {
      delete STATE.memory.generatedMs;
    }
    setStatus('memory', level, summary);
    setTile('memory', STATE.memory);
  }

  function updateApprovals(staging){
    if (!staging) {
      const summary = STATE.unauthorized ? 'Authorize to view approvals queue' : 'Approvals queue unavailable';
      STATE.approvals = { level: 'unknown', summary, meta: [] };
      setStatus('approvals', 'unknown', summary);
      setTile('approvals', STATE.approvals);
      return;
    }
    const items = Array.isArray(staging?.items) ? staging.items : [];
    const pending = items.filter(it => !it.status || String(it.status).toLowerCase() === 'pending');
    const count = pending.length;
    let level = 'ok';
    let summary = 'No approvals waiting';
    if (count > 0) {
      summary = `${count} approval${count === 1 ? '' : 's'} waiting`;
      if (count > 3) level = 'bad'; else level = 'warn';
    }
    const meta = [];
    if (count) {
      const oldestTs = pending
        .map(it => parseTimestamp(it.time_ms || it.ts_ms || it.created_ms || it.created_at))
        .filter(Boolean)
        .sort((a, b) => a - b)[0];
      if (oldestTs) meta.push(['Oldest request', formatRelativeWithAbs(oldestTs)]);
      const lanes = new Set(pending.map(it => it.lane || it.kind || it.scope).filter(Boolean));
      if (lanes.size) meta.push(['Lanes', Array.from(lanes).join(', ')]);
    }
    STATE.approvals = { level, summary, meta };
    setStatus('approvals', level, summary);
    setTile('approvals', STATE.approvals);
  }

  function updateSafety(status, guardrails){
    if (!guardrails) {
      const summary = STATE.unauthorized ? 'Authorize to read guardrail metrics' : 'Guardrail metrics unavailable';
      STATE.safety = { level: 'unknown', summary, meta: [] };
      setStatus('safety', 'unknown', summary);
      setTile('safety', STATE.safety);
      return;
    }
    const metrics = guardrails || {};
    const cbOpen = Number(metrics.cb_open ?? metrics.cbOpen ?? 0) > 0;
    const httpErrors = Number(metrics.http_errors ?? metrics.httpErrors ?? 0);
    const retries = Number(metrics.retries ?? 0);
    const trips = Number(metrics.cb_trips ?? metrics.cbTrips ?? 0);

    let level = 'ok';
    let summary = 'Guardrails stable';
    if (cbOpen) {
      level = 'bad';
      summary = 'Circuit breaker open';
    } else if (httpErrors > 0 || retries > 0) {
      level = 'warn';
      summary = 'Guardrails recovering';
    }

    const meta = [
      ['Retries', retries.toString()],
      ['HTTP errors', httpErrors.toString()],
      ['CB trips', trips.toString()],
    ];

    if (status?.safe_mode?.active) {
      meta.push(['Safe mode', 'engaged']);
    }

    STATE.safety = { level, summary, meta };
    setStatus('safety', level, summary);
    setTile('safety', STATE.safety);
  }
  function updateAutonomy(payload){
    const currentOperator = STATE.autonomy?.operator || null;
    const fallbackSummary = STATE.unauthorized
      ? 'Authorize to manage the autonomy lane.'
      : 'No autonomy lane configured.';
    if (!payload || !Array.isArray(payload?.lanes)) {
      STATE.autonomy = {
        level: 'unknown',
        summary: fallbackSummary,
        meta: [],
        lane: null,
        snapshot: null,
        line: fallbackSummary,
        operator: currentOperator,
        alerts: [],
        updatedMs: null,
        lastEvent: null,
        lastReason: null,
      };
      setStatus('autonomy', 'unknown', fallbackSummary);
      setTile('autonomy', { level: 'unknown', summary: fallbackSummary, meta: [] });
      syncAutonomyControlsFromState();
      return;
    }
    const lanes = payload.lanes.filter(Boolean);
    if (!lanes.length) {
      const noLane = STATE.unauthorized ? 'Authorize to view autonomy lane.' : 'Autonomy lane not configured.';
      STATE.autonomy = {
        level: 'unknown',
        summary: noLane,
        meta: [],
        lane: null,
        snapshot: null,
        line: noLane,
        operator: currentOperator,
        alerts: [],
        updatedMs: null,
        lastEvent: null,
        lastReason: null,
      };
      setStatus('autonomy', 'unknown', noLane);
      setTile('autonomy', { level: 'unknown', summary: noLane, meta: [] });
      syncAutonomyControlsFromState();
      return;
    }
    const preferredId = STATE.autonomy?.lane || 'trial-g4-autonomy';
    const lane = lanes.find(l => String(l?.lane_id || l?.laneId) === preferredId) || lanes[0];
    const laneId = String(lane?.lane_id || lane?.laneId || '');
    const mode = String(lane?.mode || 'guided').toLowerCase();
    const alerts = Array.isArray(lane?.alerts) ? lane.alerts.filter(Boolean).map(String) : [];
    const active = toNumber(lane?.active_jobs ?? lane?.activeJobs) ?? 0;
    const queued = toNumber(lane?.queued_jobs ?? lane?.queuedJobs) ?? 0;
    const updatedMs = toNumber(lane?.updated_ms ?? lane?.updatedMs);
    const lastEvent = lane?.last_event ?? lane?.lastEvent ?? null;
    const lastReason = lane?.last_reason ?? lane?.lastReason ?? null;
    const lastOperator = lane?.last_operator ?? lane?.lastOperator ?? null;

    let level = 'ok';
    let summary = 'Guided mode';
    if (alerts.length) {
      level = 'bad';
      summary = alerts[0];
    } else if (mode === 'paused') {
      level = 'warn';
      summary = 'Autonomy paused';
    } else if (mode === 'autonomous') {
      level = 'warn';
      summary = 'Autonomy running';
    } else if (active > 0 || queued > 0) {
      summary = `Guided mode (${active} active, ${queued} queued)`;
    }

    const meta = [
      ['Lane', laneId || '--'],
      ['Mode', mode],
      ['Active jobs', active.toString()],
      ['Queued jobs', queued.toString()],
    ];
    if (updatedMs) {
      meta.push(['Updated', formatRelative(updatedMs)]);
    }
    if (lastOperator) {
      meta.push(['Operator', String(lastOperator)]);
    }
    if (lastReason) {
      meta.push(['Reason', String(lastReason)]);
    }
    const budgets = lane?.budgets || lane?.Budgets;
    if (budgets && typeof budgets === 'object') {
      const parts = [];
      const wall = toNumber(budgets.wall_clock_remaining_secs ?? budgets.wallClockRemainingSecs);
      const tokens = toNumber(budgets.tokens_remaining ?? budgets.tokensRemaining);
      const spend = toNumber(budgets.spend_remaining_cents ?? budgets.spendRemainingCents);
      if (wall != null) parts.push(`${formatSeconds(wall)} wall clock`);
      if (tokens != null) parts.push(`${tokens.toLocaleString()} tokens`);
      if (spend != null) parts.push(`$${(spend / 100).toFixed(2)} spend`);
      if (parts.length) meta.push(['Budgets', parts.join(' | ')]);
    }

    const overviewLine = `${summary}${laneId ? ` (${laneId})` : ''}`;

    STATE.autonomy = {
      level,
      summary,
      meta,
      lane: laneId,
      snapshot: lane,
      line: overviewLine,
      operator: currentOperator,
      alerts,
      updatedMs,
      lastEvent,
      lastReason,
    };

    setStatus('autonomy', level, summary);
    setTile('autonomy', { level, summary, meta });
    syncAutonomyControlsFromState();
  }

  function syncAutonomyControlsFromState() {
    const snapshot = STATE.autonomy?.snapshot || null;
    updateAutonomyControls(
      snapshot,
      STATE.autonomy?.level || 'unknown',
      STATE.autonomy?.summary || '',
      STATE.autonomy?.alerts || [],
      STATE.autonomy?.updatedMs || snapshot?.updated_ms || snapshot?.updatedMs,
      STATE.autonomy?.lastEvent || snapshot?.last_event || snapshot?.lastEvent,
      STATE.autonomy?.lastReason || snapshot?.last_reason || snapshot?.lastReason,
    );
  }

  function updateAutonomyControls(lane, level, summary, alerts, updatedMs, lastEvent, lastReason) {
    const summaryEl = document.getElementById('autonomySummary');
    const lastEl = document.getElementById('autonomyLastAction');
    const alertsEl = document.getElementById('autonomyAlerts');
    const unauthorized = STATE.unauthorized;
    const modeRaw = lane ? (lane.mode ?? lane.Mode ?? lane.state ?? 'guided') : 'guided';
    const mode = String(modeRaw).toLowerCase();
    if (summaryEl) {
      summaryEl.textContent = summary || (lane ? '' : (unauthorized ? 'Authorize to manage the autonomy lane.' : 'No autonomy lane configured.'));
    }
    if (lastEl) {
      if (!lane) {
        lastEl.textContent = '';
      } else {
        const bits = [];
        if (lastEvent) bits.push(readableAutonomyEvent(lastEvent));
        if (updatedMs) bits.push(formatRelative(updatedMs));
        if (lastReason) bits.push(`"${lastReason}"`);
        lastEl.textContent = bits.length ? `Last change: ${bits.join(' | ')}` : '';
      }
    }
    if (alertsEl) {
      alertsEl.innerHTML = '';
      if (!lane || !alerts || !alerts.length) {
        alertsEl.classList.add('hidden');
      } else {
        alertsEl.classList.remove('hidden');
        alerts.forEach(alert => {
          const li = document.createElement('li');
          li.textContent = alert;
          alertsEl.appendChild(li);
        });
      }
    }
    const disableAll = unauthorized || !lane;
    const pauseBtn = document.getElementById('btn-autonomy-pause');
    if (pauseBtn && pauseBtn.dataset.autonomyBusy !== '1') {
      pauseBtn.disabled = disableAll || modeIsPaused(mode);
    }
    const resumeBtn = document.getElementById('btn-autonomy-resume');
    if (resumeBtn && resumeBtn.dataset.autonomyBusy !== '1') {
      resumeBtn.disabled = disableAll || modeIsGuided(mode);
    }
    const flushBtn = document.getElementById('btn-autonomy-flush');
    if (flushBtn && flushBtn.dataset.autonomyBusy !== '1') {
      const activeJobs = toNumber(lane?.active_jobs ?? lane?.activeJobs) ?? 0;
      const queuedJobs = toNumber(lane?.queued_jobs ?? lane?.queuedJobs) ?? 0;
      flushBtn.disabled = disableAll || (activeJobs === 0 && queuedJobs === 0);
    }
  }

  function markAutonomyBusy(flag) {
    ['btn-autonomy-pause', 'btn-autonomy-resume', 'btn-autonomy-flush'].forEach(id => {
      const btn = document.getElementById(id);
      if (!btn) return;
      if (flag) {
        btn.dataset.autonomyBusy = '1';
        btn.disabled = true;
      } else {
        delete btn.dataset.autonomyBusy;
      }
    });
  }

  function getStoredAutonomyOperator() {
    try {
      const val = localStorage.getItem(AUTONOMY_OPERATOR_KEY);
      return val ? String(val) : null;
    } catch {
      return null;
    }
  }

  function rememberAutonomyOperator(name) {
    try {
      if (name) localStorage.setItem(AUTONOMY_OPERATOR_KEY, name);
    } catch {}
  }

  function ensureAutonomyOperator() {
    const cached = STATE.autonomy?.operator;
    if (cached && String(cached).trim()) return String(cached).trim();
    const stored = getStoredAutonomyOperator();
    if (stored && stored.trim()) {
      STATE.autonomy.operator = stored.trim();
      return stored.trim();
    }
    const input = prompt('Operator name for autonomy actions?');
    if (!input) return null;
    const trimmed = input.trim();
    if (!trimmed) return null;
    STATE.autonomy.operator = trimmed;
    rememberAutonomyOperator(trimmed);
    return trimmed;
  }

  function promptAutonomyReason(defaultText) {
    const input = prompt('Reason for autonomy action?', defaultText);
    if (input === null) return null;
    const trimmed = input.trim();
    return trimmed || defaultText;
  }

  async function pauseAutonomy() {
    if (!STATE.base) {
      ARW.toast('Start the server first');
      return;
    }
    const lane = STATE.autonomy?.lane;
    if (!lane) {
      ARW.toast('No autonomy lane configured');
      return;
    }
    const operator = ensureAutonomyOperator();
    if (!operator) {
      ARW.toast('Operator required');
      return;
    }
    const reason = promptAutonomyReason('Kill switch from Trial Control Center');
    if (reason === null) return;
    markAutonomyBusy(true);
    try {
      const resp = await ARW.http.fetch(STATE.base, `/admin/autonomy/${encodeURIComponent(lane)}/pause`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ operator, reason }),
      });
      if (!resp.ok) throw new Error(`HTTP ${resp.status}`);
      ARW.toast('Autonomy paused');
      await refresh();
    } catch (err) {
      console.error('Pause autonomy failed', err);
      ARW.toast(err && err.message ? err.message : 'Pause failed');
    } finally {
      markAutonomyBusy(false);
      syncAutonomyControlsFromState();
    }
  }

  async function resumeAutonomy() {
    if (!STATE.base) {
      ARW.toast('Start the server first');
      return;
    }
    const lane = STATE.autonomy?.lane;
    if (!lane) {
      ARW.toast('No autonomy lane configured');
      return;
    }
    const operator = ensureAutonomyOperator();
    if (!operator) {
      ARW.toast('Operator required');
      return;
    }
    const reason = promptAutonomyReason('Resume guided operations');
    if (reason === null) return;
    markAutonomyBusy(true);
    try {
      const resp = await ARW.http.fetch(STATE.base, `/admin/autonomy/${encodeURIComponent(lane)}/resume`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ operator, reason, mode: 'guided' }),
      });
      if (!resp.ok) throw new Error(`HTTP ${resp.status}`);
      ARW.toast('Autonomy set to guided');
      await refresh();
    } catch (err) {
      console.error('Resume autonomy failed', err);
      ARW.toast(err && err.message ? err.message : 'Resume failed');
    } finally {
      markAutonomyBusy(false);
      syncAutonomyControlsFromState();
    }
  }

  async function flushAutonomy() {
    if (!STATE.base) {
      ARW.toast('Start the server first');
      return;
    }
    const lane = STATE.autonomy?.lane;
    if (!lane) {
      ARW.toast('No autonomy lane configured');
      return;
    }
    const confirmFlush = confirm('Flush all in-flight and queued autonomy jobs?');
    if (!confirmFlush) return;
    markAutonomyBusy(true);
    try {
      const resp = await ARW.http.fetch(STATE.base, `/admin/autonomy/${encodeURIComponent(lane)}/jobs`, {
        method: 'DELETE',
      });
      if (!resp.ok) throw new Error(`HTTP ${resp.status}`);
      ARW.toast('Autonomy jobs flushed');
      await refresh();
    } catch (err) {
      console.error('Flush autonomy failed', err);
      ARW.toast(err && err.message ? err.message : 'Flush failed');
    } finally {
      markAutonomyBusy(false);
      syncAutonomyControlsFromState();
    }
  }

  function readableAutonomyEvent(event) {
    const value = String(event || '').toLowerCase();
    switch (value) {
      case 'paused':
        return 'Paused';
      case 'resumed':
        return 'Resumed';
      case 'autonomous':
        return 'Autonomy started';
      case 'jobs_flushed':
        return 'Jobs flushed';
      case 'budgets_updated':
        return 'Budgets updated';
      case 'jobs_updated':
        return 'Jobs updated';
      default:
        return value ? value.replace(/_/g, ' ') : 'Updated';
    }
  }

  function modeIsPaused(mode) {
    return String(mode || '').toLowerCase() === 'paused';
  }

  function modeIsGuided(mode) {
    return String(mode || '').toLowerCase() === 'guided';
  }


  function updateFocus(memoryRecent){
    if (!memoryRecent) {
      STATE.focus = [];
      STATE.focusUpdatedMs = null;
      setFocus([]);
      setFocusUpdated(null);
      return;
    }
    const items = Array.isArray(memoryRecent?.items) ? memoryRecent.items : [];
    let updatedMs = toNumber(memoryRecent.generated_ms ?? memoryRecent.generatedMs);
    if (!Number.isFinite(updatedMs)) {
      updatedMs = parseTimestamp(memoryRecent.generated);
    }
    const focusEntries = items.slice(0, 5).map((item) => {
      const lane = item.lane || item.kind || (item.ptr && item.ptr.lane) || 'memory';
      const title = resolveMemoryTitle(item);
      const ts = parseTimestamp(item.time_ms || item.ts_ms || item.created_ms || item.time);
      const rel = ts ? formatRelative(ts) : 'recent';
      const project = item.project_id || item.project || (item.spec && item.spec.project);
      if (!updatedMs && ts) updatedMs = ts;
      return { lane, title, rel, project };
    });
    if (!updatedMs && Number.isFinite(STATE.memory?.generatedMs)) {
      updatedMs = STATE.memory.generatedMs;
    }
    STATE.focus = focusEntries;
    STATE.focusUpdatedMs = updatedMs || null;
    setFocus(STATE.focus);
    setFocusUpdated(STATE.focusUpdatedMs);
  }

  function updateLists(routeStats){
    const systemLine = `${STATE.systems.summary}${STATE.systems.meta[0] ? ` (${STATE.systems.meta[0][1]})` : ''}`;
    const memoryStamp = STATE.focusUpdatedMs ? ` (updated ${formatRelativeWithAbs(STATE.focusUpdatedMs)})` : '';
    const memoryLine = `${STATE.memory.summary}${memoryStamp}`;
    const approvalLine = STATE.approvals.summary;
    const safetyLine = STATE.safety.summary;
    const autonomyLine = STATE.autonomy?.line || (STATE.unauthorized ? 'Authorize to view autonomy lane.' : 'Autonomy lane idle.');

    const topRoutes = Array.isArray(routeStats?.routes)
      ? [...routeStats.routes]
          .filter(r => typeof r?.path === 'string')
          .sort((a, b) => (Number(b?.p95_ms) || 0) - (Number(a?.p95_ms) || 0))
          .slice(0, 3)
          .map(r => `${r.path} | p95 ${(Number(r.p95_ms) || 0).toFixed(0)} ms (hits ${(Number(r.hits) || 0).toLocaleString()})`)
      : [];

    STATE.overview = [systemLine, memoryLine, approvalLine, safetyLine, autonomyLine];
    STATE.workflows = [
      MEMORY_WORKFLOW_TEXT(STATE.memory, STATE.focus),
      topRoutes.length ? `Slowest routes: ${topRoutes.join('; ')}` : 'Route latencies steady.',
      approvalLine,
      autonomyLine,
    ];
    STATE.safeguards = [
      safetyLine,
      STATE.systems.meta.find(([label]) => label === 'Safe mode')?.join(': ') || 'Safe mode off',
      `Guardrails retries: ${STATE.safety.meta[0]?.[1] || '0'}`,
      autonomyLine,
    ];
  }

  function MEMORY_WORKFLOW_TEXT(memoryState, focus){
    if (!focus || !focus.length) return 'Working set ready for launch.';
    const laneCounts = new Map();
    for (const item of focus) {
      laneCounts.set(item.lane, (laneCounts.get(item.lane) || 0) + 1);
    }
    const laneSummary = Array.from(laneCounts.entries()).map(([lane, count]) => `${lane}: ${count}`).join(', ');
    return `Recent focus lanes - ${laneSummary}. ${memoryState.summary}`;
  }

  function setFocus(entries){
    const list = document.getElementById('focusList');
    if (!list) return;
    list.innerHTML = '';
    if (!entries || !entries.length) {
      const li = document.createElement('li');
      li.textContent = STATE.unauthorized ? 'Authorize to load focus summary.' : 'No recent context items.';
      list.appendChild(li);
      return;
    }
    entries.forEach(item => {
      const li = document.createElement('li');
      const title = document.createElement('span');
      title.textContent = item.title;
      title.className = 'title';
      li.appendChild(title);
      const meta = document.createElement('span');
      meta.className = 'meta dim';
      meta.textContent = `[${item.lane}] ${item.rel}${item.project ? ` | ${item.project}` : ''}`;
      li.appendChild(meta);
      list.appendChild(li);
    });
  }

  function setFocusUpdated(ms){
    const el = document.getElementById('focusUpdated');
    if (!el) return;
    const focusBtn = document.getElementById('btn-focus-sources');
    if (focusBtn) {
      const disabled = STATE.unauthorized || !STATE.base;
      focusBtn.disabled = disabled;
      focusBtn.setAttribute('aria-disabled', disabled ? 'true' : 'false');
      focusBtn.title = disabled
        ? 'Connect with an admin token to open memory sources'
        : 'Open memory sources in debug view';
    }
    if (!ms || !Number.isFinite(ms)) {
      el.textContent = '';
      el.title = '';
      el.classList.add('hidden');
      el.setAttribute('aria-hidden', 'true');
      return;
    }
    el.classList.remove('hidden');
    el.setAttribute('aria-hidden', 'false');
    el.textContent = `Updated ${formatRelative(ms)} (${formatRelativeAbs(ms)})`;
    el.title = `Snapshot captured ${formatRelativeAbs(ms)}`;
  }

  function renderLists(){
    setList('list-overview', STATE.overview);
    setList('list-workflows', STATE.workflows);
    setList('list-safeguards', STATE.safeguards);
  }

  function setList(id, entries){
    const el = document.getElementById(id);
    if (!el) return;
    el.innerHTML = '';
    if (!entries || !entries.length) {
      const li = document.createElement('li');
      li.textContent = STATE.unauthorized ? 'Authorize to view metrics.' : 'No data yet.';
      el.appendChild(li);
      return;
    }
    entries.forEach(text => {
      if (!text) return;
      const li = document.createElement('li');
      li.textContent = text;
      el.appendChild(li);
    });
  }

  function setStatus(kind, level, summary){
    const pill = document.querySelector(`.status-pill[data-kind="${kind}"]`);
    if (!pill) return;
    const dot = pill.querySelector('.dot');
    const value = pill.querySelector('.value');
    pill.classList.remove('ok','warn','bad');
    const cls = level === 'ok' || level === 'warn' || level === 'bad' ? level : 'unknown';
    if (cls !== 'unknown') pill.classList.add(cls);
    if (dot) {
      dot.classList.remove('ok','warn','bad');
      if (cls !== 'unknown') dot.classList.add(cls);
    }
    if (value) value.textContent = STATUS_LABELS[cls] || STATUS_LABELS.unknown;
    if (summary) pill.title = summary;
  }

  function setTile(kind, data){
    const tile = document.querySelector(`.tile[data-kind="${kind}"]`);
    if (!tile) return;
    const pill = tile.querySelector('.pill');
    const body = tile.querySelector('.tile-body');
    const metaList = tile.querySelector('.tile-meta');
    tile.classList.remove('ok','warn','bad');
    if (data.level === 'ok' || data.level === 'warn' || data.level === 'bad') {
      tile.classList.add(data.level);
    }
    if (pill) {
      pill.textContent = STATUS_LABELS[data.level] || STATUS_LABELS.unknown;
      pill.classList.remove('ok','warn','bad');
      if (data.level === 'ok' || data.level === 'warn' || data.level === 'bad') {
        pill.classList.add(data.level);
      }
    }
    if (body) body.textContent = data.summary || '--';
    if (metaList) {
      metaList.innerHTML = '';
      (data.meta || []).forEach(([label, value]) => {
        if (!label || value == null) return;
        const dt = document.createElement('dt');
        dt.textContent = label;
        const dd = document.createElement('dd');
        dd.textContent = value;
        metaList.appendChild(dt);
        metaList.appendChild(dd);
      });
    }
  }

  function resolveMemoryTitle(item){
    if (!item || typeof item !== 'object') return 'Memory item';
    const summary = item.summary;
    if (typeof summary === 'string' && summary.trim()) return summary.trim();
    if (summary && typeof summary === 'object') {
      if (typeof summary.text === 'string' && summary.text.trim()) return summary.text.trim();
      if (typeof summary.title === 'string' && summary.title.trim()) return summary.title.trim();
    }
    const value = item.value;
    if (typeof value === 'string' && value.trim()) return value.trim();
    if (value && typeof value === 'object') {
      if (typeof value.text === 'string' && value.text.trim()) return value.text.trim();
      if (typeof value.title === 'string' && value.title.trim()) return value.title.trim();
      if (value.summary && typeof value.summary === 'string' && value.summary.trim()) return value.summary.trim();
    }
    return 'Memory item';
  }

  function summaryFromPayload(payload){
    if (!payload) return '';
    if (typeof payload === 'string') {
      const trimmed = payload.trim();
      return trimmed;
    }
    if (typeof payload === 'object') {
      if (typeof payload.text === 'string' && payload.text.trim()) return payload.text.trim();
      if (typeof payload.title === 'string' && payload.title.trim()) return payload.title.trim();
      if (typeof payload.summary === 'string' && payload.summary.trim()) return payload.summary.trim();
    }
    return '';
  }

  function showNotice(text){
    const notice = document.getElementById('dataNotice');
    if (!notice) return;
    notice.textContent = text || '';
    notice.classList.toggle('hidden', !text);
  }

  async function openFocusSources(){
    if (!STATE.base) {
      ARW.toast('Start the server first');
      return;
    }
    try {
      await ARW.invoke('open_url', { url: `${STATE.base}/admin/debug#memory` });
    } catch (err) {
      console.error('Open focus sources failed', err);
      ARW.toast('Unable to open memory view');
    }
  }

  async function openRunbook(){
    if (!STATE.base) {
      ARW.toast('Start the server first');
      return;
    }
    try {
      await ARW.invoke('open_url', { url: `${STATE.base}/docs/ops/trial_runbook/` });
    } catch (err) {
      console.error('Open runbook failed', err);
      ARW.toast('Unable to open runbook');
    }
  }

  async function runPreflight(){
    const btn = document.getElementById('btn-preflight');
    const logEl = document.getElementById('preflightLog');
    if (btn) btn.disabled = true;
    if (logEl) {
      logEl.textContent = '';
      logEl.classList.add('hidden');
    }
    try {
      const raw = await ARW.invoke('run_trials_preflight');
      const output = String(raw ?? '');
      const now = Date.now();
      updatePreflightStatus(new Date(now), output, true);
      storePreflight(now, output);
      if (logEl) {
        logEl.textContent = output;
        logEl.classList.remove('hidden');
      }
      ARW.toast('Preflight completed');
    } catch (err) {
      console.warn('Preflight command failed', err);
      fallbackPreflight();
    } finally {
      if (btn) btn.disabled = false;
    }
  }

  async function fallbackPreflight(){
    updatePreflightStatus(null, '', false, 'Automation unavailable. Run "just trials-preflight" manually. Command copied to clipboard.');
    try {
      await navigator.clipboard.writeText('just trials-preflight');
    } catch {}
    ARW.toast('Command copied');
  }

  function updatePreflightStatus(dateObj, log, includeTime, fallbackMsg){
    const statusEl = document.getElementById('preflightStatus');
    if (!statusEl) return;
    if (fallbackMsg) {
      statusEl.textContent = fallbackMsg;
      return;
    }
    if (!dateObj) {
      statusEl.textContent = 'Preflight not yet run.';
      return;
    }
    const ts = includeTime ? ` at ${dateObj.toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' })}` : '';
    statusEl.textContent = `Preflight completed ${formatRelative(dateObj.getTime())}${ts}.`;
    if (log) statusEl.title = log;
  }

  function parseTimestamp(value){
    if (value == null) return null;
    if (typeof value === 'number') {
      if (value > 1e16) return Math.round(value / 1000); // prevent overflow
      return value > 1e12 ? value : value * 1000;
    }
    if (typeof value === 'string' && value.trim()) {
      const digits = Number(value);
      if (!Number.isNaN(digits)) {
        return digits > 1e12 ? digits : digits * 1000;
      }
      const parsed = Date.parse(value);
      if (!Number.isNaN(parsed)) return parsed;
    }
    return null;
  }

  function minutesAgo(ms){
    const diff = Date.now() - ms;
    return diff / 60000;
  }

  function formatRelative(ms){
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

  function formatRelativeAbs(ms){
    const date = new Date(ms);
    return `${date.toLocaleDateString()} ${date.toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' })}`;
  }

  function formatRelativeWithAbs(ms){
    if (!Number.isFinite(ms)) return '--';
    return `${formatRelative(ms)} (${formatRelativeAbs(ms)})`;
  }

  function formatSeconds(sec){
    const value = Number(sec);
    if (!Number.isFinite(value) || value < 0) return '--';
    if (value >= 3600) return `${Math.round(value / 3600)} h`;
    if (value >= 120) return `${Math.round(value / 60)} min`;
    if (value >= 1) return `${Math.round(value)} s`;
    if (value > 0) return '<1 s';
    return '0 s';
  }

  function toNumber(value){
    const num = Number(value);
    return Number.isFinite(num) ? num : null;
  }
})();
