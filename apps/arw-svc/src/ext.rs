use axum::{
    Router,
    routing::{get, post},
    extract::State,
    response::{IntoResponse, Html},
    Json
};
use axum::http::StatusCode;
use serde::Deserialize;
use serde_json::{json, Value};
use std::sync::OnceLock;
use tokio::sync::RwLock;
use std::fs;
use tokio::fs as afs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::AppState;
use arw_core;

// ---------- state paths & file helpers ----------
fn state_dir() -> PathBuf {
    let v = arw_core::load_effective_paths();
    let s = v.get("state_dir").and_then(|x| x.as_str()).unwrap_or(".");
    PathBuf::from(s.replace('\\', "/"))
}
fn memory_path() -> PathBuf { state_dir().join("memory.json") }
fn models_path() -> PathBuf { state_dir().join("models.json") }

fn load_json_file(p: &Path) -> Option<Value> {
    let s = fs::read_to_string(p).ok()?;
    serde_json::from_str(&s).ok()
}
fn save_json_file(p: &Path, v: &Value) -> std::io::Result<()> {
    if let Some(parent) = p.parent() { let _ = fs::create_dir_all(parent); }
    let s = serde_json::to_string_pretty(v).unwrap_or_else(|_| "{}".to_string());
    fs::write(p, s.as_bytes())
}

async fn load_json_file_async(p: &Path) -> Option<Value> {
    let s = afs::read_to_string(p).await.ok()?;
    serde_json::from_str(&s).ok()
}
async fn save_json_file_async(p: &Path, v: &Value) -> std::io::Result<()> {
    if let Some(parent) = p.parent() { let _ = afs::create_dir_all(parent).await; }
    let s = serde_json::to_string_pretty(v).unwrap_or_else(|_| "{}".to_string());
    afs::write(p, s.as_bytes()).await
}

// ---------- Global stores ----------
fn default_memory() -> Value {
    json!({
        "ephemeral":  [],
        "episodic":   [],
        "semantic":   [],
        "procedural": []
    })
}
static MEMORY: OnceLock<RwLock<Value>> = OnceLock::new();
fn memory() -> &'static RwLock<Value> {
    MEMORY.get_or_init(|| {
        let initial = load_json_file(&memory_path()).unwrap_or_else(default_memory);
        RwLock::new(initial)
    })
}

fn default_models() -> Vec<Value> {
    vec![
        json!({"id":"llama-3.1-8b-instruct","provider":"local","status":"available"}),
        json!({"id":"qwen2.5-coder-7b","provider":"local","status":"available"}),
    ]
}
static MODELS: OnceLock<RwLock<Vec<Value>>> = OnceLock::new();
fn models() -> &'static RwLock<Vec<Value>> {
    MODELS.get_or_init(|| {
        let initial = load_json_file(&models_path())
            .and_then(|v| v.as_array().cloned())
            .unwrap_or_else(default_models);
        RwLock::new(initial)
    })
}

// ---------- memory ring-buffer limit ----------
static MEM_LIMIT: OnceLock<RwLock<usize>> = OnceLock::new();
fn initial_mem_limit() -> usize {
    std::env::var("ARW_MEM_LIMIT").ok().and_then(|s| s.parse::<usize>().ok()).unwrap_or(200)
}
fn mem_limit() -> &'static RwLock<usize> {
    MEM_LIMIT.get_or_init(|| RwLock::new(initial_mem_limit()))
}

// ---------- Tool runner (minimal builtins) ----------
static TOOL_LIST: &[(&str, &str)] = &[
    ("math.add", "Add two numbers: input {\"a\": number, \"b\": number} -> {\"sum\": number}"),
    ("time.now", "UTC time in ms: input {} -> {\"now_ms\": number}")
];

fn run_tool_internal(id: &str, input: &Value) -> Result<Value, String> {
    match id {
        "math.add" => {
            let a = input.get("a").and_then(|v| v.as_f64()).ok_or("missing or invalid 'a'")?;
            let b = input.get("b").and_then(|v| v.as_f64()).ok_or("missing or invalid 'b'")?;
            Ok(json!({"sum": a + b}))
        }
        "time.now" => {
            let now = SystemTime::now().duration_since(UNIX_EPOCH).map_err(|e| e.to_string())?.as_millis() as i64;
            Ok(json!({"now_ms": now}))
        }
        _ => Err(format!("unknown tool id: {}", id))
    }
}

// ---------- Public: mountable routes ----------
pub fn extra_routes() -> Router<AppState> {
    let mut r = Router::new()
        .route("/version", get(version))
        .route("/about", get(about))
        // memory
        .route("/memory", get(memory_get))
        .route("/memory/apply", post(memory_apply))
        .route("/memory/save", post(memory_save))
        .route("/memory/load", post(memory_load))
        .route("/memory/limit", get(memory_limit_get))
        .route("/memory/limit", post(memory_limit_set))
        // models
        .route("/models", get(list_models))
        .route("/models/refresh", post(refresh_models))
        .route("/models/save", post(models_save))
        .route("/models/load", post(models_load))
        // tools
        .route("/tools", get(list_tools))
        .route("/tools/run", post(run_tool_endpoint));

    // debug UI gated via ARW_DEBUG=1
    if std::env::var("ARW_DEBUG").ok().as_deref() == Some("1") {
        r = r.route("/debug", get(debug_ui));
    }
    r
}

// ---------- Handlers ----------
async fn version() -> impl IntoResponse {
    Json(json!({
        "service": env!("CARGO_PKG_NAME"),
        "version": env!("CARGO_PKG_VERSION"),
    }))
}

async fn about() -> impl IntoResponse {
    Json(json!({
        "service": "arw-svc",
        "version": env!("CARGO_PKG_VERSION"),
        "endpoints": [
          "/healthz",
          "/events",
          "/version",
          "/about",
          "/introspect/tools",
          "/introspect/schemas/:id",
          "/probe",
          "/memory",
          "/memory/apply",
          "/memory/save",
          "/memory/load",
          "/memory/limit",
          "/models",
          "/models/refresh",
          "/models/save",
          "/models/load",
          "/tools",
          "/tools/run",
          "/debug"
        ]
    }))
}

#[derive(Deserialize)]
struct ApplyMemory {
    kind: String,        // ephemeral|episodic|semantic|procedural
    value: Value,
    #[serde(default)]
    ttl_ms: Option<u64>,
}

async fn memory_get() -> impl IntoResponse {
    let snap = memory().read().await.clone();
    Json::<Value>(snap)
}
async fn memory_save() -> impl IntoResponse {
    let snap = memory().read().await.clone();
    match save_json_file_async(&memory_path(), &snap).await {
        Ok(_) => Json(json!({"ok": true})).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"ok": false, "error": e.to_string()}))).into_response(),
    }
}
async fn memory_load() -> impl IntoResponse {
    match load_json_file_async(&memory_path()).await {
        Some(v) => {
            let mut m = memory().write().await;
            *m = v.clone();
            Json::<Value>(v).into_response()
        }
        None => (StatusCode::NOT_FOUND, Json(json!({"ok": false, "error":"no memory.json"}))).into_response(),
    }
}
async fn memory_limit_get() -> impl IntoResponse {
    let n = { *mem_limit().read().await };
    Json(json!({ "limit": n }))
}
#[derive(Deserialize)]
struct SetLimit { limit: usize }
async fn memory_limit_set(Json(req): Json<SetLimit>) -> impl IntoResponse {
    {
        let mut n = mem_limit().write().await;
        *n = req.limit.max(1);
    }
    Json(json!({ "ok": true }))
}

async fn memory_apply(State(state): State<AppState>, Json(req): Json<ApplyMemory>) -> impl IntoResponse {
    let mut mem = memory().write().await;
    let lane = match req.kind.as_str() {
        "ephemeral"  => mem.get_mut("ephemeral").and_then(Value::as_array_mut),
        "episodic"   => mem.get_mut("episodic").and_then(Value::as_array_mut),
        "semantic"   => mem.get_mut("semantic").and_then(Value::as_array_mut),
        "procedural" => mem.get_mut("procedural").and_then(Value::as_array_mut),
        _ => None,
    };

    if let Some(arr) = lane {
        arr.push(req.value.clone());
        let cap = { *mem_limit().read().await };
        while arr.len() > cap { arr.remove(0); }

        // auto-save snapshot
        let snap = mem.clone();
        let _ = save_json_file_async(&memory_path(), &snap).await;

        // event
        let evt = json!({"kind":"Memory.Applied","payload":{"kind": req.kind, "value": req.value, "ttl_ms": req.ttl_ms}});
        state.bus.publish("Memory.Applied", &evt);
        (StatusCode::ACCEPTED, Json(json!({"ok": true}))).into_response()
    } else {
        (StatusCode::BAD_REQUEST, Json(json!({"ok": false, "error": "invalid kind"}))).into_response()
    }
}

async fn list_models() -> impl IntoResponse {
    let v = models().read().await.clone();
    Json::<Vec<Value>>(v)
}
async fn refresh_models(State(state): State<AppState>) -> impl IntoResponse {
    let new = default_models();
    {
        let mut m = models().write().await;
        *m = new.clone();
    }
    let _ = save_json_file_async(&models_path(), &Value::Array(new.clone())).await;
    state.bus.publish("Models.Refreshed", &json!({"count": new.len()}));
    Json::<Vec<Value>>(new)
}
async fn models_save() -> impl IntoResponse {
    let v = models().read().await.clone();
    match save_json_file_async(&models_path(), &Value::Array(v)).await {
        Ok(_) => Json(json!({"ok": true})).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"ok": false, "error": e.to_string()}))).into_response(),
    }
}
async fn models_load() -> impl IntoResponse {
    match load_json_file_async(&models_path()).await.and_then(|v| v.as_array().cloned()) {
        Some(arr) => {
            {
                let mut m = models().write().await;
                *m = arr.clone();
            }
            Json::<Vec<Value>>(arr).into_response()
        }
        None => (StatusCode::NOT_FOUND, Json(json!({"ok": false, "error":"no models.json"}))).into_response(),
    }
}

// ---- Tools ----
async fn list_tools() -> impl IntoResponse {
    let out: Vec<Value> = TOOL_LIST.iter().map(|(id, summary)| json!({"id": id, "summary": summary})).collect();
    Json(out)
}
#[derive(Deserialize)]
struct ToolRunReq { id: String, input: Value }
async fn run_tool_endpoint(State(state): State<AppState>, Json(req): Json<ToolRunReq>) -> impl IntoResponse {
    match run_tool_internal(&req.id, &req.input) {
        Ok(out) => {
            state.bus.publish("Tool.Ran", &json!({"id": req.id, "output": out}));
            Json(out).into_response()
        }
        Err(e) => (StatusCode::BAD_REQUEST, Json(json!({"ok": false, "error": e}))).into_response(),
    }
}

async fn debug_ui() -> impl IntoResponse {
    use axum::http::header::{CACHE_CONTROL, REFERRER_POLICY, X_CONTENT_TYPE_OPTIONS};
    (
        [
            (X_CONTENT_TYPE_OPTIONS, "nosniff"),
            (REFERRER_POLICY, "no-referrer"),
            (CACHE_CONTROL, "no-store"),
        ],
        Html(DEBUG_HTML),
    )
}

// === HTML (debug UI with Save/Load, self-tests, tools panel) ===
static DEBUG_HTML: &str = r#"<!doctype html>
<html lang="en">
<head>
  <meta charset="utf-8">
  <title>ARW Debug</title>
  <meta name="viewport" content="width=device-width,initial-scale=1">
  <style>
    :root{color-scheme:light dark}
    body{font-family:system-ui,Segoe UI,Roboto,Ubuntu,Arial,sans-serif;margin:20px;line-height:1.45}
    header{display:flex;gap:12px;align-items:center;margin-bottom:16px}
    code,pre{background:#0b0b0c10;padding:2px 4px;border-radius:4px}
    .row{display:flex;gap:8px;flex-wrap:wrap;margin:8px 0}
    button,input,select,textarea{font:inherit}
    button{padding:8px 12px;border:1px solid #ddd;background:#fff;border-radius:6px;cursor:pointer}
    button:hover{background:#f3f4f6}
    .cols{display:grid;grid-template-columns:1fr 1fr;gap:16px}
    .box{border:1px solid #e5e7eb;border-radius:6px;padding:12px;background:#fff}
    #log{max-height:40vh;overflow:auto;border:1px solid #e5e7eb;border-radius:6px;padding:8px;background:#fff}
    .evt{padding:4px 6px;border-bottom:1px dashed #eee;font-family:ui-monospace,Menlo,Consolas,monospace}
    .key{color:#6b7280}
    textarea{width:100%;min-height:100px}
    .pass{color:#16a34a}.fail{color:#dc2626}
    @media (max-width:900px){ .cols{grid-template-columns:1fr} }
  </style>
</head>
<body>
  <header>
    <h1>ARW Debug</h1>
    <span class="key">port</span><code id="port"></code>
  </header>

  <div class="row">
    <button onclick="hit('/version')">/version</button>
    <button onclick="hit('/about')">/about</button>
    <button onclick="hit('/introspect/tools')">/introspect/tools</button>
    <button onclick="hit('/probe')">/probe</button>
    <button onclick="hit('/models')">/models</button>
    <button onclick="post('/models/refresh')">POST /models/refresh</button>
  </div>

  <div class="cols">
    <div class="box">
      <h3>Memory</h3>
      <div class="row">
        <button onclick="refreshMemory()">Refresh</button>
        <button onclick="saveMemory()">Save</button>
        <button onclick="loadMemory()">Load</button>
        <select id="memKind">
          <option value="ephemeral">ephemeral</option>
          <option value="episodic">episodic</option>
          <option value="semantic">semantic</option>
          <option value="procedural">procedural</option>
        </select>
        <button onclick="quickApply()">Apply</button>
      </div>
      <div class="row">
        <button onclick="getLimit()">Get limit</button>
        <button onclick="setLimit()">Set limit</button>
        <input id="limitVal" type="number" min="1" value="200" style="width: 90px;">
      </div>
      <textarea id="memBody">{ "msg": "hello from debug UI", "t": Date.now() }</textarea>
      <pre id="memOut">{}</pre>
    </div>

    <div class="box">
      <h3>Events</h3>
      <div class="row">
        <label><input type="checkbox" id="fService" checked> Service</label>
        <label><input type="checkbox" id="fMemory" checked> Memory</label>
        <label><input type="checkbox" id="fModels" checked> Models</label>
      </div>
      <div id="log"></div>
    </div>
  </div>

  <div class="cols" style="margin-top:16px">
    <div class="box">
      <h3>Tools</h3>
      <div class="row">
        <select id="toolId">
          <option value="math.add">math.add</option>
          <option value="time.now">time.now</option>
        </select>
        <button onclick="runTool()">Run tool</button>
      </div>
      <textarea id="toolBody">{ "a": 1.5, "b": 2.25 }</textarea>
      <pre id="toolOut">{}</pre>
    </div>

    <div class="box">
      <h3>Self‑tests</h3>
      <div class="row">
        <button onclick="runSelfTests()">Run self‑tests</button>
        <button onclick="clearTests()">Clear</button>
      </div>
      <pre id="tests">Ready.</pre>
    </div>
  </div>

  <h3>Response</h3>
  <pre id="out">{}</pre>

<script>
const base = location.origin;
document.getElementById('port').textContent = location.host;

async function hit(path){
  const r = await fetch(base + path); 
  document.getElementById('out').textContent = JSON.stringify(await r.json(), null, 2);
}
async function post(path, body){
  const r = await fetch(base + path, { method:'POST', headers:{'Content-Type':'application/json'}, body: body ? JSON.stringify(body) : '{}' });
  const txt = await r.text(); try{ document.getElementById('out').textContent = JSON.stringify(JSON.parse(txt), null, 2); }catch{ document.getElementById('out').textContent = txt; }
}
async function refreshMemory(){
  const r = await fetch(base + '/memory'); const j = await r.json();
  document.getElementById('memOut').textContent = JSON.stringify(j, null, 2);
}
async function saveMemory(){ await post('/memory/save'); await refreshMemory(); }
async function loadMemory(){ await post('/memory/load'); await refreshMemory(); }
async function quickApply(){
  let kind = document.getElementById('memKind').value;
  let bodyTxt = document.getElementById('memBody').value;
  let value; try{ value = JSON.parse(bodyTxt); }catch(e){ alert('Invalid JSON'); return; }
  await post('/memory/apply', { kind, value });
  await refreshMemory();
}
async function getLimit(){ const r = await fetch(base + '/memory/limit'); document.getElementById('out').textContent = JSON.stringify(await r.json(), null, 2); }
async function setLimit(){ const n = parseInt(document.getElementById('limitVal').value||'200',10); await post('/memory/limit', { limit: n }); await getLimit(); }

// Tools
async function runTool(){
  const id = document.getElementById('toolId').value;
  let bodyTxt = document.getElementById('toolBody').value;
  let input; try{ input = JSON.parse(bodyTxt); }catch(e){ alert('Invalid JSON'); return; }
  const r = await fetch(base + '/tools/run', { method:'POST', headers:{'Content-Type':'application/json'}, body: JSON.stringify({ id, input }) });
  const txt = await r.text(); try{ document.getElementById('toolOut').textContent = JSON.stringify(JSON.parse(txt), null, 2); }catch{ document.getElementById('toolOut').textContent = txt; }
}

// Global EventSource + logger
const es = new EventSource(base + '/events');
function allow(kind){
  const s=document.getElementById('fService').checked;
  const m=document.getElementById('fMemory').checked;
  const o=document.getElementById('fModels').checked;
  if(kind.startsWith('Service.')) return s;
  if(kind.startsWith('Memory.'))  return m;
  if(kind.startsWith('Models.'))  return o;
  return true;
}
function pushEvt(kind, data){
  if(!allow(kind)) return;
  const div = document.createElement('div');
  div.className='evt';
  div.textContent = `[${new Date().toLocaleTimeString()}] ${kind}: ${data}`;
  const log = document.getElementById('log');
  log.prepend(div);
  while (log.childElementCount > 200) log.removeChild(log.lastChild);
}
es.onmessage = (e) => pushEvt('message', e.data);
['Service.Connected','Service.Health','Service.Test','Memory.Applied','Models.Refreshed','Tool.Ran'].forEach(k => {
  es.addEventListener(k, (e)=>pushEvt(k, e.data));
});

// --- Self-tests (unchanged from last patch; reuses global SSE) ---
function clearTests(){ document.getElementById('tests').textContent = 'Ready.'; }
function logT(msg, cls){
  const el = document.getElementById('tests');
  if(el.textContent === 'Ready.') el.textContent = '';
  const line = document.createElement('div');
  line.textContent = msg; if(cls) line.className = cls;
  el.appendChild(line);
}
async function jget(path){ const r = await fetch(base+path); if(!r.ok) throw new Error(r.status); return r.json(); }
async function jpost(path, body){ const r = await fetch(base+path,{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify(body||{})}); if(!r.ok) throw new Error(r.status); return r.json(); }
function extractTestId(obj){
  try{
    if (obj?.payload?.payload?.value?.test_id) return obj.payload.payload.value.test_id;
    if (obj?.payload?.value?.test_id) return obj.payload.value.test_id;
    if (obj?.value?.test_id) return obj.value.test_id;
    if (obj?.test_id) return obj.test_id;
  }catch{}
  return undefined;
}
function waitForMemoryApplied(target, timeoutMs){
  return new Promise(resolve => {
    let settled = false;
    const done = ok => { if(!settled){ settled=true; clearTimeout(timer); es.removeEventListener('Memory.Applied', onNamed); es.removeEventListener('message', onMsg); resolve(ok); } };
    function maybeResolve(dataText){
      try{
        const obj = JSON.parse(dataText || '{}');
        const kind = obj?.kind;
        if (kind && kind !== 'Memory.Applied') return;
        const tid = extractTestId(obj);
        if (tid && target && tid !== target) return;
        done(true);
      }catch{}
    }
    const onNamed = (e)=> maybeResolve(e.data);
    const onMsg   = (e)=> maybeResolve(e.data);
    es.addEventListener('Memory.Applied', onNamed);
    es.addEventListener('message',       onMsg);
    const timer = setTimeout(()=> done(false), timeoutMs || 3500);
  });
}
async function runSelfTests(){
  clearTests();
  const pass = m => logT('✔ ' + m, 'pass');
  const fail = (m,e) => logT('✘ ' + m + ' — ' + (e && e.message ? e.message : e), 'fail');

  try { const v = await jget('/version'); if(v.service && v.version){ pass('/version'); } else { fail('/version','missing keys'); } } catch(e){ fail('/version',e); }
  try { const a = await jget('/about'); if(Array.isArray(a.endpoints)){ pass('/about'); } else { fail('/about','missing endpoints'); } } catch(e){ fail('/about',e); }
  try { const t = await jget('/introspect/tools'); if(Array.isArray(t) && t.length>=2){ pass('/introspect/tools'); } else { fail('/introspect/tools','unexpected'); } } catch(e){ fail('/introspect/tools',e); }

  try { await jpost('/memory/apply',{kind:'ephemeral',value:{ from:'selftest', t: Date.now() }}); const m = await jget('/memory'); if(m && m.ephemeral){ pass('POST /memory/apply + GET /memory'); } else { fail('memory','unexpected'); } } catch(e){ fail('memory',e); }

  try {
    const target = 'ui_selftest_' + Date.now();
    const waiter = waitForMemoryApplied(target, 3500);
    await jpost('/memory/apply',{kind:'ephemeral',value:{ test:'selftest', test_id: target, t: Date.now() }});
    const ok = await waiter;
    if(ok){ pass('SSE Memory.Applied'); } else { fail('SSE Memory.Applied','timeout'); }
  } catch(e){ fail('SSE Memory.Applied',e); }

  logT('Done.');
}
</script>
</body>
</html>
"#;
