// Minimal logic to keep the mascot lively and semi-empathetic
(function(){
  const HAS_TAURI = !!(window.__TAURI__ && window.__TAURI__.invoke);

  let state = 'unknown';
  let healthTimer = null;
  const hint = () => document.getElementById('mascotHint');

  function setState(next){
    const s = String(next||'').trim().toLowerCase();
    state = ['ready','thinking','concern','error'].includes(s) ? s : 'ready';
    document.body.setAttribute('data-state', state);
  }

  async function healthCheck(){
    try{
      const ok = await ARW.invoke('check_service_health');
      if (ok){ setState('ready'); setHint('Online and ready'); }
      else { setState('concern'); setHint('Trying to reach the service…'); }
    }catch{ setState('concern'); setHint('Awaiting service…'); }
  }

  function setHint(text){ const el = hint(); if (el) el.textContent = text || ''; }

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

    // First-run guidance for interactions
    setTimeout(()=>{
      if (!document.body.classList.contains('allow-interactions')){
        setHint('Click-through enabled (Ctrl/⌘+D to toggle)');
      }
    }, 1200);
  }

  document.addEventListener('DOMContentLoaded', init);
  window.addEventListener('beforeunload', ()=>{ if (healthTimer) clearInterval(healthTimer); });
})();
