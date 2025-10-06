(function(){
  const STORAGE_KEY = 'arw:trial:last-preflight';
  const STATUS_LABELS = { ok: 'All good', warn: 'Check soon', bad: 'Action needed', unknown: 'Unknown' };
  const AUTONOMY_OPERATOR_KEY = 'arw:trial:autonomy-operator';
  const APPROVAL_REVIEWER_KEY = 'arw:trial:approvals-reviewer';
  const AUTO_REFRESH_INTERVALS = Object.freeze({ approvals: 25000, feedback: 30000, connections: 35000, autonomy: 45000, quarantine: 45000 });
  const AUTONOMY_INTERRUPT_KEYS = Object.freeze(['pause', 'stop_flush_all', 'stop_flush_inflight', 'stop_flush_queued']);
  const AUTONOMY_INTERRUPT_LABELS = Object.freeze({
    pause: 'Pause',
    stop_flush_all: 'Stop · flush all',
    stop_flush_inflight: 'Stop · flush in-flight',
    stop_flush_queued: 'Stop · flush queued',
  });

  let approvalsTimer = null;
  let feedbackTimer = null;
  let connectionsTimer = null;
  let autonomyTimer = null;
  let quarantineTimer = null;
  let approvalsInflight = false;
  let feedbackInflight = false;
  let connectionsInflight = false;
  let autonomyInflight = false;
  let quarantineInflight = false;
  let visibilityHandlerAttached = false;
  let baseMeta = null;
  const updateBaseMeta = () => ARW.applyBaseMeta({ portInputId: 'port', badgeId: 'baseBadge', label: 'Base' });

  const STATE = {
    systems: { level: 'unknown', summary: 'Loading...', meta: [] },
    memory: { level: 'unknown', summary: 'Loading...', meta: [] },
    approvals: {
      level: 'unknown',
      summary: 'Loading...',
      meta: [],
      pending: [],
      recent: [],
      generatedMs: null,
      reviewer: null,
      loading: true,
      error: null,
    },
    quarantine: {
      entries: [],
      summary: 'Loading…',
      counts: {},
      generatedMs: null,
      loading: true,
      error: null,
      total: 0,
    },
    feedback: {
      loading: true,
      delta: [],
      updatedMs: null,
      autoApply: false,
      lastVersion: null,
      error: null,
    },
    safety: { level: 'unknown', summary: 'Loading...', meta: [], lastApplied: null, metrics: null },
    autonomy: {
      level: 'unknown',
      summary: 'Loading...',
      meta: [],
      lane: null,
      snapshot: null,
      line: 'Autonomy status loading...',
      operator: null,
      alerts: [],
      interruptCounts: {},
      newInterruptCounts: {},
      newInterrupts: 0,
      updatedMs: null,
      lastEvent: null,
      lastReason: null,
    },
    connections: { nodes: [], summary: 'Loading connections…', error: null, loading: true, updatedMs: null },
    overview: [],
    workflows: [],
    safeguards: [],
    focus: [],
    focusUpdatedMs: null,
    errors: [],
    unauthorized: false,
    base: null,
    connectionsOpen: false,
    connectionsRestore: null,
  };

  document.addEventListener('DOMContentLoaded', init); // eslint-disable-line no-undef

  async function init(){
    try{ await ARW.applyPortFromPrefs('port'); }catch{}
    baseMeta = updateBaseMeta();
    try {
      const port = ARW.getPortFromInput('port');
      STATE.base = (baseMeta && baseMeta.base) || ARW.base(port);
    } catch {}
    loadStoredPreflight();
    bindEvents();
    renderQuarantineLane();
    STATE.approvals.reviewer = getStoredApprovalReviewer();
    setTab('overview');
    try {
      await refresh();
    } finally {
      startAutoRefreshLoops();
      window.addEventListener('beforeunload', stopAutoRefreshLoops, { once: true });
    }
  }

  function bindEvents(){
    const refreshBtn = document.getElementById('btn-refresh');
    if (refreshBtn) refreshBtn.addEventListener('click', refresh);

    const portInput = document.getElementById('port');
    if (portInput) {
      portInput.addEventListener('change', () => {
        baseMeta = updateBaseMeta();
        try {
          const port = ARW.getPortFromInput('port');
          STATE.base = (baseMeta && baseMeta.base) || ARW.base(port);
        } catch {}
      });
    }

    const runbookBtn = document.getElementById('btn-open-runbook');
    if (runbookBtn) runbookBtn.addEventListener('click', openRunbook);

    const connectionsBtn = document.getElementById('btn-open-connections');
    if (connectionsBtn) connectionsBtn.addEventListener('click', openConnectionsDrawer);

    const connectionsClose = document.getElementById('btn-close-connections');
    if (connectionsClose) connectionsClose.addEventListener('click', closeConnectionsDrawer);

    const connectionsOverlay = document.getElementById('connectionsOverlay');
    if (connectionsOverlay) connectionsOverlay.addEventListener('click', closeConnectionsDrawer);

    const connectionsRefresh = document.getElementById('btn-connections-refresh');
    if (connectionsRefresh) connectionsRefresh.addEventListener('click', refreshConnections);

    const preflightBtn = document.getElementById('btn-preflight');
    if (preflightBtn) preflightBtn.addEventListener('click', runPreflight);

    const focusSourcesBtn = document.getElementById('btn-focus-sources');
    if (focusSourcesBtn) focusSourcesBtn.addEventListener('click', openFocusSources);

    const approvalsRefresh = document.getElementById('btn-approvals-refresh');
    if (approvalsRefresh) approvalsRefresh.addEventListener('click', () => {
      if (STATE.approvals.loading) return;
      STATE.approvals.loading = true;
      renderApprovalsLane();
      refreshApprovalsLane(false);
    });

    const approvalsReviewer = document.getElementById('btn-approvals-reviewer');
    if (approvalsReviewer) approvalsReviewer.addEventListener('click', () => requestApprovalReviewerChange());

    const approvalsOpenDebug = document.getElementById('btn-approvals-open-debug');
    if (approvalsOpenDebug) approvalsOpenDebug.addEventListener('click', openApprovalsInDebug);

    const quarantineRefresh = document.getElementById('btn-quarantine-refresh');
    if (quarantineRefresh) quarantineRefresh.addEventListener('click', () => {
      if (STATE.quarantine.loading) return;
      STATE.quarantine.loading = true;
      renderQuarantineLane();
      refreshQuarantineLane(false);
    });

    const quarantineDocs = document.getElementById('btn-quarantine-open-docs');
    if (quarantineDocs) quarantineDocs.addEventListener('click', openQuarantineDocs);

    const quarantineDebug = document.getElementById('btn-quarantine-open-debug');
    if (quarantineDebug) quarantineDebug.addEventListener('click', openFocusSources);

    const feedbackRefresh = document.getElementById('btn-feedback-refresh');
    if (feedbackRefresh) feedbackRefresh.addEventListener('click', () => refreshFeedbackDelta(false));

    const feedbackOpen = document.getElementById('btn-feedback-open-debug');
    if (feedbackOpen) feedbackOpen.addEventListener('click', openFeedbackInDebug);

    const autoPauseBtn = document.getElementById('btn-autonomy-pause');
    if (autoPauseBtn) autoPauseBtn.addEventListener('click', pauseAutonomy);

    const autoResumeBtn = document.getElementById('btn-autonomy-resume');
    if (autoResumeBtn) autoResumeBtn.addEventListener('click', resumeAutonomy);

    const autoStopBtn = document.getElementById('btn-autonomy-stop');
    if (autoStopBtn) autoStopBtn.addEventListener('click', stopAutonomy);

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

    document.addEventListener('keydown', handleGlobalKeydown);
    window.addEventListener('arw:base-override-changed', () => {
      baseMeta = updateBaseMeta();
      try {
        const port = ARW.getPortFromInput('port');
        STATE.base = (baseMeta && baseMeta.base) || ARW.base(port);
      } catch {
        STATE.base = null;
      }
      refresh().catch(() => {});
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
    STATE.safety.lastApplied = null;
    STATE.safety.metrics = null;
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
    STATE.approvals.loading = true;
    STATE.approvals.summary = 'Loading approvals…';
    STATE.approvals.pending = [];
    STATE.approvals.recent = [];
    STATE.approvals.generatedMs = null;
    STATE.approvals.error = null;
    renderApprovalsLane();
    STATE.feedback = {
      loading: true,
      delta: [],
      updatedMs: null,
      autoApply: false,
      lastVersion: null,
      error: null,
    };
    renderFeedbackDelta();
    STATE.connections.loading = true;
    STATE.connections.nodes = [];
    STATE.connections.summary = 'Loading connections…';
    STATE.connections.error = null;
    STATE.connections.updatedMs = null;
    renderConnections();
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
      baseMeta = updateBaseMeta();
      const port = ARW.getPortFromInput('port');
      STATE.base = (baseMeta && baseMeta.base) || ARW.base(port);
      const payload = await fetchSnapshots(STATE.base);
      STATE.errors = payload.errors;
      STATE.unauthorized = payload.unauthorized;
      updateSystems(payload.serviceStatus, payload.routeStats);
      updateMemory(payload.telemetry, payload.memoryRecent);
      updateApprovals(payload.stagingPending, payload.stagingRecent);
      updateQuarantine(payload.memoryQuarantine);
      updateSafety(payload.serviceStatus, payload.guardrails);
      updateAutonomy(payload.autonomy);
      updateFeedback(payload.feedbackState);
      updateConnections(payload.cluster);
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

    const [
      serviceStatus,
      routeStats,
      stagingPending,
      telemetry,
      guardrails,
      memoryRecent,
      stagingRecent,
      autonomy,
      cluster,
      feedbackState,
      memoryQuarantine,
    ] = await Promise.all([
      safeJson('/state/service_status'),
      safeJson('/state/route_stats'),
      safeJson('/state/staging/actions?status=pending&limit=50'),
      safeJson('/state/training/telemetry'),
      safeJson('/state/guardrails_metrics'),
      safeJson('/state/memory/recent?limit=5'),
      safeJson('/state/staging/actions?limit=30'),
      safeJson('/state/autonomy/lanes'),
      safeJson('/state/cluster'),
      safeJson('/admin/feedback/state'),
      safeJson('/admin/memory/quarantine'),
    ]);

    return {
      serviceStatus,
      routeStats,
      stagingPending,
      telemetry,
      guardrails,
      memoryRecent,
      stagingRecent,
      autonomy,
      cluster,
      feedbackState,
      memoryQuarantine,
      errors,
      unauthorized,
    };
  }

  function startAutoRefreshLoops(){
    stopAutoRefreshLoops();
    const approvalsLoop = () => {
      if (!document.hidden) refreshApprovalsLane(true);
    };
    const feedbackLoop = () => {
      if (!document.hidden) refreshFeedbackDelta(true);
    };
    const connectionsLoop = () => {
      if (!document.hidden) refreshConnectionsSnapshot({ auto: true });
    };
    const autonomyLoop = () => {
      if (!document.hidden) refreshAutonomySnapshot(true);
    };
    const quarantineLoop = () => {
      if (!document.hidden) refreshQuarantineLane(true);
    };
    if (AUTO_REFRESH_INTERVALS.approvals > 0) {
      approvalsTimer = setInterval(approvalsLoop, AUTO_REFRESH_INTERVALS.approvals);
    }
    if (AUTO_REFRESH_INTERVALS.feedback > 0) {
      feedbackTimer = setInterval(feedbackLoop, AUTO_REFRESH_INTERVALS.feedback);
    }
    if (AUTO_REFRESH_INTERVALS.connections > 0) {
      connectionsTimer = setInterval(connectionsLoop, AUTO_REFRESH_INTERVALS.connections);
    }
    if (AUTO_REFRESH_INTERVALS.autonomy > 0) {
      autonomyTimer = setInterval(autonomyLoop, AUTO_REFRESH_INTERVALS.autonomy);
    }
    if (AUTO_REFRESH_INTERVALS.quarantine > 0) {
      quarantineTimer = setInterval(quarantineLoop, AUTO_REFRESH_INTERVALS.quarantine);
    }
    if (!visibilityHandlerAttached) {
      document.addEventListener('visibilitychange', () => {
        if (!document.hidden) {
          approvalsLoop();
          feedbackLoop();
          connectionsLoop();
          autonomyLoop();
          quarantineLoop();
        }
      });
      visibilityHandlerAttached = true;
    }
  }

  function stopAutoRefreshLoops(){
    if (approvalsTimer) {
      clearInterval(approvalsTimer);
      approvalsTimer = null;
    }
    if (feedbackTimer) {
      clearInterval(feedbackTimer);
      feedbackTimer = null;
    }
    if (connectionsTimer) {
      clearInterval(connectionsTimer);
      connectionsTimer = null;
    }
    if (autonomyTimer) {
      clearInterval(autonomyTimer);
      autonomyTimer = null;
    }
    if (quarantineTimer) {
      clearInterval(quarantineTimer);
      quarantineTimer = null;
    }
  }

async function refreshApprovalsLane(auto = false){
    if (approvalsInflight || (auto && STATE.approvals.loading)) return;
    if (!STATE.base) {
      try {
        const port = ARW.getPortFromInput('port');
        STATE.base = ARW.base(port);
      } catch {}
    }
    if (!STATE.base) {
      if (!auto) ARW.toast('Start the server first');
      return;
    }
    approvalsInflight = true;
    try {
      const { pending, recent, errors, unauthorized } = await fetchStagingSnapshot(STATE.base);
      if (unauthorized) STATE.unauthorized = true;
      updateApprovals(pending, recent, { errors });
      if (auto && errors.length) {
        console.debug('Approvals auto-refresh warnings', errors);
      }
    } catch (err) {
      console.error('Approvals auto-refresh failed', err);
    } finally {
      approvalsInflight = false;
    }
  }

  async function refreshFeedbackDelta(auto = false){
    if (feedbackInflight || (auto && STATE.feedback.loading)) return;
    if (!STATE.base) {
      try {
        const port = ARW.getPortFromInput('port');
        STATE.base = ARW.base(port);
      } catch {}
    }
    if (!STATE.base) {
      if (!auto) ARW.toast('Start the server first');
      return;
    }

    if (!auto) {
      STATE.feedback.loading = true;
      renderFeedbackDelta();
    }

    feedbackInflight = true;
    try {
      const { data, error, unauthorized } = await safeJsonWithErrors(STATE.base, '/admin/feedback/state');
      if (unauthorized) STATE.unauthorized = true;
      STATE.feedback.loading = false;
      if (data && typeof data === 'object') {
        STATE.feedback.autoApply = !!data.auto_apply;
        const rawLog = Array.isArray(data.delta_log) ? data.delta_log : [];
        STATE.feedback.delta = rawLog
          .slice()
          .sort((a, b) => (Number(b?.version ?? 0) || 0) - (Number(a?.version ?? 0) || 0));
        const latest = STATE.feedback.delta[0];
        STATE.feedback.lastVersion = latest && typeof latest.version !== 'undefined' ? latest.version : null;
        STATE.feedback.updatedMs = latest ? parseTimestamp(latest.generated || latest.time || latest.ts_ms) : null;
        STATE.feedback.error = null;
      } else {
        STATE.feedback.delta = [];
        STATE.feedback.lastVersion = null;
        STATE.feedback.updatedMs = null;
        if (data && typeof data === 'object' && 'auto_apply' in data) {
          STATE.feedback.autoApply = !!data.auto_apply;
        }
        STATE.feedback.error = error || STATE.feedback.error;
      }
      renderFeedbackDelta();
      if (auto && error) {
        console.debug('Feedback delta auto-refresh warning', error);
      }
    } catch (err) {
      console.error('Feedback delta refresh failed', err);
      STATE.feedback.loading = false;
      STATE.feedback.error = err?.message || 'Failed to refresh feedback delta log';
      renderFeedbackDelta();
    } finally {
      feedbackInflight = false;
    }
  }

  async function refreshQuarantineLane(auto = false){
    if (quarantineInflight || (auto && STATE.quarantine.loading)) return;
    if (!STATE.base) {
      try {
        const port = ARW.getPortFromInput('port');
        STATE.base = ARW.base(port);
      } catch {}
    }
    if (!STATE.base) {
      if (!auto) ARW.toast('Start the server first');
      return;
    }

    if (!auto) {
      STATE.quarantine.loading = true;
      renderQuarantineLane();
    }

    quarantineInflight = true;
    try {
      const snapshot = await fetchQuarantineSnapshot(STATE.base);
      if (snapshot.unauthorized) STATE.unauthorized = true;
      updateQuarantine(snapshot.entries, {
        error: snapshot.errors[0] || null,
        errors: snapshot.errors,
        loading: false,
      });
      if (auto && snapshot.errors.length) {
        console.debug('Quarantine auto-refresh warnings', snapshot.errors);
      }
    } catch (err) {
      console.error('Quarantine refresh failed', err);
      updateQuarantine([], {
        error: err?.message || 'Failed to refresh memory quarantine',
        errors: [],
        loading: false,
      });
    } finally {
      quarantineInflight = false;
    }
  }

  async function refreshConnectionsSnapshot({ auto = false, showLoading = false } = {}){
    if (connectionsInflight || (STATE.connections.loading && showLoading)) return;
    if (!STATE.base) {
      try {
        const port = ARW.getPortFromInput('port');
        STATE.base = ARW.base(port);
      } catch {}
    }
    if (!STATE.base) {
      if (!auto) ARW.toast('Start the server first');
      return;
    }
    if (showLoading) {
      STATE.connections.loading = true;
      renderConnections();
    }
    connectionsInflight = true;
    try {
      const { payload, errors, unauthorized } = await fetchClusterSnapshot(STATE.base);
      if (unauthorized) STATE.unauthorized = true;
      updateConnections(payload, { errors, preservePrevious: auto });
      if (auto && errors.length) {
        console.debug('Connections auto-refresh warnings', errors);
      }
    } catch (err) {
      console.error('Connections refresh failed', err);
      updateConnections(null, { errors: [`/state/cluster: ${err?.message || 'unknown error'}`], preservePrevious: auto });
    } finally {
      connectionsInflight = false;
    }
  }

  async function refreshAutonomySnapshot(auto = false){
    if (autonomyInflight) return;
    if (!STATE.base) {
      try {
        const port = ARW.getPortFromInput('port');
        STATE.base = ARW.base(port);
      } catch {}
    }
    if (!STATE.base) {
      if (!auto) ARW.toast('Start the server first');
      return;
    }
    autonomyInflight = true;
    try {
      const { payload, errors, unauthorized } = await fetchAutonomySnapshot(STATE.base);
      if (unauthorized) STATE.unauthorized = true;
      if (payload) {
        updateAutonomy(payload);
      }
      if (auto && errors.length) {
        console.debug('Autonomy auto-refresh warnings', errors);
      }
    } catch (err) {
      console.error('Autonomy auto-refresh failed', err);
    } finally {
      autonomyInflight = false;
    }
  }

  async function fetchStagingSnapshot(base){
    const [pendingRes, recentRes] = await Promise.all([
      safeJsonWithErrors(base, '/state/staging/actions?status=pending&limit=50'),
      safeJsonWithErrors(base, '/state/staging/actions?limit=30'),
    ]);
    const errors = [];
    if (pendingRes.error) errors.push(pendingRes.error);
    if (recentRes.error) errors.push(recentRes.error);
    return {
      pending: pendingRes.data,
      recent: recentRes.data,
      errors,
      unauthorized: pendingRes.unauthorized || recentRes.unauthorized,
    };
  }

  async function fetchClusterSnapshot(base){
    const result = await safeJsonWithErrors(base, '/state/cluster');
    const errors = result.error ? [result.error] : [];
    return {
      payload: result.data,
      errors,
      unauthorized: result.unauthorized,
    };
  }

  async function fetchAutonomySnapshot(base){
    const [lanesRes, statsRes] = await Promise.all([
      safeJsonWithErrors(base, '/state/autonomy/lanes'),
      safeJsonWithErrors(base, '/state/route_stats'),
    ]);
    const errors = [];
    if (lanesRes.error) errors.push(lanesRes.error);
    if (statsRes.error) errors.push(statsRes.error);
    const interruptCounts = extractAutonomyInterrupts(statsRes);
    const payload = lanesRes.data ? { ...lanesRes.data, interruptCounts } : null;
    return {
      payload,
      errors,
      unauthorized: lanesRes.unauthorized || statsRes.unauthorized,
    };
  }

  async function fetchQuarantineSnapshot(base){
    const result = await safeJsonWithErrors(base, '/admin/memory/quarantine');
    const entries = Array.isArray(result.data) ? result.data : [];
    const errors = result.error ? [result.error] : [];
    return {
      entries,
      errors,
      unauthorized: result.unauthorized,
    };
  }

  async function safeJsonWithErrors(base, path){
    try {
      const data = await ARW.http.json(base, path);
      return { data, error: null, unauthorized: false };
    } catch (err) {
      const msg = err && err.message ? String(err.message) : 'unknown error';
      const unauthorized = /401/.test(msg);
      return { data: null, error: `${path}: ${msg}`, unauthorized };
    }
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

    const healthState = ARW.runtime.state(lastHealth.status || lastHealth.state);
    const healthStatus = healthState.slug;
    if (safe) {
      level = 'bad';
      summary = 'Safe mode engaged';
    } else if (healthStatus && healthStatus !== 'ok' && healthStatus !== 'healthy') {
      if (healthStatus === 'degraded') {
        level = 'warn';
      } else if (healthStatus === 'ready') {
        level = 'ok';
      } else if (healthStatus === 'unknown') {
        level = 'warn';
      } else {
        level = 'bad';
      }
      const label = healthState.label || healthStatus;
      summary = `Health ${label}`;
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

    const needsMore = asFiniteNumber(coverage.needs_more_ratio ?? coverage.needsMoreRatio);
    const riskRatio = asFiniteNumber(recall.at_risk_ratio ?? recall.atRiskRatio);
    const avgScore = asFiniteNumber(recall.avg_score ?? recall.avgScore);
    const latestScore = asFiniteNumber(recallLatest.score);
    const latestLevel = typeof recallLatest.level === 'string' ? recallLatest.level : '';

    let level = 'ok';
    let summary = 'Context coverage steady';
    const needsMoreBad = Number.isFinite(needsMore) && needsMore > 0.25;
    const needsMoreWarn = Number.isFinite(needsMore) && needsMore > 0.0;
    const riskBad = Number.isFinite(riskRatio) && riskRatio > 0.25;
    const riskWarn = Number.isFinite(riskRatio) && riskRatio > 0.0;

    if (needsMoreBad || riskBad) {
      level = 'bad';
      summary = 'Context underfilled';
    } else if (needsMoreWarn || riskWarn) {
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
    if (Number.isFinite(needsMore)) meta.push(['Needs more ratio', percentLabel(needsMore)]);
    if (Number.isFinite(riskRatio)) meta.push(['Recall risk', percentLabel(riskRatio)]);
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

    const coverageTopSlots = Array.isArray(coverage.top_slots) ? coverage.top_slots : [];
    if (coverageTopSlots.length) {
      const highlight = coverageTopSlots
        .slice(0, 2)
        .map((item) => {
          const slotLabel = formatSlotName(item?.slot);
          const gaps = formatCountLabel(item?.count, 'gap');
          return `${slotLabel} · ${gaps}`;
        })
        .join(' • ');
      if (highlight) meta.push(['Coverage slots', highlight]);
    }

    const recallTopSlots = Array.isArray(recall.top_slots) ? recall.top_slots : [];
    if (recallTopSlots.length) {
      const highlight = recallTopSlots
        .slice(0, 2)
        .map((item) => {
          const slotLabel = formatSlotName(item?.slot);
          const avg = percentLabel(item?.avg_gap);
          const max = percentLabel(item?.max_gap);
          const samples = formatCountLabel(item?.samples, 'sample');
          return `${slotLabel} · avg ${avg} · max ${max} · ${samples}`;
        })
        .join(' • ');
      if (highlight) meta.push(['Recall slots', highlight]);
    }

    const capsules = telemetry?.capsules || {};
    if (capsules.accessible_summary) {
      meta.push(['Policy capsules', capsules.accessible_summary]);
    }
    const nextExpiryMs = toNumber(capsules.next_expiry_ms ?? capsules.nextExpiryMs);
    if (Number.isFinite(nextExpiryMs)) {
      const label = typeof capsules.next_expiry_label === 'string' ? `${capsules.next_expiry_label} · ` : '';
      meta.push(['Next capsule expiry', `${label}${formatRelative(nextExpiryMs)} (${formatRelativeAbs(nextExpiryMs)})`]);
    }

    STATE.memory = { level, summary, meta };
    if (Number.isFinite(generatedMs)) {
      STATE.memory.generatedMs = generatedMs;
    } else {
      delete STATE.memory.generatedMs;
    }
    setStatus('memory', level, summary);
    setTile('memory', STATE.memory);

    const coverageValue = document.getElementById('memoryCoverageValue');
    if (coverageValue) coverageValue.textContent = percentLabel(needsMore);
    updateMeter('memoryCoverageBar', needsMore, {
      preferLow: true,
      warn: 0.2,
      bad: 0.4,
      formatText: (_value, pct) => `${pct}% needing more coverage`,
    });

    const recallValue = document.getElementById('memoryRecallValue');
    if (recallValue) recallValue.textContent = percentLabel(riskRatio);
    updateMeter('memoryRecallBar', riskRatio, {
      preferLow: true,
      warn: 0.2,
      bad: 0.4,
      formatText: (_value, pct) => `${pct}% flagged at risk`,
    });
  }

  function updateApprovals(pendingPayload, recentPayload, opts = {}){
    const errorList = errorsForPath('/state/staging/actions', opts);
    const errorMsg = errorList.length ? errorList.join('; ') : null;
    let pending = STATE.approvals.pending || [];
    if (Array.isArray(pendingPayload?.items)) {
      pending = pendingPayload.items.filter(it => !it.status || String(it.status).toLowerCase() === 'pending');
    }
    let recent = STATE.approvals.recent || [];
    if (Array.isArray(recentPayload?.items)) {
      recent = recentPayload.items;
    }

    if (!Array.isArray(pendingPayload?.items) && !Array.isArray(recentPayload?.items) && !pending.length && !recent.length) {
      const summary = STATE.unauthorized
        ? 'Authorize to view approvals queue'
        : errorMsg || 'Approvals queue unavailable';
      STATE.approvals.level = 'unknown';
      STATE.approvals.summary = summary;
      STATE.approvals.meta = [];
      STATE.approvals.pending = [];
      STATE.approvals.recent = [];
      STATE.approvals.generatedMs = null;
      STATE.approvals.loading = false;
      STATE.approvals.error = errorMsg;
      setStatus('approvals', 'unknown', summary);
      setTile('approvals', { level: 'unknown', summary, meta: [] });
      renderApprovalsLane();
      return;
    }

    const count = pending.length;
    let level = 'ok';
    let summary = 'No approvals waiting';
    if (STATE.unauthorized) {
      level = 'unknown';
      summary = 'Authorize to view approvals queue';
    } else if (count > 0) {
      summary = `${count} approval${count === 1 ? '' : 's'} waiting`;
      if (count > 3) level = 'bad'; else level = 'warn';
    } else if (errorMsg && !pendingPayload) {
      level = 'unknown';
      summary = 'Approvals queue unavailable';
    }

    const meta = [];
    if (count) {
      const oldestTs = pending
        .map(it => parseTimestamp(it.created_ms || it.created_at || it.created))
        .filter(Boolean)
        .sort((a, b) => a - b)[0];
      if (oldestTs) meta.push(['Oldest request', formatRelativeWithAbs(oldestTs)]);
      const projects = new Set(
        pending
          .map(it => (it.project || '').toString().trim())
          .filter(Boolean)
      );
      if (projects.size) meta.push(['Projects', Array.from(projects).join(', ')]);
      const requesters = new Set(
        pending
          .map(it => (it.requested_by || '').toString().trim())
          .filter(Boolean)
      );
      if (requesters.size) meta.push(['Requested by', Array.from(requesters).join(', ')]);
    }

    setStatus('approvals', level, summary);
    setTile('approvals', { level, summary, meta });

    STATE.approvals.level = level;
    STATE.approvals.summary = summary;
    STATE.approvals.meta = meta;
    STATE.approvals.pending = pending;
    STATE.approvals.recent = recent;
    STATE.approvals.loading = false;
    STATE.approvals.error = errorMsg;

    const hasSnapshot = Array.isArray(pendingPayload?.items) || Array.isArray(recentPayload?.items);
    if (hasSnapshot) {
      let generatedMs = readGeneratedMs(pendingPayload);
      if (!Number.isFinite(generatedMs) || generatedMs == null) {
        generatedMs = readGeneratedMs(recentPayload);
      }
      if (!Number.isFinite(generatedMs) && (pending.length || recent.length)) {
        generatedMs = Date.now();
      }
      if (Number.isFinite(generatedMs)) {
        STATE.approvals.generatedMs = generatedMs;
      } else {
        STATE.approvals.generatedMs = null;
      }
    }

    renderApprovalsLane();
  }

  function updateSafety(status, guardrails){
    if (!guardrails) {
      const summary = STATE.unauthorized ? 'Authorize to read guardrail metrics' : 'Guardrail metrics unavailable';
      STATE.safety = { level: 'unknown', summary, meta: [], lastApplied: null };
      setStatus('safety', 'unknown', summary);
      setTile('safety', STATE.safety);
      return;
    }
    const metrics = guardrails || {};
    const cbOpen = Number(metrics.cb_open ?? metrics.cbOpen ?? 0) > 0;
    const httpErrors = Number(metrics.http_errors ?? metrics.httpErrors ?? 0);
    const retries = Number(metrics.retries ?? 0);
    const trips = Number(metrics.cb_trips ?? metrics.cbTrips ?? 0);
    const lastAppliedRaw = metrics.last_applied || metrics.lastApplied || null;
    let lastApplied = null;
    if (lastAppliedRaw && typeof lastAppliedRaw === 'object') {
      const preset = typeof lastAppliedRaw.preset === 'string' ? lastAppliedRaw.preset : null;
      const digest = typeof lastAppliedRaw.digest === 'string' ? lastAppliedRaw.digest : null;
      const path = typeof lastAppliedRaw.path === 'string' ? lastAppliedRaw.path : null;
      let appliedMs = toNumber(lastAppliedRaw.applied_ms ?? lastAppliedRaw.appliedMs);
      if (!Number.isFinite(appliedMs)) {
        const iso = typeof lastAppliedRaw.applied_iso === 'string' ? lastAppliedRaw.applied_iso : null;
        if (iso) {
          const parsed = parseTimestamp(iso);
          if (parsed) appliedMs = parsed;
        }
      }
      const appliedIso = typeof lastAppliedRaw.applied_iso === 'string' ? lastAppliedRaw.applied_iso : null;
      lastApplied = { preset, digest, path, appliedMs: Number.isFinite(appliedMs) ? appliedMs : null, appliedIso };
    }

    let level = 'ok';
    let summary = 'Guardrails stable';
    if (cbOpen) {
      level = 'bad';
      summary = 'Circuit breaker open';
    } else if (httpErrors > 0 || retries > 0) {
      level = 'warn';
      summary = 'Guardrails recovering';
    } else if (lastApplied?.preset) {
      summary = `Preset ${lastApplied.preset} steady`;
    }

    const meta = [
      ['Retries', retries.toString()],
      ['HTTP errors', httpErrors.toString()],
      ['CB trips', trips.toString()],
    ];

    if (status?.safe_mode?.active) {
      meta.push(['Safe mode', 'engaged']);
    }
    if (lastApplied?.preset) {
      meta.push(['Preset', lastApplied.preset]);
    }
    if (lastApplied?.appliedMs) {
      meta.push(['Applied', formatRelativeWithAbs(lastApplied.appliedMs)]);
    } else if (lastApplied?.appliedIso) {
      meta.push(['Applied at', lastApplied.appliedIso]);
    }

    STATE.safety = { level, summary, meta, lastApplied, metrics: { retries, httpErrors, trips, cbOpen } };
    setStatus('safety', level, summary);
    setTile('safety', STATE.safety);
  }

  function updateConnections(cluster, opts = {}){
    const preservePrevious = opts && typeof opts.preservePrevious === 'boolean' ? opts.preservePrevious : false;
    const errorList = errorsForPath('/state/cluster', opts);
    const errorMsg = errorList.length ? errorList.join('; ') : null;

    if (!cluster || !Array.isArray(cluster.nodes)) {
      if (!preservePrevious) {
        STATE.connections.nodes = [];
        STATE.connections.updatedMs = null;
      }
      const summary = STATE.unauthorized
        ? 'Authorize to view connections'
        : errorMsg || 'Connections data unavailable';
      STATE.connections.summary = summary;
      STATE.connections.error = errorMsg;
      STATE.connections.loading = false;
      renderConnections();
      return;
    }

    if (Array.isArray(cluster.nodes)) {
      const nodes = cluster.nodes
        .filter(Boolean)
        .map(raw => {
          const caps = raw && typeof raw.capabilities === 'object' && raw.capabilities !== null ? raw.capabilities : {};
          const os = typeof caps.os === 'string' ? caps.os : null;
          const arch = typeof caps.arch === 'string' ? caps.arch : null;
          const version = typeof caps.arw_version === 'string' ? caps.arw_version : null;
          const healthRaw = raw && raw.health ? String(raw.health).toLowerCase() : 'unknown';
          return {
            id: raw && raw.id ? String(raw.id) : (raw && raw.name ? String(raw.name) : 'node'),
            name: raw && raw.name ? String(raw.name) : null,
            role: raw && raw.role ? String(raw.role) : 'member',
            health: healthRaw || 'unknown',
            capabilities: { os, arch, version },
          };
        });
      nodes.sort((a, b) => (a.name || a.id).localeCompare(b.name || b.id));
      STATE.connections.nodes = nodes;

      let generatedMs = readGeneratedMs(cluster);
      if (!Number.isFinite(generatedMs)) {
        generatedMs = Date.now();
      }
      if (Number.isFinite(generatedMs)) {
        STATE.connections.updatedMs = generatedMs;
      }
    }

    const nodes = Array.isArray(STATE.connections.nodes) ? STATE.connections.nodes : [];
    const onlineCount = nodes.filter(node => node.health === 'ok').length;
    let summaryBase = nodes.length
      ? `${onlineCount}/${nodes.length} connection${nodes.length === 1 ? '' : 's'} online`
      : (STATE.unauthorized ? 'Authorize to view connections' : 'No remote connections');

    if (errorMsg) {
      summaryBase = errorMsg;
    }

    STATE.connections.summary = summaryBase;
    STATE.connections.error = errorMsg;
    STATE.connections.loading = false;
    renderConnections();
  }
  function updateAutonomy(payload){
    const currentOperator = STATE.autonomy?.operator || null;
    const fallbackSummary = STATE.unauthorized
      ? 'Authorize to manage the autonomy lane.'
      : 'No autonomy lane configured.';
    const previousCounts = STATE.autonomy?.interruptCounts || {};
    const interruptCounts = normalizeInterrupts(
      payload?.interruptCounts || payload?.stats?.autonomy?.interrupts,
    );
    const newInterruptCounts = diffInterrupts(previousCounts, interruptCounts);
    const newInterruptTotal = Object.values(newInterruptCounts).reduce((acc, value) => acc + value, 0);
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
        interruptCounts,
        newInterruptCounts: {},
        newInterrupts: 0,
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
        interruptCounts,
        newInterruptCounts: {},
        newInterrupts: 0,
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

    const laneSuffix = laneId ? ` (${laneId})` : '';
    let overviewLine = `${summary}${laneSuffix}`;
    if (newInterruptTotal > 0) {
      overviewLine = `Autonomy interrupts (${newInterruptTotal} new)${laneSuffix}`;
    }
    const interruptLine = formatInterruptSummary(interruptCounts);
    const newInterruptLine = newInterruptTotal > 0 ? formatInterruptSummary(newInterruptCounts) : null;
    const augmentedAlerts = newInterruptTotal > 0
      ? [...alerts, `${newInterruptTotal} new interrupt${newInterruptTotal === 1 ? '' : 's'} detected`]
      : alerts;
    if (newInterruptTotal > 0 && level === 'ok') {
      level = 'warn';
      summary = `Autonomy interrupts (${newInterruptTotal} new)`;
    }
    const extendedMeta = interruptLine
      ? [...meta, ['Interrupts', interruptLine]].concat(newInterruptLine ? [['New interrupts', newInterruptLine]] : [])
      : meta;

    STATE.autonomy = {
      level,
      summary,
      meta: extendedMeta,
      lane: laneId,
      snapshot: lane,
      line: overviewLine,
      operator: currentOperator,
      alerts: augmentedAlerts,
      interruptCounts,
      newInterruptCounts,
      newInterrupts: newInterruptTotal,
      updatedMs,
      lastEvent,
      lastReason,
    };

    setStatus('autonomy', level, summary);
    setTile('autonomy', { level, summary, meta: extendedMeta });
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
  const interruptsEl = document.getElementById('autonomyInterrupts');
  if (interruptsEl) {
    interruptsEl.innerHTML = '';
    const counts = STATE.autonomy?.interruptCounts || {};
    const entries = AUTONOMY_INTERRUPT_KEYS.map(key => [key, counts[key] || 0]);
    const totalInterrupts = entries.reduce((acc, [, value]) => acc + Number(value || 0), 0);
    if (!lane || !entries.some(([, value]) => value > 0)) {
      interruptsEl.classList.add('hidden');
    } else {
      interruptsEl.classList.remove('hidden');
      const header = document.createElement('li');
      header.className = 'interrupt-total';
      header.textContent = `Total interrupts: ${totalInterrupts}`;
      interruptsEl.appendChild(header);

      entries.forEach(([reason, value]) => {
        if (value <= 0) return;
        const li = document.createElement('li');
        const label = AUTONOMY_INTERRUPT_LABELS[reason] || reason.replace(/_/g, ' ');
        const delta = STATE.autonomy?.newInterruptCounts?.[reason] || 0;
        li.textContent = delta > 0
          ? `${label} — ${value} (+${delta})`
          : `${label} — ${value}`;
        if (delta > 0) {
          li.classList.add('is-new');
        }
        interruptsEl.appendChild(li);
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
    const stopBtn = document.getElementById('btn-autonomy-stop');
    if (stopBtn && stopBtn.dataset.autonomyBusy !== '1') {
      const activeJobs = toNumber(lane?.active_jobs ?? lane?.activeJobs) ?? 0;
      const queuedJobs = toNumber(lane?.queued_jobs ?? lane?.queuedJobs) ?? 0;
      const disabled = disableAll || (activeJobs === 0 && queuedJobs === 0);
      stopBtn.disabled = disabled;
      stopBtn.classList.toggle('danger', !disabled);
      stopBtn.textContent = 'Stop now';
      const title = disabled
        ? (disableAll
            ? (unauthorized ? 'Authorize to manage the autonomy lane.' : 'No autonomy lane configured.')
            : 'No autonomy jobs to flush')
        : 'Pause lane and flush jobs';
      stopBtn.title = title;
    }
  }

  function markAutonomyBusy(flag) {
    ['btn-autonomy-pause', 'btn-autonomy-resume', 'btn-autonomy-stop'].forEach(id => {
      const btn = document.getElementById(id);
      if (!btn) return;
      if (flag) {
        btn.dataset.autonomyBusy = '1';
        btn.disabled = true;
      } else {
        delete btn.dataset.autonomyBusy;
        if (id === 'btn-autonomy-stop') {
          btn.textContent = 'Stop now';
        }
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

  async function stopAutonomy() {
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
    const reason = promptAutonomyReason('Emergency stop (kill switch)');
    if (reason === null) return;
    const confirmStop = confirm('Immediately pause and flush all autonomy jobs?');
    if (!confirmStop) return;
    markAutonomyBusy(true);
    const stopBtn = document.getElementById('btn-autonomy-stop');
    if (stopBtn) {
      stopBtn.textContent = 'Stopping…';
      stopBtn.classList.add('danger');
    }
    try {
      const resp = await ARW.http.fetch(STATE.base, `/admin/autonomy/${encodeURIComponent(lane)}/stop`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ operator, reason }),
      });
      if (!resp.ok) throw new Error(`HTTP ${resp.status}`);
      ARW.toast('Autonomy stopped');
      await refresh();
    } catch (err) {
      console.error('Stop autonomy failed', err);
      ARW.toast(err && err.message ? err.message : 'Stop failed');
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
    const quarantineLine = STATE.quarantine.summary;
    const safetyLine = STATE.safety.summary;
    const autonomyLine = STATE.autonomy?.line || (STATE.unauthorized ? 'Authorize to view autonomy lane.' : 'Autonomy lane idle.');
    let connectionsLine = STATE.connections.summary || (STATE.unauthorized ? 'Authorize to view connections' : 'Connections idle.');
    if (STATE.connections.updatedMs) {
      connectionsLine = `${connectionsLine} (updated ${formatRelativeWithAbs(STATE.connections.updatedMs)})`;
    }

    let guardrailLine = safetyLine;
    if (STATE.safety.lastApplied?.preset) {
      const preset = STATE.safety.lastApplied.preset;
      const appliedMs = STATE.safety.lastApplied.appliedMs;
      const appliedText = appliedMs ? `applied ${formatRelativeWithAbs(appliedMs)}`
        : (STATE.safety.lastApplied.appliedIso ? `applied ${STATE.safety.lastApplied.appliedIso}` : 'applied recently');
      guardrailLine = `Guardrails preset ${preset} (${appliedText})`;
    }

    let guardrailMetricLine = null;
    if (STATE.safety.metrics) {
      const { retries = 0, httpErrors = 0, trips = 0, cbOpen = false } = STATE.safety.metrics;
      guardrailMetricLine = `Guardrail metrics – retries ${retries}, HTTP errors ${httpErrors}, trips ${trips}, circuit breaker ${cbOpen ? 'open' : 'closed'}.`;
    }

    const standupLine = 'Daily stand-up template lives in docs/ops/trials/standup_template.md (dashboard → approvals → highlights → risks → next steps).';
    const rollbackLine = 'Rollback drill: run scripts/autonomy_rollback.sh --dry-run and follow docs/ops/trials/autonomy_rollback_playbook.md.';
    const guardrailWorkflowLine = `Guardrail preset helper: scripts/trials_guardrails.sh --preset ${STATE.safety.lastApplied?.preset || 'trial'} (see docs/ops/trial_runbook.md).`;

    const topRoutes = Array.isArray(routeStats?.routes)
      ? [...routeStats.routes]
          .filter(r => typeof r?.path === 'string')
          .sort((a, b) => (Number(b?.p95_ms) || 0) - (Number(a?.p95_ms) || 0))
          .slice(0, 3)
          .map(r => `${r.path} | p95 ${(Number(r.p95_ms) || 0).toFixed(0)} ms (hits ${(Number(r.hits) || 0).toLocaleString()})`)
      : [];

    STATE.overview = [systemLine, memoryLine, approvalLine, quarantineLine, guardrailLine, autonomyLine, connectionsLine];
    STATE.workflows = [
      MEMORY_WORKFLOW_TEXT(STATE.memory, STATE.focus),
      approvalLine,
      standupLine,
      rollbackLine,
      (topRoutes.length ? `Slowest routes: ${topRoutes.join('; ')}` : 'Route latencies steady.'),
    ];
    STATE.workflows.push(guardrailWorkflowLine);
    STATE.safeguards = [
      guardrailLine,
      guardrailMetricLine || 'Guardrail metrics steady.',
      (STATE.systems.meta.find(([label]) => label === 'Safe mode')?.join(': ') || 'Safe mode off'),
      autonomyLine,
      connectionsLine,
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

  function updateFeedback(feedbackState){
    STATE.feedback.loading = false;
    const errorEntry = Array.isArray(STATE.errors)
      ? STATE.errors.find(msg => typeof msg === 'string' && msg.startsWith('/admin/feedback/state'))
      : null;
    STATE.feedback.error = errorEntry ? errorEntry.split(': ').slice(1).join(': ').trim() : null;
    if (feedbackState && typeof feedbackState === 'object') {
      STATE.feedback.autoApply = !!feedbackState.auto_apply;
      const rawLog = Array.isArray(feedbackState.delta_log) ? feedbackState.delta_log : [];
      STATE.feedback.delta = rawLog
        .slice()
        .sort((a, b) => (Number(b?.version ?? 0) || 0) - (Number(a?.version ?? 0) || 0));
      const latest = STATE.feedback.delta[0];
      STATE.feedback.lastVersion = latest && typeof latest.version !== 'undefined' ? latest.version : null;
      STATE.feedback.updatedMs = latest ? parseTimestamp(latest.generated || latest.time || latest.ts_ms) : null;
    } else {
      STATE.feedback.delta = [];
      if (feedbackState && typeof feedbackState === 'object' && 'auto_apply' in feedbackState) {
        STATE.feedback.autoApply = !!feedbackState.auto_apply;
      }
      STATE.feedback.lastVersion = null;
      STATE.feedback.updatedMs = null;
    }
    renderFeedbackDelta();
  }

  function renderFeedbackDelta(){
    const summaryEl = document.getElementById('feedbackDeltaSummary');
    const autoBadgeEl = document.getElementById('feedbackDeltaAutoBadge');
    const noticeEl = document.getElementById('feedbackDeltaNotice');
    const listEl = document.getElementById('feedbackDeltaList');
    const emptyEl = document.getElementById('feedbackDeltaEmpty');
    const refreshBtn = document.getElementById('btn-feedback-refresh');
    const openBtn = document.getElementById('btn-feedback-open-debug');

    if (refreshBtn) {
      refreshBtn.disabled = !STATE.base;
    }

    if (openBtn) {
      const disabled = !STATE.base || STATE.unauthorized;
      openBtn.disabled = disabled;
      openBtn.setAttribute('aria-disabled', disabled ? 'true' : 'false');
      openBtn.title = disabled
        ? 'Authorize with an admin token to open the feedback panel'
        : 'Open feedback panel in debug view';
    }

    if (autoBadgeEl) {
      autoBadgeEl.textContent = STATE.feedback.autoApply ? 'Auto-apply on' : 'Auto-apply off';
      autoBadgeEl.classList.toggle('on', STATE.feedback.autoApply);
      autoBadgeEl.classList.toggle('off', !STATE.feedback.autoApply);
    }

    if (summaryEl) {
      let summaryText;
      if (STATE.feedback.loading) {
        summaryText = 'Loading feedback delta log…';
      } else if (STATE.unauthorized) {
        summaryText = 'Authorize to view feedback delta log.';
      } else if (STATE.feedback.delta.length) {
        const latest = STATE.feedback.delta[0];
        const added = (latest.added || []).length;
        const removed = (latest.removed || []).length;
        const changed = (latest.changed || []).length;
        const parts = [];
        const version = typeof latest.version !== 'undefined' ? `v${latest.version}` : null;
        if (version) parts.push(version);
        const counts = [];
        if (added) counts.push(`+${added}`);
        if (removed) counts.push(`-${removed}`);
        if (changed) counts.push(`±${changed}`);
        if (counts.length) parts.push(counts.join(' / '));
        if (STATE.feedback.updatedMs) parts.push(`updated ${formatRelativeWithAbs(STATE.feedback.updatedMs)}`);
        summaryText = parts.join(' • ');
      } else if (STATE.feedback.error) {
        summaryText = 'Feedback delta log unavailable.';
      } else {
        summaryText = 'No feedback deltas captured yet.';
      }
      summaryEl.textContent = summaryText;
      summaryEl.title = STATE.feedback.updatedMs ? formatRelativeAbs(STATE.feedback.updatedMs) : '';
    }

    if (noticeEl) {
      if (STATE.feedback.error && !STATE.unauthorized) {
        noticeEl.textContent = STATE.feedback.error;
        noticeEl.classList.remove('hidden');
      } else {
        noticeEl.textContent = '';
        noticeEl.classList.add('hidden');
      }
    }

    if (!listEl || !emptyEl) return;

    if (STATE.feedback.loading) {
      listEl.innerHTML = '';
      emptyEl.textContent = 'Loading feedback delta log…';
      emptyEl.classList.remove('hidden');
      return;
    }

    if (STATE.unauthorized) {
      listEl.innerHTML = '';
      emptyEl.textContent = 'Authorize with an admin token to view feedback deltas.';
      emptyEl.classList.remove('hidden');
      return;
    }

    if (STATE.feedback.error) {
      listEl.innerHTML = '';
      emptyEl.textContent = STATE.feedback.error;
      emptyEl.classList.remove('hidden');
      return;
    }

    const deltas = STATE.feedback.delta;
    if (!deltas.length) {
      listEl.innerHTML = '';
      emptyEl.textContent = STATE.feedback.autoApply
        ? 'Auto-apply is enabled; no new deltas yet.'
        : 'No feedback deltas captured yet.';
      emptyEl.classList.remove('hidden');
      return;
    }

    emptyEl.classList.add('hidden');
    listEl.innerHTML = '';
    deltas.slice(0, 5).forEach(entry => {
      const node = buildFeedbackDeltaItem(entry);
      if (node) listEl.appendChild(node);
    });
  }

  function buildFeedbackDeltaItem(entry){
    if (!entry || typeof entry !== 'object') return null;
    const li = document.createElement('li');
    li.className = 'feedback-delta-item';

    const header = document.createElement('div');
    header.className = 'feedback-delta-header';

    const title = document.createElement('h3');
    title.className = 'feedback-delta-title';
    title.textContent = typeof entry.version !== 'undefined' ? `Version ${entry.version}` : 'Version —';
    header.appendChild(title);

    const generatedMs = parseTimestamp(entry.generated || entry.time || entry.generated_ms || entry.ts_ms);
    if (generatedMs) {
      const time = document.createElement('span');
      time.className = 'feedback-delta-time dim';
      time.textContent = formatRelativeWithAbs(generatedMs);
      time.title = formatRelativeAbs(generatedMs);
      header.appendChild(time);
    }

    const counts = document.createElement('div');
    counts.className = 'feedback-delta-counts';
    const added = Array.isArray(entry.added) ? entry.added.length : 0;
    const removed = Array.isArray(entry.removed) ? entry.removed.length : 0;
    const changed = Array.isArray(entry.changed) ? entry.changed.length : 0;
    if (added) counts.appendChild(buildDeltaCountPill(`+${added}`, 'added'));
    if (removed) counts.appendChild(buildDeltaCountPill(`-${removed}`, 'removed'));
    if (changed) counts.appendChild(buildDeltaCountPill(`±${changed}`, 'changed'));
    if (counts.childElementCount > 0) header.appendChild(counts);

    li.appendChild(header);

    if (added) {
      li.appendChild(buildDeltaGroup('Added', entry.added, 'added', formatSuggestionSummary));
    }
    if (removed) {
      li.appendChild(buildDeltaGroup('Removed', entry.removed, 'removed', formatSuggestionSummary));
    }
    if (changed) {
      li.appendChild(buildDeltaGroup('Changed', entry.changed, 'changed', summarizeChange));
    }

    return li;
  }

  function buildDeltaCountPill(text, variant){
    const span = document.createElement('span');
    span.className = `feedback-delta-pill ${variant}`;
    span.textContent = text;
    return span;
  }

  function buildDeltaGroup(label, items, variant, formatter){
    const block = document.createElement('div');
    block.className = `feedback-delta-group ${variant}`;

    const heading = document.createElement('h4');
    heading.textContent = `${label} (${items.length})`;
    block.appendChild(heading);

    const list = document.createElement('ul');
    list.className = 'feedback-delta-sublist';
    items.slice(0, 3).forEach(item => {
      const li = document.createElement('li');
      li.textContent = formatter(item);
      list.appendChild(li);
    });
    if (items.length > 3) {
      const more = document.createElement('li');
      more.className = 'dim';
      more.textContent = `+${items.length - 3} more…`;
      list.appendChild(more);
    }
    block.appendChild(list);
    return block;
  }

  function formatSuggestionSummary(summary){
    if (!summary || typeof summary !== 'object') return 'Suggestion';
    const id = summary.id || 'suggestion';
    const action = summary.action || 'action';
    const conf = typeof summary.confidence === 'number' && Number.isFinite(summary.confidence)
      ? ` (confidence ${formatConfidence(summary.confidence)})`
      : '';
    return `${id} · ${action}${conf}`;
  }

  function summarizeChange(change){
    if (!change || typeof change !== 'object') return 'Suggestion updated';
    const lines = [];
    const before = change.before || {};
    const after = change.after || {};
    const beforeParams = (before && typeof before === 'object' ? before.params : null) || {};
    const afterParams = (after && typeof after === 'object' ? after.params : null) || {};
    const keys = new Set([
      ...Object.keys(beforeParams),
      ...Object.keys(afterParams),
    ]);
    keys.forEach(key => {
      const beforeVal = formatParam(beforeParams[key]);
      const afterVal = formatParam(afterParams[key]);
      if (beforeVal !== afterVal) {
        lines.push(`${key}: ${beforeVal} → ${afterVal}`);
      }
    });
    const beforeConf = formatConfidence(before.confidence);
    const afterConf = formatConfidence(after.confidence);
    if (beforeConf !== afterConf) {
      lines.push(`confidence: ${beforeConf} → ${afterConf}`);
    }
    const beforeRat = (before && before.rationale) || null;
    const afterRat = (after && after.rationale) || null;
    if (beforeRat !== afterRat) {
      lines.push('rationale updated');
    }
    const summary = formatSuggestionSummary(after);
    if (!lines.length) return summary;
    return `${summary} — ${lines.slice(0, 2).join('; ')}`;
  }

  function formatConfidence(value){
    if (typeof value !== 'number' || !Number.isFinite(value)) return '—';
    return value.toFixed(2);
  }

  function formatParam(value){
    if (value == null) return '—';
    if (typeof value === 'number') {
      if (!Number.isFinite(value)) return '—';
      return value % 1 === 0 ? value.toString() : value.toFixed(2);
    }
    if (typeof value === 'boolean') {
      return value ? 'true' : 'false';
    }
    if (typeof value === 'string') {
      return value.length > 40 ? `${value.slice(0, 37)}…` : value;
    }
    try {
      const json = JSON.stringify(value);
      if (!json) return '—';
      return json.length > 40 ? `${json.slice(0, 37)}…` : json;
    } catch {
      return '—';
    }
  }

  function renderApprovalsLane(){
    const summaryEl = document.getElementById('approvalsLaneSummary');
    const listEl = document.getElementById('approvalsLaneList');
    const emptyEl = document.getElementById('approvalsLaneEmpty');
    const noticeEl = document.getElementById('approvalsLaneNotice');
    const recentWrap = document.getElementById('approvalsRecent');
    const recentList = document.getElementById('approvalsRecentList');

    const { summary, reviewer, generatedMs, pending, recent, loading, error } = STATE.approvals;

    if (summaryEl) {
      let line = summary || '';
      if (reviewer) line = line ? `${line} • Reviewer: ${reviewer}` : `Reviewer: ${reviewer}`;
      if (generatedMs) line = line ? `${line} • updated ${formatRelativeWithAbs(generatedMs)}` : `Updated ${formatRelativeWithAbs(generatedMs)}`;
      summaryEl.textContent = line;
      summaryEl.title = generatedMs ? formatRelativeAbs(generatedMs) : '';
    }

    if (noticeEl) {
      if (error && !STATE.unauthorized) {
        noticeEl.textContent = error;
        noticeEl.classList.remove('hidden');
      } else {
        noticeEl.textContent = '';
        noticeEl.classList.add('hidden');
      }
    }

    if (listEl) {
      listEl.innerHTML = '';
      if (loading) {
        if (emptyEl) {
          emptyEl.textContent = 'Loading approvals…';
          emptyEl.classList.remove('hidden');
        }
      } else if (!pending || pending.length === 0) {
        if (emptyEl) {
          emptyEl.textContent = summary || (STATE.unauthorized ? 'Authorize to view approvals queue' : 'No approvals waiting');
          emptyEl.classList.remove('hidden');
        }
      } else {
        if (emptyEl) emptyEl.classList.add('hidden');
        pending.slice(0, 12).forEach(item => {
          const card = buildApprovalCard(item);
          if (card) listEl.appendChild(card);
        });
      }
    }

    if (recentWrap && recentList) {
      recentList.innerHTML = '';
      const decided = Array.isArray(recent)
        ? recent.filter(it => it && String(it.status || '').toLowerCase() !== 'pending').slice(0, 8)
        : [];
      if (!decided.length) {
        recentWrap.classList.add('hidden');
      } else {
        recentWrap.classList.remove('hidden');
        decided.forEach(item => {
          const li = document.createElement('li');
          const createdMs = parseTimestamp(item.decided_at || item.updated || item.created);
          const status = item.status ? humanizeStatus(item.status) : 'Decided';
          const actor = item.decided_by || item.requested_by || 'unknown';
          const kind = humanizeActionKind(item.action_kind);
          const parts = [status, `by ${actor}`];
          li.textContent = `${kind} — ${parts.join(' ')}${createdMs ? ` (${formatRelativeWithAbs(createdMs)})` : ''}`;
          if (createdMs) li.title = formatRelativeAbs(createdMs);
          recentList.appendChild(li);
        });
      }
    }
  }

  function updateQuarantine(payload, opts = {}){
    const errorsSource = Array.isArray(opts?.errors) ? opts.errors : undefined;
    const pathErrors = errorsForPath('/admin/memory/quarantine', { errors: errorsSource });
    const explicitError = opts.error || (pathErrors.length ? pathErrors[0] : null);
    const sanitizedError = explicitError
      ? String(explicitError).replace('/admin/memory/quarantine: ', '')
      : null;
    const loading = opts.loading === true;

    let entries = [];
    if (Array.isArray(payload)) {
      entries = payload.filter(Boolean);
    } else if (Array.isArray(payload?.items)) {
      entries = payload.items.filter(Boolean);
    }

    entries = entries
      .map(item => (item && typeof item === 'object' ? item : null))
      .filter(Boolean)
      .sort((a, b) => {
        const ta = parseTimestamp(a?.time || a?.created || a?.generated_at || a?.generated);
        const tb = parseTimestamp(b?.time || b?.created || b?.generated_at || b?.generated);
        const da = Number.isFinite(ta) ? ta : 0;
        const db = Number.isFinite(tb) ? tb : 0;
        return db - da;
      });

    const counts = entries.reduce((map, entry) => {
      const state = String(entry.state || 'unknown').toLowerCase();
      map[state] = (map[state] || 0) + 1;
      return map;
    }, {});

    const total = entries.length;
    const newestMs = entries.reduce((latest, entry) => {
      const ts = parseTimestamp(entry.time || entry.created || entry.generated_at || entry.generated);
      if (!Number.isFinite(ts)) return latest;
      if (!Number.isFinite(latest) || ts > latest) return ts;
      return latest;
    }, null);

    const trackedStates = ['queued', 'needs_extractor', 'admitted', 'rejected'];
    const summaryParts = [];
    if (total) summaryParts.push(`Total ${total}`);
    trackedStates.forEach((state) => {
      if (counts[state]) summaryParts.push(`${humanizeQuarantineState(state)} ${counts[state]}`);
    });

    let summary;
    if (total) {
      summary = summaryParts.join(' • ') || `Total ${total}`;
      if (Number.isFinite(newestMs)) summary += ` • updated ${formatRelativeWithAbs(newestMs)}`;
    } else if (STATE.unauthorized) {
      summary = 'Authorize to view memory quarantine queue';
    } else if (explicitError) {
      summary = 'Memory quarantine unavailable';
    } else {
      summary = 'No items in memory quarantine';
    }

    STATE.quarantine.entries = entries.slice(0, 12);
    STATE.quarantine.total = total;
    STATE.quarantine.counts = counts;
    STATE.quarantine.summary = summary;
    STATE.quarantine.generatedMs = Number.isFinite(newestMs) ? newestMs : null;
    STATE.quarantine.loading = loading;
    STATE.quarantine.error = STATE.unauthorized ? null : sanitizedError;

    renderQuarantineLane();
  }

  function renderQuarantineLane(){
    const summaryEl = document.getElementById('quarantineSummary');
    const listEl = document.getElementById('quarantineList');
    const emptyEl = document.getElementById('quarantineEmpty');
    const noticeEl = document.getElementById('quarantineNotice');

    if (summaryEl) {
      summaryEl.textContent = STATE.quarantine.summary || '';
      summaryEl.title = STATE.quarantine.generatedMs
        ? formatRelativeAbs(STATE.quarantine.generatedMs)
        : '';
    }

    if (noticeEl) {
      const error = STATE.quarantine.error;
      if (error && !STATE.unauthorized) {
        noticeEl.textContent = error;
        noticeEl.classList.remove('hidden');
      } else {
        noticeEl.textContent = '';
        noticeEl.classList.add('hidden');
      }
    }

    if (!listEl || !emptyEl) return;

    listEl.innerHTML = '';
    if (STATE.quarantine.loading) {
      emptyEl.textContent = 'Loading memory quarantine…';
      emptyEl.classList.remove('hidden');
      return;
    }

    if (!STATE.quarantine.entries.length) {
      emptyEl.textContent = STATE.quarantine.summary || 'No items in memory quarantine';
      emptyEl.classList.remove('hidden');
      return;
    }

    emptyEl.classList.add('hidden');
    STATE.quarantine.entries.forEach(entry => {
      const card = buildQuarantineCard(entry);
      if (card) listEl.appendChild(card);
    });
  }

  function buildQuarantineCard(entry){
    if (!entry || typeof entry !== 'object') return null;
    const li = document.createElement('li');
    li.className = 'quarantine-card';
    if (entry.id) li.dataset.quarantineId = String(entry.id);

    const headline = document.createElement('div');
    headline.className = 'quarantine-headline';

    const statePill = document.createElement('span');
    const stateSlug = String(entry.state || 'unknown').toLowerCase();
    statePill.className = `pill pill-small state-${stateSlug}`;
    statePill.textContent = humanizeQuarantineState(stateSlug);
    headline.appendChild(statePill);

    const scoreSpan = document.createElement('span');
    scoreSpan.className = 'quarantine-score';
    scoreSpan.textContent = `Score ${formatEvidenceScore(entry.evidence_score)}`;
    headline.appendChild(scoreSpan);

    const ts = parseTimestamp(entry.time || entry.created || entry.generated_at || entry.generated);
    if (Number.isFinite(ts)) {
      const timeSpan = document.createElement('span');
      timeSpan.className = 'quarantine-time';
      timeSpan.textContent = formatRelativeWithAbs(ts);
      timeSpan.title = formatRelativeAbs(ts);
      headline.appendChild(timeSpan);
    }

    li.appendChild(headline);

    const metaRow = document.createElement('div');
    metaRow.className = 'quarantine-meta';
    const source = formatQuarantineSource(entry.source);
    if (source) metaRow.appendChild(createMetaChip(`Source ${source}`));
    const project = entry.project_id || entry.project;
    if (project) metaRow.appendChild(createMetaChip(`Project ${project}`));
    if (Array.isArray(entry.risk_markers) && entry.risk_markers.length) {
      metaRow.appendChild(createMetaChip(`Markers ${entry.risk_markers.join(', ')}`));
    }
    if (entry.provenance) {
      metaRow.appendChild(createMetaChip(truncatePayload(entry.provenance, 80)));
    }
    li.appendChild(metaRow);

    const previewText = typeof entry.content_preview === 'string' ? entry.content_preview.trim() : '';
    if (previewText) {
      const preview = document.createElement('p');
      preview.className = 'quarantine-preview';
      preview.textContent = truncatePayload(previewText, 220);
      li.appendChild(preview);
    }

    if (entry.review && typeof entry.review === 'object') {
      const review = entry.review;
      const reviewLineParts = [];
      if (review.decision) {
        reviewLineParts.push(`Decision ${humanizeQuarantineState(review.decision)}`);
      }
      if (review.by) reviewLineParts.push(`By ${review.by}`);
      if (review.time) {
        const reviewMs = parseTimestamp(review.time);
        reviewLineParts.push(`At ${reviewMs ? formatRelativeWithAbs(reviewMs) : review.time}`);
      }
      if (review.note) reviewLineParts.push(`Note: ${review.note}`);
      if (reviewLineParts.length) {
        const reviewLine = document.createElement('div');
        reviewLine.className = 'quarantine-review';
        reviewLine.textContent = reviewLineParts.join(' • ');
        li.appendChild(reviewLine);
      }
    }

    return li;
  }

  function humanizeQuarantineState(value){
    if (!value) return 'Unknown';
    const normalized = String(value).toLowerCase();
    switch (normalized) {
      case 'queued':
        return 'Queued';
      case 'needs_extractor':
        return 'Needs extractor';
      case 'admitted':
        return 'Admitted';
      case 'reject':
      case 'rejected':
        return 'Rejected';
      case 'extract_again':
        return 'Extract again';
      default:
        return normalized.replace(/_/g, ' ').replace(/\b\w/g, ch => ch.toUpperCase());
    }
  }

  function formatQuarantineSource(value){
    if (!value) return '';
    const normalized = String(value).toLowerCase();
    switch (normalized) {
      case 'tool':
        return 'Tool';
      case 'ingest':
        return 'Ingest';
      case 'world_diff':
        return 'World diff';
      case 'manual':
        return 'Manual';
      default:
        return normalized.replace(/_/g, ' ').replace(/\b\w/g, ch => ch.toUpperCase());
    }
  }

  function formatEvidenceScore(value){
    if (typeof value !== 'number' || !Number.isFinite(value)) return '—';
    return value.toFixed(2);
  }

  function buildApprovalCard(item){
    if (!item || typeof item !== 'object') return null;
    const li = document.createElement('li');
    li.className = 'approval-card';
    if (item.id) li.dataset.approvalId = String(item.id);

    const headline = document.createElement('div');
    headline.className = 'approval-headline';
    const title = document.createElement('h3');
    title.className = 'approval-kind';
    title.textContent = humanizeActionKind(item.action_kind);
    headline.appendChild(title);
    if (item.project) {
      const project = document.createElement('span');
      project.className = 'approval-project';
      project.textContent = item.project;
      headline.appendChild(project);
    }
    li.appendChild(headline);

    const metaRow = document.createElement('div');
    metaRow.className = 'approval-meta';
    const createdMs = parseTimestamp(item.created_ms || item.created_at || item.created);
    if (createdMs) metaRow.appendChild(createMetaChip(`Created ${formatRelative(createdMs)}`));
    if (item.requested_by) metaRow.appendChild(createMetaChip(`Requested by ${item.requested_by}`));
    if (item.project) metaRow.appendChild(createMetaChip(`Project ${item.project}`));
    metaRow.appendChild(createMetaChip(`ID ${item.id || 'unknown'}`));
    li.appendChild(metaRow);

    const payloadText = formatActionInput(item.action_input);
    if (payloadText) {
      const pre = document.createElement('pre');
      pre.className = 'approval-payload';
      pre.textContent = payloadText;
      li.appendChild(pre);
    }

    const actionsRow = document.createElement('div');
    actionsRow.className = 'approval-actions';
    const approveBtn = document.createElement('button');
    approveBtn.type = 'button';
    approveBtn.className = 'primary';
    approveBtn.textContent = 'Approve';
    const holdBtn = document.createElement('button');
    holdBtn.type = 'button';
    holdBtn.className = 'danger';
    holdBtn.textContent = 'Hold';
    approveBtn.addEventListener('click', () => decideApproval(item, 'approve', approveBtn, holdBtn));
    holdBtn.addEventListener('click', () => decideApproval(item, 'deny', approveBtn, holdBtn));
    actionsRow.appendChild(approveBtn);
    actionsRow.appendChild(holdBtn);
    li.appendChild(actionsRow);

    return li;
  }

  function createMetaChip(text){
    const span = document.createElement('span');
    span.textContent = text;
    return span;
  }

  function humanizeActionKind(kind){
    if (!kind) return 'Action';
    const cleaned = String(kind).replace(/[._]/g, ' ').replace(/\s+/g, ' ').trim();
    if (!cleaned) return 'Action';
    return cleaned.charAt(0).toUpperCase() + cleaned.slice(1);
  }

  function humanizeStatus(value){
    if (!value) return 'Unknown';
    const cleaned = String(value).trim();
    if (!cleaned) return 'Unknown';
    return cleaned.charAt(0).toUpperCase() + cleaned.slice(1);
  }

  function formatActionInput(input){
    if (input == null) return '';
    try {
      const text = JSON.stringify(input, null, 2);
      if (!text) return '';
      if (text.length > 900) return `${text.slice(0, 880)}…`;
      return text;
    } catch {
      return String(input);
    }
  }

  async function decideApproval(item, action, approveBtn, holdBtn){
    if (!item || !item.id) {
      ARW.toast('Approval id missing');
      return;
    }
    if (!STATE.base) {
      const port = ARW.getPortFromInput('port');
      STATE.base = ARW.base(port);
    }
    if (!STATE.base) {
      ARW.toast('Start the server first');
      return;
    }
    const reviewer = ensureApprovalReviewer();
    if (!reviewer) {
      ARW.toast('Reviewer required');
      return;
    }
    let reason = null;
    if (action === 'deny') {
      const input = prompt('Reason to hold?', 'Needs teammate approval');
      if (input === null) return;
      reason = input.trim();
    }
    const buttons = [approveBtn, holdBtn].filter(Boolean);
    buttons.forEach(btn => {
      btn.dataset.approvalBusy = '1';
      btn.disabled = true;
    });
    try {
      const body = { decided_by: reviewer };
      if (reason) body.reason = reason;
      const path = action === 'approve'
        ? `/staging/actions/${encodeURIComponent(item.id)}/approve`
        : `/staging/actions/${encodeURIComponent(item.id)}/deny`;
      const resp = await ARW.http.fetch(STATE.base, path, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify(body),
      });
      if (!resp.ok) throw new Error(`HTTP ${resp.status}`);
      ARW.toast(action === 'approve' ? 'Approved' : 'Held');
      await refresh();
    } catch (err) {
      console.error('Approval decision failed', err);
      ARW.toast(err && err.message ? err.message : 'Decision failed');
    } finally {
      buttons.forEach(btn => {
        btn.disabled = false;
        delete btn.dataset.approvalBusy;
      });
    }
  }

  function requestApprovalReviewerChange(){
    const current = STATE.approvals?.reviewer || '';
    const input = prompt('Reviewer name (shown on the audit log)?', current);
    if (input === null) return;
    const trimmed = input.trim();
    if (!trimmed) {
      STATE.approvals.reviewer = null;
      rememberApprovalReviewer('');
      ARW.toast('Reviewer cleared');
    } else {
      STATE.approvals.reviewer = trimmed;
      rememberApprovalReviewer(trimmed);
      ARW.toast(`Reviewer set to ${trimmed}`);
    }
    renderApprovalsLane();
  }

  function ensureApprovalReviewer(){
    const cached = STATE.approvals?.reviewer;
    if (cached && cached.trim()) return cached.trim();
    const stored = getStoredApprovalReviewer();
    if (stored) {
      STATE.approvals.reviewer = stored;
      renderApprovalsLane();
      return stored;
    }
    const input = prompt('Reviewer name (shown on the audit log)?');
    if (input === null) return null;
    const trimmed = input.trim();
    if (!trimmed) return null;
    STATE.approvals.reviewer = trimmed;
    rememberApprovalReviewer(trimmed);
    renderApprovalsLane();
    return trimmed;
  }

  function getStoredApprovalReviewer(){
    try {
      const raw = localStorage.getItem(APPROVAL_REVIEWER_KEY);
      if (!raw) return null;
      const trimmed = raw.trim();
      return trimmed || null;
    } catch {
      return null;
    }
  }

  function rememberApprovalReviewer(name){
    try {
      if (name && name.trim()) {
        localStorage.setItem(APPROVAL_REVIEWER_KEY, name.trim());
      } else {
        localStorage.removeItem(APPROVAL_REVIEWER_KEY);
      }
    } catch {}
  }

  function renderConnections(){
    const summaryEl = document.getElementById('connectionsSummary');
    const noticeEl = document.getElementById('connectionsNotice');
    const listEl = document.getElementById('connectionsList');
    const emptyEl = document.getElementById('connectionsEmpty');

    if (summaryEl) {
      let text = STATE.connections.summary || '';
      if (STATE.connections.updatedMs) {
        text = text ? `${text} • updated ${formatRelativeWithAbs(STATE.connections.updatedMs)}` : `Updated ${formatRelativeWithAbs(STATE.connections.updatedMs)}`;
        summaryEl.title = formatRelativeAbs(STATE.connections.updatedMs);
      } else {
        summaryEl.title = '';
      }
      summaryEl.textContent = text;
    }

    if (noticeEl) {
      if (STATE.connections.error && !STATE.unauthorized) {
        noticeEl.textContent = STATE.connections.error;
        noticeEl.classList.remove('hidden');
      } else {
        noticeEl.textContent = '';
        noticeEl.classList.add('hidden');
      }
    }

    if (!listEl || !emptyEl) return;
    listEl.innerHTML = '';
    if (STATE.connections.loading) {
      emptyEl.textContent = 'Loading connections…';
      emptyEl.classList.remove('hidden');
      return;
    }

    const nodes = STATE.connections.nodes || [];
    if (!nodes.length) {
      const fallback = STATE.connections.summary || (STATE.unauthorized ? 'Authorize to view connections' : 'No remote connections');
      emptyEl.textContent = fallback;
      emptyEl.classList.remove('hidden');
      return;
    }

    emptyEl.classList.add('hidden');
    nodes.forEach(node => {
      const li = document.createElement('li');
      li.className = `connection-card ${node.health || 'unknown'}`;
      const title = document.createElement('h3');
      title.textContent = node.name || node.id;
      li.appendChild(title);
      const metaRow = document.createElement('div');
      metaRow.className = 'connection-meta';
      metaRow.appendChild(createMetaChip(`Role ${node.role || 'member'}`));
      metaRow.appendChild(createMetaChip(`Health ${humanizeStatus(node.health)}`));
      if (node.capabilities?.os || node.capabilities?.arch) {
        const parts = [node.capabilities.os, node.capabilities.arch].filter(Boolean).join('/');
        metaRow.appendChild(createMetaChip(parts));
      }
      if (node.capabilities?.version) {
        metaRow.appendChild(createMetaChip(`v${node.capabilities.version}`));
      }
      metaRow.appendChild(createMetaChip(`ID ${node.id}`));
      li.appendChild(metaRow);
      listEl.appendChild(li);
    });
  }

  function openConnectionsDrawer(){
    const overlay = document.getElementById('connectionsOverlay');
    const drawer = document.getElementById('connectionsDrawer');
    if (!overlay || !drawer) return;
    STATE.connectionsRestore = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    overlay.classList.remove('hidden');
    drawer.classList.remove('hidden');
    requestAnimationFrame(() => {
      overlay.classList.add('open');
      drawer.classList.add('open');
    });
    overlay.setAttribute('aria-hidden', 'false');
    drawer.setAttribute('aria-hidden', 'false');
    drawer.focus();
    STATE.connectionsOpen = true;
    if (!STATE.connections.loading && (!STATE.connections.nodes || STATE.connections.nodes.length === 0)) {
      refreshConnections();
    }
  }

  function closeConnectionsDrawer(){
    const overlay = document.getElementById('connectionsOverlay');
    const drawer = document.getElementById('connectionsDrawer');
    if (!overlay || !drawer) return;
    overlay.classList.remove('open');
    drawer.classList.remove('open');
    overlay.setAttribute('aria-hidden', 'true');
    drawer.setAttribute('aria-hidden', 'true');
    STATE.connectionsOpen = false;
    setTimeout(() => {
      if (!STATE.connectionsOpen) {
        overlay.classList.add('hidden');
        drawer.classList.add('hidden');
      }
    }, 220);
    const restore = STATE.connectionsRestore;
    STATE.connectionsRestore = null;
    if (restore && typeof restore.focus === 'function') {
      try { restore.focus(); } catch {}
    }
  }

  async function refreshConnections(){
    if (!STATE.base) {
      const port = ARW.getPortFromInput('port');
      STATE.base = ARW.base(port);
    }
    if (!STATE.base) {
      ARW.toast('Start the server first');
      return;
    }
    await refreshConnectionsSnapshot({ showLoading: true });
  }

  function handleGlobalKeydown(evt){
    if (evt.key === 'Escape' && STATE.connectionsOpen) {
      evt.preventDefault();
      closeConnectionsDrawer();
    }
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

  function updateMeter(id, value, options = {}){
    const node = document.getElementById(id);
    if (!node) return;
    let fill = node.querySelector('i');
    if (!fill) {
      fill = document.createElement('i');
      node.appendChild(fill);
    }
    const preferLow = options.preferLow === true;
    const warn = typeof options.warn === 'number' ? options.warn : (preferLow ? 0.25 : 0.65);
    const bad = typeof options.bad === 'number' ? options.bad : (preferLow ? 0.5 : 0.4);
    const formatText = typeof options.formatText === 'function' ? options.formatText : null;

    node.classList.remove('ok','warn','bad','empty');

    if (!Number.isFinite(value)) {
      fill.style.width = '0%';
      node.classList.add('empty');
      node.setAttribute('aria-valuenow', '0');
      node.setAttribute('aria-valuetext', 'No data');
      node.title = 'No data';
      return;
    }

    const clamped = Math.min(1, Math.max(0, value));
    const percent = Math.round(clamped * 100);
    fill.style.width = `${percent}%`;
    node.setAttribute('aria-valuenow', clamped.toFixed(2));
    const label = formatText ? formatText(clamped, percent) : `${percent}%`;
    node.setAttribute('aria-valuetext', label);
    node.title = label;

    let state = 'ok';
    if (preferLow) {
      if (clamped >= bad) state = 'bad';
      else if (clamped >= warn) state = 'warn';
    } else {
      if (clamped <= bad) state = 'bad';
      else if (clamped <= warn) state = 'warn';
    }
    node.classList.add(state);
  }

  function errorsForPath(path, opts){
    const source = Array.isArray(opts?.errors) ? opts.errors : (STATE.errors || []);
    return source.filter(line => line && line.includes(path));
  }

  function readGeneratedMs(payload){
    if (!payload || typeof payload !== 'object') return null;
    const direct = toNumber(payload.generated_ms ?? payload.generatedMs);
    if (Number.isFinite(direct)) return direct;
    return parseTimestamp(payload.generated);
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

  function percentLabel(value, digits = 0){
    if (!Number.isFinite(value)) return '—';
    const clamped = Math.min(1, Math.max(0, value));
    return `${(clamped * 100).toFixed(digits)}%`;
  }

  function asFiniteNumber(value){
    const num = Number(value);
    return Number.isFinite(num) ? num : NaN;
  }

  function formatSlotName(slot){
    if (!slot) return '—';
    const text = String(slot).replace(/[_-]/g, ' ').trim();
    return text || '—';
  }

  function formatCountLabel(count, singular){
    const value = Number(count) || 0;
    if (value === 1) return `1 ${singular}`;
    return `${value} ${singular}s`;
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

  async function openApprovalsInDebug(){
    if (!STATE.base) {
      ARW.toast('Start the server first');
      return;
    }
    try {
      await ARW.invoke('open_url', { url: `${STATE.base}/admin/debug#approvals` });
    } catch (err) {
      console.error('Open approvals queue failed', err);
      ARW.toast('Unable to open approvals queue');
    }
  }

  async function openFeedbackInDebug(){
    if (!STATE.base) {
      ARW.toast('Start the server first');
      return;
    }
    if (STATE.unauthorized) {
      ARW.toast('Authorize with an admin token to open the feedback panel');
      return;
    }
    try {
      await ARW.invoke('open_url', { url: `${STATE.base}/admin/debug#feedback` });
    } catch (err) {
      console.error('Open feedback panel failed', err);
      ARW.toast('Unable to open feedback panel');
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

  async function openQuarantineDocs(){
    if (!STATE.base) {
      try {
        const port = ARW.getPortFromInput('port');
        STATE.base = ARW.base(port);
      } catch {}
    }
    if (!STATE.base) {
      ARW.toast('Start the server first');
      return;
    }
    try {
      await ARW.invoke('open_url', { url: `${STATE.base}/docs/api/memory_world_schemas/#memory-quarantine-payload` });
    } catch (err) {
      console.error('Open quarantine docs failed', err);
      ARW.toast('Unable to open memory quarantine docs');
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

  function normalizeInterrupts(raw) {
    const normalized = {};
    AUTONOMY_INTERRUPT_KEYS.forEach(key => {
      const value = raw && typeof raw === 'object' ? Number(raw[key]) : 0;
      normalized[key] = Number.isFinite(value) && value > 0 ? Math.trunc(value) : 0;
    });
    return normalized;
  }

  function diffInterrupts(previous, next) {
    const deltas = {};
    AUTONOMY_INTERRUPT_KEYS.forEach(key => {
      const prevVal = Number(previous?.[key]) || 0;
      const nextVal = Number(next?.[key]) || 0;
      deltas[key] = nextVal > prevVal ? nextVal - prevVal : 0;
    });
    return deltas;
  }

  function formatInterruptSummary(counts, includeZeros = false) {
    if (!counts || typeof counts !== 'object') return '';
    const parts = AUTONOMY_INTERRUPT_KEYS
      .map(key => ({
        key,
        label: AUTONOMY_INTERRUPT_LABELS[key] || key.replace(/_/g, ' '),
        value: Number(counts[key]) || 0,
      }))
      .filter(item => includeZeros || item.value > 0);
    if (!parts.length) return '';
    return parts.map(({ label, value }) => `${label}: ${value}`).join(' · ');
  }

  function extractAutonomyInterrupts(statsRes) {
    if (!statsRes || !statsRes.data) return normalizeInterrupts();
    const raw = statsRes.data?.autonomy?.interrupts;
    return normalizeInterrupts(raw);
  }
})();
