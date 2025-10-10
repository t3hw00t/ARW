// Minimal logic to keep the mascot lively and semi-empathetic
(function(){
  const HAS_TAURI = !!(window.__TAURI__ && window.__TAURI__.invoke);

  let state = 'unknown';
  let healthTimer = null;
  let idleTimer = null;
  let dragging = false;
  let pointer = { x: null, y: null };
  let config = {
    allowInteractions: false,
    intensity: 'normal',
    snapWindows: true,
    quietMode: false,
    compactMode: false,
  };
  let previewAnchor = null;
  let previewTimer = null;
  let savedHint = null;
  let streamingCount = 0;
  let statusOverride = null;
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
  const hint = () => document.getElementById('mascotHint');
  const statusPill = () => document.getElementById('mascotStatus');

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
    if (el && !document.body.classList.contains('compact-mode')) {
      el.textContent = text || '';
    }
    const pill = statusPill();
    if (pill && document.body.classList.contains('compact-mode')) {
      pill.dataset.hint = text || '';
    }
  }

  function updateStatusPill(){
    const pill = statusPill();
    if (!pill) return;
    const label = statusOverride || STATE_LABELS[state] || state;
    pill.textContent = label;
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
    updateStatusPill();
  }

  function setStreaming(active, message){
    if (active) streamingCount += 1;
    else streamingCount = Math.max(0, streamingCount - 1);
    const activeNow = streamingCount > 0;
    if (activeNow) {
      document.body.dataset.stream = 'active';
      statusOverride = 'Streaming';
      updateStatusPill();
      if (message) setHint(message);
    } else {
      delete document.body.dataset.stream;
      statusOverride = null;
      updateStatusPill();
      if (message) setHint(message);
    }
  }

  async function init(){
    setState('thinking');
    setHint('Warming up…');
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

    try{
      if (ARW?.sse?.subscribe) {
        ARW.sse.subscribe((kind) => {
          if (!kind) return false;
          const lower = String(kind).toLowerCase();
          return lower.includes('chat') && (lower.includes('turn') || lower.includes('stream') || lower.includes('response'));
        }, ({ kind, env }) => {
          const lower = String(kind || '').toLowerCase();
          if (lower.includes('error') || env?.error) {
            setStreaming(false, 'Response failed');
            setState('error');
            return;
          }
          if (lower.includes('start') || lower.includes('open')) {
            setStreaming(true, 'Streaming response…');
            setState('thinking');
          } else if (lower.includes('delta')) {
            setStreaming(true, 'Streaming response…');
            setState('thinking');
          } else if (lower.includes('complete') || lower.includes('finish') || lower.includes('done') || lower.includes('close')) {
            setStreaming(false, 'Response ready');
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
          if (action === 'start') {
            setStreaming(true, payload.hint || 'Streaming response…');
            if (payload.state) setState(payload.state);
          } else if (action === 'stop' || action === 'end') {
            setStreaming(false, payload.hint || 'Response ready');
            if (payload.state) setState(payload.state);
          } else if (action === 'error') {
            setStreaming(false, payload.hint || 'Response failed');
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
                const prefs = await ARW.getPrefs('mascot') || {};
                const next = !(prefs.quietMode ?? false);
                prefs.quietMode = next;
                prefs.compactMode = prefs.compactMode ?? config.compactMode;
                await ARW.setPrefs('mascot', prefs);
                if (window.__TAURI__?.event?.emit) {
                  await window.__TAURI__.event.emit('mascot:config', {
                    allowInteractions: !(prefs.clickThrough ?? true),
                    intensity: prefs.intensity || 'normal',
                    snapWindows: prefs.snapWindows !== false,
                    quietMode: next,
                    compactMode: prefs.compactMode ?? false,
                  });
                }
                setHint(next ? 'Quiet mode enabled' : 'Quiet mode disabled');
              } catch (err) {
                console.error(err);
              }
              break;
            }
            case 'toggle-compact': {
              try {
                const prefs = await ARW.getPrefs('mascot') || {};
                const next = !(prefs.compactMode ?? false);
                prefs.compactMode = next;
                prefs.quietMode = prefs.quietMode ?? config.quietMode;
                await ARW.setPrefs('mascot', prefs);
                if (window.__TAURI__?.event?.emit) {
                  await window.__TAURI__.event.emit('mascot:config', {
                    allowInteractions: !(prefs.clickThrough ?? true),
                    intensity: prefs.intensity || 'normal',
                    snapWindows: prefs.snapWindows !== false,
                    quietMode: prefs.quietMode ?? false,
                    compactMode: next,
                  });
                }
                setHint(next ? 'Compact mode enabled' : 'Compact mode disabled');
              } catch (err) {
                console.error(err);
              }
              break;
            }
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
          }
        });
      }
    }catch{}
  }

  document.addEventListener('DOMContentLoaded', init);
  window.addEventListener('beforeunload', ()=>{ if (healthTimer) clearInterval(healthTimer); clearTimeout(idleTimer); });
})();
