#![allow(dead_code)]

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
use std::collections::HashMap;
use std::sync::OnceLock;
use tokio::sync::RwLock;
use std::fs;
use tokio::fs as afs;
use tokio::io::AsyncWriteExt;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};
use std::sync::atomic::{AtomicU64, Ordering};
use std::collections::VecDeque;
use futures_util::StreamExt;

use crate::AppState;

// ---------- state paths & file helpers ----------
fn state_dir() -> PathBuf {
    let v = arw_core::load_effective_paths();
    let s = v.get("state_dir").and_then(|x| x.as_str()).unwrap_or(".");
    PathBuf::from(s.replace('\\', "/"))
}
fn memory_path() -> PathBuf { state_dir().join("memory.json") }
fn models_path() -> PathBuf { state_dir().join("models.json") }
fn orch_path() -> PathBuf { state_dir().join("orchestration.json") }
fn feedback_path() -> PathBuf { state_dir().join("feedback.json") }
fn audit_path() -> PathBuf { state_dir().join("audit.log") }

fn load_json_file(p: &Path) -> Option<Value> {
    let s = fs::read_to_string(p).ok()?;
    serde_json::from_str(&s).ok()
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

// ---------- persistence bootstrap ----------
#[derive(serde::Deserialize)]
struct OrchFile { #[serde(default)] profile: Option<String>, #[serde(default)] hints: Option<Hints>, #[serde(default)] memory_limit: Option<usize> }

pub async fn load_persisted() {
    // orchestration
    if let Some(v) = load_json_file(&orch_path()) {
        let of: Result<OrchFile, _> = serde_json::from_value(v);
        if let Ok(of) = of {
            if let Some(p) = of.profile { let mut g = governor_profile().write().await; *g = p; }
            if let Some(h) = of.hints { let mut hh = hints().write().await; *hh = h; }
            if let Some(m) = of.memory_limit { let mut ml = mem_limit().write().await; *ml = m.max(1); }
        }
    }
    // feedback
    if let Some(v) = load_json_file(&feedback_path()) {
        if let Ok(fb) = serde_json::from_value::<FeedbackState>(v) { let mut st = feedback_cell().write().await; *st = fb; }
    }
}

async fn persist_orch() {
    let profile = { governor_profile().read().await.clone() };
    let hints = { hints().read().await.clone() };
    let ml = { *mem_limit().read().await };
    let v = json!({"profile": profile, "hints": hints, "memory_limit": ml});
    let _ = save_json_file_async(&orch_path(), &v).await;
}
async fn persist_feedback() {
    let st = { feedback_cell().read().await.clone() };
    let v = serde_json::to_value(st).unwrap_or_else(|_| json!({}));
    let _ = save_json_file_async(&feedback_path(), &v).await;
}

async fn audit_event(action: &str, details: &Value) {
    let ts = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
    let line = serde_json::json!({"time": ts, "action": action, "details": details});
    let s = serde_json::to_string(&line).unwrap_or_else(|_| "{}".to_string()) + "\n";
    let p = audit_path();
    if let Some(parent) = p.parent() { let _ = afs::create_dir_all(parent).await; }
    use tokio::io::AsyncWriteExt;
    if let Ok(mut f) = afs::OpenOptions::new().create(true).append(true).open(&p).await {
        let _ = f.write_all(s.as_bytes()).await;
    }
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

static DEFAULT_MODEL: OnceLock<RwLock<String>> = OnceLock::new();
fn default_model() -> &'static RwLock<String> {
    DEFAULT_MODEL.get_or_init(|| {
        let initial = default_models()
            .first()
            .and_then(|v| v.get("id").and_then(|s| s.as_str()))
            .unwrap_or("")
            .to_string();
        RwLock::new(initial)
    })
}

// ---------- governor profile ----------
static GOV_PROFILE: OnceLock<RwLock<String>> = OnceLock::new();
fn governor_profile() -> &'static RwLock<String> {
    GOV_PROFILE.get_or_init(|| {
        let initial = std::env::var("ARW_PROFILE").unwrap_or_else(|_| "balanced".to_string());
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
        // governor
        .route("/governor/profile", get(governor_get))
        .route("/governor/profile", post(governor_set))
        .route("/governor/hints", get(governor_hints_get))
        .route("/governor/hints", post(governor_hints_set))
        // feedback (self-learning)
        .route("/feedback/state", get(feedback_state_get))
        .route("/feedback/signal", post(feedback_signal_post))
        .route("/feedback/analyze", post(feedback_analyze_post))
        .route("/feedback/apply", post(feedback_apply_post))
        .route("/feedback/auto", post(feedback_auto_post))
        .route("/feedback/reset", post(feedback_reset_post))
        // stats
        .route("/introspect/stats", get(stats_get))
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
        .route("/models/add", post(models_add))
        .route("/models/delete", post(models_delete))
        .route("/models/default", get(models_default_get))
        .route("/models/default", post(models_default_set))
        .route("/models/download", post(models_download))
        // tools
        .route("/tools", get(list_tools))
        .route("/tools/run", post(run_tool_endpoint))
        // chat
        .route("/chat", get(chat_get))
        .route("/chat/send", post(chat_send))
        .route("/chat/clear", post(chat_clear));

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
        "docs_url": std::env::var("ARW_DOCS_URL").ok(),
        "endpoints": [
          "/governor/profile",
          "/governor/hints",
          "/healthz",
          "/events",
          "/version",
          "/about",
          "/introspect/stats",
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
          "/models/add",
          "/models/delete",
          "/models/default",
          "/tools",
          "/tools/run",
          "/chat",
          "/chat/send",
          "/chat/clear",
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
    persist_orch().await;
    Json(json!({ "ok": true }))
}

// Governor profile
#[derive(Deserialize)]
struct SetProfile { name: String }
async fn governor_get() -> impl IntoResponse {
    let p = { governor_profile().read().await.clone() };
    Json(json!({ "profile": p }))
}
async fn governor_set(State(state): State<AppState>, Json(req): Json<SetProfile>) -> impl IntoResponse {
    {
        let mut g = governor_profile().write().await;
        *g = req.name.clone();
    }
    state.bus.publish("Governor.Changed", &json!({"profile": req.name.clone()}));
    persist_orch().await;
    Json(json!({ "ok": true }))
}

#[derive(Deserialize, serde::Serialize, Clone)]
struct Hints { #[serde(default)] max_concurrency: Option<usize>, #[serde(default)] event_buffer: Option<usize>, #[serde(default)] http_timeout_secs: Option<u64> }
static HINTS: OnceLock<RwLock<Hints>> = OnceLock::new();
fn hints() -> &'static RwLock<Hints> { HINTS.get_or_init(|| RwLock::new(Hints{ max_concurrency: None, event_buffer: None, http_timeout_secs: None })) }
async fn governor_hints_get() -> impl IntoResponse { let h = hints().read().await.clone(); Json(serde_json::to_value(h).unwrap_or(json!({}))) }
async fn governor_hints_set(Json(req): Json<Hints>) -> impl IntoResponse {
    {
        let mut h = hints().write().await;
        if req.max_concurrency.is_some() { h.max_concurrency = req.max_concurrency; }
        if req.event_buffer.is_some() { h.event_buffer = req.event_buffer; }
        if req.http_timeout_secs.is_some() { h.http_timeout_secs = req.http_timeout_secs; }
    }
    persist_orch().await;
    Json(json!({"ok": true}))
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

#[derive(Deserialize)]
struct ModelId { id: String, #[serde(default)] provider: Option<String> }
async fn models_add(State(state): State<AppState>, Json(req): Json<ModelId>) -> impl IntoResponse {
    let mut v = models().write().await;
    if !v.iter().any(|m| m.get("id").and_then(|s| s.as_str()) == Some(&req.id)) {
        v.push(json!({"id": req.id, "provider": req.provider.unwrap_or_else(|| "local".to_string()), "status":"available"}));
        state.bus.publish("Models.Changed", &json!({"op":"add","id": v.last().and_then(|m| m.get("id")).cloned()}));
    }
    audit_event("models.add", &json!({"id": req.id})).await;
    Json(json!({"ok": true}))
}
async fn models_delete(State(state): State<AppState>, Json(req): Json<ModelId>) -> impl IntoResponse {
    let mut v = models().write().await;
    let before = v.len();
    v.retain(|m| m.get("id").and_then(|s| s.as_str()) != Some(&req.id));
    if v.len() != before { state.bus.publish("Models.Changed", &json!({"op":"delete","id": req.id})); }
    audit_event("models.delete", &json!({"id": req.id})).await;
    Json(json!({"ok": true}))
}
async fn models_default_get() -> impl IntoResponse {
    let id = { default_model().read().await.clone() };
    Json(json!({"default": id }))
}
async fn models_default_set(State(state): State<AppState>, Json(req): Json<ModelId>) -> impl IntoResponse {
    {
        let mut d = default_model().write().await;
        *d = req.id.clone();
    }
    state.bus.publish("Models.Changed", &json!({"op":"default","id": req.id}));
    let _ = save_json_file_async(&models_path(), &Value::Array(models().read().await.clone())).await;
    persist_orch().await;
    audit_event("models.default", &json!({"id": req.id})).await;
    Json(json!({"ok": true}))
}

#[derive(Deserialize)]
struct DownloadReq { id: String, url: String, #[serde(default)] provider: Option<String> }
async fn models_download(State(state): State<AppState>, Json(req): Json<DownloadReq>) -> impl IntoResponse {
    // ensure model exists with status
    {
        let mut v = models().write().await;
        if let Some(m) = v.iter_mut().find(|m| m.get("id").and_then(|s| s.as_str()) == Some(&req.id)) {
            *m = json!({"id": req.id, "provider": req.provider.clone().unwrap_or("local".into()), "status":"downloading"});
        } else {
            v.push(json!({"id": req.id, "provider": req.provider.clone().unwrap_or("local".into()), "status":"downloading"}));
        }
    }
    state.bus.publish("Models.Download", &json!({"id": req.id}));
    audit_event("models.download", &json!({"id": req.id})).await;
    let id = req.id.clone();
    let url = req.url.clone();
    let provider = req.provider.clone().unwrap_or("local".into());
    let sp = state.clone();
    tokio::spawn(async move {
        let file_name = url.rsplit('/').next().unwrap_or(&id).to_string();
        let target = state_dir().join("models").join(&file_name);
        if let Some(parent) = target.parent() { let _ = afs::create_dir_all(parent).await; }
        let client = reqwest::Client::new();
        match client.get(&url).send().await {
            Ok(resp) => {
                let total = resp.content_length().unwrap_or(0);
                if let Ok(mut file) = afs::File::create(&target).await {
                        let mut downloaded: u64 = 0;
                        let mut stream = resp.bytes_stream();
                        while let Some(chunk) = stream.next().await {
                            match chunk {
                                Ok(bytes) => {
                                    if file.write_all(&bytes).await.is_ok() {
                                        downloaded += bytes.len() as u64;
                                        if total > 0 {
                                            let pct = ((downloaded * 100) / total).min(100);
                                            sp.bus.publish("Models.DownloadProgress", &json!({"id": id, "progress": pct}));
                                        }
                                    } else {
                                        return;
                                    }
                                }
                                Err(_) => return,
                            }
                        }
                        {
                            let mut v = models().write().await;
                            if let Some(m) = v.iter_mut().find(|m| m.get("id").and_then(|s| s.as_str()) == Some(&id)) {
                                *m = json!({"id": id, "provider": provider, "status":"available", "path": target.to_string_lossy()});
                            }
                        }
                        let _ = save_json_file_async(&models_path(), &Value::Array(models().read().await.clone())).await;
                        sp.bus.publish("Models.Changed", &json!({"op":"downloaded","id": id}));
                }
            }
            Err(_) => {
                sp.bus.publish("Models.DownloadProgress", &json!({"id": id, "error": "download failed"}));
            }
        }
    });
    Json(json!({"ok": true}))
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

// ---- Chat ----
static CHAT_LOG: OnceLock<RwLock<Vec<Value>>> = OnceLock::new();
fn chat_log() -> &'static RwLock<Vec<Value>> { CHAT_LOG.get_or_init(|| RwLock::new(Vec::new())) }

#[derive(Deserialize)]
struct ChatSendReq { message: String, #[serde(default)] model: Option<String> }

fn synth_reply(msg: &str, model: &str) -> String {
    match model.to_ascii_lowercase().as_str() {
        "reverse" => msg.chars().rev().collect(),
        "time" => {
            let now = SystemTime::now().duration_since(UNIX_EPOCH).ok().map(|d| d.as_secs()).unwrap_or(0);
            format!("[{}] {}", now, msg)
        }
        _ => format!("You said: {}", msg),
    }
}

// ---- Self-learning / Feedback Layer ----
#[derive(Clone, serde::Serialize, serde::Deserialize)]
struct FeedbackSignal { id: String, ts: String, kind: String, target: String, confidence: f64, severity: u8, note: Option<String> }
#[derive(Clone, serde::Serialize, serde::Deserialize)]
struct Suggestion { id: String, action: String, params: Value, rationale: String, confidence: f64 }
#[derive(Clone, Default, serde::Serialize, serde::Deserialize)]
struct FeedbackState { auto_apply: bool, signals: Vec<FeedbackSignal>, suggestions: Vec<Suggestion> }
static FEEDBACK: OnceLock<RwLock<FeedbackState>> = OnceLock::new();
fn feedback_cell() -> &'static RwLock<FeedbackState> { FEEDBACK.get_or_init(|| RwLock::new(FeedbackState::default())) }
static SUGG_SEQ: OnceLock<AtomicU64> = OnceLock::new();
fn next_id() -> String { let s = SUGG_SEQ.get_or_init(|| AtomicU64::new(1)).fetch_add(1, Ordering::Relaxed); format!("sug-{}", s) }

async fn analyze_feedback() {
    let routes = route_stats_cell().read().await.clone();
    let events = stats_cell().read().await.clone();
    let mut out: Vec<Suggestion> = Vec::new();

    // Heuristic 1: High route latency -> suggest increasing http timeout hint
    let mut worst: Option<(&String, &RouteStat)> = None;
    for (p, st) in &routes.by_path {
        if worst.as_ref().map(|(_, s)| s.ewma_ms).unwrap_or(0.0) < st.ewma_ms {
            worst = Some((p, st));
        }
    }
    if let Some((p, st)) = worst { if st.ewma_ms > 800.0 {
        let desired = (((st.ewma_ms/1000.0)*2.0)+10.0).clamp(20.0, 180.0) as u64;
        out.push(Suggestion{
            id: next_id(), action: "hint".into(), params: json!({"http_timeout_secs": desired}),
            rationale: format!("High latency on {} (~{:.0} ms); suggest http timeout {}s", p, st.ewma_ms, desired), confidence: 0.6
        });
    } }

    // Heuristic 2: High error rate -> suggest balanced profile
    let mut high_err = false;
    for st in routes.by_path.values() { if st.hits>=10 && (st.errors as f64)/(st.hits as f64) > 0.2 { high_err = true; break; } }
    if high_err { out.push(Suggestion{ id: next_id(), action: "profile".into(), params: json!({"name":"balanced"}), rationale: "High error rate observed across routes".into(), confidence: 0.55 }); }

    // Heuristic 3: Many memory applications -> suggest increasing memory limit modestly
    let mem_applied = events.kinds.get("Memory.Applied").cloned().unwrap_or(0);
    if mem_applied > 200 {
        let cur = { *mem_limit().read().await } as u64;
        if cur < 300 { let new = (cur*3/2).clamp(200, 600); out.push(Suggestion{ id: next_id(), action: "mem_limit".into(), params: json!({"limit": new}), rationale: format!("Frequent memory updates ({}); suggest limit {}", mem_applied, new), confidence: 0.5 }); }
    }

    let mut st = feedback_cell().write().await;
    st.suggestions = out;
}

async fn feedback_state_get() -> impl IntoResponse { let st = feedback_cell().read().await.clone(); Json(st) }

#[derive(serde::Deserialize)]
struct FeedbackSignalPost { kind: String, target: String, confidence: f64, severity: u8, note: Option<String> }
async fn feedback_signal_post(State(state): State<AppState>, Json(req): Json<FeedbackSignalPost>) -> impl IntoResponse {
    let ts = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
    let sig = FeedbackSignal { id: next_id(), ts, kind: req.kind, target: req.target, confidence: req.confidence.clamp(0.0,1.0), severity: req.severity.clamp(1, 5), note: req.note };
    {
        let mut st = feedback_cell().write().await;
        st.signals.push(sig.clone());
        if st.signals.len()>200 { st.signals.remove(0); }
    }
    state.bus.publish("Feedback.Signal", &json!({"signal": sig}));
    analyze_feedback().await;
    let st = feedback_cell().read().await.clone();
    persist_feedback().await;
    Json(st)
}

async fn feedback_analyze_post() -> impl IntoResponse { analyze_feedback().await; let st = feedback_cell().read().await.clone(); persist_feedback().await; Json(st) }

#[derive(serde::Deserialize)]
struct ApplyReq { id: String }
async fn feedback_apply_post(State(state): State<AppState>, Json(req): Json<ApplyReq>) -> impl IntoResponse {
    let sug_opt = { feedback_cell().read().await.suggestions.iter().find(|s| s.id==req.id).cloned() };
    if let Some(sug) = sug_opt { let ok = apply_suggestion(&sug, &state).await; if ok { state.bus.publish("Feedback.Applied", &json!({"id": sug.id, "action": sug.action, "params": sug.params})); persist_orch().await; } return Json(json!({"ok": ok})); }
    Json(json!({"ok": false}))
}

#[derive(serde::Deserialize)]
struct AutoReq { enabled: bool }
async fn feedback_auto_post(Json(req): Json<AutoReq>) -> impl IntoResponse { let mut st = feedback_cell().write().await; st.auto_apply = req.enabled; drop(st); persist_feedback().await; Json(json!({"ok": true})) }
async fn feedback_reset_post() -> impl IntoResponse { let mut st = feedback_cell().write().await; st.signals.clear(); st.suggestions.clear(); drop(st); persist_feedback().await; Json(json!({"ok": true})) }

async fn apply_suggestion(s: &Suggestion, state: &AppState) -> bool {
    match s.action.as_str() {
        "hint" => {
            if let Some(v) = s.params.get("http_timeout_secs").and_then(|x| x.as_u64()) { let mut h = hints().write().await; h.http_timeout_secs = Some(v); true } else { false }
        }
        "profile" => {
            if let Some(name) = s.params.get("name").and_then(|x| x.as_str()) { let mut g = governor_profile().write().await; *g = name.to_string(); state.bus.publish("Governor.Changed", &json!({"profile": name})); true } else { false }
        }
        "mem_limit" => {
            if let Some(new) = s.params.get("limit").and_then(|x| x.as_u64()) { let mut m = mem_limit().write().await; *m = (new as usize).max(1); true } else { false }
        }
        _ => false
    }
}
async fn chat_get() -> impl IntoResponse {
    let msgs = chat_log().read().await.clone();
    Json(json!({"messages": msgs}))
}
async fn chat_clear() -> impl IntoResponse {
    chat_log().write().await.clear();
    Json(json!({"ok": true}))
}
async fn chat_send(State(state): State<AppState>, Json(req): Json<ChatSendReq>) -> impl IntoResponse {
    let model = req.model.clone().unwrap_or_else(|| "echo".to_string());
    let user = json!({"role":"user","content": req.message});
    let reply_txt = synth_reply(&req.message, &model);
    let assist = json!({"role":"assistant","content": reply_txt, "model": model});
    {
        let mut log = chat_log().write().await;
        log.push(user.clone());
        log.push(assist.clone());
        while log.len() > 200 { log.remove(0); }
    }
    state.bus.publish("Chat.Message", &json!({"dir":"in","msg": user}));
    state.bus.publish("Chat.Message", &json!({"dir":"out","msg": assist}));
    Json(assist)
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
static DEBUG_HTML: &str = r##"<!doctype html>
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
    .iconbtn{padding:6px 8px;font-size:12px}
    .cols{display:grid;grid-template-columns:1fr 1fr;gap:16px}
    .box{border:1px solid #e5e7eb;border-radius:6px;padding:12px;background:#fff}
    #log{max-height:40vh;overflow:auto;border:1px solid #e5e7eb;border-radius:6px;padding:8px;background:#fff}
    .evt{padding:4px 6px;border-bottom:1px dashed #eee;font-family:ui-monospace,Menlo,Consolas,monospace}
    .key{color:#6b7280}
    textarea{width:100%;min-height:100px}
    .pass{color:#16a34a}.fail{color:#dc2626}
    a.help{display:inline-block;margin-left:8px;width:18px;height:18px;border-radius:50%;border:1px solid #d1d5db;color:#6b7280;text-align:center;line-height:16px;text-decoration:none;font-weight:600;font-size:12px}
    a.help:hover{background:#f3f4f6;color:#374151}
    .tip{position:fixed;z-index:50;max-width:360px;background:#ffffff;border:1px solid #e5e7eb;border-radius:8px;box-shadow:0 6px 24px rgba(0,0,0,0.08);padding:10px 12px}
    .tip .t-title{font-weight:600;margin-bottom:4px}
    .tip .t-more{color:#2563eb;text-decoration:none}
    .t-hidden{display:none}
    .toast{position:fixed;left:50%;transform:translateX(-50%);bottom:24px;background:#fef3c7;border:1px solid #f59e0b;color:#92400e;padding:8px 12px;border-radius:8px;display:none;max-width:70vw}
    #insights{position:fixed;right:16px;bottom:16px;background:#fff;border:1px solid #e5e7eb;border-radius:10px;box-shadow:0 10px 30px rgba(0,0,0,0.08);padding:10px 12px;display:none;min-width:260px;max-width:50vw}
    #insights .kv{display:flex;justify-content:space-between;color:#374151}
    #insights h4{margin:4px 0 6px 0;font-size:14px}
    @media (max-width:900px){ .cols{grid-template-columns:1fr} }
  </style>
</head>
<body>
  <header>
    <h1>ARW Debug</h1>
    <span class="key">port</span><code id="port"></code>
    <a id="docsLink" class="help" title="Open docs" href="#" target="_blank" style="margin-left:auto;display:none">?</a>
    <button id="toggleInsights" class="iconbtn" title="Insights" style="display:inline-block">Insights</button>
    <button id="copyStatsBtn" class="iconbtn" title="Copy stats JSON" style="display:inline-block">Copy stats</button>
    <button id="copyCurlBtn" class="iconbtn" style="display:none" title="Copy last request as curl">Copy curl</button>
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
      <h3>Chat <a href="#" class="help" data-doc="orchestrator" data-tip="Super simple debug chat; models are stubbed (echo/reverse/time)." title="Chat docs">?</a></h3>
      <div class="row">
        <select id="chatModel">
          <option value="echo">echo</option>
          <option value="reverse">reverse</option>
          <option value="time">time</option>
        </select>
        <input id="chatTemp" type="number" step="0.1" min="0" max="2" value="0.7" style="width: 80px;">
      </div>
      <textarea id="chatInput" placeholder="Say hello"></textarea>
      <div class="row"><button onclick="chatSend()">Send</button> <button onclick="chatClear()">Clear</button></div>
      <pre id="chatOut">[]</pre>
    </div>
    <div class="box">
      <h3>Orchestration <a href="#" class="help" data-doc="orchestrator" data-tip="Common actions to manage the local agent service." title="Orchestration docs">?</a></h3>
      <div class="row">
        <button onclick="orProbe()">Probe now</button>
        <button onclick="orEmitTest()">Emit test</button>
        <button onclick="orRefreshModels()">Refresh models</button>
        <button onclick="runSelfTests()">Self‑tests</button>
        <button onclick="orShutdown()">Shutdown</button>
      </div>
      <div class="row">
        <label class="key">profile</label>
        <select id="profileSel">
          <option value="performance">performance</option>
          <option value="balanced" selected>balanced</option>
          <option value="power-saver">power-saver</option>
        </select>
        <button onclick="orProfileApply()">Apply</button>
        <button onclick="orProfileGet()">Get</button>
      </div>
      <div class="row">
        <label class="key">hints</label>
        <input id="hintConc" type="number" min="1" placeholder="concurrency" style="width:120px;">
        <input id="hintBuf" type="number" min="1" placeholder="event buffer" style="width:140px;">
        <input id="hintTimeout" type="number" min="1" placeholder="http timeout s" style="width:140px;">
        <button onclick="orHintsApply()">Apply</button>
        <button onclick="orHintsGet()">Get</button>
      </div>
      <div class="row"><span class="key">time</span> <code id="rt-orch">–</code></div>
    </div>
    <div class="box">
      <h3>Memory <a href="#" class="help" data-doc="memory" data-tip="Understand memory kinds and how /memory endpoints work." title="Memory docs">?</a></h3>
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
      <div class="row"><span class="key">time</span> <code id="rt-mem">–</code></div>
      <div class="row">
        <button onclick="getLimit()">Get limit</button>
        <button onclick="setLimit()">Set limit</button>
        <input id="limitVal" type="number" min="1" value="200" style="width: 90px;">
      </div>
      <textarea id="memBody">{ "msg": "hello from debug UI", "t": Date.now() }</textarea>
      <div class="row"><button class="iconbtn" id="copyJsonMem" title="Copy JSON">Copy</button></div>
      <pre id="memOut">{}</pre>
    </div>

    <div class="box">
      <h3>Events <a href="#" class="help" data-doc="events" data-tip="Live event stream from /events (SSE)." title="Events docs">?</a></h3>
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
      <h3>Tools <a href="#" class="help" data-doc="tools" data-tip="Tools, schemas, and how to call them." title="Tools docs">?</a></h3>
      <div class="row">
        <select id="toolId">
          <option value="math.add">math.add</option>
          <option value="time.now">time.now</option>
        </select>
        <button onclick="runTool()">Run tool</button>
      </div>
      <div class="row"><span class="key">time</span> <code id="rt-tool">–</code></div>
      <textarea id="toolBody">{ "a": 1.5, "b": 2.25 }</textarea>
      <div class="row"><button class="iconbtn" id="copyJsonTool" title="Copy JSON">Copy</button></div>
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
    <div class="box">
      <h3>Self‑Learning <a href="#" class="help" data-doc="orchestrator" data-tip="Signals → suggestions. Gently embedded feedback." title="Self‑Learning docs">?</a></h3>
      <div class="row">
        <select id="sigKind">
          <option value="latency">latency</option>
          <option value="errors">errors</option>
          <option value="memory">memory</option>
          <option value="cpu">cpu</option>
        </select>
        <input id="sigTarget" placeholder="target (/path or id)" style="width:220px;">
        <input id="sigConf" type="number" step="0.05" min="0" max="1" value="0.7" style="width:90px;">
        <input id="sigSev" type="number" step="1" min="1" max="5" value="2" style="width:70px;">
      </div>
      <div class="row">
        <input id="sigNote" placeholder="note (optional)" style="width:60%">
        <button onclick="fbSignal()">Signal</button>
        <button onclick="fbAnalyze()">Analyze now</button>
        <label><input type="checkbox" id="autoApply"> auto‑apply safe</label>
        <button onclick="fbAuto()">Set</button>
      </div>
      <div class="row">
        <button onclick="fbState()">Refresh</button>
        <button onclick="fbReset()">Reset</button>
        <input id="fbApplyId" placeholder="suggestion id" style="width:160px;">
        <button onclick="fbApply()">Apply</button>
      </div>
      <pre id="fbOut">{"signals":[],"suggestions":[]}</pre>
    </div>
    <div class="box">
      <h3>Models <a href="#" class="help" data-doc="orchestrator" data-tip="Add/delete and set default model. Future: downloads." title="Models docs">?</a></h3>
      <div class="row">
        <input id="mId" placeholder="model id" style="width:220px;">
        <input id="mProv" placeholder="provider (local)" style="width:160px;">
      </div>
      <div class="row">
        <button onclick="modelsList()">List</button>
        <button onclick="modelsAdd()">Add</button>
        <button onclick="modelsDelete()">Delete</button>
        <button onclick="modelsDefaultGet()">Get default</button>
        <button onclick="modelsDefaultSet()">Set default</button>
        <button onclick="modelsDownload()">Download</button>
      </div>
      <pre id="modelsOut">[]</pre>
    </div>
  </div>

  <h3>Response <span class="key">time</span> <code id="rt-main">–</code> <button class="iconbtn" id="copyJsonMain" title="Copy JSON">Copy</button></h3>
  <pre id="out">{}</pre>

<script>
const base = location.origin;
document.getElementById('port').textContent = location.host;
let lastCurl = null;
function mkcurl(method, url, body){
  const parts = ["curl -sS", "-X", method, `'${url}'`];
  if (body != null){ parts.push("-H", `'Content-Type: application/json'`); parts.push("-d", `'${JSON.stringify(body)}'`); }
  return parts.join(' ');
}
function setLastCurl(s){
  lastCurl = s; const b = document.getElementById('copyCurlBtn'); if (s){ b.style.display='inline-block'; } else { b.style.display='none'; }
}
document.getElementById('copyCurlBtn').addEventListener('click', async ()=>{
  if(!lastCurl) return; try{ await navigator.clipboard.writeText(lastCurl); const b = document.getElementById('copyCurlBtn'); const old = b.textContent; b.textContent='Copied'; setTimeout(()=> b.textContent=old, 900);}catch{}
});
async function req(method, path, body, outId, rtId){
  const url = base + path; const init = { method, headers: {}, body: undefined };
  if (body != null){ init.headers['Content-Type'] = 'application/json'; init.body = JSON.stringify(body); }
  setLastCurl(mkcurl(method, url, body));
  const t0 = performance.now();
  try{
    const resp = await fetch(url, init);
    const txt = await resp.text();
    const dt = Math.round(performance.now() - t0);
    if(rtId){ const el = document.getElementById(rtId); if (el) el.textContent = dt + ' ms'; }
    const out = document.getElementById(outId);
    try{ out.textContent = JSON.stringify(JSON.parse(txt), null, 2); }catch{ out.textContent = txt; }
    if(!resp.ok){ showToast('Request failed: ' + resp.status + ' ' + (resp.statusText||'')); }
    return txt;
  }catch(e){
    if(rtId){ const el = document.getElementById(rtId); if (el) el.textContent = 'error'; }
    showToast('Network error');
    throw e;
  }
}
async function hit(path){ await req('GET', path, null, 'out', 'rt-main'); }
async function post(path, body){ await req('POST', path, body, 'out', 'rt-main'); }
async function refreshMemory(){
  const txt = await req('GET', '/memory', null, 'memOut', 'rt-mem'); return txt;
}
async function saveMemory(){ await post('/memory/save'); await refreshMemory(); }
async function loadMemory(){ await post('/memory/load'); await refreshMemory(); }
async function quickApply(){
  let kind = document.getElementById('memKind').value;
  let bodyTxt = document.getElementById('memBody').value;
  let value; try{ value = JSON.parse(bodyTxt); }catch(e){ alert('Invalid JSON'); return; }
  await req('POST', '/memory/apply', { kind, value }, 'out', 'rt-main');
  await refreshMemory();
}
async function getLimit(){ await req('GET', '/memory/limit', null, 'out', 'rt-main'); }
async function setLimit(){ const n = parseInt(document.getElementById('limitVal').value||'200',10); await req('POST', '/memory/limit', { limit: n }, 'out', 'rt-main'); await getLimit(); }

// Tools
async function runTool(){
  const id = document.getElementById('toolId').value;
  let bodyTxt = document.getElementById('toolBody').value;
  let input; try{ input = JSON.parse(bodyTxt); }catch(e){ alert('Invalid JSON'); return; }
  await req('POST', '/tools/run', { id, input }, 'toolOut', 'rt-tool');
}

// Orchestration actions
async function orProbe(){ await req('GET', '/probe', null, 'out', 'rt-orch'); }
async function orEmitTest(){ await req('GET', '/emit/test', null, 'out', 'rt-orch'); }
async function orRefreshModels(){ await req('POST', '/models/refresh', null, 'out', 'rt-orch'); }
async function orShutdown(){ await req('GET', '/shutdown', null, 'out', 'rt-orch'); }
async function orProfileApply(){ const name = document.getElementById('profileSel').value; await req('POST','/governor/profile',{name},'out','rt-orch'); }
async function orProfileGet(){ await req('GET','/governor/profile',null,'out','rt-orch'); }
async function orHintsApply(){ const h={}; const c=document.getElementById('hintConc').value; const b=document.getElementById('hintBuf').value; const t=document.getElementById('hintTimeout').value; if(c) h.max_concurrency=parseInt(c,10); if(b) h.event_buffer=parseInt(b,10); if(t) h.http_timeout_secs=parseInt(t,10); if(Object.keys(h).length===0) return; await req('POST','/governor/hints',h,'out','rt-orch'); }
async function orHintsGet(){ await req('GET','/governor/hints',null,'out','rt-orch'); }

// Models UI
async function modelsList(){ await req('GET','/models',null,'modelsOut','rt-orch'); }
async function modelsAdd(){ const id=document.getElementById('mId').value.trim(); if(!id) return; const provider=(document.getElementById('mProv').value||'local').trim(); await req('POST','/models/add',{id,provider},'modelsOut','rt-orch'); await modelsList(); }
async function modelsDelete(){ const id=document.getElementById('mId').value.trim(); if(!id) return; await req('POST','/models/delete',{id},'modelsOut','rt-orch'); await modelsList(); }
async function modelsDefaultGet(){ await req('GET','/models/default',null,'modelsOut','rt-orch'); }
async function modelsDefaultSet(){ const id=document.getElementById('mId').value.trim(); if(!id) return; await req('POST','/models/default',{id},'modelsOut','rt-orch'); }
async function modelsDownload(){ const id=document.getElementById('mId').value.trim(); if(!id) return; const provider=(document.getElementById('mProv').value||'local').trim(); await req('POST','/models/download',{id,provider},'modelsOut','rt-orch'); }

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

// Insights aggregation overlay
const agg = { total:0, kinds:{} };
let routesCache = {};
function renderInsights(){
  const el = document.getElementById('insights'); if(!el) return;
  const lines = [ `<h4>Events</h4>`, `<div class='kv'><b>Total</b><span>${agg.total}</span></div>` ];
  Object.entries(agg.kinds).slice(0,6).forEach(([k,v])=>{ lines.push(`<div class='kv'><span>${k}</span><span>${v}</span></div>`); });
  const entries = Object.entries(routesCache || {}).map(([p,st])=>({p,st})).sort((a,b)=> (b.st.p95_ms||0)-(a.st.p95_ms||0)).slice(0,3);
  if(entries.length){
    lines.push('<h4>Routes</h4>');
    entries.forEach(({p,st})=>{ lines.push(`<div class='kv'><span>${p}</span><span>p95:${st.p95_ms||0} ms · ewma:${(st.ewma_ms||0).toFixed(0)} ms · e:${st.errors||0}</span></div>`); });
  }
  el.innerHTML = lines.join('');
}
['Service.Connected','Service.Health','Service.Test','Memory.Applied','Models.Refreshed','Tool.Ran','Chat.Message','Governor.Changed'].forEach(k => {
  es.addEventListener(k, (e)=>{ agg.total++; agg.kinds[k]=(agg.kinds[k]||0)+1; renderInsights(); });
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
// Self-learning fns
async function fbState(){ await req('GET','/feedback/state',null,'fbOut','rt-orch'); }
async function fbAnalyze(){ await req('POST','/feedback/analyze',{},'fbOut','rt-orch'); }
async function fbSignal(){
  const kind=document.getElementById('sigKind').value;
  const target=document.getElementById('sigTarget').value||'';
  const confidence=parseFloat(document.getElementById('sigConf').value||'0.7');
  const severity=parseInt(document.getElementById('sigSev').value||'2',10);
  const note=document.getElementById('sigNote').value||undefined;
  await req('POST','/feedback/signal',{kind,target,confidence,severity,note},'fbOut','rt-orch');
}
async function fbAuto(){ const enabled=document.getElementById('autoApply').checked; await req('POST','/feedback/auto',{enabled},'fbOut','rt-orch'); }
async function fbReset(){ await req('POST','/feedback/reset',{},'fbOut','rt-orch'); }
async function fbApply(){ const id=(document.getElementById('fbApplyId').value||'').trim(); if(!id){ showToast('Enter suggestion id'); return; } await req('POST','/feedback/apply',{id},'fbOut','rt-orch'); }
// Docs wiring
let docsBase = null;
function docPath(key){
  switch(key){
    case 'memory': return '/memory_and_training/';
    case 'events': return '/guide/quickstart/';
    case 'tools': return '/api_and_schema/';
    case 'orchestrator': return '/guide/quickstart/';
    case 'deployment': return '/deployment/';
    default: return '/';
  }
}
function positionTip(anchor, el){
  const r = anchor.getBoundingClientRect();
  el.style.left = Math.round(r.left + window.scrollX + 22) + 'px';
  el.style.top  = Math.round(r.top  + window.scrollY - 6) + 'px';
}
function showTip(anchor, text, key){
  const tip = document.createElement('div');
  tip.className = 'tip';
  const more = docsBase ? `<a class="t-more" target="_blank" href="${docsBase}${docPath(key)}">Read more →</a>` : '';
  tip.innerHTML = `<div class="t-title">${key.charAt(0).toUpperCase()+key.slice(1)}</div><div>${text}</div><div style="margin-top:6px">${more}</div>`;
  document.body.appendChild(tip);
  positionTip(anchor, tip);
  const close = () => { tip.remove(); document.removeEventListener('click', onClickAway); window.removeEventListener('resize', onResize); };
  const onClickAway = (e) => { if(!tip.contains(e.target) && e.target !== anchor) close(); };
  const onResize = () => positionTip(anchor, tip);
  setTimeout(()=> document.addEventListener('click', onClickAway), 0);
  window.addEventListener('resize', onResize);
}
async function initDocs(){
  try{
    const r = await fetch(base + '/about');
    const a = await r.json();
    const port = location.port || '8090';
    document.getElementById('port').textContent = port;
    if (a && a.docs_url){
      docsBase = (a.docs_url || '').replace(/\/$/, '');
      const link = document.getElementById('docsLink');
      link.href = docsBase + '/';
      link.style.display = 'inline-block';
      link.title = 'Open docs';
    }
  }catch{}
  // Bind help icons
  document.querySelectorAll('a.help[data-doc]').forEach(a => {
    a.addEventListener('click', (e)=>{
      e.preventDefault();
      const key = a.getAttribute('data-doc');
      const tip = a.getAttribute('data-tip') || '';
      showTip(a, tip, key);
    });
  });
}
document.addEventListener('DOMContentLoaded', initDocs);
</script>
<div id="toast" class="toast"></div>
<script>
function showToast(msg){ const t=document.getElementById('toast'); if(!t) return; t.textContent=msg; t.style.display='block'; clearTimeout(window.__toastTimer); window.__toastTimer = setTimeout(()=>{ t.style.display='none'; }, 2500); }
// Bind copy buttons and insights toggle after DOM ready
document.addEventListener('DOMContentLoaded', ()=>{
  const copyMain=document.getElementById('copyJsonMain'); if(copyMain){ copyMain.addEventListener('click', ()=>{ try{ navigator.clipboard.writeText(document.getElementById('out').textContent||''); }catch{} }); }
  const copyMem=document.getElementById('copyJsonMem'); if(copyMem){ copyMem.addEventListener('click', ()=>{ try{ navigator.clipboard.writeText(document.getElementById('memOut').textContent||''); }catch{} }); }
  const copyTool=document.getElementById('copyJsonTool'); if(copyTool){ copyTool.addEventListener('click', ()=>{ try{ navigator.clipboard.writeText(document.getElementById('toolOut').textContent||''); }catch{} }); }
  const tgl=document.getElementById('toggleInsights'); if (tgl){ tgl.addEventListener('click', ()=>{ const el = document.getElementById('insights'); const on = (el.style.display==='none' || !el.style.display); el.style.display = on ? 'block' : 'none'; if(on){ if(window.__insightsTimer) clearInterval(window.__insightsTimer); const pull=async()=>{ try{ const r=await fetch(base+'/introspect/stats'); const j=await r.json(); routesCache = (j && j.routes && j.routes.by_path) || {}; renderInsights(); }catch{} }; pull(); window.__insightsTimer=setInterval(pull,3000); } else { if(window.__insightsTimer) clearInterval(window.__insightsTimer); } }); }
  const copyStats=document.getElementById('copyStatsBtn'); if(copyStats){ copyStats.addEventListener('click', async ()=>{ try{ const r=await fetch(base + '/introspect/stats'); const t=await r.text(); await navigator.clipboard.writeText(t); showToast('Stats copied'); }catch{ showToast('Stats copy failed'); } }); }
});
</script>
  <div id="insights" style="display:none"></div>
</body>
</html>
"##;

// ---------- Simple stats aggregator ----------
#[derive(Clone, Default, serde::Serialize)]
struct Stats { start: String, total: u64, kinds: HashMap<String, u64> }
static STATS: OnceLock<RwLock<Stats>> = OnceLock::new();
fn stats_cell() -> &'static RwLock<Stats> {
    STATS.get_or_init(|| {
        let start = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
        RwLock::new(Stats{ start, total: 0, kinds: HashMap::new() })
    })
}
pub(crate) async fn stats_on_event(kind: &str) {
    let mut s = stats_cell().write().await;
    s.total += 1;
    *s.kinds.entry(kind.to_string()).or_default() += 1;
}
#[derive(Clone, Default, serde::Serialize)]
struct RouteStat {
    hits: u64,
    errors: u64,
    ewma_ms: f64,
    last_ms: u64,
    max_ms: u64,
    last_status: u16,
    p95_ms: u64,
    #[serde(skip_serializing)]
    sample: VecDeque<u64>,
}
#[derive(Clone, Default, serde::Serialize)]
struct RouteStats { by_path: HashMap<String, RouteStat> }
static ROUTE_STATS: OnceLock<RwLock<RouteStats>> = OnceLock::new();
fn route_stats_cell() -> &'static RwLock<RouteStats> { ROUTE_STATS.get_or_init(|| RwLock::new(RouteStats::default())) }
pub(crate) async fn route_obs(path: &str, status: u16, ms: u64) {
    let mut rs = route_stats_cell().write().await;
    let ent = rs.by_path.entry(path.to_string()).or_default();
    ent.hits += 1;
    if status >= 400 { ent.errors += 1; }
    // EWMA with alpha=0.2
    let a = 0.2f64; let v = ms as f64; ent.ewma_ms = if ent.ewma_ms == 0.0 { v } else { (1.0 - a) * ent.ewma_ms + a * v };
    ent.last_ms = ms; ent.max_ms = ent.max_ms.max(ms); ent.last_status = status;
    // p95 with small sliding sample
    if ent.sample.len() >= 50 { ent.sample.pop_front(); }
    ent.sample.push_back(ms);
    let mut tmp: Vec<u64> = ent.sample.iter().copied().collect();
    if !tmp.is_empty() {
        tmp.sort_unstable();
        let idx = ((tmp.len() as f64) * 0.95).ceil() as usize;
        let idx = idx.saturating_sub(1).min(tmp.len()-1);
        ent.p95_ms = tmp[idx];
    }
}
async fn stats_get() -> impl IntoResponse {
    let events = stats_cell().read().await.clone();
    let routes = route_stats_cell().read().await.clone();
    Json(json!({ "events": events, "routes": routes }))
}
