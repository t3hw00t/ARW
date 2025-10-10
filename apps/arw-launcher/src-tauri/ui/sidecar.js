const waitForARW = () => new Promise((resolve, reject) => {
  const hasARW = () => typeof window !== 'undefined' && window.ARW;
  if (hasARW()) {
    resolve(window.ARW);
    return;
  }
  let settled = false;
  let attempts = 0;
  const maxAttempts = 600;
  let timer = null;
  const cleanup = () => {
    settled = true;
    if (timer) clearInterval(timer);
    if (typeof window !== 'undefined' && typeof window.removeEventListener === 'function') {
      window.removeEventListener('arw:ready', onReady);
    }
  };
  const onReady = (event) => {
    if (settled) return;
    cleanup();
    resolve(event?.detail || window.ARW);
  };
  if (typeof window !== 'undefined' && typeof window.addEventListener === 'function') {
    window.addEventListener('arw:ready', onReady, { once: true });
  }
  timer = setInterval(() => {
    attempts += 1;
    if (hasARW()) {
      cleanup();
      resolve(window.ARW);
      return;
    }
    if (attempts >= maxAttempts) {
      cleanup();
      reject(new Error('ARW helpers unavailable'));
    }
  }, 10);
});

const installSidecar = (ARW) => {
  if (!ARW || !ARW.sse || (ARW.sidecar && typeof ARW.sidecar.mount === "function")) return;
  ARW.sidecar = {
    mount(el, lanes = ["timeline","provenance","metrics","models"], opts = {}) {
      const node = (typeof el === 'string') ? document.getElementById(el) : el;
      if (!node) return { dispose(){} };
      node.classList.add('arw-sidecar');
      node.innerHTML = '';
      const laneSet = new Set(Array.isArray(lanes) ? lanes : []);
      const hasTimeline = laneSet.has('timeline');
      const hasProvenance = laneSet.has('provenance');
      const hasMetrics = laneSet.has('metrics');
      const hasModels = laneSet.has('models');
      const hasApprovals = laneSet.has('approvals');
      const hasPolicy = laneSet.has('policy');
      const hasContext = laneSet.has('context');
      const hasActivity = laneSet.has('activity');
      const laneHints = Object.assign({
        timeline: 'Live events appear here once the project streams updates.',
        context: 'Assembled context items populate after the next cascade.',
        provenance: 'Policy and memory provenance will list the latest capsules.',
        metrics: 'Key health metrics load after the first telemetry fetch.',
        models: 'Managed runtime activity shows here when models change.',
        approvals: 'Approvals queue items arrive when reviewers are assigned.',
        policy: 'Policy capsules, leases, and guardrail notes appear on activity.',
        activity: 'Recent actions, runs, and snapshots populate automatically.'
      }, opts?.laneHints || {});
      const sections = [];
      const mountUid = `arw-sidecar-${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 6)}`;
      let laneIndex = 0;
      for (const name of lanes) {
        const sec = document.createElement('section');
        sec.dataset.lane = name;
        const h = document.createElement('h3');
        const toggle = document.createElement('button');
        toggle.type = 'button';
        toggle.className = 'lane-toggle';
        toggle.textContent = name;
        toggle.setAttribute('aria-expanded', 'true');
        const bodyId = `${mountUid}-${laneIndex++}-body`;
        toggle.setAttribute('aria-controls', bodyId);
        const updateExpanded = () => {
          const collapsed = sec.classList.contains('collapsed');
          toggle.setAttribute('aria-expanded', collapsed ? 'false' : 'true');
        };
        toggle.addEventListener('click', () => {
          sec.classList.toggle('collapsed');
          updateExpanded();
        });
        h.appendChild(toggle);
        const summary = document.createElement('div');
        summary.className = 'lane-summary';
        summary.hidden = true;
        const body = document.createElement('div');
        body.className = 'lane-body';
        body.id = bodyId;
        const placeholder = document.createElement('div');
        placeholder.className = 'lane-placeholder';
        placeholder.textContent = laneHints[name] || 'Live data will appear here once the project connects.';
        body.dataset.placeholder = 'true';
        body.appendChild(placeholder);
        sec.append(h, summary, body);
        node.appendChild(sec);
        updateExpanded();
        sections.push([name, body, summary]);
      }
      const bodyFor = (lane) => sections.find(([n]) => n === lane)?.[1] || null;
      const summaryFor = (lane) => sections.find(([n]) => n === lane)?.[2] || null;
      const clearLanePlaceholder = (laneOrEl) => {
        const el = typeof laneOrEl === 'string' ? bodyFor(laneOrEl) : laneOrEl;
        if (!el) return;
        if (el.dataset.placeholder) {
          el.innerHTML = '';
          delete el.dataset.placeholder;
        }
      };
      const relativeTime = (value) => {
        if (value === null || value === undefined) return '';
        const dt = value instanceof Date
          ? value
          : typeof value === 'number'
          ? new Date(value)
          : new Date(String(value));
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
        for (const unit of units) {
          if (absSec < unit.limit) {
            const value = Math.max(1, Math.floor(absSec / unit.div));
            return diffMs >= 0 ? `${value}${unit.label} ago` : `in ${value}${unit.label}`;
          }
        }
        const years = Math.max(1, Math.floor(absSec / 31536000));
        return diffMs >= 0 ? `${years}y ago` : `in ${years}y`;
      };
      const renderLaneMessage = (lane, text, tone = 'info') => {
        const el = bodyFor(lane);
        if (!el) return;
        clearLanePlaceholder(el);
        el.dataset.emptyMsg = 'true';
        const span = document.createElement('div');
        span.className = 'context-msg';
        if (tone === 'warn') span.classList.add('warn');
        span.textContent = text;
        el.innerHTML = '';
        el.appendChild(span);
      };
      let provenanceSummaryData = null;
      let provSummarySub = null;
      let provenanceSummaryFetched = false;
      const normalizeModularSummary = (raw) => {
        if (!raw || typeof raw !== 'object') return null;
        const pending = Number(raw.pending_human_review ?? raw.pending ?? 0) || 0;
        const blocked = Number(raw.blocked ?? 0) || 0;
        let generated = raw.generated;
        let generatedMs = Number(raw.generated_ms);
        if (!generated) generated = new Date().toISOString();
        if (!Number.isFinite(generatedMs)) {
          const parsed = Date.parse(generated);
          generatedMs = Number.isFinite(parsed) ? parsed : Date.now();
        }
        const recentRaw = Array.isArray(raw.recent) ? raw.recent : [];
        const sanitizeCaps = (caps) => {
          if (!Array.isArray(caps)) return [];
          return caps
            .map((cap) => (typeof cap === 'string' ? cap.trim() : ''))
            .filter((cap) => cap.length > 0);
        };
        const recent = recentRaw
          .filter((entry) => entry && typeof entry === 'object')
          .map((entry) => {
            const clone = { ...entry };
            const kind = typeof clone.payload_kind === 'string' ? clone.payload_kind.trim() : '';
            clone.payload_kind = kind;
            if (clone.payload_summary && typeof clone.payload_summary === 'object') {
              clone.payload_summary = { ...clone.payload_summary };
            }
            if (clone.policy_scope && typeof clone.policy_scope === 'object') {
              const scope = clone.policy_scope;
              const leases = Array.isArray(scope.leases) ? scope.leases : [];
              clone.policy_scope = {
                ...scope,
                leases: leases.map((lease) => (lease && typeof lease === 'object' ? { ...lease } : lease)),
              };
            }
            if (Array.isArray(clone.required_capabilities)) {
              clone.required_capabilities = sanitizeCaps(clone.required_capabilities);
            }
            if (!clone.required_capabilities || !clone.required_capabilities.length) {
              const summaryCaps = clone.payload_summary && clone.payload_summary.required_capabilities;
              clone.required_capabilities = sanitizeCaps(summaryCaps || []);
            }
            if (kind === 'tool_invocation' && typeof clone.requested_by !== 'string' && typeof clone.agent_id === 'string') {
              // keep existing agent_id if present
            } else if (kind === 'tool_invocation' && typeof clone.agent_id !== 'string' && typeof clone.requested_by === 'string') {
              clone.agent_id = clone.requested_by;
            }
            if (!clone.result_status) {
              const summaryStatus = clone.payload_summary && clone.payload_summary.result_status;
              if (typeof summaryStatus === 'string' && summaryStatus.trim()) {
                clone.result_status = summaryStatus.trim();
              } else if (typeof clone.result_status === 'string') {
                clone.result_status = clone.result_status.trim();
              }
            } else if (typeof clone.result_status === 'string') {
              clone.result_status = clone.result_status.trim();
            }
            return clone;
          });
        return {
          pending_human_review: pending,
          blocked,
          recent,
          generated,
          generated_ms: generatedMs,
        };
      };
      const renderProvenanceSummary = () => {
        const summaryEl = summaryFor('provenance');
        if (!summaryEl) return;
        const data = provenanceSummaryData;
        if (!data) {
          summaryEl.hidden = true;
          summaryEl.classList.remove('provenance-summary');
          summaryEl.innerHTML = '';
          return;
        }
        summaryEl.hidden = false;
        summaryEl.classList.add('provenance-summary');
        summaryEl.innerHTML = '';
        const counts = document.createElement('div');
        counts.className = 'provenance-summary-counts';
        const makePill = (label, value, tone) => {
          const pill = document.createElement('span');
          pill.className = 'pill';
          if (tone) pill.classList.add(tone);
          pill.textContent = `${label}: ${value}`;
          return pill;
        };
        counts.appendChild(makePill('Pending review', data.pending_human_review, data.pending_human_review ? 'warn' : 'good'));
        counts.appendChild(makePill('Blocked', data.blocked, data.blocked ? 'bad' : 'good'));
        const updated = document.createElement('span');
        updated.className = 'provenance-summary-updated dim';
        const updatedDate = Number.isFinite(data.generated_ms) ? new Date(data.generated_ms) : new Date();
        const updatedRel = relativeTime(updatedDate);
        updated.textContent = updatedRel ? `Updated ${updatedRel}` : 'Updated just now';
        updated.title = updatedDate.toLocaleString();
        counts.appendChild(updated);
        summaryEl.appendChild(counts);
        const recent = Array.isArray(data.recent) ? data.recent : [];
        if (recent.length) {
          const list = document.createElement('ul');
          list.className = 'provenance-summary-list';
          recent.slice(0, 5).forEach((item) => {
            const li = document.createElement('li');
            li.className = 'provenance-summary-item';
            const title = document.createElement('span');
            title.className = 'provenance-summary-title';
            const kindRaw = typeof item.payload_kind === 'string' ? item.payload_kind.trim() : '';
            const isTool = kindRaw === 'tool_invocation';
            const labelParts = [];
            if (isTool) {
              const toolId = typeof item.tool_id === 'string' && item.tool_id.trim() ? item.tool_id.trim() : 'tool';
              const statusLabel = typeof item.result_status === 'string' && item.result_status.trim() ? item.result_status.trim() : '';
              labelParts.push(toolId);
              if (statusLabel) labelParts.push(statusLabel);
            } else {
              const agent = typeof item.agent_id === 'string' && item.agent_id.trim() ? item.agent_id.trim() : 'agent';
              const intent = typeof item.intent === 'string' && item.intent.trim() ? item.intent.trim() : '';
              labelParts.push(agent);
              if (intent) {
                labelParts.push(intent);
              } else if (kindRaw) {
                labelParts.push(kindRaw);
              }
            }
            const turn = typeof item.turn_id === 'string' && item.turn_id.trim() ? item.turn_id.trim() : '';
            const invocationId = typeof item.invocation_id === 'string' && item.invocation_id.trim() ? item.invocation_id.trim() : '';
            const fallback = isTool ? invocationId || 'modular tool' : turn || 'modular turn';
            title.textContent = labelParts.filter(Boolean).join(' · ') || fallback;
            li.appendChild(title);
            const stage = typeof item.lifecycle_stage === 'string' ? item.lifecycle_stage.replace(/_/g, ' ') : '';
            if (stage) {
              const stagePill = document.createElement('span');
              stagePill.className = 'pill';
              stagePill.textContent = stage;
              li.appendChild(stagePill);
            }
            const gate = typeof item.validation_gate === 'string' ? item.validation_gate.replace(/_/g, ' ') : '';
            if (gate) {
              const gatePill = document.createElement('span');
              gatePill.className = 'pill';
              gatePill.textContent = `gate ${gate}`;
              li.appendChild(gatePill);
            }
            if (isTool) {
              const statusLabel = typeof item.result_status === 'string' && item.result_status.trim() ? item.result_status.trim() : '';
              if (statusLabel) {
                const statusPill = document.createElement('span');
                statusPill.className = 'pill';
                const tone = statusLabel === 'ok' ? 'good' : statusLabel === 'error' ? 'bad' : 'warn';
                statusPill.classList.add(tone);
                statusPill.textContent = `status ${statusLabel}`;
                li.appendChild(statusPill);
              }
              const reqCaps = Array.isArray(item.required_capabilities) ? item.required_capabilities.filter((cap) => typeof cap === 'string' && cap.trim()).map((cap) => cap.trim()) : [];
              if (reqCaps.length) {
                const capsPill = document.createElement('span');
                capsPill.className = 'pill';
                capsPill.textContent = `caps ${reqCaps.slice(0, 3).join(', ')}` + (reqCaps.length > 3 ? '…' : '');
                li.appendChild(capsPill);
              }
              const summary = item.payload_summary && typeof item.payload_summary === 'object' ? item.payload_summary : null;
              if (summary && summary.needs_network) {
                const netPill = document.createElement('span');
                netPill.className = 'pill warn';
                netPill.textContent = 'needs network';
                li.appendChild(netPill);
              }
              if (summary && Number(summary.filesystem_scopes) > 0) {
                const fsPill = document.createElement('span');
                fsPill.className = 'pill';
                fsPill.textContent = `fs scopes ${summary.filesystem_scopes}`;
                li.appendChild(fsPill);
              }
              const policy = item.policy_scope && typeof item.policy_scope === 'object' ? item.policy_scope : null;
              if (policy && policy.requires_human_review) {
                const reviewPill = document.createElement('span');
                reviewPill.className = 'pill warn';
                reviewPill.textContent = 'review required';
                li.appendChild(reviewPill);
              }
              const leasesCount = Array.isArray(item.policy_scope?.leases) ? item.policy_scope.leases.length : 0;
              if (leasesCount) {
                const leasePill = document.createElement('span');
                leasePill.className = 'pill';
                leasePill.textContent = `leases ${leasesCount}`;
                li.appendChild(leasePill);
              }
            }
            const confVal = Number(item.confidence);
            if (!isTool && Number.isFinite(confVal)) {
              const confPill = document.createElement('span');
              confPill.className = 'pill';
              confPill.textContent = `confidence ${(confVal * 100).toFixed(0)}%`;
              li.appendChild(confPill);
            }
            const createdRaw = item.created_ms ?? item.created;
            let createdDate = null;
            if (typeof createdRaw === 'number') {
              createdDate = new Date(createdRaw);
            } else if (typeof createdRaw === 'string' && createdRaw.trim()) {
              const num = Number(createdRaw);
              if (Number.isFinite(num)) createdDate = new Date(num);
              else {
                const parsed = Date.parse(createdRaw);
                if (Number.isFinite(parsed)) createdDate = new Date(parsed);
              }
            }
            if (createdDate && !Number.isNaN(createdDate.getTime())) {
              const timeEl = document.createElement('time');
              timeEl.dateTime = createdDate.toISOString();
              timeEl.textContent = relativeTime(createdDate) || createdDate.toLocaleTimeString();
              li.appendChild(timeEl);
            }
            const excerpt = typeof item.summary_excerpt === 'string' && item.summary_excerpt.trim()
              ? item.summary_excerpt.trim()
              : (item.payload_summary && typeof item.payload_summary === 'object' && typeof item.payload_summary.text_preview === 'string'
                  ? item.payload_summary.text_preview.trim()
                  : '');
            if (excerpt) {
              const preview = document.createElement('div');
              preview.className = 'provenance-summary-preview';
              preview.textContent = excerpt.length > 140 ? `${excerpt.slice(0, 137)}…` : excerpt;
              li.appendChild(preview);
            }
            list.appendChild(li);
          });
          summaryEl.appendChild(list);
        } else {
          const empty = document.createElement('div');
          empty.className = 'provenance-summary-empty';
          empty.textContent = 'No recent modular turns';
          summaryEl.appendChild(empty);
        }
      };
      const primeProvenanceSummary = async () => {
        if (!opts.base || provenanceSummaryFetched) return;
        provenanceSummaryFetched = true;
        try {
          const data = await ARW.http.json(opts.base, '/state/memory/modular?limit=200');
          const normalized = normalizeModularSummary(data);
          if (normalized) {
            provenanceSummaryData = normalized;
            ARW.read._store.set('memory_modular_review', normalized);
            ARW.read._emit('memory_modular_review');
            renderProvenanceSummary();
          }
        } catch (err) {
          console.warn('provenance summary fetch failed', err);
          provenanceSummaryFetched = false;
        }
      };
      let approvalsSub = null;
      let approvalsState = null;
      if (hasApprovals) {
        approvalsState = {
          error: null,
          detail: null,
          loading: false,
          reviewer: null,
          reviewerLoaded: false,
          filter: '',
          filterMode: 'text',
          filterCaret: null,
          staleThresholdMs: 60 * 60 * 1000,
          lanePrefsLoaded: false,
          shortcutHandler: null,
          shortcutMap: {},
          sortMode: 'newest',
        };
        const fmtRelative = (iso) => {
          if (!iso) return '';
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
          for (const unit of units) {
            if (absSec < unit.limit) {
              const value = Math.max(1, Math.floor(absSec / unit.div));
              return diffMs >= 0 ? `${value}${unit.label} ago` : `in ${value}${unit.label}`;
            }
          }
          const years = Math.max(1, Math.floor(absSec / 31536000));
          return diffMs >= 0 ? `${years}y ago` : `in ${years}y`;
        };
        const formatJson = (value, maxLen = 2000) => {
          try {
            let text = JSON.stringify(value ?? {}, null, 2);
            if (text === '{}' || text === '[]') {
              text = JSON.stringify(value);
            }
            if (typeof text !== 'string') {
              text = String(value ?? '');
            }
            if (text.length > maxLen) {
              return `${text.slice(0, maxLen - 1)}…`;
            }
            return text;
          } catch {
            const str = typeof value === 'string' ? value : String(value ?? '');
            return str.length > maxLen ? `${str.slice(0, maxLen - 1)}…` : str;
          }
        };
        const setReviewerPref = async (name) => {
          try {
            const prefs = (await ARW.getPrefs('launcher')) || {};
            if (name) {
              prefs.approvalsReviewer = name;
            } else {
              delete prefs.approvalsReviewer;
            }
            await ARW.setPrefs('launcher', prefs);
          } catch {}
        };
        const setFilterPref = async (value) => {
          try {
            const prefs = (await ARW.getPrefs('launcher')) || {};
            if (value) {
              prefs.approvalsFilter = value;
            } else {
              delete prefs.approvalsFilter;
            }
            await ARW.setPrefs('launcher', prefs);
          } catch {}
        };
        const setStalePref = async (ms) => {
          try {
            const prefs = (await ARW.getPrefs('launcher')) || {};
            prefs.approvalsStaleMs = ms;
            await ARW.setPrefs('launcher', prefs);
          } catch {}
        };
        const setFilterModePref = async (mode) => {
          try {
            const prefs = (await ARW.getPrefs('launcher')) || {};
            prefs.approvalsFilterMode = mode;
            await ARW.setPrefs('launcher', prefs);
          } catch {}
        };
        const setSortPref = async (mode) => {
          try {
            const prefs = (await ARW.getPrefs('launcher')) || {};
            prefs.approvalsSortMode = mode;
            await ARW.setPrefs('launcher', prefs);
          } catch {}
        };
        const promptReviewer = async () => {
          const current = approvalsState.reviewer || '';
          const result = await ARW.modal.form({
            title: 'Set reviewer',
            description: 'Identify who is approving or denying this action.',
            submitLabel: 'Save reviewer',
            focusField: 'reviewer',
            fields: [
              {
                name: 'reviewer',
                label: 'Reviewer',
                value: current,
                placeholder: 'Name or handle',
                autocomplete: 'off',
                hint: 'Leave blank to clear the reviewer.',
              },
            ],
          });
          if (!result) {
            return approvalsState.reviewer;
          }
          const trimmed = String(result.reviewer || '').trim();
          if (!trimmed) {
            approvalsState.reviewer = null;
            await setReviewerPref(null);
            return null;
          }
          approvalsState.reviewer = trimmed;
          await setReviewerPref(trimmed);
          return trimmed;
        };
        const ensureReviewer = async () => {
          if (approvalsState.reviewer) {
            return approvalsState.reviewer;
          }
          return await promptReviewer();
        };
        const parseIso = (maybeIso) => {
          if (!maybeIso) return null;
          const ts = Date.parse(maybeIso);
          return Number.isFinite(ts) ? ts : null;
        };
        const ageMs = (item) => {
          const ts = parseIso(item?.created) ?? parseIso(item?.updated);
          if (ts == null) return null;
          return Date.now() - ts;
        };
        const formatAge = (ms) => {
          if (!Number.isFinite(ms) || ms < 0) return '';
          const min = Math.round(ms / 60000);
          if (min < 1) return '<1m';
          if (min < 60) return `${min}m`;
          const hr = Math.floor(min / 60);
          const rem = min % 60;
          if (hr < 24) {
            return rem ? `${hr}h ${rem}m` : `${hr}h`;
          }
          const days = Math.floor(hr / 24);
          const hRem = hr % 24;
          return hRem ? `${days}d ${hRem}h` : `${days}d`;
        };
        const makePill = (label, value, { mono = false } = {}) => {
          if (value === null || value === undefined || value === '') return null;
          const pill = document.createElement('span');
          pill.className = 'pill';
          const tag = document.createElement('span');
          tag.className = 'tag';
          tag.textContent = label;
          const val = document.createElement('span');
          if (mono) val.classList.add('mono');
          val.textContent = String(value);
          pill.append(tag, val);
          return pill;
        };
        const makeJsonBlock = (label, value) => {
          const wrap = document.createElement('div');
          wrap.className = 'approval-evidence-block';
          const head = document.createElement('div');
          head.className = 'approval-evidence-head';
          const title = document.createElement('span');
          title.className = 'approval-evidence-title';
          title.textContent = label;
          head.appendChild(title);
          const copyBtn = document.createElement('button');
          copyBtn.type = 'button';
          copyBtn.className = 'ghost btn-small';
          copyBtn.textContent = 'Copy';
          copyBtn.addEventListener('click', (ev) => {
            ev.preventDefault();
            ev.stopPropagation();
            try {
              ARW.copy(JSON.stringify(value ?? {}, null, 2));
            } catch {
              ARW.toast('Copy failed');
            }
          });
          head.appendChild(copyBtn);
          wrap.appendChild(head);
          const pre = document.createElement('pre');
          pre.className = 'approval-evidence-json mono';
          pre.textContent = formatJson(value);
          wrap.appendChild(pre);
          return wrap;
        };
        const appendReviewerRow = (parent) => {
          const wrap = document.createElement('div');
          wrap.className = 'approval-reviewer';
          const label = document.createElement('span');
          label.className = 'dim';
          label.textContent = approvalsState.reviewer
            ? `Reviewer: ${approvalsState.reviewer}`
            : 'Reviewer not set';
          const btn = document.createElement('button');
          btn.type = 'button';
          btn.className = 'ghost btn-small';
          btn.textContent = approvalsState.reviewer ? 'Change reviewer' : 'Set reviewer';
          btn.addEventListener('click', async (ev) => {
            ev.preventDefault();
            ev.stopPropagation();
            const prev = approvalsState.reviewer;
            const updated = await promptReviewer();
            if (updated === prev) {
              return;
            }
            if (updated) {
              ARW.toast(`Reviewer set to ${updated}`);
            } else {
              ARW.toast('Reviewer cleared');
            }
            renderApprovals();
          });
          wrap.append(label, btn);
          parent.appendChild(wrap);
        };
        const createApprovalCard = (item = {}, autoOpen = false) => {
          const card = document.createElement('article');
          card.className = 'approval-card';
          const itemAge = ageMs(item);
          if (Number.isFinite(itemAge) && itemAge >= approvalsState.staleThresholdMs) {
            card.classList.add('stale');
          }
          const head = document.createElement('div');
          head.className = 'approval-head';
          const kindPill = makePill('Kind', item.action_kind || 'unknown', { mono: true });
          if (kindPill) head.appendChild(kindPill);
          const projPill = makePill('Project', item.project);
          if (projPill) head.appendChild(projPill);
          const reqPill = makePill('By', item.requested_by);
          if (reqPill) head.appendChild(reqPill);
          if (Number.isFinite(itemAge)) {
            const agePill = makePill('Age', formatAge(itemAge), { mono: true });
            if (agePill) head.appendChild(agePill);
          }
          if (head.childElementCount) card.appendChild(head);
          const meta = document.createElement('div');
          meta.className = 'approval-meta';
          if (item.created) {
            const timeEl = document.createElement('time');
            timeEl.dateTime = item.created;
            timeEl.title = new Date(item.created).toLocaleString();
            timeEl.textContent = fmtRelative(item.created) || item.created;
            meta.appendChild(timeEl);
          }
          if (item.status && item.status !== 'pending') {
            const statusSpan = document.createElement('span');
            statusSpan.textContent = item.status;
            meta.appendChild(statusSpan);
          }
          if (item.project && !projPill) {
            const projectSpan = document.createElement('span');
            projectSpan.textContent = item.project;
            meta.appendChild(projectSpan);
          }
          if (meta.childElementCount) card.appendChild(meta);
          const details = document.createElement('details');
          details.className = 'approval-details';
          if (autoOpen) details.open = true;
          const summary = document.createElement('summary');
          summary.textContent = 'Review details';
          details.appendChild(summary);
          const body = document.createElement('div');
          body.className = 'approval-evidence';
          body.appendChild(makeJsonBlock('Action input', item.action_input ?? {}));
          const hasEvidence =
            item.evidence &&
            ((typeof item.evidence === 'object' && Object.keys(item.evidence).length > 0) ||
              typeof item.evidence === 'string');
          if (hasEvidence) {
            body.appendChild(makeJsonBlock('Evidence', item.evidence));
          } else {
            const none = document.createElement('div');
            none.className = 'dim';
            none.textContent = 'No evidence provided';
            body.appendChild(none);
          }
          details.appendChild(body);
          card.appendChild(details);

          const addDecisionButtons = () => {
            if (!opts.base || !item.id) return;
            const actionsRow = document.createElement('div');
            actionsRow.className = 'row approval-actions';

            const runDecision = async (verb, payload = {}) => {
              approvalsState.loading = true;
              renderApprovals();
              const bodyPayload = { ...payload };
              if (approvalsState.reviewer && !bodyPayload.decided_by) {
                bodyPayload.decided_by = approvalsState.reviewer;
              }
              try {
                const path = `/staging/actions/${encodeURIComponent(item.id)}/${verb}`;
                const hasBody = Object.keys(bodyPayload).length > 0;
                const fetchOpts = { method: 'POST' };
                if (hasBody) {
                  fetchOpts.headers = { 'Content-Type': 'application/json' };
                  fetchOpts.body = JSON.stringify(bodyPayload);
                }
                const resp = await ARW.http.fetch(opts.base, path, fetchOpts);
                if (!resp.ok) {
                  throw new Error(`HTTP ${resp.status}`);
                }
                const toastMsg = verb === 'approve' ? 'Action approved' : 'Action denied';
                ARW.toast(toastMsg);
              } catch (err) {
                console.error('decision failed', err);
                ARW.toast('Decision failed');
              } finally {
                approvalsState.loading = false;
              }
              await primeApprovals();
            };

            const approveBtn = document.createElement('button');
            approveBtn.type = 'button';
            approveBtn.className = 'primary btn-small';
            approveBtn.textContent = 'Approve';
          approveBtn.addEventListener('click', async (ev) => {
            ev.preventDefault();
            ev.stopPropagation();
            const confirmMsg = `Approve ${item.action_kind || 'action'}${item.project ? ` in ${item.project}` : ''}?`;
            const confirmed = await ARW.modal.confirm({
              title: 'Approve action',
              body: confirmMsg,
              submitLabel: 'Approve',
              cancelLabel: 'Cancel',
            });
            if (!confirmed) return;
            const reviewer = approvalsState.reviewer || await ensureReviewer();
            if (!reviewer) {
              ARW.toast('Reviewer required');
              return;
            }
              await runDecision('approve', { decided_by: reviewer });
          });

          const denyBtn = document.createElement('button');
          denyBtn.type = 'button';
          denyBtn.className = 'ghost btn-small';
          denyBtn.textContent = 'Deny';
          denyBtn.addEventListener('click', async (ev) => {
            ev.preventDefault();
            ev.stopPropagation();
            const contextLine = `${item.action_kind || 'action'}${item.project ? ` in ${item.project}` : ''}`;
            const denyModal = await ARW.modal.form({
              title: 'Deny action',
              body: `Deny ${contextLine}? Capture a reason for teammates (optional).`,
              submitLabel: 'Deny action',
              cancelLabel: 'Cancel',
              focusField: 'reason',
              destructive: true,
              fields: [
                {
                  name: 'reason',
                  label: 'Reason',
                  type: 'textarea',
                  rows: 3,
                  hint: 'Shared via the audit trail. Leave blank to deny without a note.',
                  trim: true,
                },
              ],
            });
            if (!denyModal) return;
            const reviewer = approvalsState.reviewer || await ensureReviewer();
            if (!reviewer) {
              ARW.toast('Reviewer required');
              return;
            }
            const trimmedReason = String(denyModal.reason || '').trim();
            const payload = { decided_by: reviewer };
            if (trimmedReason) payload.reason = trimmedReason;
            await runDecision('deny', payload);
          });

            actionsRow.append(approveBtn, denyBtn);
            card.appendChild(actionsRow);
          };

          addDecisionButtons();
          return card;
        };
        const renderApprovals = (restoreFilterFocus = false) => {
          if (!approvalsState) return;
          const el = bodyFor('approvals');
          if (!el) return;
          clearLanePlaceholder(el);
          el.innerHTML = '';
          if (approvalsState.error) {
            const msg = document.createElement('div');
            msg.className = 'dim';
            msg.textContent = approvalsState.error;
            if (approvalsState.detail) msg.title = approvalsState.detail;
            el.appendChild(msg);
            return;
          }
          const model = ARW.read.get('staging_actions');
          if (approvalsState.loading && !model) {
            const msg = document.createElement('div');
            msg.className = 'dim';
            msg.textContent = 'Loading approvals…';
            el.appendChild(msg);
            return;
          }
          if (!model) {
            const msg = document.createElement('div');
            msg.className = 'dim';
            msg.textContent = 'Waiting for approvals data';
            el.appendChild(msg);
            return;
          }
          const pending = Array.isArray(model.pending) ? model.pending : [];
          const recent = Array.isArray(model.recent) ? model.recent : [];
          const filterMode = approvalsState.filterMode || 'text';
          const sortMode = approvalsState.sortMode || 'newest';
          const filterNeedle = filterMode === 'text' ? (approvalsState.filter || '').trim().toLowerCase() : '';
          const matchesFilter = (item) => {
            if (filterMode === 'stale') {
              const age = ageMs(item);
              return Number.isFinite(age) && age >= approvalsState.staleThresholdMs;
            }
            if (!filterNeedle) return true;
            const haystackParts = [
              item?.action_kind,
              item?.project,
              item?.requested_by,
              item?.id,
            ];
            try {
              if (item?.action_input) {
                haystackParts.push(JSON.stringify(item.action_input));
              }
            } catch {}
            return haystackParts
              .filter(Boolean)
              .some((part) =>
                String(part)
                  .toLowerCase()
                  .includes(filterNeedle),
              );
          };
          const applyFilterChip = (mode, value, caret) => {
            if (mode === 'stale') {
              if (approvalsState.filterMode === 'stale') return;
              approvalsState.filterMode = 'stale';
              approvalsState.filter = '';
              approvalsState.filterCaret = null;
              setFilterModePref('stale');
              setFilterPref('');
              window.requestAnimationFrame(() => renderApprovals(true));
              return;
            }
            const next = value || '';
            const caretPos = caret ?? next.length;
            if (
              approvalsState.filterMode === 'text' &&
              approvalsState.filter === next &&
              approvalsState.filterCaret === caretPos
            ) {
              return;
            }
            approvalsState.filterMode = 'text';
            approvalsState.filter = next;
            approvalsState.filterCaret = caretPos;
            setFilterModePref('text');
            setFilterPref(next.trim());
            window.requestAnimationFrame(() => renderApprovals(true));
          };

          const filtered =
            filterMode === 'text' && filterNeedle
              ? pending.filter(matchesFilter)
              : filterMode === 'stale'
              ? pending.filter(matchesFilter)
              : pending;
          const sorted = filtered.slice();
          if (sortMode === 'oldest') {
            sorted.sort((a, b) => {
              const ageA = ageMs(a) ?? -Infinity;
              const ageB = ageMs(b) ?? -Infinity;
              return ageB - ageA;
            });
          } else if (sortMode === 'project') {
            sorted.sort((a, b) => {
              const projA = (a?.project || 'unassigned').toLowerCase();
              const projB = (b?.project || 'unassigned').toLowerCase();
              if (projA !== projB) return projA.localeCompare(projB);
              const ageA = ageMs(a) ?? -Infinity;
              const ageB = ageMs(b) ?? -Infinity;
              return ageB - ageA;
            });
          }
          const summary = document.createElement('div');
          summary.className = 'approval-summary';
          const count = document.createElement('strong');
          if (!pending.length) {
            count.textContent = 'No approvals waiting';
          } else if (filterMode === 'stale') {
            count.textContent = `${sorted.length}/${pending.length} stale (≥ ${formatAge(
              approvalsState.staleThresholdMs,
            )})`;
          } else if (filterNeedle) {
            count.textContent = `${sorted.length}/${pending.length} pending`;
          } else {
            count.textContent = `${pending.length} pending`;
          }
          summary.appendChild(count);
          if (model.generated) {
            const timeEl = document.createElement('time');
            timeEl.dateTime = model.generated;
            timeEl.title = new Date(model.generated).toLocaleString();
            timeEl.textContent = fmtRelative(model.generated) || model.generated;
            summary.appendChild(timeEl);
          }
          let oldestTs = null;
          if (pending.length) {
            oldestTs = pending.reduce((acc, item) => {
              const ts = item?.created || item?.updated || null;
              if (!ts) return acc;
              return !acc || new Date(ts).getTime() < new Date(acc).getTime() ? ts : acc;
            }, oldestTs);
            if (oldestTs) {
              const span = document.createElement('span');
              span.className = 'dim';
              span.textContent = `oldest ${fmtRelative(oldestTs) || oldestTs}`;
              summary.appendChild(span);
            }
          }
          el.appendChild(summary);
          const filterRow = document.createElement('div');
          filterRow.className = 'approval-filter row';
          const filterLabel = document.createElement('span');
          filterLabel.className = 'dim';
          filterLabel.textContent = 'Filter';
          const filterInput = document.createElement('input');
          filterInput.type = 'search';
          filterInput.placeholder = 'project, action, reviewer…';
          filterInput.dataset.approvalsFilter = '1';
          filterInput.value = filterMode === 'text' ? approvalsState.filter || '' : '';
          filterInput.addEventListener('input', (ev) => {
            const caret = ev.target.selectionStart ?? ev.target.value.length;
            applyFilterChip('text', ev.target.value, caret);
          });
          filterRow.append(filterLabel, filterInput);
          el.appendChild(filterRow);
          const staleRow = document.createElement('div');
          staleRow.className = 'approval-stale row';
          const staleLabel = document.createElement('span');
          staleLabel.className = 'dim';
          staleLabel.textContent = 'Highlight ≥';
          const staleSelect = document.createElement('select');
          const staleOptions = [
            { label: '15m', value: 15 * 60 * 1000 },
            { label: '30m', value: 30 * 60 * 1000 },
            { label: '1h', value: 60 * 60 * 1000 },
            { label: '4h', value: 4 * 60 * 60 * 1000 },
            { label: '1d', value: 24 * 60 * 60 * 1000 },
          ];
          staleOptions.forEach((opt) => {
            const option = document.createElement('option');
            option.value = String(opt.value);
            option.textContent = opt.label;
            if (opt.value === approvalsState.staleThresholdMs) option.selected = true;
            staleSelect.appendChild(option);
          });
          staleSelect.addEventListener('change', (ev) => {
            const next = parseInt(ev.target.value, 10);
            if (!Number.isFinite(next) || next <= 0) return;
            approvalsState.staleThresholdMs = next;
            window.requestAnimationFrame(() => renderApprovals());
            (async () => setStalePref(next))();
          });
          staleRow.append(staleLabel, staleSelect);
          el.appendChild(staleRow);
          const sortRow = document.createElement('div');
          sortRow.className = 'approval-sort row';
          const sortLabel = document.createElement('span');
          sortLabel.className = 'dim';
          sortLabel.textContent = 'Sort';
          const sortSelect = document.createElement('select');
          const sortOptions = [
            { label: 'Newest first', value: 'newest' },
            { label: 'Oldest first', value: 'oldest' },
            { label: 'Project', value: 'project' },
          ];
          sortOptions.forEach((opt) => {
            const option = document.createElement('option');
            option.value = opt.value;
            option.textContent = opt.label;
            if (opt.value === sortMode) option.selected = true;
            sortSelect.appendChild(option);
          });
          sortSelect.addEventListener('change', (ev) => {
            const next = String(ev.target.value || 'newest');
            if (next === approvalsState.sortMode) return;
            approvalsState.sortMode = next;
            setSortPref(next);
            window.requestAnimationFrame(() => renderApprovals(true));
          });
          sortRow.append(sortLabel, sortSelect);
          el.appendChild(sortRow);
          const chips = [];
          chips.push({ label: 'Clear', value: '', mode: 'text' });
          chips.push({ label: 'Stale only', value: '', mode: 'stale' });
          if (approvalsState.reviewer) {
            chips.push({
              label: `Mine (${approvalsState.reviewer})`,
              value: approvalsState.reviewer,
              mode: 'text',
            });
          }
          const projectSeen = new Set();
          for (const item of pending) {
            const proj = (item?.project || '').trim();
            if (!proj || projectSeen.has(proj)) continue;
            projectSeen.add(proj);
            chips.push({ label: `Project: ${proj}`, value: proj, mode: 'text' });
            if (projectSeen.size >= 3) break;
          }
          const shortcutKeys = ['1', '2', '3', '4', '5'];
          let shortcutIndex = 0;
          chips.forEach((chip) => {
            if (shortcutIndex < shortcutKeys.length) {
              chip.shortcut = shortcutKeys[shortcutIndex++];
            }
          });
          approvalsState.shortcutMap = {};
          const quickWrap = document.createElement('div');
          quickWrap.className = 'approval-filter-chips row';
          const makeChip = (chip) => {
            const { label, value, mode, shortcut } = chip;
            const btn = document.createElement('button');
            btn.type = 'button';
            btn.className = 'ghost btn-small';
            const isActive =
              mode === 'stale'
                ? filterMode === 'stale'
                : filterMode === 'text' && (approvalsState.filter || '') === (value || '');
            if (isActive) {
              btn.classList.add('active');
            }
            btn.dataset.mode = mode;
            btn.textContent = label;
            btn.addEventListener('click', (ev) => {
              ev.preventDefault();
              if (mode === 'stale') {
                applyFilterChip('stale', '');
              } else {
                applyFilterChip('text', value || '');
              }
            });
            if (shortcut) {
              btn.dataset.shortcut = shortcut;
              btn.title = `${label} (Alt+${shortcut})`;
              approvalsState.shortcutMap[shortcut] = chip;
            } else {
              btn.title = label;
            }
            return btn;
          };
          chips.forEach((chip) => quickWrap.appendChild(makeChip(chip)));
          if (quickWrap.childElementCount) {
            el.appendChild(quickWrap);
          }
          if (!approvalsState.shortcutHandler) {
            approvalsState.shortcutHandler = (ev) => {
              if (!ev.altKey || ev.ctrlKey || ev.metaKey || ev.shiftKey) return;
              const key = (ev.key || '').toLowerCase();
              if (!key) return;
              const chip = approvalsState.shortcutMap?.[key];
              if (!chip) return;
              const node = bodyFor('approvals');
              if (!node || !node.isConnected) return;
              const tag = (ev.target?.tagName || '').toLowerCase();
              if (['input', 'textarea', 'select'].includes(tag)) return;
              ev.preventDefault();
              if (chip.mode === 'stale') {
                applyFilterChip('stale', '');
              } else {
                applyFilterChip('text', chip.value || '');
              }
            };
            window.addEventListener('keydown', approvalsState.shortcutHandler);
          }
          appendReviewerRow(el);
          if (!pending.length) {
            const empty = document.createElement('div');
            empty.className = 'dim';
            empty.textContent = 'Queue is clear.';
            el.appendChild(empty);
            approvalsState.filterCaret = null;
            return;
          }
          if (!sorted.length) {
            const empty = document.createElement('div');
            empty.className = 'dim';
            empty.textContent = 'No approvals match filter.';
            el.appendChild(empty);
          } else {
            const maxItems = 8;
            const frag = document.createDocumentFragment();
            sorted.slice(0, maxItems).forEach((item) => {
              frag.appendChild(createApprovalCard(item, sorted.length <= 2));
            });
            el.appendChild(frag);
            if (sorted.length > maxItems) {
              const more = document.createElement('div');
              more.className = 'dim';
              more.textContent = `+${sorted.length - maxItems} more pending`;
              el.appendChild(more);
            }
          }
          const projectMap = new Map();
          const staleProjectMap = new Map();
          let staleTotal = 0;
          sorted.forEach((item) => {
            const proj = (item?.project || 'unassigned').trim() || 'unassigned';
            projectMap.set(proj, (projectMap.get(proj) || 0) + 1);
            const age = ageMs(item);
            if (Number.isFinite(age) && age >= approvalsState.staleThresholdMs) {
              staleTotal += 1;
              staleProjectMap.set(proj, (staleProjectMap.get(proj) || 0) + 1);
            }
          });
          if (staleTotal > 0) {
            const badge = document.createElement('span');
            badge.className = 'badge warn';
            badge.textContent = `≥${formatAge(approvalsState.staleThresholdMs)}: ${staleTotal}`;
            summary.appendChild(badge);
          }
          if (projectMap.size) {
            const stats = Array.from(projectMap.entries())
              .map(([proj, count]) => ({ proj, count }))
              .sort((a, b) => b.count - a.count || a.proj.localeCompare(b.proj))
              .slice(0, 5);
            const statsWrap = document.createElement('div');
            statsWrap.className = 'approval-project-stats';
            const headingRow = document.createElement('div');
            headingRow.className = 'approval-project-stats-header row';
            const heading = document.createElement('div');
            heading.className = 'dim';
            heading.textContent = 'Projects waiting';
            const copyBtn = document.createElement('button');
            copyBtn.type = 'button';
            copyBtn.className = 'ghost btn-small';
            copyBtn.textContent = 'Copy summary';
            copyBtn.addEventListener('click', (ev) => {
              ev.preventDefault();
              ev.stopPropagation();
              const lines = [];
              lines.push(
                `Approvals pending: ${sorted.length}${
                  filterMode === 'text' && filterNeedle ? ` (filtered from ${pending.length})` : ''
                }`,
              );
              lines.push(`Sort mode: ${sortMode}`);
              if (filterMode === 'stale') {
                lines.push(
                  `Mode: stale (≥ ${formatAge(approvalsState.staleThresholdMs)})`,
                );
              }
              if (oldestTs) {
                const rel = fmtRelative(oldestTs) || oldestTs;
                lines.push(`Oldest pending: ${rel}`);
              }
              if (approvalsState.reviewer) {
                lines.push(`Reviewer: ${approvalsState.reviewer}`);
              }
              if (staleTotal > 0) {
                lines.push(`Stale (≥ ${formatAge(approvalsState.staleThresholdMs)}): ${staleTotal}`);
              }
              const projectSummary = stats
                .map(({ proj, count }) => {
                  const staleCount = staleProjectMap.get(proj) || 0;
                  return staleCount
                    ? `${proj}: ${count} (${staleCount} stale)`
                    : `${proj}: ${count}`;
                })
                .join(', ');
              if (projectSummary) {
                lines.push(`Projects: ${projectSummary}`);
              }
              if (projectMap.size > stats.length) {
                lines.push(`(+${projectMap.size - stats.length} more projects)`);
              }
              const text = lines.join('\n');
              try {
                ARW.copy(text);
                ARW.toast('Summary copied');
              } catch (err) {
                console.error('copy summary failed', err);
                ARW.toast('Copy failed');
              }
            });
            headingRow.append(heading, copyBtn);
            statsWrap.appendChild(headingRow);
            const list = document.createElement('ul');
            stats.forEach(({ proj, count }) => {
              const li = document.createElement('li');
              const staleCount = staleProjectMap.get(proj) || 0;
              li.innerHTML = `<span class="mono">${proj}</span> <span class="badge">${count}</span>${
                staleCount ? ` <span class="badge warn">${staleCount} stale</span>` : ''
              }`;
              list.appendChild(li);
            });
            if (projectMap.size > stats.length) {
              const remaining = projectMap.size - stats.length;
              const li = document.createElement('li');
              li.className = 'dim';
              li.textContent = `+${remaining} more project${remaining === 1 ? '' : 's'}`;
              list.appendChild(li);
            }
            statsWrap.appendChild(list);
            el.appendChild(statsWrap);
          }
          if (recent.length) {
            const details = document.createElement('details');
            details.className = 'approval-recent';
            const sum = document.createElement('summary');
            sum.textContent = 'Recent decisions';
            details.appendChild(sum);
            const list = document.createElement('ul');
            recent.slice(0, 5).forEach((item) => {
              const li = document.createElement('li');
              const label = `${item.decision || item.status || 'updated'} · ${item.action_kind || ''}`.trim();
              const span = document.createElement('span');
              span.textContent = label;
              li.appendChild(span);
              const ts = item.updated || item.decided_at || item.created;
              if (ts) {
                li.appendChild(document.createTextNode(' — '));
                const timeEl = document.createElement('time');
                timeEl.dateTime = ts;
                timeEl.title = new Date(ts).toLocaleString();
                timeEl.textContent = fmtRelative(ts) || ts;
                li.appendChild(timeEl);
              }
              list.appendChild(li);
            });
            details.appendChild(list);
            el.appendChild(details);
          }
          if (restoreFilterFocus) {
            window.requestAnimationFrame(() => {
              const field = bodyFor('approvals')?.querySelector('[data-approvals-filter]');
              if (field instanceof HTMLInputElement) {
                field.focus();
                const caret = approvalsState.filterCaret ?? field.value.length;
                try {
                  field.setSelectionRange(caret, caret);
                } catch {}
              }
            });
          } else {
            approvalsState.filterCaret = null;
          }
        };
        const loadLanePrefs = async () => {
          const stateToken = approvalsState;
          if (!stateToken || stateToken.lanePrefsLoaded) return;
          stateToken.lanePrefsLoaded = true;
          try {
            const prefs = await ARW.getPrefs('launcher');
            if (approvalsState !== stateToken || !stateToken) return;
            if (prefs && typeof prefs.approvalsFilter === 'string') {
              stateToken.filter = prefs.approvalsFilter;
            }
            if (prefs && typeof prefs.approvalsFilterMode === 'string') {
              stateToken.filterMode = prefs.approvalsFilterMode === 'stale' ? 'stale' : 'text';
            }
            if (prefs && Number.isFinite(prefs.approvalsStaleMs)) {
              stateToken.staleThresholdMs = prefs.approvalsStaleMs;
            }
            if (prefs && typeof prefs.approvalsSortMode === 'string') {
              stateToken.sortMode = ['newest', 'oldest', 'project'].includes(
                prefs.approvalsSortMode,
              )
                ? prefs.approvalsSortMode
                : 'newest';
            }
          } catch {}
        };
        const loadReviewerPref = async () => {
          const stateToken = approvalsState;
          if (!stateToken || stateToken.reviewerLoaded) return;
          stateToken.reviewerLoaded = true;
          try {
            const prefs = await ARW.getPrefs('launcher');
            if (approvalsState !== stateToken || !stateToken) return;
            const saved =
              prefs && typeof prefs.approvalsReviewer === 'string'
                ? prefs.approvalsReviewer.trim()
                : '';
            if (saved) {
              stateToken.reviewer = saved;
              renderApprovals();
            }
          } catch {}
        };
        const primeApprovals = async () => {
          if (!opts.base || !approvalsState) return;
          const stateToken = approvalsState;
          stateToken.loading = true;
          renderApprovals();
          try {
            const pendingSnap = await ARW.http.json(opts.base, '/state/staging/actions?status=pending&limit=50');
            let recentSnap = null;
            try {
              recentSnap = await ARW.http.json(opts.base, '/state/staging/actions?limit=30');
            } catch (err) {
              console.warn('approvals recent fetch failed', err);
            }
            const current = ARW.read.get('staging_actions') || {};
            const next = { ...current };
            next.generated = new Date().toISOString();
            next.pending = Array.isArray(pendingSnap?.items) ? pendingSnap.items : [];
            if (recentSnap && Array.isArray(recentSnap.items)) {
              next.recent = recentSnap.items;
            }
            ARW.read._store.set('staging_actions', next);
            ARW.read._emit('staging_actions');
            if (approvalsState !== stateToken) return;
            stateToken.error = null;
            stateToken.detail = null;
            stateToken.loading = false;
            renderApprovals();
          } catch (err) {
            const msg = err?.message || String(err);
            if (approvalsState !== stateToken || !stateToken) return;
            stateToken.loading = false;
            stateToken.detail = msg;
            stateToken.error = /HTTP\s+401/.test(msg)
              ? 'Authorize to view approvals queue'
              : 'Approvals queue unavailable';
            renderApprovals();
          }
        };
        Promise.all([loadLanePrefs(), loadReviewerPref()]).then(() => renderApprovals());
        approvalsSub = ARW.read.subscribe('staging_actions', () => renderApprovals());
        if (!approvalsState.lanePrefsLoaded) {
          renderApprovals();
        }
        if (opts.base) {
          primeApprovals();
        }
      }
      if (hasProvenance) {
        const existingSummary = normalizeModularSummary(ARW.read.get('memory_modular_review'));
        if (existingSummary) {
          provenanceSummaryData = existingSummary;
        }
        renderProvenanceSummary();
        provSummarySub = ARW.read.subscribe('memory_modular_review', (model) => {
          const normalized = normalizeModularSummary(model || ARW.read.get('memory_modular_review'));
          if (!normalized) return;
          provenanceSummaryData = normalized;
          renderProvenanceSummary();
        });
        if (opts.base) {
          primeProvenanceSummary();
        }
      }
      // Micro-batched updaters to reduce DOM thrash
      let tlQ = []; let tlTimer = null;
      const rTimeline = (env) => {
        if (!hasTimeline || !env) return;
        tlQ.push(env);
        if (tlTimer) return;
        tlTimer = setTimeout(() => {
          try {
            const el = sections.find(([n]) => n === 'timeline')?.[1];
            if (!el) {
              tlQ.length = 0;
              return;
            }
            if (el.dataset.placeholder) {
              el.innerHTML = '';
              delete el.dataset.placeholder;
            }
            const frag = document.createDocumentFragment();
            const take = tlQ.splice(0, tlQ.length);
            for (const e of take) {
              const d = document.createElement('div');
              d.className = 'evt mono';
              d.textContent = `${e.kind}: ${safeJson(e.env?.payload)}`.slice(0, 800);
              frag.prepend ? frag.prepend(d) : frag.appendChild(d);
            }
            el.prepend(frag);
            while (el.childElementCount > 100) el.removeChild(el.lastChild);
          } finally {
            tlTimer = null;
          }
        }, 50);
      };
      let mdQ = []; let mdTimer = null;
      const rModels = (env) => {
        if (!hasModels || !(env && (env.kind.startsWith('models.') || env.kind === 'state.read.model.patch'))) return;
        mdQ.push(env);
        if (mdTimer) return;
        mdTimer = setTimeout(() => {
          try {
            const el = sections.find(([n]) => n === 'models')?.[1];
            if (!el) {
              mdQ.length = 0;
              return;
            }
            if (el.dataset.placeholder) {
              el.innerHTML = '';
              delete el.dataset.placeholder;
            }
            const frag = document.createDocumentFragment();
            const take = mdQ.splice(0, mdQ.length);
            for (const e of take) {
              const d = document.createElement('div');
              d.className = 'evt mono';
              d.textContent = `${e.kind}: ${safeJson(e.env?.payload)}`.slice(0, 800);
              frag.prepend ? frag.prepend(d) : frag.appendChild(d);
            }
            el.prepend(frag);
            while (el.childElementCount > 60) el.removeChild(el.lastChild);
          } finally {
            mdTimer = null;
          }
        }, 50);
      };
      let provQ = []; let provTimer = null;
      const rProvenance = ({ kind, env }) => {
        if (!hasProvenance || !kind || !kind.startsWith('modular.')) return;
        provQ.push({ kind, env });
        if (provTimer) return;
        provTimer = setTimeout(() => {
          try {
            const el = sections.find(([n]) => n === 'provenance')?.[1];
            if (!el) {
              provQ = [];
              return;
            }
            if (el.dataset.placeholder) {
              el.innerHTML = '';
              delete el.dataset.placeholder;
            }
            if (el.dataset.emptyMsg) {
              el.innerHTML = '';
              delete el.dataset.emptyMsg;
            }
            const frag = document.createDocumentFragment();
            const take = provQ.splice(0, provQ.length);
            for (const entry of take) {
              const payload = entry.env?.payload || entry.env || {};
              const ts = entry.env?.time ? new Date(entry.env.time) : new Date();
              const card = document.createElement('article');
              card.className = 'evt provenance-card';
              card.setAttribute('tabindex', '0');
              const header = document.createElement('div');
              header.className = 'dim';
              header.textContent = `${ts.toLocaleTimeString()} · ${entry.kind}`;
              card.appendChild(header);
              const body = document.createElement('div');
              body.className = 'prov-body';
              const addLine = (label, value) => {
                if (value === null || value === undefined) return;
                let text = '';
                if (Array.isArray(value)) {
                  if (!value.length) return;
                  const joined = value.map((v) => (typeof v === 'string' ? v : safeJson(v))).join(', ');
                  text = joined.length > 180 ? `${joined.slice(0, 177)}…` : joined;
                } else if (typeof value === 'object') {
                  try { text = JSON.stringify(value); } catch { text = String(value); }
                } else {
                  text = String(value);
                  if (!text.trim()) return;
                }
                const row = document.createElement('div');
                row.className = 'prov-row';
                const tag = document.createElement('span');
                tag.className = 'pill';
                tag.textContent = label;
                const span = document.createElement('span');
                span.textContent = text;
                row.append(tag, span);
                body.appendChild(row);
              };
              const addPolicyScope = (scope) => {
                if (!scope || typeof scope !== 'object') return;
                const caps = Array.isArray(scope.capabilities) ? scope.capabilities : [];
                if (caps.length) addLine('Capabilities', caps);
                const leases = Array.isArray(scope.leases)
                  ? scope.leases
                      .map((lease) => (lease && typeof lease === 'object' ? (lease.capability || lease.id || '') : String(lease || '')))
                      .filter((item) => item && typeof item === 'string' && item.trim())
                  : [];
                if (leases.length) addLine('Leases', leases);
                if (scope.requires_human_review) addLine('Requires review', 'yes');
              };
              if (entry.kind === 'modular.agent.accepted') {
                addLine('Agent', payload.agent_id || 'unknown');
                addLine('Intent', payload.intent);
                if (Number.isFinite(payload.confidence)) {
                  addLine('Confidence', `${(Number(payload.confidence) * 100).toFixed(1)}%`);
                }
                addLine('Latency budget', Number.isFinite(payload.latency_budget_ms) ? `${payload.latency_budget_ms} ms` : null);
                if (Array.isArray(payload.context_refs) && payload.context_refs.length) {
                  addLine('Context refs', payload.context_refs.slice(0, 4));
                }
                addPolicyScope(payload.policy_scope);
              } else if (entry.kind === 'modular.tool.accepted') {
                addLine('Tool', payload.tool_id || 'unknown');
                addLine('Operation', payload.operation_id);
                addLine('Requested by', payload.requested_by);
                const statusLabel = typeof payload.result_status === 'string' && payload.result_status.trim() ? payload.result_status.trim() : null;
                if (statusLabel) addLine('Result status', statusLabel);
                if (Number.isFinite(payload.result_latency_ms)) {
                  addLine('Latency', `${payload.result_latency_ms} ms`);
                }
                const reqCaps = Array.isArray(payload.required_capabilities)
                  ? payload.required_capabilities.filter((cap) => typeof cap === 'string' && cap.trim())
                  : [];
                if (reqCaps.length) addLine('Required capabilities', reqCaps);
                if (payload.sandbox_requirements && typeof payload.sandbox_requirements === 'object') {
                  const req = payload.sandbox_requirements;
                  const details = [];
                  if (req.needs_network) details.push('network');
                  if (Array.isArray(req.filesystem_scopes) && req.filesystem_scopes.length) {
                    details.push(`fs: ${req.filesystem_scopes.slice(0, 4).join(', ')}`);
                  }
                  if (req.environment && typeof req.environment === 'object') {
                    const keys = Object.keys(req.environment);
                    if (keys.length) details.push(`env vars: ${keys.length}`);
                  }
                  if (details.length) addLine('Sandbox', details.join(' · '));
                }
                addPolicyScope(payload.policy_scope);
                if (payload.payload_summary && typeof payload.payload_summary === 'object') {
                  const summary = payload.payload_summary;
                  if (summary.needs_network) addLine('Needs network', 'yes');
                  if (Number(summary.filesystem_scopes) > 0) addLine('Filesystem scopes', summary.filesystem_scopes);
                }
                if (Array.isArray(payload.result_output_keys) && payload.result_output_keys.length) {
                  addLine('Result keys', payload.result_output_keys.slice(0, 5));
                }
                addLine('Evidence', payload.evidence_id);
              }
              if (Number.isFinite(payload.created_ms)) {
                const created = new Date(Number(payload.created_ms));
                if (!Number.isNaN(created.getTime())) {
                  addLine('Created', created.toLocaleTimeString());
                }
              }
              const actions = document.createElement('div');
              actions.className = 'row';
              const copyBtn = document.createElement('button');
              copyBtn.type = 'button';
              copyBtn.className = 'ghost btn-small';
              copyBtn.textContent = 'Copy JSON';
              copyBtn.addEventListener('click', () => {
                try { ARW.copy(JSON.stringify(payload, null, 2)); } catch {}
              });
              actions.appendChild(copyBtn);
              card.appendChild(body);
              card.appendChild(actions);
              frag.prepend ? frag.prepend(card) : frag.appendChild(card);
            }
            const elBody = sections.find(([n]) => n === 'provenance')?.[1];
            if (elBody) {
              elBody.prepend(frag);
              while (elBody.childElementCount > 30) elBody.removeChild(elBody.lastChild);
            }
          } finally {
            provTimer = null;
          }
        }, 75);
      };
      // Policy lane: poll /state/policy (read-only) if base provided
      let policyTimer = null;
      const rPolicy = async () => {
        if (!hasPolicy) return;
        const el = sections.find(([n])=>n==='policy')?.[1]; if (!el || !opts.base) return;
        try {
          const j = await ARW.http.json(opts.base, '/state/policy');
          const leases = j?.leases || j?.data?.leases || [];
          el.innerHTML = '';
          if (!Array.isArray(leases) || leases.length===0) { el.innerHTML = '<div class="dim">No active leases</div>'; return; }
          for (const l of leases) {
            const p = document.createElement('div'); p.className='pill';
            const capability = String(l.capability || l.cap || l.scope || l.key || '').trim();
            const ttlMs = Number(l.ttl_ms ?? l.ttl ?? 0);
            const ttlText = Number.isFinite(ttlMs) && ttlMs > 0 ? `${ttlMs} ms` : '—';
            const who = String(l.principal || l.subject || l.owner || '').trim();
            const scopeIndex = window.__scopeCapabilityIndex;
            const scopeMatch = capability && scopeIndex && scopeIndex.get(capability);

            const parts = [];
            parts.push(`<span class="tag">${escapeHtml(capability || 'unknown')}</span>`);
            if (scopeMatch) {
              parts.push(`<span class="scope-tag">scope:${escapeHtml(scopeMatch.label)}</span>`);
            }
            parts.push(`<span class="dim">${escapeHtml(ttlText)}</span>`);
            if (who) parts.push(`<span class="dim">${escapeHtml(who)}</span>`);
            p.innerHTML = parts.join(' ');
            el.appendChild(p);
          }
        } catch {}
      };
      if (opts.base) {
        rPolicy(); policyTimer = setInterval(rPolicy, 5000);
      }
      // Context lane: fetch top claims (world.select)
      let contextTimer = null;
      let contextAbort = null;
      const contextCache = new Map();
      let contextLastProject = null;
      const pointerKeyCandidates = ['ptr','pointer','stable_ptr','stablePointer'];
      const textFields = ['summary','text','description','body','content'];
      const isPointerLike = (value) => {
        if (!value || typeof value !== 'object' || Array.isArray(value)) return false;
        const kind = String(value.kind || '').trim();
        if (!kind) return false;
        if (value.id || value.path || value.sha || value.url || value.pointer || value.offset != null) return true;
        return false;
      };
      const pointerKey = (ptr) => {
        try {
          const sorted = Object.keys(ptr || {})
            .sort()
            .map((k) => [k, ptr[k]])
            .reduce((acc, [k, v]) => {
              acc[k] = v;
              return acc;
            }, {});
          return JSON.stringify(sorted);
        } catch {
          return null;
        }
      };
      const pointerSupportsRehydrate = (ptr) => {
        const kind = String(ptr?.kind || '').toLowerCase();
        return kind === 'memory' || kind === 'file';
      };
      const contextFormatRelative = (iso) => {
        if (!iso) return '';
        try {
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
          for (const unit of units) {
            if (absSec < unit.limit) {
              const value = Math.max(1, Math.floor(absSec / unit.div));
              return diffMs >= 0 ? `${value}${unit.label} ago` : `in ${value}${unit.label}`;
            }
          }
          const years = Math.max(1, Math.floor(absSec / 31536000));
          return diffMs >= 0 ? `${years}y ago` : `in ${years}y`;
        } catch {
          return '';
        }
      };
      const extractPointer = (value) => {
        if (isPointerLike(value)) return value;
        if (!value || typeof value !== 'object') return null;
        for (const key of pointerKeyCandidates) {
          const candidate = value[key];
          if (isPointerLike(candidate)) return candidate;
        }
        if (value.artifact) {
          const nested = extractPointer(value.artifact);
          if (nested) return nested;
        }
        if (value.memory) {
          const nested = extractPointer(value.memory);
          if (nested) return nested;
        }
        if (value.context && typeof value.context === 'object') {
          for (const key of pointerKeyCandidates) {
            const nested = value.context[key];
            if (isPointerLike(nested)) return nested;
          }
        }
        return null;
      };
      const collectPointers = (item) => {
        const pointers = [];
        const seen = new Set();
        const pushPtr = (ptr, label, detail) => {
          if (!isPointerLike(ptr)) return;
          const key = pointerKey(ptr);
          if (!key || seen.has(key)) return;
          seen.add(key);
          pointers.push({ ptr, label, detail });
        };
        pushPtr(item?.ptr, 'Pointer', null);
        if (item?.props && typeof item.props === 'object') {
          pushPtr(item.props.ptr, 'Props pointer', null);
          pushPtr(item.props.pointer, 'Props pointer', null);
        }
        const provenance = Array.isArray(item?.provenance) ? item.provenance : [];
        provenance.forEach((entry, idx) => {
          const base = entry?.kind ? `Provenance · ${entry.kind}` : 'Provenance';
          const observed = entry?.observed_at || entry?.observedAt || null;
          const sources = Array.isArray(entry?.sources) ? entry.sources : [];
          if (!sources.length) {
            const ptr = extractPointer(entry);
            if (ptr) pushPtr(ptr, base, observed);
            return;
          }
          sources.forEach((src, sIdx) => {
            const ptr = extractPointer(src);
            if (!ptr) return;
            const hint = src?.label || src?.kind || src?.lane || `source ${sIdx + 1}`;
            pushPtr(ptr, `${base} · ${hint}`, observed);
          });
        });
        return pointers;
      };
      const renderContextMessage = (el, text, tone = 'info') => {
        if (!el) return;
        const span = document.createElement('div');
        span.className = 'context-msg';
        if (tone === 'warn') span.classList.add('warn');
        span.textContent = text;
        el.innerHTML = '';
        el.appendChild(span);
      };
      const trimText = (text, cap = 2000) => {
        if (typeof text !== 'string') return '';
        if (text.length <= cap) return text;
        return `${text.slice(0, cap - 1)}…`;
      };
      const resolveTitle = (item) => {
        const props = item?.props || {};
        const candidates = [props.title, props.name, props.heading, props.summary, props.text, item?.id];
        const found = candidates.find((val) => typeof val === 'string' && val.trim());
        return (found || 'Untitled').toString().slice(0, 160);
      };
      const resolveExcerpt = (item) => {
        const props = item?.props || {};
        for (const field of textFields) {
          const raw = props[field];
          if (typeof raw === 'string' && raw.trim()) {
            return trimText(raw.trim(), 360);
          }
        }
        return '';
      };
      const renderPointerBlock = (ptrData, card) => {
        const block = document.createElement('div');
        block.className = 'context-pointer-block';
        const row = document.createElement('div');
        row.className = 'context-pointer';
        const label = document.createElement('span');
        label.className = 'context-pointer-label';
        label.textContent = ptrData.label || `Pointer (${ptrData.ptr.kind || 'unknown'})`;
        if (ptrData.detail) label.title = ptrData.detail;
        row.appendChild(label);
        const btns = document.createElement('div');
        btns.className = 'context-pointer-buttons';
        const copyBtn = document.createElement('button');
        copyBtn.type = 'button';
        copyBtn.className = 'ghost btn-small';
        copyBtn.textContent = 'Copy pointer';
        copyBtn.addEventListener('click', () => {
          try {
            ARW.copy(JSON.stringify(ptrData.ptr, null, 2));
          } catch {
            ARW.toast('Copy failed');
          }
        });
        btns.appendChild(copyBtn);
        const preview = document.createElement('div');
        preview.className = 'context-pointer-preview';
        preview.hidden = true;
        const supportsRehydrate = pointerSupportsRehydrate(ptrData.ptr);
        if (supportsRehydrate && opts.base) {
          const reBtn = document.createElement('button');
          reBtn.type = 'button';
          reBtn.className = 'btn-small';
          reBtn.textContent = 'Rehydrate';
          reBtn.addEventListener('click', async () => {
            if (!opts.base) {
              ARW.toast('Start the server first');
              return;
            }
            reBtn.disabled = true;
            reBtn.textContent = 'Loading…';
            preview.hidden = false;
            preview.innerHTML = '<div class="dim">Fetching…</div>';
            try {
              const key = pointerKey(ptrData.ptr);
              if (key && contextCache.has(key)) {
                renderRehydrateResult(preview, contextCache.get(key));
              } else {
                const body = JSON.stringify({ ptr: ptrData.ptr });
                const resp = await ARW.http.fetch(opts.base, '/context/rehydrate', {
                  method: 'POST',
                  headers: { 'Content-Type': 'application/json' },
                  body,
                });
                if (!resp.ok) {
                  throw new Error(`HTTP ${resp.status}`);
                }
                const data = await resp.json();
                if (key) contextCache.set(key, data);
                renderRehydrateResult(preview, data);
              }
            } catch (err) {
              preview.innerHTML = `<div class="context-preview-meta">${err?.message || 'Rehydrate failed'}</div>`;
            } finally {
              reBtn.disabled = false;
              reBtn.textContent = 'Rehydrate';
            }
          });
          btns.appendChild(reBtn);
        }
        row.appendChild(btns);
        block.appendChild(row);
        block.appendChild(preview);
        card.appendChild(block);
      };
      const renderRehydrateResult = (previewNode, data) => {
        if (!previewNode) return;
        const renderHeader = (text) => {
          const meta = document.createElement('div');
          meta.className = 'context-preview-meta';
          meta.textContent = text;
          previewNode.appendChild(meta);
        };
        previewNode.innerHTML = '';
        if (data?.file) {
          const info = data.file;
          renderHeader(`File · ${info.path || ''} (${info.head_bytes ?? '0'} bytes)`);
          const tools = document.createElement('div');
          tools.className = 'row';
          const copyBtn = document.createElement('button');
          copyBtn.type = 'button';
          copyBtn.className = 'ghost btn-small';
          copyBtn.textContent = 'Copy content';
          copyBtn.addEventListener('click', () => {
            try { ARW.copy(data.content || ''); } catch {}
          });
          tools.appendChild(copyBtn);
          previewNode.appendChild(tools);
          const pre = document.createElement('pre');
          pre.textContent = trimText(data.content || '', 4000);
          previewNode.appendChild(pre);
        } else if (data?.memory) {
          const record = data.memory;
          const lane = record.lane || record.kind || 'memory';
          renderHeader(`Memory · ${lane}${record.id ? ` · ${record.id}` : ''}`);
          const tools = document.createElement('div');
          tools.className = 'row';
          const copyBtn = document.createElement('button');
          copyBtn.type = 'button';
          copyBtn.className = 'ghost btn-small';
          copyBtn.textContent = 'Copy JSON';
          copyBtn.addEventListener('click', () => {
            try { ARW.copy(JSON.stringify(record, null, 2)); } catch {}
          });
          tools.appendChild(copyBtn);
          previewNode.appendChild(tools);
          const value = record.value || record.body || record.text || record.content || record.data;
          const pre = document.createElement('pre');
          if (typeof value === 'string') {
            pre.textContent = trimText(value, 4000);
          } else {
            try {
              pre.textContent = trimText(JSON.stringify(value ?? record, null, 2), 4000);
            } catch {
              pre.textContent = '[unserializable]';
            }
          }
          previewNode.appendChild(pre);
        } else {
          renderHeader('No preview available');
          const pre = document.createElement('pre');
          try {
            pre.textContent = trimText(JSON.stringify(data ?? {}, null, 2), 4000);
          } catch {
            pre.textContent = '[unserializable]';
          }
          previewNode.appendChild(pre);
        }
      };
      const renderContextItems = (items) => {
        const el = sections.find(([n])=>n==='context')?.[1];
        if (!el) return;
        el.innerHTML = '';
        if (!items.length) {
          renderContextMessage(el, 'No beliefs yet', 'info');
          return;
        }
        for (const item of items) {
          const card = document.createElement('article');
          card.className = 'context-item';
          const head = document.createElement('div');
          head.className = 'context-head';
          const title = document.createElement('div');
          title.className = 'context-title';
          title.textContent = resolveTitle(item);
          head.appendChild(title);
          const badges = document.createElement('div');
          badges.className = 'context-badges';
          const confidence = Number(item?.confidence ?? item?.props?.confidence);
          if (Number.isFinite(confidence)) {
            const badge = document.createElement('span');
            badge.className = 'badge';
            badge.textContent = `Conf ${(confidence * 100).toFixed(0)}%`;
            badges.appendChild(badge);
          }
          const last = item?.last || item?.props?.last || item?.props?.updated;
          if (last) {
            const badge = document.createElement('span');
            badge.className = 'badge';
            badge.textContent = contextFormatRelative(last) || 'Observed';
            badge.title = new Date(last).toLocaleString();
            badges.appendChild(badge);
          }
          if (badges.childElementCount) head.appendChild(badges);
          card.appendChild(head);
          const excerpt = resolveExcerpt(item);
          if (excerpt) {
            const para = document.createElement('div');
            para.className = 'context-excerpt';
            para.textContent = excerpt;
            card.appendChild(para);
          }
          const trace = item?.trace;
          if (trace && typeof trace === 'object') {
            const parts = [];
            if (Number.isFinite(trace.hits_id)) parts.push(`id hits ${trace.hits_id}`);
            if (Number.isFinite(trace.hits_props)) parts.push(`props hits ${trace.hits_props}`);
            if (Number.isFinite(trace.conf)) parts.push(`conf ${trace.conf.toFixed(2)}`);
            if (Number.isFinite(trace.recency)) parts.push(`recency ${trace.recency.toFixed(2)}`);
            if (parts.length) {
              const meta = document.createElement('div');
              meta.className = 'context-trace';
              meta.textContent = parts.join(' · ');
              card.appendChild(meta);
            }
          }
          const pointers = collectPointers(item);
          if (pointers.length) {
            pointers.forEach((ptr) => renderPointerBlock(ptr, card));
          }
          const more = document.createElement('details');
          more.className = 'context-more';
          const summary = document.createElement('summary');
          summary.textContent = 'Inspect raw';
          more.appendChild(summary);
          const pre = document.createElement('pre');
          try {
            pre.textContent = trimText(JSON.stringify(item, null, 2), 4000);
          } catch {
            pre.textContent = '[unserializable]';
          }
          more.appendChild(pre);
          card.appendChild(more);
          el.appendChild(card);
        }
      };
      const refreshContext = async (force = false, reason = '') => {
        const el = sections.find(([n])=>n==='context')?.[1];
        if (!el) return;
        const project = typeof opts.getProject === 'function' ? opts.getProject() : null;
        const base = opts.base;
        if (!base) {
          renderContextMessage(el, 'Connect to the server to inspect context', 'warn');
          contextLastProject = project || null;
          return;
        }
        if (contextAbort) {
          contextAbort.abort();
          contextAbort = null;
        }
        contextAbort = new AbortController();
        const params = new URLSearchParams();
        params.set('k', '12');
        if (project) params.set('proj', project);
        try {
          if (force || contextLastProject !== project) {
            renderContextMessage(el, project ? `Loading context for ${project}…` : 'Loading context…', 'info');
          }
          const path = `/state/world/select?${params.toString()}`;
          const j = await ARW.http.json(base, path, { signal: contextAbort.signal });
          const items = j?.items || j?.data?.items || [];
          contextLastProject = project || null;
          renderContextItems(items);
        } catch (err) {
          if (err?.name === 'AbortError') return;
          const msg = err?.message || 'Context unavailable';
          renderContextMessage(el, msg, 'warn');
        } finally {
          contextAbort = null;
        }
      };
      const scheduleContextRefresh = (immediate = false, reason = '') => {
        if (!hasContext) return;
        if (contextTimer) {
          clearInterval(contextTimer);
          contextTimer = null;
        }
        if (!opts.base) {
          refreshContext(true, reason);
          return;
        }
        if (immediate) {
          refreshContext(true, reason);
        }
        contextTimer = setInterval(() => {
          refreshContext(false, 'interval');
        }, 15000);
      };
      if (hasContext) {
        if (opts.base) {
          scheduleContextRefresh(true, 'initial');
        } else {
          refreshContext(true, 'initial');
        }
      }
      // client-side trend store for p95 sparkline
      ARW.metricsTrend = ARW.metricsTrend || { _m: new Map(), push(route,p){ const a=this._m.get(route)||[]; a.push(Number(p)||0); if(a.length>32)a.shift(); this._m.set(route,a); }, get(route){ return this._m.get(route)||[] } };
      function sparkline(vals){ const v=(vals||[]).slice(-32); if(!v.length) return ''; const w=90,h=18,max=Math.max(1,...v); const pts=v.map((x,i)=>{const xx=Math.round(i*(w-2)/Math.max(1,v.length-1))+1; const yy=h-1-Math.round((x/max)*(h-2)); return `${xx},${yy}`;}).join(' '); return `<svg class="spark" viewBox="0 0 ${w} ${h}" xmlns="http://www.w3.org/2000/svg"><polyline fill="none" stroke="var(--status-accent)" stroke-width="1.5" points="${pts}"/></svg>`; }
      const rMetrics = async () => {
        if (!hasMetrics) return;
        const el = sections.find(([n])=>n==='metrics')?.[1]; if (!el) return;
        const model = ARW.read.get('route_stats') || {};
        const by = model.by_path || {};
        const rows = Object.entries(by)
          .map(([p, s]) => ({ p, hits: s.hits||0, p95: s.p95_ms||0, ewma: s.ewma_ms||0, max: s.max_ms||0 }))
          .sort((a,b)=> b.hits - a.hits)
          .slice(0, 6);
        el.innerHTML = '';
        const tbl = document.createElement('table');
        const slo = await ARW.slo();
        const thead = document.createElement('thead'); thead.innerHTML = `<tr><th>route</th><th>hits</th><th>p95 ≤ ${slo}</th><th>ewma</th><th>max</th><th></th></tr>`;
        tbl.appendChild(thead);
        const tb = document.createElement('tbody');
        for (const r of rows) {
          const tr = document.createElement('tr');
          const p95c = r.p95 <= slo ? 'ok' : '';
          ARW.metricsTrend.push(r.p, r.p95);
          const sp = sparkline(ARW.metricsTrend.get(r.p));
          tr.innerHTML = `<td class="mono">${r.p}</td><td>${r.hits}</td><td class="${p95c}">${r.p95}</td><td>${r.ewma.toFixed ? r.ewma.toFixed(1) : r.ewma}</td><td>${r.max}</td><td>${sp}</td>`;
          tb.appendChild(tr);
        }
        tbl.appendChild(tb);
        el.appendChild(tbl);

        const snappy = ARW.read.get('snappy') || null;
        const snappyBox = document.createElement('div');
        snappyBox.style.marginTop = '12px';
        snappyBox.className = 'snappy-detail';
        if (snappy && snappy.observed) {
          const breach = !!(snappy.breach && snappy.breach.full_result);
          if (breach) {
            snappyBox.style.borderLeft = '4px solid var(--color-warn, #d97706)';
            snappyBox.style.paddingLeft = '8px';
          }
          const budget = snappy?.budgets?.full_result_p95_ms;
          const header = document.createElement('div');
          header.className = 'dim';
          header.textContent = `Snappy budget ≤ ${budget ?? '–'} ms — observed max: ${snappy.observed.max_p95_ms ?? '–'} ms (${snappy.observed.max_path || 'n/a'})`;
          snappyBox.appendChild(header);
          const routes = Object.entries(snappy.observed.routes || {})
            .map(([path, stats]) => ({
              path,
              p95: Number(stats?.p95_ms ?? 0),
              hits: Number(stats?.hits ?? 0),
            }))
            .sort((a, b) => b.p95 - a.p95)
            .slice(0, 4);
          if (routes.length) {
            const tblRoutes = document.createElement('table');
            tblRoutes.innerHTML = '<thead><tr><th>path</th><th>p95</th><th>hits</th></tr></thead>';
            const body = document.createElement('tbody');
            routes.forEach((r) => {
              const tr = document.createElement('tr');
              tr.innerHTML = `<td class="mono">${r.path}</td><td>${r.p95}</td><td>${r.hits}</td>`;
              body.appendChild(tr);
            });
            tblRoutes.appendChild(body);
            snappyBox.appendChild(tblRoutes);
          } else {
            const empty = document.createElement('div');
            empty.className = 'dim';
            empty.textContent = 'Snappy: no protected routes observed yet';
            snappyBox.appendChild(empty);
          }
        } else {
          const wait = document.createElement('div');
          wait.className = 'dim';
          wait.textContent = 'Snappy detail: waiting for data';
          snappyBox.appendChild(wait);
        }
        el.appendChild(snappyBox);
      };
      function safeJson(v){ try { return JSON.stringify(v); } catch { return String(v) } }
      const idAll = hasTimeline ? ARW.sse.subscribe('*', rTimeline) : null;
      const idModels = hasModels ? ARW.sse.subscribe((k)=> k.startsWith('models.'), rModels) : null;
      const idProvenance = hasProvenance ? ARW.sse.subscribe((k)=> k.startsWith('modular.'), rProvenance) : null;
      const idMetrics = hasMetrics ? ARW.read.subscribe('route_stats', rMetrics) : null;
      const idSnappy = hasMetrics ? ARW.read.subscribe('snappy', rMetrics) : null;
      // Activity lane: listen for screenshots.captured and render thumbnails
      const rActivity = ({ env }) => {
        if (!hasActivity) return;
        const el = sections.find(([n])=>n==='activity')?.[1]; if (!el) return;
        const p = env?.payload || env;
        const kind = env?.kind || '';
        if (!kind.startsWith('screenshots.')) return;
        if (kind === 'screenshots.ocr.completed') {
          const src = p?.source_path || p?.sourcePath || p?.path;
          ARW._storeOcrResult(src, p);
          return;
        }
        if (kind !== 'screenshots.captured') return;
        const box = document.createElement('div'); box.className='evt';
        const ts = env?.time || new Date().toISOString();
        const img = document.createElement('img');
        img.dataset.screenshotPath = p?.path||'';
        img.alt = ARW._bestAltForPath(p?.path, p?.path||'');
        img.style.maxWidth='100%'; img.style.maxHeight='120px';
        if (p?.preview_b64 && /^data:image\//.test(p.preview_b64)) { img.src = p.preview_b64; }
        else { img.src = ''; img.style.display='none'; }
        const cap = document.createElement('div'); cap.className='dim mono'; cap.textContent = `${ts} ${p?.path||''}`;
      const actions = document.createElement('div'); actions.className='row';
      const openBtn = document.createElement('button'); openBtn.className='ghost'; openBtn.textContent='Open'; openBtn.addEventListener('click', async ()=>{ try{ if (p?.path) await ARW.invoke('open_path', { path: p.path }); }catch(e){ console.error(e); } });
      const copyBtn = document.createElement('button'); copyBtn.className='ghost'; copyBtn.textContent='Copy path'; copyBtn.addEventListener('click', ()=>{ if (p?.path) ARW.copy(String(p.path)); });
        const mdBtn = document.createElement('button'); mdBtn.className='ghost'; mdBtn.textContent='Copy MD'; mdBtn.addEventListener('click', ()=>{ if (p?.path) ARW.copyMarkdown(p.path); });
        const annBtn = document.createElement('button'); annBtn.className='ghost'; annBtn.textContent='Annotate'; annBtn.addEventListener('click', async ()=>{ try{ if (p?.preview_b64){ const rects = await ARW.annot.start(p.preview_b64); const res = await ARW.invoke('run_tool_admin', { id: 'ui.screenshot.annotate_burn', input: { path: p.path, annotate: rects, downscale:640 }, port: ARW.toolPort() }); if (res && res.preview_b64){ img.src = res.preview_b64; cap.textContent = `${ts} ${res.path||''}`; } } else { ARW.toast('No preview for annotate'); } }catch(e){ console.error(e); }});
        const saveBtn = document.createElement('button'); saveBtn.className='ghost'; saveBtn.textContent='Save to project'; saveBtn.addEventListener('click', async ()=>{ if (p?.path){ const res = await ARW.saveToProjectPrompt(p.path); if (res) await ARW.maybeAppendToNotes(res.proj, res.dest, p.path); } });
        actions.appendChild(openBtn); actions.appendChild(copyBtn); actions.appendChild(mdBtn); actions.appendChild(annBtn); actions.appendChild(saveBtn);
        box.appendChild(img); box.appendChild(cap); box.appendChild(actions);
        el.prepend(box);
        if (p?.path) ARW._updateAltForPath(p.path);
        while (el.childElementCount>6) el.removeChild(el.lastChild);
      };
      const idActivity = hasActivity ? ARW.sse.subscribe((k)=> k.startsWith('screenshots.'), rActivity) : null;
      if (hasProvenance) {
        if (!opts.base) {
          renderLaneMessage('provenance', 'Connect to the server to see modular agent and tool evidence.', 'warn');
        } else {
          renderLaneMessage('provenance', 'Waiting for modular events…');
        }
      }
      // initial render for metrics if any
      if (hasMetrics) {
        rMetrics();
      }
      return {
        dispose() {
          if (idAll != null) ARW.sse.unsubscribe(idAll);
          if (idModels != null) ARW.sse.unsubscribe(idModels);
          if (idProvenance != null) ARW.sse.unsubscribe(idProvenance);
          if (idMetrics != null) ARW.read.unsubscribe(idMetrics);
          if (idSnappy != null) ARW.read.unsubscribe(idSnappy);
          if (approvalsSub) ARW.read.unsubscribe(approvalsSub);
          if (provSummarySub) ARW.read.unsubscribe(provSummarySub);
          if (idActivity != null) ARW.sse.unsubscribe(idActivity);
          if (policyTimer) clearInterval(policyTimer);
          if (contextTimer) {
            clearInterval(contextTimer);
            contextTimer = null;
          }
          if (contextAbort) {
            contextAbort.abort();
            contextAbort = null;
          }
          contextCache.clear();
          if (approvalsState && approvalsState.shortcutHandler) {
            window.removeEventListener('keydown', approvalsState.shortcutHandler);
            approvalsState.shortcutHandler = null;
          }
          if (approvalsState) {
            approvalsState.shortcutMap = {};
          }
          approvalsState = null;
          provenanceSummaryData = null;
          provSummarySub = null;
          provenanceSummaryFetched = false;
          node.innerHTML = '';
        },
        refresh(optsRefresh = {}) {
          const immediate = !!optsRefresh.immediate;
          scheduleContextRefresh(immediate, optsRefresh.reason || 'manual');
        },
      };
    }
  };
};

waitForARW().then((arw) => {
  installSidecar(arw);
}).catch((err) => {
  console.warn('[sidecar] bootstrap skipped:', err?.message || err);
});
