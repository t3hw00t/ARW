// Minimal logic to keep the mascot lively and semi-empathetic
(function(){
  const HAS_TAURI = !!(window.__TAURI__ && window.__TAURI__.invoke);
  const searchParams = (()=>{
    try { return new URLSearchParams(window.location.search || ''); }
    catch { return new URLSearchParams(); }
  })();
  const initialProfile = searchParams.get('profile') || 'global';
  const initialCharacter = searchParams.get('character') || 'guide';
  const initialQuiet = searchParams.get('quiet') === '1';
  const initialCompact = searchParams.get('compact') === '1';

  let state = 'unknown';
  let healthTimer = null;
  let idleTimer = null;
  let dragging = false;
  let pointer = { x: null, y: null };
  let config = {
    allowInteractions: false,
    intensity: 'normal',
    snapWindows: true,
    quietMode: initialQuiet,
    compactMode: initialCompact,
    character: initialCharacter,
    profile: initialProfile,
    profileName: initialProfile === 'global' ? 'Global mascot' : initialProfile.replace(/^project:/, ''),
  };
  try {
    document.body.dataset.profile = config.profile;
    document.body.dataset.character = config.character;
    document.body.dataset.profileName = config.profileName;
  } catch {}
  let previewAnchor = null;
  let previewTimer = null;
  let savedHint = null;
  let lastHint = '';
  let streamingCount = 0;
  let statusOverride = null;
  const activeStreams = new Map();
  const SNAP_HINTS = {
    'left': 'Docking left edge…',
    'right': 'Docking right edge…',
    'top': 'Docking top edge…',
    'bottom': 'Docking bottom edge…',
    'top-left': 'Docking top-left corner…',
    'top-right': 'Docking top-right corner…',
    'bottom-left': 'Docking bottom-left corner…',
    'bottom-right': 'Docking bottom-right corner…',
  };
  const STATE_LABELS = {
    ready: 'Ready',
    thinking: 'Working',
    concern: 'Check',
    error: 'Error',
  };
  const CHARACTER_ORDER = ['guide','engineer','researcher','navigator','guardian'];
  const hint = () => document.getElementById('mascotHint');
  const statusPill = () => document.getElementById('mascotStatus');

  async function loadMascotPrefs(){
    if (!window.ARW || typeof window.ARW.getPrefs !== 'function') return {};
    try {
      const prefs = await window.ARW.getPrefs('mascot');
      return prefs && typeof prefs === 'object' ? { ...prefs } : {};
    } catch (err) {
      console.error(err);
      return {};
    }
  }

  async function storeMascotPrefs(prefs){
    if (!window.ARW || typeof window.ARW.setPrefs !== 'function') return;
    try {
      await window.ARW.setPrefs('mascot', prefs);
    } catch (err) {
      console.error(err);
    }
  }

  function ensureProfilePrefs(prefs, profile){
    if (profile === 'global') return prefs;
    if (!prefs.profiles || typeof prefs.profiles !== 'object') {
      prefs.profiles = {};
    }
    if (!prefs.profiles[profile] || typeof prefs.profiles[profile] !== 'object') {
      prefs.profiles[profile] = {};
    }
    return prefs.profiles[profile];
  }

  function buildConfigPayload(prefs, profileKey, entryOverrides = {}, extra = {}) {
    const isGlobal = profileKey === 'global';
    const entry = isGlobal ? prefs : entryOverrides;
    const quiet = isGlobal
      ? !!(prefs.quietMode ?? false)
      : !!(entry.quietMode ?? prefs.quietMode ?? false);
    const compact = isGlobal
      ? !!(prefs.compactMode ?? false)
      : !!(entry.compactMode ?? prefs.compactMode ?? false);
    const character = isGlobal
      ? (prefs.character || config.character)
      : (entry.character || prefs.character || config.character);
    const name = isGlobal
      ? (prefs.profileName || 'Global mascot')
      : (entry.name || prefs.profileName || profileKey.replace(/^project:/, ''));
    return {
      profile: profileKey,
      allowInteractions: !(prefs.clickThrough ?? true),
      intensity: prefs.intensity || 'normal',
      snapWindows: prefs.snapWindows !== false,
      quietMode: quiet,
      compactMode: compact,
      character,
      name,
      ...extra,
    };
  }

  function slugifyName(raw) {
    return String(raw || '')
      .toLowerCase()
      .replace(/[^a-z0-9]+/g, '-')
      .replace(/^-+|-+$/g, '')
      || 'project';
  }

  function applyProfileName(raw) {
    const fallback = raw && raw.trim()
      ? raw.trim()
      : (config.profile === 'global' ? 'Global mascot' : config.profile.replace(/^project:/, ''));
    config.profileName = fallback;
    try {
      document.body.dataset.profile = config.profile;
      document.body.dataset.profileName = fallback;
      const root = document.querySelector('.mascot-root');
      if (root) {
        root.setAttribute('aria-label', `${fallback} (${config.character || 'guide'})`);
      }
    } catch {}
    updateStatusPill();
  }

  function setState(next){
    const s = String(next||'').trim().toLowerCase();
    state = ['ready','thinking','concern','error'].includes(s) ? s : 'ready';
    document.body.setAttribute('data-state', state);
    updateStatusPill();
  }

  async function healthCheck(){
    try{
      const ok = await ARW.invoke('check_service_health');
      if (ok){ setState('ready'); setHint('Online and ready'); }
      else { setState('concern'); setHint('Trying to reach the service…'); }
    }catch{ setState('concern'); setHint('Awaiting service…'); }
  }

  function setHint(text){
    const el = hint();
    const value = text || '';
    lastHint = value;
    if (el && !document.body.classList.contains('compact-mode')) {
      el.textContent = value;
    }
    const pill = statusPill();
    if (pill) {
      if (document.body.classList.contains('compact-mode')) {
        pill.dataset.hint = value;
        pill.title = value;
      } else {
        pill.removeAttribute('data-hint');
        pill.removeAttribute('title');
      }
    }
  }

  function updateStreamList() {
    const list = document.getElementById('mascotStreamList');
    if (!list) return;
    const entries = Array.from(activeStreams.values());
    if (!entries.length) {
      list.hidden = true;
      list.innerHTML = '';
      return;
    }
    list.hidden = false;
    list.innerHTML = '';
    entries.forEach((entry) => {
      const row = document.createElement('div');
      row.className = 'mascot-stream-item';
      const dot = document.createElement('span');
      dot.className = 'mascot-stream-indicator';
      const label = document.createElement('span');
      label.textContent = entry.label || 'Chat';
      row.appendChild(dot);
      row.appendChild(label);
      list.appendChild(row);
    });
  }

  function emitProfileStatus(extra = {}) {
    if (!window.__TAURI__?.event?.emit) return;
    const payload = {
      profile: config.profile,
      name: config.profileName,
      character: config.character,
      quietMode: config.quietMode,
      compactMode: config.compactMode,
      streaming: streamingCount,
      state,
      timestamp: new Date().toISOString(),
      open: true,
      ...extra,
    };
    window.__TAURI__.event.emit('mascot:profile-status', payload).catch(() => {});
  }

  function updateStatusPill(){
    const pill = statusPill();
    if (!pill) return;
    const label = statusOverride || STATE_LABELS[state] || state;
    const character = config.character || 'guide';
    const profileName = config.profileName || (config.profile === 'global' ? 'Global mascot' : config.profile.replace(/^project:/, ''));
    pill.textContent = label;
    pill.dataset.character = character;
    pill.dataset.profile = profileName;
    const hintValue = document.body.classList.contains('compact-mode') ? lastHint : '';
    if (hintValue) {
      pill.dataset.hint = hintValue;
      pill.title = hintValue;
    } else {
      pill.removeAttribute('data-hint');
      pill.removeAttribute('title');
    }
    pill.setAttribute('aria-label', `${profileName}: ${label} (${character})`);
    updateStreamList();
    emitProfileStatus();
  }

  function scheduleIdleTick(){
    clearTimeout(idleTimer);
    const mul = config.intensity === 'high' ? 0.7 : config.intensity === 'low' ? 1.6 : 1.0;
    const delay = Math.floor((4000 + Math.random()*6000) * mul);
    if (config.quietMode) return;
    idleTimer = setTimeout(()=>{
      if (dragging || config.quietMode) { scheduleIdleTick(); return; }
      const options = ['m-look-left','m-look-right','m-smile','m-ooh'];
      const key = options[Math.floor(Math.random()*options.length)];
      document.body.classList.add(key);
      setTimeout(()=> document.body.classList.remove(key), 1200);
      scheduleIdleTick();
    }, delay);
  }

  async function snapToEdges(){
    try{ await ARW.invoke('snap_window_to_edges', { label: 'mascot', threshold: 28, margin: 8 }); }catch{}
  }

  async function snapToSurfaces(){
    try{ await ARW.invoke('snap_window_to_surfaces', { label: 'mascot', threshold: 32, margin: 8 }); }catch{ await snapToEdges(); }
  }

  function applyQuietMode(flag){
    const next = !!flag;
    config.quietMode = next;
    document.body.classList.toggle('quiet-mode', next);
    if (next) {
      clearTimeout(idleTimer);
      idleTimer = null;
      document.body.classList.remove('m-look-left','m-look-right','m-smile','m-ooh');
    } else {
      scheduleIdleTick();
    }
  }

  function applyCompactMode(flag){
    const next = !!flag;
    config.compactMode = next;
    document.body.classList.toggle('compact-mode', next);
    if (next) {
      const el = hint();
      if (el) el.textContent = '';
    }
    if (next) {
      const pill = statusPill();
      if (pill) {
        pill.dataset.hint = lastHint;
        pill.title = lastHint;
      }
    }
    updateStatusPill();
  }

  function applyCharacter(name){
    const normalized = CHARACTER_ORDER.includes(name) ? name : 'guide';
    config.character = normalized;
    document.body.dataset.character = normalized;
    try {
      const root = document.querySelector('.mascot-root');
      if (root) root.setAttribute('aria-label', `${config.profileName || 'Mascot'} (${normalized})`);
    } catch {}
    updateStatusPill();
  }

  function setStreaming(id, active, payload = {}){
    const key = id || 'global';
    if (active) {
      activeStreams.set(key, {
        message: payload.hint || payload.message || '',
        label: payload.label || payload.title || (key !== 'global' ? key : 'Streaming'),
        conversationId: key,
      });
    } else {
      activeStreams.delete(key);
    }
    streamingCount = activeStreams.size;
    if (streamingCount > 0) {
      document.body.dataset.stream = 'active';
      const firstEntry = activeStreams.values().next().value;
      const label = streamingCount > 1
        ? `Streaming ×${streamingCount}`
        : `Streaming — ${firstEntry?.label || 'Chat'}`;
      statusOverride = label;
      updateStatusPill();
      const msg = firstEntry?.message || payload.hint || payload.message;
      if (msg) setHint(msg);
      emitProfileStatus({ streaming: streamingCount });
    } else {
      delete document.body.dataset.stream;
      statusOverride = null;
      updateStatusPill();
      const msg = payload.hint || payload.message;
      if (msg) setHint(msg);
      emitProfileStatus({ streaming: streamingCount });
    }
  }

  async function init(){
    setState('thinking');
    setHint('Warming up…');
    const prefs = await loadMascotPrefs();
    let profilePrefs = prefs;
    if (config.profile !== 'global') {
      profilePrefs = ensureProfilePrefs(prefs, config.profile);
    }
    if (config.profile === 'global') {
      config.quietMode = prefs.quietMode ?? config.quietMode;
      config.compactMode = prefs.compactMode ?? config.compactMode;
      config.character = prefs.character || config.character;
      config.profileName = 'Global mascot';
    } else {
      config.quietMode = profilePrefs.quietMode ?? prefs.quietMode ?? config.quietMode;
      config.compactMode = profilePrefs.compactMode ?? prefs.compactMode ?? config.compactMode;
      config.character = profilePrefs.character ?? prefs.character ?? config.character;
      config.profileName = profilePrefs.name || config.profileName;
    }
    applyProfileName(config.profileName);
    applyCharacter(config.character);
    if (config.quietMode) {
      applyQuietMode(true);
    }
    if (config.compactMode) {
      applyCompactMode(true);
    }
    updateStatusPill();
    updateStreamList();
    // Gentle periodic health checks
    await healthCheck();
    healthTimer = setInterval(healthCheck, 15000);

    // Listen to launcher events for quick empathy hints
    try{
      if (window.__TAURI__?.event){
        await window.__TAURI__.event.listen('launcher://service-log', (evt) => {
          const line = String(evt?.payload?.line || '').toLowerCase();
          if (!line) return;
          if (/(started|listening|healthz ok)/.test(line)) { setState('ready'); setHint('Online and ready'); }
          else if (/(error|failed|panic|unavailable)/.test(line)) { setState('error'); setHint('Something went wrong'); }
          else if (/(starting|boot|initializing)/.test(line)) { setState('thinking'); setHint('Starting up…'); }
        });
        await window.__TAURI__.event.listen('mascot:state', (evt) => {
          const payload = evt?.payload || {};
          if (payload && typeof payload === 'object') {
            if (payload.state) setState(payload.state);
            if (typeof payload.hint === 'string') setHint(payload.hint);
          }
        });
      }
    }catch{}

    // Toggle click-through with Ctrl/Meta+D
    document.addEventListener('keydown', (e) => {
      if ((e.ctrlKey || e.metaKey) && (e.key.toLowerCase() === 'd')){
        e.preventDefault();
        document.body.classList.toggle('allow-interactions');
        setHint(document.body.classList.contains('allow-interactions') ? 'Drag/click enabled (Ctrl/⌘+D to toggle)' : 'Click-through (Ctrl/⌘+D to toggle)');
      }
    });

    // Drag detection + edge snap
    const drag = document.querySelector('.drag-strip');
    const root = document.querySelector('.mascot-root');
    const applyPreview = (anchor) => {
      if (!anchor) return;
      previewAnchor = anchor;
      document.body.dataset.snapPreview = anchor;
      clearTimeout(previewTimer);
      previewTimer = setTimeout(() => {
        if (!dragging) {
          document.body.dataset.snapPreview = '';
          previewAnchor = null;
        }
      }, 1200);
      if (dragging && SNAP_HINTS[anchor]) {
        setHint(SNAP_HINTS[anchor]);
      }
    };
    const clearPreview = () => {
      document.body.dataset.snapPreview = '';
      previewAnchor = null;
      clearTimeout(previewTimer);
    };
    const bounce = () => {
      if (!root) return;
      root.classList.remove('snap-bounce');
      // force reflow
      void root.offsetWidth;
      root.classList.add('snap-bounce');
      setTimeout(() => root.classList.remove('snap-bounce'), 360);
    };
    const onPointerMove = async (evt) => {
      if (!dragging) return;
      pointer.x = Number.isFinite(evt.screenX) ? Math.round(evt.screenX) : pointer.x;
      pointer.y = Number.isFinite(evt.screenY) ? Math.round(evt.screenY) : pointer.y;
      if (!Number.isFinite(pointer.x) || !Number.isFinite(pointer.y)) return;
      try {
        const anchor = await ARW.invoke('smart_snap_window', {
          label: 'mascot',
          pointer_x: pointer.x,
          pointer_y: pointer.y,
          preview_only: true,
          snap_to_surfaces: !!config.snapWindows,
        });
        applyPreview(anchor);
      } catch (err) {
        console.error(err);
      }
    };
    if (drag){
      drag.addEventListener('pointerdown', (evt)=>{
        dragging = true;
        pointer.x = Number.isFinite(evt.screenX) ? Math.round(evt.screenX) : null;
        pointer.y = Number.isFinite(evt.screenY) ? Math.round(evt.screenY) : null;
        try { if (typeof drag.setPointerCapture === 'function') drag.setPointerCapture(evt.pointerId); } catch {}
        window.addEventListener('pointermove', onPointerMove);
        const currentHint = hint();
        savedHint = currentHint ? currentHint.textContent : null;
      });
      window.addEventListener('pointerup', async (evt)=>{
        if (!dragging) return;
        dragging = false;
        window.removeEventListener('pointermove', onPointerMove);
        clearPreview();
        pointer.x = Number.isFinite(evt.screenX) ? Math.round(evt.screenX) : pointer.x;
        pointer.y = Number.isFinite(evt.screenY) ? Math.round(evt.screenY) : pointer.y;
        setTimeout(async () => {
          try {
            if (Number.isFinite(pointer.x) && Number.isFinite(pointer.y)) {
              const anchor = await ARW.invoke('smart_snap_window', {
                label: 'mascot',
                pointer_x: pointer.x,
                pointer_y: pointer.y,
                preview_only: false,
                snap_to_surfaces: !!config.snapWindows,
              });
              applyPreview(anchor);
              if (SNAP_HINTS[anchor]) {
                setHint(`Docked ${SNAP_HINTS[anchor].replace('Docking ', '').replace('…', '')}`);
              }
            } else if (config.snapWindows) {
              await snapToSurfaces();
            } else {
              await snapToEdges();
            }
          } catch (err) {
            console.error(err);
            if (config.snapWindows) snapToSurfaces(); else snapToEdges();
          } finally {
            bounce();
            setTimeout(clearPreview, 600);
            if (savedHint) {
              setTimeout(() => setHint(savedHint), 800);
              savedHint = null;
            }
          }
        }, 20);
      });
    }

    // First-run guidance for interactions
    setTimeout(()=>{
      if (!document.body.classList.contains('allow-interactions')){
        setHint('Click-through enabled (Ctrl/⌘+D to toggle)');
      }
    }, 1200);

    const spawnProjectMascot = async () => {
      if (!window.ARW || typeof window.ARW.modal?.form !== 'function') {
        try { window.ARW?.toast?.('Project mascots require the desktop launcher.'); } catch {}
        return;
      }
      const result = await window.ARW.modal.form({
        title: 'Open Project Mascot',
        description: 'Spawn a mascot tuned to a project or remote node.',
        submitLabel: 'Open',
        fields: [
          { name: 'name', label: 'Project name', placeholder: 'Project Alpha', required: true },
          { name: 'character', label: 'Character', type: 'select', value: config.character || 'guide', options: CHARACTER_ORDER.map((value) => ({ value, label: value.charAt(0).toUpperCase() + value.slice(1) })) },
          { name: 'quietMode', label: 'Start in quiet mode', type: 'checkbox', value: config.quietMode },
          { name: 'compactMode', label: 'Start in compact mode', type: 'checkbox', value: config.compactMode },
          { name: 'autoOpen', label: 'Reopen automatically on launch', type: 'checkbox', value: false },
        ],
      });
      if (!result) return;
      const rawName = String(result.name || '').trim();
      if (!rawName) return;
      const slug = slugifyName(rawName);
      const profileKey = `project:${slug}`;
      const windowLabel = `mascot-${slug}`;
      const prefs = await loadMascotPrefs();
      const entry = ensureProfilePrefs(prefs, profileKey);
      entry.quietMode = !!result.quietMode;
      entry.compactMode = !!result.compactMode;
      entry.character = result.character || entry.character || prefs.character || 'guide';
      entry.name = rawName;
      entry.slug = slug;
      entry.autoOpen = !!result.autoOpen;
      await storeMascotPrefs(prefs);
      const overrides = {
        quietMode: entry.quietMode,
        compactMode: entry.compactMode,
        character: entry.character,
        name: entry.name,
      };
      try {
        await window.__TAURI__?.invoke('open_mascot_window', {
          label: windowLabel,
          profile: profileKey,
          character: entry.character,
          quiet: entry.quietMode,
          compact: entry.compactMode,
        });
        if (window.__TAURI__?.event?.emit) {
          await window.__TAURI__.event.emit('mascot:config', buildConfigPayload(prefs, profileKey, entry, overrides));
        }
        window.ARW.toast?.(`Opened mascot for ${rawName}`);
      } catch (err) {
        console.error(err);
        window.ARW.toast?.('Unable to open project mascot');
      }
    };

    try{
      if (ARW?.sse?.subscribe) {
        ARW.sse.subscribe((kind) => {
          if (!kind) return false;
          const lower = String(kind).toLowerCase();
          return lower.includes('chat') && (lower.includes('turn') || lower.includes('stream') || lower.includes('response'));
        }, ({ kind, env }) => {
          const lower = String(kind || '').toLowerCase();
          const convId = env?.conversationId || env?.conversation_id || env?.conversation?.id || env?.conversation || env?.window || env?.label || null;
          const label = env?.label || env?.title || (env?.window ? String(env.window) : (convId && convId !== 'global' ? String(convId) : 'Chat'));
          if (lower.includes('error') || env?.error) {
            setStreaming(convId, false, { hint: 'Response failed', label });
            setState('error');
            return;
          }
          if (lower.includes('start') || lower.includes('open')) {
            setStreaming(convId, true, { hint: 'Streaming response…', label });
            setState('thinking');
          } else if (lower.includes('delta')) {
            setStreaming(convId, true, { hint: 'Streaming response…', label });
            setState('thinking');
          } else if (lower.includes('complete') || lower.includes('finish') || lower.includes('done') || lower.includes('close')) {
            setStreaming(convId, false, { hint: 'Response ready', label });
            setState('ready');
          }
        });
      }
    }catch(err){ console.error(err); }

    try{
      if (window.__TAURI__?.event) {
        await window.__TAURI__.event.listen('mascot:stream', (evt) => {
          const payload = evt?.payload || {};
          const action = String(payload.action || '').toLowerCase();
          const convId = payload.conversationId || payload.window || null;
          const label = payload.label || payload.title || (convId && convId !== 'global' ? String(convId) : 'Chat');
          if (action === 'start') {
            setStreaming(convId, true, { hint: payload.hint || 'Streaming response…', label });
            if (payload.state) setState(payload.state);
          } else if (action === 'stop' || action === 'end') {
            setStreaming(convId, false, { hint: payload.hint || 'Response ready', label });
            if (payload.state) setState(payload.state);
          } else if (action === 'error') {
            setStreaming(convId, false, { hint: payload.hint || 'Response failed', label });
            setState('error');
          }
        });
      }
    }catch{}

    scheduleIdleTick();

    // Menu wiring
    const toggle = document.getElementById('mascotMenuToggle');
    const wrap = document.getElementById('mascotMenu');
    function closeMenu(){ if (wrap) { wrap.hidden = true; toggle && toggle.setAttribute('aria-expanded','false'); } }
    function openMenu(){ if (wrap) { wrap.hidden = false; toggle && toggle.setAttribute('aria-expanded','true'); const first = wrap.querySelector('button'); try{ first?.focus(); }catch{} } }
    function toggleMenu(){ if (!wrap) return; const open = !wrap.hidden; if (open) closeMenu(); else openMenu(); }
    if (toggle){ toggle.addEventListener('click', (e)=>{ e.preventDefault(); toggleMenu(); }); }
    document.addEventListener('keydown', (e)=>{ if ((e.key||'').toLowerCase()==='m') toggleMenu(); });
    document.addEventListener('pointerdown', (e)=>{ if (!wrap || wrap.hidden) return; if (!wrap.contains(e.target) && e.target !== toggle) closeMenu(); });
    if (wrap){
      wrap.addEventListener('click', async (e)=>{
        const el = e.target;
        if (!(el instanceof HTMLElement)) return;
        const act = el.dataset.action;
        if (!act) return;
        e.preventDefault(); closeMenu();
        try{
          switch (act) {
            case 'open-chat':
              await ARW.invoke('open_chat_window');
              break;
            case 'open-debug':
              await ARW.invoke('open_debug_window');
              break;
            case 'start-service':
              await ARW.invoke('start_service');
              break;
            case 'stop-service':
              await ARW.invoke('stop_service');
              break;
            case 'open-logs':
              await ARW.invoke('open_logs_window');
              break;
            case 'toggle-quiet': {
              try {
                const profileKey = config.profile || 'global';
                const prefs = await loadMascotPrefs();
                const entry = ensureProfilePrefs(prefs, profileKey);
                const currentQuiet = profileKey === 'global'
                  ? !!(prefs.quietMode ?? false)
                  : !!(entry.quietMode ?? prefs.quietMode ?? false);
                const next = !currentQuiet;
                if (profileKey === 'global') {
                  prefs.quietMode = next;
                } else {
                  entry.quietMode = next;
                  entry.slug = entry.slug || slugifyName(profileKey.replace(/^project:/, ''));
                  entry.name = entry.name || config.profileName;
                }
                await storeMascotPrefs(prefs);
                if (window.__TAURI__?.event?.emit) {
                  await window.__TAURI__.event.emit(
                    'mascot:config',
                    buildConfigPayload(
                      prefs,
                      profileKey,
                      profileKey === 'global' ? {} : entry,
                      { quietMode: next }
                    )
                  );
                }
                setHint(next ? 'Quiet mode enabled' : 'Quiet mode disabled');
              } catch (err) {
                console.error(err);
              }
              break;
            }
            case 'toggle-compact': {
              try {
                const profileKey = config.profile || 'global';
                const prefs = await loadMascotPrefs();
                const entry = ensureProfilePrefs(prefs, profileKey);
                const currentCompact = profileKey === 'global'
                  ? !!(prefs.compactMode ?? false)
                  : !!(entry.compactMode ?? prefs.compactMode ?? false);
                const next = !currentCompact;
                if (profileKey === 'global') {
                  prefs.compactMode = next;
                } else {
                  entry.compactMode = next;
                  entry.slug = entry.slug || slugifyName(profileKey.replace(/^project:/, ''));
                  entry.name = entry.name || config.profileName;
                }
                await storeMascotPrefs(prefs);
                if (window.__TAURI__?.event?.emit) {
                  await window.__TAURI__.event.emit(
                    'mascot:config',
                    buildConfigPayload(
                      prefs,
                      profileKey,
                      profileKey === 'global' ? {} : entry,
                      { compactMode: next }
                    )
                  );
                }
                setHint(next ? 'Compact mode enabled' : 'Compact mode disabled');
              } catch (err) {
                console.error(err);
              }
              break;
            }
            case 'cycle-character': {
              try {
                const profileKey = config.profile || 'global';
                const prefs = await loadMascotPrefs();
                const entry = ensureProfilePrefs(prefs, profileKey);
                const current = profileKey === 'global'
                  ? (prefs.character && CHARACTER_ORDER.includes(prefs.character) ? prefs.character : 'guide')
                  : (entry.character && CHARACTER_ORDER.includes(entry.character)
                      ? entry.character
                      : (prefs.character && CHARACTER_ORDER.includes(prefs.character) ? prefs.character : 'guide'));
                const next = CHARACTER_ORDER[(CHARACTER_ORDER.indexOf(current) + 1) % CHARACTER_ORDER.length];
                if (profileKey === 'global') {
                  prefs.character = next;
                } else {
                  entry.character = next;
                  entry.slug = entry.slug || slugifyName(profileKey.replace(/^project:/, ''));
                  entry.name = entry.name || config.profileName;
                }
                await storeMascotPrefs(prefs);
                applyCharacter(next);
                if (window.__TAURI__?.event?.emit) {
                  await window.__TAURI__.event.emit(
                    'mascot:config',
                    buildConfigPayload(
                      prefs,
                      profileKey,
                      profileKey === 'global' ? {} : entry,
                      { character: next }
                    )
                  );
                }
                setHint(`Character: ${next}`);
              } catch (err) {
                console.error(err);
              }
              break;
            }
            case 'toggle-auto-open': {
              try {
                const profileKey = config.profile || 'global';
                if (profileKey === 'global') {
                  setHint('Auto reopen applies to project mascots only.');
                  break;
                }
                const prefs = await loadMascotPrefs();
                const entry = ensureProfilePrefs(prefs, profileKey);
                entry.slug = entry.slug || slugifyName(profileKey.replace(/^project:/, ''));
                entry.name = entry.name || config.profileName || profileKey;
                const next = !(entry.autoOpen ?? false);
                entry.autoOpen = next;
                await storeMascotPrefs(prefs);
                window.ARW.toast?.(`Auto reopen ${next ? 'enabled' : 'disabled'} for ${entry.name}`);
              } catch (err) {
                console.error(err);
              }
              break;
            }
            case 'spawn-project':
              await spawnProjectMascot();
              break;
            case 'dock-left':
              await ARW.invoke('position_window', { label:'mascot', anchor:'left', margin: 12 });
              if (config.snapWindows) await snapToSurfaces();
              applyPreview('left');
              bounce();
              break;
            case 'dock-right':
              await ARW.invoke('position_window', { label:'mascot', anchor:'right', margin: 12 });
              if (config.snapWindows) await snapToSurfaces();
              applyPreview('right');
              bounce();
              break;
            case 'dock-bottom-right':
              await ARW.invoke('position_window', { label:'mascot', anchor:'bottom-right', margin: 12 });
              if (config.snapWindows) await snapToSurfaces();
              applyPreview('bottom-right');
              bounce();
              break;
            case 'reset':
              await ARW.invoke('position_window', { label:'mascot', anchor:'bottom-right', margin: 12 });
              if (config.snapWindows) await snapToSurfaces();
              applyPreview('bottom-right');
              bounce();
              break;
          }
        }catch(err){ console.error(err); }
      });
    }

    // Config updates
    try{
      if (window.__TAURI__?.event){
        await window.__TAURI__.event.listen('mascot:config', (evt)=>{
          const cfg = evt?.payload || {};
          const targetProfile = typeof cfg.profile === 'string' && cfg.profile.trim()
            ? cfg.profile.trim()
            : 'global';
          if (targetProfile !== 'global' && targetProfile !== config.profile) {
            return;
          }
          if (typeof cfg.name === 'string') {
            applyProfileName(cfg.name);
          }
          if (typeof cfg.allowInteractions === 'boolean'){
            config.allowInteractions = !!cfg.allowInteractions;
            document.body.classList.toggle('allow-interactions', config.allowInteractions);
          }
          if (typeof cfg.snapWindows === 'boolean') config.snapWindows = !!cfg.snapWindows;
          if (typeof cfg.intensity === 'string') config.intensity = cfg.intensity;
          if (typeof cfg.quietMode === 'boolean') {
            applyQuietMode(cfg.quietMode);
          } else if (!config.quietMode) {
            scheduleIdleTick();
          }
          if (typeof cfg.compactMode === 'boolean') {
            applyCompactMode(cfg.compactMode);
          } else {
            updateStatusPill();
          }
          if (typeof cfg.character === 'string') {
            applyCharacter(cfg.character);
          }
        });
      }
    }catch{}
  }

  document.addEventListener('DOMContentLoaded', init);
  window.addEventListener('beforeunload', ()=>{ if (healthTimer) clearInterval(healthTimer); clearTimeout(idleTimer); });
})();
