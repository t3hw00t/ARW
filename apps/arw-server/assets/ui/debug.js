// Minimal boot script for Debug UI (CSP-safe)
(() => {
  try {
    const KEY = 'arw-theme';
    const apply = (t) => {
      const r = document.documentElement;
      if (t === 'dark') r.setAttribute('data-theme', 'dark');
      else r.removeAttribute('data-theme');
    };
    const cur = () => localStorage.getItem(KEY) || '';
    apply(cur());
    const btn = document.getElementById('themeToggle');
    if (btn) btn.addEventListener('click', () => {
      const t = cur() === 'dark' ? 'light' : 'dark';
      localStorage.setItem(KEY, t);
      apply(t);
    });
    const back = document.getElementById('btnBack');
    if (back) back.addEventListener('click', () => {
      try {
        if (document.referrer) {
          history.back();
        } else {
          location.assign('/admin');
        }
      } catch (e) {
        location.assign('/admin');
      }
    });
  } catch {}
})();

// Toast helper + small utils
function showToast(msg){
  const t=document.getElementById('toast');
  if(!t) return;
  t.textContent=msg;
  t.style.display='block';
  clearTimeout(window.__toastTimer);
  window.__toastTimer = setTimeout(()=>{ t.style.display='none'; }, 2500);
}

// State path overrides used by debug helpers
window.__statePathOverrides = {
  actions: '/state/actions',
  episodes: '/state/episodes',
  observations: '/state/observations',
  beliefs: '/state/beliefs',
  intents: '/state/intents',
  route_stats: '/state/route_stats',
  world: '/state/world',
  guardrails_metrics: '/state/guardrails_metrics'
};

// Collapsible sections, density toggle, and expand/collapse all
document.addEventListener('DOMContentLoaded', () => {
  try {
    document.querySelectorAll('.box>h3').forEach(h => {
      const key = 'dbg_collapse_' + (h.textContent||'').trim().toLowerCase().replace(/\s+/g,'_');
      const box = h.parentElement;
      const saved = localStorage.getItem(key);
      if (saved === '1') box.setAttribute('data-collapsed','1');
      else if (saved === '0') box.removeAttribute('data-collapsed');
      h.addEventListener('click', () => {
        const collapsed = box.getAttribute('data-collapsed') === '1';
        if (collapsed) { box.removeAttribute('data-collapsed'); localStorage.setItem(key,'0'); }
        else { box.setAttribute('data-collapsed','1'); localStorage.setItem(key,'1'); }
      });
    });
    const exp = document.getElementById('btnExpandAll');
    const col = document.getElementById('btnCollapseAll');
    if (exp) exp.addEventListener('click', ()=>{
      document.querySelectorAll('.box').forEach(b => {
        b.removeAttribute('data-collapsed');
        const h = b.querySelector('h3');
        if (h) localStorage.setItem('dbg_collapse_' + (h.textContent||'').trim().toLowerCase().replace(/\s+/g,'_'), '0');
      });
    });
    if (col) col.addEventListener('click', ()=>{
      document.querySelectorAll('.box').forEach(b => {
        b.setAttribute('data-collapsed','1');
        const h = b.querySelector('h3');
        if (h) localStorage.setItem('dbg_collapse_' + (h.textContent||'').trim().toLowerCase().replace(/\s+/g,'_'), '1');
      });
    });
    // Density toggle
    const dens = document.getElementById('btnDensity');
    const setDensity = mode => { document.body.classList.toggle('density-compact', mode==='compact'); localStorage.setItem('dbg_density', mode); };
    const savedD = localStorage.getItem('dbg_density');
    if (savedD === 'compact') document.body.classList.add('density-compact');
    if (dens) dens.addEventListener('click', ()=>{ const compact = document.body.classList.contains('density-compact'); setDensity(compact? 'comfortable' : 'compact'); });
  } catch {}
});
