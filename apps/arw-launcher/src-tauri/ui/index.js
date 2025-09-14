const invoke = (cmd, args) => ARW.invoke(cmd, args);
const getPort = () => ARW.getPortFromInput('port');

async function loadPrefs() {
  try {
    const v = await ARW.getPrefs('launcher');
    if (v && typeof v === 'object') {
      if (v.port) document.getElementById('port').value = v.port;
      if (typeof v.autostart === 'boolean') document.getElementById('autostart').checked = v.autostart;
      if (typeof v.notifyOnStatus === 'boolean') document.getElementById('notif').checked = v.notifyOnStatus;
    }
  } catch {}
  try {
    const enabled = await invoke('launcher_autostart_status');
    document.getElementById('loginstart').checked = !!enabled
  } catch {}
  health();
}

async function savePrefs() {
  const v = await ARW.getPrefs('launcher') || {};
  v.port = getPort();
  v.autostart = !!document.getElementById('autostart').checked;
  v.notifyOnStatus = !!document.getElementById('notif').checked;
  await ARW.setPrefs('launcher', v);
}

async function health() {
  try{
    const ok = await invoke('check_service_health', { port: getPort() });
    document.getElementById('svc-dot').className = 'dot ' + (ok ? 'ok' : 'bad');
    document.getElementById('svc-text').innerText = ok ? 'online' : 'offline';
  }catch{}
}

// Mini downloads widget (models.*)
function miniDownloads() {
  const root=document.getElementById('dlmini');
  if(!root) return;
  const last={};
  const getPort = ()=> ARW.getPortFromInput('port') || 8090;
  (async ()=>{
    try{
      const es = new EventSource(ARW.base(getPort()) + '/events?prefix=models.');
      es.addEventListener('models.download.progress', (ev)=>{
        try{
          const j = JSON.parse(ev.data||'{}');
          const id = j.id; const pct=(j.progress||0)+'%';
          let el = root.querySelector(`[data-dlid="${id}"]`);
          if(!el){ el=document.createElement('span'); el.setAttribute('data-dlid', id); el.className='badge'; el.innerHTML='<span class="dot"></span> '+id+' '+pct; root.appendChild(el); }
          if(j.status==='complete'){ el.className='badge'; el.innerHTML = '<span class="dot"></span> '+id+' complete'; setTimeout(()=>{ if(el&&el.parentNode) el.parentNode.removeChild(el); }, 1500); return; }
          if(j.error || j.code){ el.className='badge'; el.innerHTML = '<span class="dot bad"></span> '+id+' '+(j.code||'error'); return; }
          // rate
          const now = Date.now(); let tail=''; const prev=last[id]; last[id] = { t: now, b: (j.downloaded||0) };
          if (prev && j.downloaded>=prev.b){ const dt=(now-prev.t)/1000; const db=j.downloaded-prev.b; const mbps = db/(1024*1024)/Math.max(0.001,dt); if(mbps>0.05) tail = ' · '+mbps.toFixed(2)+' MiB/s'; }
          el.className='badge'; el.innerHTML = '<span class="dot"></span> '+id+' '+pct+tail;
        }catch{}
      });
    }catch{}
  })();
}

document.addEventListener('DOMContentLoaded', () => {
  // Buttons
  document.getElementById('btn-open-window').addEventListener('click', async () => {
    try { await invoke('open_debug_window', { port: getPort() }); } catch (e) { console.error(e); }
  });
  document.getElementById('btn-events').addEventListener('click', async () => {
    try { await invoke('open_events_window'); } catch (e) { console.error(e); }
  });
  document.getElementById('btn-logs').addEventListener('click', async () => {
    try { await invoke('open_logs_window'); } catch (e) { console.error(e); }
  });
  document.getElementById('btn-models').addEventListener('click', async () => {
    try { await invoke('open_models_window'); } catch (e) { console.error(e); }
  });
  document.getElementById('btn-connections').addEventListener('click', async () => {
    try { await invoke('open_connections_window'); } catch (e) { console.error(e); }
  });
  document.getElementById('btn-start').addEventListener('click', async () => {
    try { await invoke('start_service', { port: getPort() }); ARW.toast('Service starting'); } catch (e) { console.error(e); }
  });
  document.getElementById('btn-stop').addEventListener('click', async () => {
    try { await invoke('stop_service', { port: getPort() }); ARW.toast('Service stop requested'); } catch (e) { console.error(e); }
  });
  document.getElementById('btn-save').addEventListener('click', async () => {
    try {
      await savePrefs(); ARW.toast('Preferences saved');
      const loginstart = document.getElementById('loginstart').checked;
      await invoke('set_launcher_autostart', { enabled: loginstart });
    } catch (e) { console.error(e); }
  });
  document.getElementById('btn-updates').addEventListener('click', async () => {
    try {
      await invoke('open_url', { url: 'https://github.com/t3hw00t/ARW/releases' });
    } catch (e) { console.error(e); }
  });
  // Service health polling
  health(); setInterval(health, 4000);
  // Prefs and mini SSE downloads
  loadPrefs();
  miniDownloads();
});
