#![allow(dead_code)]
// ext module: split into submodules (ui, later more)

use arw_macros::{arw_admin, arw_gate};
use axum::{
    extract::State,
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::Digest;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::OnceLock;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::fs as afs;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::RwLock;

use crate::AppState;
use arw_core::gating;
use arw_core::hierarchy as hier;
use arw_core::orchestrator::Task as OrchTask;
use arw_protocol::ProblemDetails;
pub mod budget;
pub mod chat;
pub mod chat_api;
pub mod corr;
pub mod feedback_api;
pub mod feedback_engine;
pub mod feedback_engine_api;
pub mod governor_api;
pub mod hierarchy_api;
pub mod memory;
pub mod memory_api;
pub mod models_api;
pub mod egress_api;
pub mod review_api;
pub mod self_model;
pub mod self_model_api;
pub mod self_model_agg;
pub mod logic_units_api;
pub mod experiments_api;
pub mod patch_api;
pub mod policy;
pub mod state_api;
pub mod stats;
pub mod tools_api;
pub mod tools_exec;
pub mod ui;
pub mod world;
pub mod context_api;
// internal helpers split into modules
pub mod io;
pub mod paths;
pub mod projects;
static ASSET_DEBUG_HTML: &str = include_str!("../../assets/debug.html");
pub(crate) use memory::{
    mem_limit, memory_apply, memory_get, memory_limit_get, memory_limit_set, memory_load,
    memory_save, ApplyMemory, SetLimit,
};

// Internal helper for self-model updates with a closure merge
pub async fn self_model_update_merge<F: FnOnce(&mut serde_json::Value)>(agent: &str, f: F) -> Result<(), String> {
    let mut v = super::ext::self_model::load(agent).await.unwrap_or_else(|| serde_json::json!({}));
    if !v.is_object() { v = serde_json::json!({}); }
    f(&mut v);
    // Touch updated_at
    if let Some(o) = v.as_object_mut() {
        o.insert("updated_at".into(), serde_json::Value::String(chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)));
    }
    super::ext::self_model::save(agent, &v).await
}

// ---------- Standard API envelope helpers ----------
#[derive(Serialize)]
pub struct ApiEnvelope<T> {
    ok: bool,
    data: T,
}
pub(crate) fn ok<T: Serialize>(data: T) -> axum::Json<ApiEnvelope<T>> {
    axum::Json(ApiEnvelope { ok: true, data })
}

// Error helper with ProblemDetails (uniform shape)
pub struct ApiError(pub ProblemDetails);
impl ApiError {
    pub fn new(status: axum::http::StatusCode, title: &str, detail: Option<String>) -> Self {
        ApiError(ProblemDetails {
            r#type: "about:blank".into(),
            title: title.into(),
            status: status.as_u16(),
            detail,
            instance: None,
            trace_id: None,
            code: None,
        })
    }
    pub fn bad_request(msg: &str) -> Self {
        Self::new(
            axum::http::StatusCode::BAD_REQUEST,
            "Bad Request",
            Some(msg.into()),
        )
    }
    pub fn forbidden(msg: &str) -> Self {
        Self::new(
            axum::http::StatusCode::FORBIDDEN,
            "Forbidden",
            Some(msg.into()),
        )
    }
    pub fn not_found(msg: &str) -> Self {
        Self::new(
            axum::http::StatusCode::NOT_FOUND,
            "Not Found",
            Some(msg.into()),
        )
    }
    pub fn internal(msg: &str) -> Self {
        Self::new(
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            "Internal Server Error",
            Some(msg.into()),
        )
    }
}
impl IntoResponse for ApiError {
    fn into_response(self) -> axum::response::Response {
        let status = axum::http::StatusCode::from_u16(self.0.status)
            .unwrap_or(axum::http::StatusCode::INTERNAL_SERVER_ERROR);
        (status, axum::Json(self.0)).into_response()
    }
}

// ---------- persistence bootstrap ----------
#[derive(serde::Deserialize)]
struct OrchFile {
    #[serde(default)]
    profile: Option<String>,
    #[serde(default)]
    hints: Option<Hints>,
    #[serde(default)]
    memory_limit: Option<usize>,
}

pub async fn load_persisted() {
    // orchestration
    if let Some(v) = io::load_json_file(&paths::orch_path()) {
        let of: Result<OrchFile, _> = serde_json::from_value(v);
        if let Ok(of) = of {
            if let Some(p) = of.profile {
                let mut g = governor_profile().write().await;
                *g = p;
            }
            if let Some(h) = of.hints {
                let mut hh = hints().write().await;
                *hh = h;
            }
            if let Some(m) = of.memory_limit {
                let mut ml = mem_limit().write().await;
                *ml = m.max(1);
            }
        }
    }
    // feedback
    if let Some(v) = io::load_json_file(&paths::feedback_path()) {
        if let Ok(fb) = serde_json::from_value::<FeedbackState>(v) {
            let mut st = feedback_cell().write().await;
            *st = fb;
        }
    }
    // world model (best-effort)
    world::load_persisted().await;

    // Seed a default self‑model if none exist yet (best‑effort)
    {
        use tokio::fs as afs;
        let dir = paths::self_dir();
        let _ = afs::create_dir_all(&dir).await;
        let mut has_any = false;
        if let Ok(mut rd) = afs::read_dir(&dir).await {
            while let Ok(Some(ent)) = rd.next_entry().await {
                if let Some(name) = ent.file_name().to_str() {
                    if name.ends_with('.' .to_string().as_str()) { /* improbable */ }
                    if name.ends_with(".json") { has_any = true; break; }
                }
            }
        }
        if !has_any {
            let agent = std::env::var("ARW_SELF_SEED_ID").ok().filter(|s| !s.trim().is_empty()).unwrap_or_else(|| "dev-assistant".to_string());
            let family = default_model().read().await.clone();
            let now = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
            let seed = json!({
                "version": "0",
                "updated_at": now,
                "identity": {
                    "model_family": family,
                    "model_hash": null,
                    "logic_units": ["metacognition","abstain-gate","resource-forecaster"],
                    "policies": ["default"],
                    "leases": []
                },
                "capability_map": { "tools": ["introspect.tools","memory.probe"], "modalities": ["text"] },
                "competence_map": {},
                "calibration": { "ece": 0.15, "brier": 0.25, "bias": "ok" },
                "resource_curve": { "recipes": { "chat": { "tokens_mean": 800, "latency_ms_mean": 2000 } } },
                "failure_modes": [ { "name": "ocr-tables", "hint": "route to structured-OCR" } ],
                "interaction_contract": { "style": "concise", "constraints": ["cite sources", "never write fs without lease"] }
            });
            let _ = self_model::save(&agent, &seed).await;
        }
    }

    // Migrate legacy model files into CAS layout (best-effort, non-blocking)
    tokio::spawn(async move {
        crate::resources::models_service::ModelsService::migrate_legacy_to_cas().await;
    });
}

pub(crate) async fn persist_orch() {
    let profile = { governor_profile().read().await.clone() };
    let hints = { hints().read().await.clone() };
    let ml = { *mem_limit().read().await };
    let v = json!({"profile": profile, "hints": hints, "memory_limit": ml});
    let _ = io::save_json_file_async(&paths::orch_path(), &v).await;
}
async fn persist_feedback() {
    let st = { feedback_cell().read().await.clone() };
    let v = serde_json::to_value(st).unwrap_or_else(|_| json!({}));
    let _ = io::save_json_file_async(&paths::feedback_path(), &v).await;
}

// audit_event provided by ext::io

// ---------- Global stores ----------
fn default_memory() -> Value {
    json!({
        "ephemeral":  [],
        "episodic":   [],
        "semantic":   [],
        "procedural": []
    })
}
// moved to memory module

pub(crate) fn default_models() -> Vec<Value> {
    vec![
        json!({"id":"llama-3.1-8b-instruct","provider":"local","status":"available"}),
        json!({"id":"qwen2.5-coder-7b","provider":"local","status":"available"}),
    ]
}
static MODELS: OnceLock<RwLock<Vec<Value>>> = OnceLock::new();
pub(crate) fn models() -> &'static RwLock<Vec<Value>> {
    MODELS.get_or_init(|| {
        let initial = io::load_json_file(&paths::models_path())
            .and_then(|v| v.as_array().cloned())
            .unwrap_or_else(default_models);
        RwLock::new(initial)
    })
}

static DEFAULT_MODEL: OnceLock<RwLock<String>> = OnceLock::new();
pub(crate) fn default_model() -> &'static RwLock<String> {
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
pub(crate) fn governor_profile() -> &'static RwLock<String> {
    GOV_PROFILE.get_or_init(|| {
        let initial = std::env::var("ARW_PROFILE").unwrap_or_else(|_| "balanced".to_string());
        RwLock::new(initial)
    })
}

// governor hints defined below alongside routes

// ---------- memory ring-buffer limit ----------
// moved to memory module

// ---------- Tool runner (minimal builtins) ----------
fn run_tool_internal(id: &str, input: &Value) -> Result<Value, String> {
    tools_exec::run(id, input)
}

// ---------- Public: mountable routes ----------
pub fn extra_routes() -> Router<AppState> {
    let mut r = Router::new()
        // hierarchy (local view + role change)
        .route("/hierarchy/state", get(hierarchy_state))
        .route("/hierarchy/role", post(hierarchy_role_set))
        // governor
        .route("/governor/profile", get(governor_api::governor_get))
        .route("/governor/profile", post(governor_api::governor_set))
        .route("/governor/hints", get(governor_api::governor_hints_get))
        .route("/governor/hints", post(governor_api::governor_hints_set))
        // feedback (self-learning)
        .route("/feedback/state", get(feedback_api::feedback_state_get))
        .route("/feedback/signal", post(feedback_api::feedback_signal_post))
        .route(
            "/feedback/analyze",
            post(feedback_api::feedback_analyze_post),
        )
        .route("/feedback/apply", post(feedback_api::feedback_apply_post))
        .route("/feedback/auto", post(feedback_api::feedback_auto_post))
        .route("/feedback/reset", post(feedback_api::feedback_reset_post))
        // feedback engine (near-live suggestions)
        .route(
            "/feedback/suggestions",
            get(feedback_engine_api::feedback_suggestions),
        )
        .route(
            "/feedback/updates",
            get(feedback_engine_api::feedback_updates),
        )
        .route(
            "/feedback/policy",
            get(feedback_engine_api::feedback_policy_get),
        )
        .route(
            "/feedback/versions",
            get(feedback_engine_api::feedback_versions),
        )
        .route(
            "/feedback/rollback",
            post(feedback_engine_api::feedback_rollback),
        )
        // stats
        .route("/introspect/stats", get(stats::stats_get))
        // memory
        .route("/memory", get(memory_api::memory_get))
        .route("/memory/apply", post(memory_api::memory_apply))
        .route("/memory/save", post(memory_api::memory_save))
        .route("/memory/load", post(memory_api::memory_load))
        .route("/memory/limit", get(memory_api::memory_limit_get))
        .route("/memory/limit", post(memory_api::memory_limit_set))
        // models
        .route("/models", get(models_api::list_models))
        .route("/models/refresh", post(models_api::refresh_models))
        .route("/models/save", post(models_api::models_save))
        .route("/models/load", post(models_api::models_load))
        .route("/models/add", post(models_api::models_add))
        .route("/models/delete", post(models_api::models_delete))
        .route("/models/default", get(models_api::models_default_get))
        .route("/models/default", post(models_api::models_default_set))
        .route("/models/download", post(models_api::models_download))
        .route(
            "/models/download/cancel",
            post(models_api::models_download_cancel),
        )
        .route("/models/cas_gc", post(models_api::models_cas_gc))
        // tools
        .route("/tools", get(tools_api::list_tools))
        .route("/tools/run", post(tools_api::run_tool_endpoint))
        // context assembly
        .route("/context/assemble", get(context_api::assemble_get))
        .route("/context/rehydrate", post(context_api::rehydrate_post))
        // logic units & patch engine (MVP stubs)
        .route("/logic-units/install", post(logic_units_api::install))
        .route("/logic-units/apply", post(logic_units_api::apply))
        .route("/logic-units/revert", post(logic_units_api::revert))
        .route("/patch/dry-run", post(patch_api::dry_run))
        .route("/patch/apply", post(patch_api::apply))
        .route("/patch/revert", post(patch_api::revert))
        // experiments (MVP stubs)
        .route("/experiments/start", post(experiments_api::start))
        .route("/experiments/stop", post(experiments_api::stop))
        .route("/experiments/assign", post(experiments_api::assign))
        // hierarchy negotiation (HTTP scaffolding)
        .route("/hierarchy/hello", post(hierarchy_api::hello))
        .route("/hierarchy/offer", post(hierarchy_api::offer))
        .route("/hierarchy/accept", post(hierarchy_api::accept))
        // orchestration MVP
        .route("/tasks/enqueue", post(tasks_enqueue))
        // chat
        .route("/chat", get(chat_api::chat_get))
        .route("/chat/send", post(chat_api::chat_send))
        .route("/chat/clear", post(chat_api::chat_clear))
        .route("/chat/status", get(chat::chat_status))
        // state (read-models)
        .route("/state/observations", get(state_api::observations_get))
        .route("/state/beliefs", get(state_api::beliefs_get))
        .route("/state/world", get(world::world_get))
        .route("/state/world/select", get(world::world_select_get))
        .route("/state/intents", get(state_api::intents_get))
        .route("/state/actions", get(state_api::actions_get))
        .route("/state/episodes", get(state_api::episodes_get))
        .route("/state/logic_units", get(state_api::logic_units_get))
        .route("/state/experiments", get(state_api::experiments_get))
        .route("/state/runtime_matrix", get(state_api::runtime_matrix_get))
        .route("/state/models_hashes", get(models_api::models_hashes_get))
        .route("/state/egress/ledger", get(egress_api::egress_ledger_get))
        .route(
            "/state/memory/quarantine",
            get(review_api::memory_quarantine_get),
        )
        .route("/state/world_diffs", get(review_api::world_diffs_get))
        // admin: quarantine review operations
        .route(
            "/admin/memory/quarantine",
            post(review_api::memory_quarantine_add),
        )
        .route(
            "/admin/memory/quarantine/admit",
            post(review_api::memory_quarantine_admit),
        )
        .route(
            "/admin/world_diffs/queue",
            post(review_api::world_diffs_queue),
        )
        .route(
            "/admin/world_diffs/decision",
            post(review_api::world_diffs_decision),
        )
        .route(
            "/state/episode/:id/snapshot",
            get(|State(state), axum::extract::Path(id)| async move {
                state_api::episode_snapshot_get(State(state), axum::extract::Path(id)).await
            }),
        )
        .route("/state/policy", get(state_api::policy_state_get))
        .route("/projects/list", get(projects::projects_list))
        .route("/projects/create", post(projects::projects_create))
        .route("/projects/tree", get(projects::projects_tree))
        .route("/projects/notes", get(projects::projects_notes_get))
        .route("/projects/notes", post(projects::projects_notes_set))
        // project file ops (safe, gated)
        .route("/projects/file", get(projects::projects_file_get))
        .route("/projects/file", post(projects::projects_file_set))
        .route("/projects/patch", post(projects::projects_file_patch));

    // debug UI gated via ARW_DEBUG=1
    if std::env::var("ARW_DEBUG").ok().as_deref() == Some("1") {
        r = r
            .route("/debug", get(ui::debug_ui))
            .route("/ui/models", get(ui::models_ui))
            .route("/ui/agents", get(ui::agents_ui))
            .route("/ui/projects", get(ui::projects_ui));
    }
    r
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tools_math_add() {
        let out = run_tool_internal("math.add", &json!({"a": 1.0, "b": 2.0})).unwrap();
        assert_eq!(out.get("sum").and_then(|v| v.as_f64()).unwrap(), 3.0);
    }
}

// ---------- Handlers ----------
pub async fn version() -> impl IntoResponse {
    ok(json!({
        "service": env!("CARGO_PKG_NAME"),
        "version": env!("CARGO_PKG_VERSION"),
    }))
}

#[derive(Deserialize, utoipa::ToSchema)]
pub struct EnqueueReq {
    kind: String,
    payload: Value,
    #[serde(default)]
    shard_key: Option<String>,
    #[serde(default)]
    priority: Option<i32>,
    #[serde(default)]
    idem_key: Option<String>,
}

#[arw_admin(
    method = "POST",
    path = "/admin/tasks/enqueue",
    summary = "Enqueue orchestrator task"
)]
#[arw_gate("queue:enqueue")]
async fn tasks_enqueue(
    State(state): State<AppState>,
    Json(req): Json<EnqueueReq>,
) -> impl IntoResponse {
    // Gate by task kind (in addition to macro-level queue:enqueue gate)
    if !gating::allowed(&format!("task:{}", req.kind)) {
        return ApiError::forbidden("gated by policy").into_response();
    }
    let mut t = OrchTask::new(req.kind, req.payload);
    t.shard_key = req.shard_key;
    t.priority = req.priority.unwrap_or(0);
    t.idem_key = req.idem_key;
    match state.queue.enqueue(t).await {
        Ok(id) => ok(json!({"id": id})).into_response(),
        Err(e) => ApiError::internal(&e.to_string()).into_response(),
    }
}

/// Spawn a simple local task worker that dequeues tasks and runs built-in tools.
pub fn start_local_task_worker(state: AppState) {
    let bus = state.bus.clone();
    let q = state.queue.clone();
    tokio::spawn(async move {
        loop {
            match q.dequeue("workers").await {
                Ok((t, lease)) => {
                    let t0 = std::time::Instant::now();
                    // Ingress gating for task execution
                    let ingress_key = format!("io:ingress:task.{}", t.kind);
                    if !arw_core::gating::allowed(&ingress_key) {
                        let _ = q.ack(lease).await;
                        if arw_core::gating::allowed("events:Task.Completed") {
                            bus.publish(
                                "Task.Completed",
                                &json!({"id": t.id, "ok": false, "error": "gated:ingress"}),
                            );
                        }
                        continue;
                    }
                    let (ok, out, err) = match run_tool_internal(&t.kind, &t.payload) {
                        Ok(v) => (true, v, None),
                        Err(e) => (false, json!({}), Some(e)),
                    };
                    let _ = q.ack(lease).await;
                    let dt = t0.elapsed().as_millis() as u64;
                    // Egress gating for task output (policy-level)
                    let egress_key = format!("io:egress:task.{}", t.kind);
                    if gating::allowed("events:Task.Completed")
                        && arw_core::gating::allowed(&egress_key)
                    {
                        let mut payload = json!({"id": t.id, "ok": ok, "latency_ms": dt, "error": err, "output": out});
                        crate::ext::corr::ensure_corr(&mut payload);
                        bus.publish("Task.Completed", &payload);
                    } else if gating::allowed("events:Task.Completed") {
                        let mut payload = json!({"id": t.id, "ok": false, "latency_ms": dt, "error": "gated:egress"});
                        crate::ext::corr::ensure_corr(&mut payload);
                        bus.publish("Task.Completed", &payload);
                    }
                }
                Err(_e) => {
                    // backoff a bit on unexpected errors
                    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                }
            }
        }
    });
}

pub async fn about() -> impl IntoResponse {
    Json(json!({
        "service": "arw-svc",
        "version": env!("CARGO_PKG_VERSION"),
        "role": format!("{:?}", hier::get_state().self_node.role),
        "docs_url": std::env::var("ARW_DOCS_URL").ok(),
        "endpoints": [
          "/spec/openapi.yaml",
          "/spec/asyncapi.yaml",
          "/spec/mcp-tools.json",
          "/healthz",
          "/version",
          "/about",
          // admin endpoints:
          "/admin/events",
          "/admin/probe",
          "/admin/introspect/stats",
          "/admin/introspect/tools",
          "/admin/introspect/schemas/:id",
          "/admin/hierarchy/state",
          "/admin/hierarchy/role",
          "/admin/governor/profile",
          "/admin/governor/hints",
          "/admin/memory",
          "/admin/memory/apply",
          "/admin/memory/save",
          "/admin/memory/load",
          "/admin/memory/limit",
          "/admin/models",
          "/admin/models/refresh",
          "/admin/models/save",
          "/admin/models/load",
          "/admin/models/add",
          "/admin/models/delete",
          "/admin/models/default",
          "/admin/tools",
          "/admin/tools/run",
          "/admin/chat",
          "/admin/chat/send",
          "/admin/chat/clear",
          "/admin/experiments/start",
          "/admin/experiments/stop",
          "/admin/experiments/assign",
          "/admin/debug"
        ]
    }))
}

#[arw_admin(
    method = "GET",
    path = "/admin/hierarchy/state",
    summary = "Get hierarchy state"
)]
#[arw_macros::arw_gate("hierarchy:state:get")]
async fn hierarchy_state(State(state): State<AppState>) -> impl IntoResponse {
    if let Some(svc) = state
        .resources
        .get::<crate::resources::hierarchy_service::HierarchyService>()
    {
        let st: arw_core::hierarchy::HierarchyState = svc.state_event(&state).await;
        return Json::<arw_core::hierarchy::HierarchyState>(st).into_response();
    }
    let st = hier::get_state();
    let mut p = serde_json::json!({"epoch": st.epoch});
    crate::ext::corr::ensure_corr(&mut p);
    state.bus.publish("Hierarchy.State", &p);
    Json(st).into_response()
}

#[derive(Deserialize, utoipa::ToSchema)]
struct RoleSet {
    role: String,
}
#[arw_admin(
    method = "POST",
    path = "/admin/hierarchy/role",
    summary = "Set hierarchy role"
)]
#[arw_gate("hierarchy:role:set")]
async fn hierarchy_role_set(
    State(state): State<AppState>,
    Json(req): Json<RoleSet>,
) -> impl IntoResponse {
    if let Some(svc) = state
        .resources
        .get::<crate::resources::hierarchy_service::HierarchyService>()
    {
        svc.role_set(&state, &req.role).await;
        return ok(json!({})).into_response();
    }
    let role = match req.role.as_str() {
        "root" => hier::Role::Root,
        "regional" => hier::Role::Regional,
        "edge" => hier::Role::Edge,
        "connector" => hier::Role::Connector,
        _ => hier::Role::Observer,
    };
    hier::set_role(role);
    let gate_role = match role {
        hier::Role::Root => arw_core::gating::Role::Root,
        hier::Role::Regional => arw_core::gating::Role::Regional,
        hier::Role::Edge => arw_core::gating::Role::Edge,
        hier::Role::Connector => arw_core::gating::Role::Connector,
        hier::Role::Observer => arw_core::gating::Role::Observer,
    };
    arw_core::gating::apply_role_defaults(gate_role);
    let mut p = serde_json::json!({"role": req.role});
    crate::ext::corr::ensure_corr(&mut p);
    state.bus.publish("Hierarchy.RoleChanged", &p);
    ok(json!({})).into_response()
}

// moved to memory module

// moved to memory module

// Governor profile
#[derive(Deserialize)]
struct SetProfile {
    name: String,
}
async fn governor_get() -> impl IntoResponse {
    let p = { governor_profile().read().await.clone() };
    ok(json!({ "profile": p }))
}
async fn governor_set(
    State(state): State<AppState>,
    Json(req): Json<SetProfile>,
) -> impl IntoResponse {
    {
        let mut g = governor_profile().write().await;
        *g = req.name.clone();
    }
    state
        .bus
        .publish("Governor.Changed", &json!({"profile": req.name.clone()}));
    persist_orch().await;
    ok(json!({}))
}

#[derive(Deserialize, serde::Serialize, Clone)]
pub(crate) struct Hints {
    #[serde(default)]
    pub(crate) max_concurrency: Option<usize>,
    #[serde(default)]
    pub(crate) event_buffer: Option<usize>,
    #[serde(default)]
    pub(crate) http_timeout_secs: Option<u64>,
    #[serde(default)]
    pub(crate) mode: Option<String>,
    #[serde(default)]
    pub(crate) slo_ms: Option<u64>,
}
static HINTS: OnceLock<RwLock<Hints>> = OnceLock::new();
pub(crate) fn hints() -> &'static RwLock<Hints> {
    HINTS.get_or_init(|| {
        RwLock::new(Hints {
            max_concurrency: None,
            event_buffer: None,
            http_timeout_secs: None,
            mode: None,
            slo_ms: None,
        })
    })
}
async fn governor_hints_get() -> impl IntoResponse {
    let h = hints().read().await.clone();
    ok(serde_json::to_value(h).unwrap_or(json!({})))
}
async fn governor_hints_set(
    State(state): State<AppState>,
    Json(req): Json<Hints>,
) -> impl IntoResponse {
    {
        let mut h = hints().write().await;
        if req.max_concurrency.is_some() {
            h.max_concurrency = req.max_concurrency;
        }
        if req.event_buffer.is_some() {
            h.event_buffer = req.event_buffer;
        }
        if req.http_timeout_secs.is_some() {
            h.http_timeout_secs = req.http_timeout_secs;
        }
        if req.mode.is_some() { h.mode = req.mode.clone(); }
        if req.slo_ms.is_some() { h.slo_ms = req.slo_ms; }
    }
    // Apply dynamic HTTP timeout immediately if provided
    let mut applied_timeout: Option<u64> = None;
    if let Some(secs) = req.http_timeout_secs {
        crate::dyn_timeout::set_global_timeout_secs(secs);
        applied_timeout = Some(secs);
    } else if let Some(ms) = req.slo_ms {
        // Derive a sane HTTP timeout from SLO (round up to nearest second, min 1s)
        let secs = ((ms + 999) / 1000).max(1);
        crate::dyn_timeout::set_global_timeout_secs(secs);
        applied_timeout = Some(secs);
    }
    if let Some(secs) = applied_timeout {
        let mut payload = json!({"action":"hint","params":{"http_timeout_secs": secs, "source": "slo|mode"},"ok": true});
        let _cid = crate::ext::corr::ensure_corr(&mut payload);
        state.bus.publish("Actions.HintApplied", &payload);
    }
    persist_orch().await;
    ok(json!({})).into_response()
}

// moved to memory module

async fn list_models() -> impl IntoResponse {
    let v = models().read().await.clone();
    ok::<Vec<Value>>(v)
}
async fn refresh_models(State(state): State<AppState>) -> impl IntoResponse {
    let new = default_models();
    {
        let mut m = models().write().await;
        *m = new.clone();
    }
    let _ = io::save_json_file_async(&paths::models_path(), &Value::Array(new.clone())).await;
    state
        .bus
        .publish("Models.Refreshed", &json!({"count": new.len()}));
    ok::<Vec<Value>>(new)
}
async fn models_save() -> impl IntoResponse {
    let v = models().read().await.clone();
    match io::save_json_file_async(&paths::models_path(), &Value::Array(v)).await {
        Ok(_) => ok(json!({})).into_response(),
        Err(e) => ApiError::internal(&e.to_string()).into_response(),
    }
}
async fn models_load() -> impl IntoResponse {
    match io::load_json_file_async(&paths::models_path())
        .await
        .and_then(|v| v.as_array().cloned())
    {
        Some(arr) => {
            {
                let mut m = models().write().await;
                *m = arr.clone();
            }
            ok::<Vec<Value>>(arr).into_response()
        }
        None => ApiError::not_found("no models.json").into_response(),
    }
}

#[derive(Deserialize)]
pub(crate) struct ModelId {
    id: String,
    #[serde(default)]
    provider: Option<String>,
}
async fn models_add(State(state): State<AppState>, Json(req): Json<ModelId>) -> impl IntoResponse {
    if let Some(svc) = state
        .resources
        .get::<crate::resources::models_service::ModelsService>()
    {
        svc.add(&state, req.id, req.provider).await;
        return ok(json!({})).into_response();
    }
    let mut v = models().write().await;
    if !v
        .iter()
        .any(|m| m.get("id").and_then(|s| s.as_str()) == Some(&req.id))
    {
        v.push(json!({"id": req.id, "provider": req.provider.unwrap_or_else(|| "local".to_string()), "status":"available"}));
        state.bus.publish(
            "Models.Changed",
            &json!({"op":"add","id": v.last().and_then(|m| m.get("id")).cloned()}),
        );
    }
    io::audit_event("models.add", &json!({"id": req.id})).await;
    ok(json!({})).into_response()
}
async fn models_delete(
    State(state): State<AppState>,
    Json(req): Json<ModelId>,
) -> impl IntoResponse {
    if let Some(svc) = state
        .resources
        .get::<crate::resources::models_service::ModelsService>()
    {
        svc.delete(&state, req.id).await;
        return ok(json!({})).into_response();
    }
    let mut v = models().write().await;
    let before = v.len();
    v.retain(|m| m.get("id").and_then(|s| s.as_str()) != Some(&req.id));
    if v.len() != before {
        state
            .bus
            .publish("Models.Changed", &json!({"op":"delete","id": req.id}));
    }
    io::audit_event("models.delete", &json!({"id": req.id})).await;
    ok(json!({})).into_response()
}
async fn models_default_get() -> impl IntoResponse {
    // No state here; fall back to ext default
    let id = { default_model().read().await.clone() };
    ok(json!({"default": id }))
}
async fn models_default_set(
    State(state): State<AppState>,
    Json(req): Json<ModelId>,
) -> impl IntoResponse {
    if let Some(svc) = state
        .resources
        .get::<crate::resources::models_service::ModelsService>()
    {
        let _ = svc.default_set(&state, req.id).await;
        return ok(json!({})).into_response();
    }
    {
        let mut d = default_model().write().await;
        *d = req.id.clone();
    }
    state
        .bus
        .publish("Models.Changed", &json!({"op":"default","id": req.id}));
    let _ = io::save_json_file_async(
        &paths::models_path(),
        &Value::Array(models().read().await.clone()),
    )
    .await;
    persist_orch().await;
    io::audit_event("models.default", &json!({"id": req.id})).await;
    ok(json!({})).into_response()
}

#[derive(Deserialize)]
pub(crate) struct DownloadReq {
    pub(crate) id: String,
    pub(crate) url: String,
    #[serde(default)]
    pub(crate) provider: Option<String>,
    #[serde(default)]
    pub(crate) sha256: Option<String>,
}
async fn models_download(
    State(state): State<AppState>,
    Json(req): Json<DownloadReq>,
) -> impl IntoResponse {
    // Delegate to ModelsService when available (consolidated path)
    if let Some(svc) = state
        .resources
        .get::<crate::resources::models_service::ModelsService>()
    {
        match svc
            .download(&state, req.id, req.url, req.provider, req.sha256)
            .await
        {
            Ok(()) => return ok(json!({})).into_response(),
            Err(e) => return ApiError::bad_request(&e).into_response(),
        }
    }
    // ensure model exists with status
    let mut already_in_progress = false;
    {
        let mut v = models().write().await;
        if let Some(m) = v
            .iter_mut()
            .find(|m| m.get("id").and_then(|s| s.as_str()) == Some(&req.id))
        {
            let prev = m.get("status").and_then(|s| s.as_str()).unwrap_or("");
            if prev.eq_ignore_ascii_case("downloading") {
                already_in_progress = true;
            } else {
                *m = json!({"id": req.id, "provider": req.provider.clone().unwrap_or("local".into()), "status":"downloading"});
            }
        } else {
            v.push(json!({"id": req.id, "provider": req.provider.clone().unwrap_or("local".into()), "status":"downloading"}));
        }
    }
    if already_in_progress {
        let mut p = json!({"id": req.id, "status":"already-in-progress"});
        crate::ext::corr::ensure_corr(&mut p);
        state.bus.publish("Models.DownloadProgress", &p);
        return ok(json!({})).into_response();
    }
    // Validate URL scheme (accept http/https only)
    if !(req.url.starts_with("http://") || req.url.starts_with("https://")) {
        return ApiError::bad_request("invalid url scheme").into_response();
    }
    {
        let mut p = json!({"id": req.id, "url": req.url});
        crate::ext::corr::ensure_corr(&mut p);
        state.bus.publish("Models.Download", &p);
    }
    io::audit_event("models.download", &json!({"id": req.id})).await;
    let id = req.id.clone();
    let url = req.url.clone();
    let provider = req.provider.clone().unwrap_or("local".into());
    let expect_sha = req.sha256.clone().map(|s| s.to_lowercase());
    let sp = state.clone();
    tokio::spawn(async move {
        // sanitize filename and compute target paths
        let file_name = url.rsplit('/').next().unwrap_or(&id).to_string();
        let safe_name = file_name.replace(['\\', '/'], "_");
        let target_dir = paths::state_dir().join("models");
        let target = target_dir.join(&safe_name);
        let tmp = target.with_extension("part");
        if let Err(e) = afs::create_dir_all(&target_dir).await {
            let mut p = json!({"id": id, "error": format!("mkdir failed: {}", e)});
            crate::ext::corr::ensure_corr(&mut p);
            sp.bus.publish("Models.DownloadProgress", &p);
            return;
        }
        let client = reqwest::Client::builder()
            .connect_timeout(std::time::Duration::from_secs(10))
            .timeout(std::time::Duration::from_secs(300))
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());
        // Try resuming if a partial exists
        let mut resume_from: u64 = 0;
        if let Ok(meta) = afs::metadata(&tmp).await {
            resume_from = meta.len();
        }
        let mut reqb = client.get(&url);
        if resume_from > 0 {
            reqb = reqb.header(reqwest::header::RANGE, format!("bytes={}-", resume_from));
        }
        match reqb.send().await {
            Ok(resp) => {
                let total_rem = resp.content_length().unwrap_or(0);
                let status = resp.status();
                // If server honored Range, recalc overall size
                let total_all =
                    if resume_from > 0 && status == axum::http::StatusCode::PARTIAL_CONTENT {
                        let mut p = json!({"id": id, "status":"resumed", "offset": resume_from});
                        crate::ext::corr::ensure_corr(&mut p);
                        sp.bus.publish("Models.DownloadProgress", &p);
                        resume_from + total_rem
                    } else {
                        if resume_from > 0 {
                            // Server ignored Range; restart from scratch
                            resume_from = 0;
                            let _ = afs::remove_file(&tmp).await;
                        }
                        total_rem
                    };
                // Open partial (append) or fresh file
                let mut file = if resume_from > 0 {
                    match afs::OpenOptions::new().append(true).open(&tmp).await {
                        Ok(f) => f,
                        Err(e) => {
                            let mut p = json!({"id": id, "error": format!("open failed: {}", e)});
                            crate::ext::corr::ensure_corr(&mut p);
                            sp.bus.publish("Models.DownloadProgress", &p);
                            return;
                        }
                    }
                } else {
                    match afs::File::create(&tmp).await {
                        Ok(f) => f,
                        Err(e) => {
                            let mut p = json!({"id": id, "error": format!("create failed: {}", e)});
                            crate::ext::corr::ensure_corr(&mut p);
                            sp.bus.publish("Models.DownloadProgress", &p);
                            return;
                        }
                    }
                };
                // Stream body
                let mut downloaded: u64 = 0;
                let mut stream = resp.bytes_stream();
                while let Some(chunk) = stream.next().await {
                    match chunk {
                        Ok(bytes) => {
                            if is_canceled(&id).await {
                                let _ = afs::remove_file(&tmp).await;
                                let mut p = json!({"id": id, "status":"canceled"});
                                crate::ext::corr::ensure_corr(&mut p);
                                sp.bus.publish("Models.DownloadProgress", &p);
                                clear_cancel(&id).await;
                                return;
                            }
                            if let Err(e) = file.write_all(&bytes).await {
                                let mut p =
                                    json!({"id": id, "error": format!("write failed: {}", e)});
                                crate::ext::corr::ensure_corr(&mut p);
                                sp.bus.publish("Models.DownloadProgress", &p);
                                return;
                            }
                            downloaded += bytes.len() as u64;
                            if total_all > 0 {
                                let pct = (((resume_from + downloaded) * 100) / total_all).min(100);
                                let mut p = json!({"id": id, "progress": pct, "downloaded": resume_from + downloaded, "total": total_all});
                                crate::ext::corr::ensure_corr(&mut p);
                                sp.bus.publish("Models.DownloadProgress", &p);
                            }
                        }
                        Err(e) => {
                            let mut p = json!({"id": id, "error": format!("read failed: {}", e)});
                            crate::ext::corr::ensure_corr(&mut p);
                            sp.bus.publish("Models.DownloadProgress", &p);
                            return;
                        }
                    }
                }
                // flush and rename atomically into place
                if let Err(e) = file.flush().await {
                    let mut p = json!({"id": id, "error": format!("flush failed: {}", e)});
                    crate::ext::corr::ensure_corr(&mut p);
                    sp.bus.publish("Models.DownloadProgress", &p);
                    return;
                }
                if let Err(e) = afs::rename(&tmp, &target).await {
                    let mut p = json!({"id": id, "error": format!("finalize failed: {}", e)});
                    crate::ext::corr::ensure_corr(&mut p);
                    sp.bus.publish("Models.DownloadProgress", &p);
                    return;
                }
                // checksum verification (required when provided by UI; service path requires it)
                if let Some(exp) = expect_sha {
                    // Compute full-file hash to support resume
                    let mut f = match afs::File::open(&target).await {
                        Ok(f) => f,
                        Err(_) => {
                            return;
                        }
                    };
                    let mut h = sha2::Sha256::new();
                    let mut buf = vec![0u8; 1024 * 1024];
                    loop {
                        match f.read(&mut buf).await {
                            Ok(0) => break,
                            Ok(n) => {
                                use sha2::Digest;
                                h.update(&buf[..n]);
                            }
                            Err(_) => break,
                        }
                    }
                    let actual = format!("{:x}", h.finalize());
                    if actual != exp {
                        let _ = afs::remove_file(&target).await;
                        let mut p = json!({"id": id, "error": "checksum mismatch", "expected": exp, "actual": actual});
                        crate::ext::corr::ensure_corr(&mut p);
                        sp.bus.publish("Models.DownloadProgress", &p);
                        return;
                    }
                }
                let mut p =
                    json!({"id": id, "status":"complete", "file": safe_name, "provider": provider});
                crate::ext::corr::ensure_corr(&mut p);
                sp.bus.publish("Models.DownloadProgress", &p);
                {
                    let mut v = models().write().await;
                    if let Some(m) = v
                        .iter_mut()
                        .find(|m| m.get("id").and_then(|s| s.as_str()) == Some(&id))
                    {
                        *m = json!({"id": id, "provider": provider, "status":"available", "path": target.to_string_lossy()});
                    }
                }
                let _ = io::save_json_file_async(
                    &paths::models_path(),
                    &Value::Array(models().read().await.clone()),
                )
                .await;
                sp.bus
                    .publish("Models.Changed", &json!({"op":"downloaded","id": id}));
            }
            Err(e) => {
                let mut p = json!({"id": id, "error": format!("request failed: {}", e)});
                crate::ext::corr::ensure_corr(&mut p);
                sp.bus.publish("Models.DownloadProgress", &p);
            }
        }
    });
    ok(json!({})).into_response()
}

// ----- download cancel helpers -----
use std::collections::HashSet;
static DL_CANCEL: OnceLock<RwLock<HashSet<String>>> = OnceLock::new();
fn cancel_cell() -> &'static RwLock<HashSet<String>> {
    DL_CANCEL.get_or_init(|| RwLock::new(HashSet::new()))
}
async fn is_canceled(id: &str) -> bool {
    cancel_cell().read().await.contains(id)
}
async fn clear_cancel(id: &str) {
    cancel_cell().write().await.remove(id);
}
async fn set_cancel(id: &str) {
    cancel_cell().write().await.insert(id.to_string());
}

#[derive(Deserialize)]
pub(crate) struct CancelReq {
    id: String,
}
async fn models_download_cancel(
    State(state): State<AppState>,
    Json(req): Json<CancelReq>,
) -> impl IntoResponse {
    // Delegate to ModelsService when available (consolidated path)
    if let Some(svc) = state
        .resources
        .get::<crate::resources::models_service::ModelsService>()
    {
        svc.cancel_download(&state, req.id).await;
        return ok(json!({})).into_response();
    }
    set_cancel(&req.id).await;
    state.bus.publish(
        "Models.DownloadProgress",
        &json!({"id": req.id, "status":"cancel-requested"}),
    );
    ok(json!({})).into_response()
}

// ---- Tools ----
async fn list_tools() -> impl IntoResponse {
    let out: Vec<Value> = tools_exec::list()
        .into_iter()
        .map(|(id, summary)| json!({"id": id, "summary": summary}))
        .collect();
    ok(out)
}
#[derive(Deserialize)]
pub(crate) struct ToolRunReq {
    id: String,
    input: Value,
}
async fn run_tool_endpoint(
    State(state): State<AppState>,
    Json(req): Json<ToolRunReq>,
) -> impl IntoResponse {
    match run_tool_internal(&req.id, &req.input) {
        Ok(out) => {
            let mut payload = json!({"id": req.id, "output": out});
            crate::ext::corr::ensure_corr(&mut payload);
            state.bus.publish("Tool.Ran", &payload);
            Json(payload.get("output").cloned().unwrap_or_else(|| json!({}))).into_response()
        }
        Err(e) => ApiError::bad_request(&e).into_response(),
    }
}

// ---- Chat ----
static CHAT_LOG: OnceLock<RwLock<Vec<Value>>> = OnceLock::new();
fn chat_log() -> &'static RwLock<Vec<Value>> {
    CHAT_LOG.get_or_init(|| RwLock::new(Vec::new()))
}

#[derive(Deserialize)]
pub(crate) struct ChatSendReq {
    message: String,
    #[serde(default)]
    model: Option<String>,
    #[serde(default)]
    temperature: Option<f64>,
}

fn synth_reply(msg: &str, model: &str) -> String {
    match model.to_ascii_lowercase().as_str() {
        "reverse" => msg.chars().rev().collect(),
        "time" => {
            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .ok()
                .map(|d| d.as_secs())
                .unwrap_or(0);
            format!("[{}] {}", now, msg)
        }
        _ => format!("You said: {}", msg),
    }
}

// ---- Self-learning / Feedback Layer ----
#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub(crate) struct FeedbackSignal {
    id: String,
    ts: String,
    kind: String,
    target: String,
    confidence: f64,
    severity: u8,
    note: Option<String>,
}
#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub(crate) struct Suggestion {
    id: String,
    action: String,
    params: Value,
    rationale: String,
    confidence: f64,
}
#[derive(Clone, Default, serde::Serialize, serde::Deserialize)]
pub(crate) struct FeedbackState {
    auto_apply: bool,
    signals: Vec<FeedbackSignal>,
    suggestions: Vec<Suggestion>,
}
static FEEDBACK: OnceLock<RwLock<FeedbackState>> = OnceLock::new();
fn feedback_cell() -> &'static RwLock<FeedbackState> {
    FEEDBACK.get_or_init(|| RwLock::new(FeedbackState::default()))
}
static SUGG_SEQ: OnceLock<AtomicU64> = OnceLock::new();
fn next_id() -> String {
    let s = SUGG_SEQ
        .get_or_init(|| AtomicU64::new(1))
        .fetch_add(1, Ordering::Relaxed);
    format!("sug-{}", s)
}

async fn analyze_feedback() {
    let routes_map = stats::routes_for_analysis().await;
    let mut out: Vec<Suggestion> = Vec::new();

    // Heuristic 1: High route latency -> suggest increasing http timeout hint
    let mut worst: Option<(String, f64)> = None;
    for (p, (ewma_ms, _, _)) in &routes_map {
        if worst.as_ref().map(|(_, v)| *v).unwrap_or(0.0) < *ewma_ms {
            worst = Some((p.clone(), *ewma_ms));
        }
    }
    if let Some((p, ewma_ms)) = worst {
        if ewma_ms > 800.0 {
            let desired = (((ewma_ms / 1000.0) * 2.0) + 10.0).clamp(20.0, 180.0) as u64;
            out.push(Suggestion {
                id: next_id(),
                action: "hint".into(),
                params: json!({"http_timeout_secs": desired}),
                rationale: format!(
                    "High latency on {} (~{:.0} ms); suggest http timeout {}s",
                    p, ewma_ms, desired
                ),
                confidence: 0.6,
            });
        }
    }

    // Heuristic 2: High error rate -> suggest balanced profile
    let mut high_err = false;
    for &(_, hits, errors) in routes_map.values() {
        if hits >= 10 && (errors as f64) / (hits as f64) > 0.2 {
            high_err = true;
            break;
        }
    }
    if high_err {
        out.push(Suggestion {
            id: next_id(),
            action: "profile".into(),
            params: json!({"name":"balanced"}),
            rationale: "High error rate observed across routes".into(),
            confidence: 0.55,
        });
    }

    // Heuristic 3: Many memory applications -> suggest increasing memory limit modestly
    let mem_applied = stats::event_kind_count("Memory.Applied").await;
    if mem_applied > 200 {
        let cur = { *mem_limit().read().await } as u64;
        if cur < 300 {
            let new = (cur * 3 / 2).clamp(200, 600);
            out.push(Suggestion {
                id: next_id(),
                action: "mem_limit".into(),
                params: json!({"limit": new}),
                rationale: format!(
                    "Frequent memory updates ({}); suggest limit {}",
                    mem_applied, new
                ),
                confidence: 0.5,
            });
        }
    }

    let mut st = feedback_cell().write().await;
    st.suggestions = out;
}

async fn feedback_state_get() -> impl IntoResponse {
    let st = feedback_cell().read().await.clone();
    Json(st)
}

#[derive(serde::Deserialize)]
struct FeedbackSignalPost {
    kind: String,
    target: String,
    confidence: f64,
    severity: u8,
    note: Option<String>,
}
async fn feedback_signal_post(
    State(state): State<AppState>,
    Json(req): Json<FeedbackSignalPost>,
) -> impl IntoResponse {
    let ts = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
    let sig = FeedbackSignal {
        id: next_id(),
        ts,
        kind: req.kind,
        target: req.target,
        confidence: req.confidence.clamp(0.0, 1.0),
        severity: req.severity.clamp(1, 5),
        note: req.note,
    };
    {
        let mut st = feedback_cell().write().await;
        st.signals.push(sig.clone());
        if st.signals.len() > 200 {
            st.signals.remove(0);
        }
    }
    state
        .bus
        .publish("Feedback.Signal", &json!({"signal": sig}));
    analyze_feedback().await;
    let st = feedback_cell().read().await.clone();
    persist_feedback().await;
    Json(st)
}

async fn feedback_analyze_post() -> impl IntoResponse {
    analyze_feedback().await;
    let st = feedback_cell().read().await.clone();
    persist_feedback().await;
    Json(st)
}

#[derive(serde::Deserialize)]
struct ApplyReq {
    id: String,
}
async fn feedback_apply_post(
    State(state): State<AppState>,
    Json(req): Json<ApplyReq>,
) -> impl IntoResponse {
    // Look in legacy suggestion store
    let mut sug_opt = {
        feedback_cell()
            .read()
            .await
            .suggestions
            .iter()
            .find(|s| s.id == req.id)
            .cloned()
    };
    // If not found, consult engine snapshot
    if sug_opt.is_none() {
        let (_v, list) = feedback_engine::snapshot().await;
        if let Some(s) = list.into_iter().find_map(|v| {
            let id = v.get("id").and_then(|x| x.as_str())?;
            let action = v.get("action").and_then(|x| x.as_str())?.to_string();
            let params = v.get("params").cloned().unwrap_or_else(|| json!({}));
            if id == req.id {
                Some(Suggestion {
                    id: id.to_string(),
                    action,
                    params,
                    rationale: String::new(),
                    confidence: 0.0,
                })
            } else {
                None
            }
        }) {
            sug_opt = Some(s);
        }
    }
    if let Some(sug) = sug_opt {
        // Policy-gated apply with intents events
        let applied_ok = match policy::allow_apply(&sug.action, &sug.params).await {
            Ok(()) => {
                let mut intent = json!({
                    "status": "approved",
                    "suggestion": {"id": sug.id, "action": sug.action, "params": sug.params}
                });
                crate::ext::corr::ensure_corr(&mut intent);
                state.bus.publish("Intents.Approved", &intent);
                apply_suggestion(&sug, &state).await
            }
            Err(reason) => {
                let mut intent = json!({
                    "status": "rejected",
                    "reason": reason,
                    "suggestion": {"id": sug.id, "action": sug.action, "params": sug.params}
                });
                crate::ext::corr::ensure_corr(&mut intent);
                state.bus.publish("Intents.Rejected", &intent);
                return ApiError::forbidden("gated by policy").into_response();
            }
        };
        if applied_ok {
            state.bus.publish(
                "Feedback.Applied",
                &json!({"id": sug.id, "action": sug.action, "params": sug.params}),
            );
            // Also emit a generic Actions event for episodes
            let mut payload = json!({
                "ok": true,
                "source": "feedback.apply",
                "suggestion": {"id": sug.id, "action": sug.action, "params": sug.params}
            });
            crate::ext::corr::ensure_corr(&mut payload);
            state.bus.publish("Actions.Applied", &payload);
            persist_orch().await;
        }
        return ok(json!({"ok": applied_ok})).into_response();
    }
    ApiError::not_found("unknown suggestion id").into_response()
}

#[derive(serde::Deserialize)]
struct AutoReq {
    enabled: bool,
}
async fn feedback_auto_post(Json(req): Json<AutoReq>) -> impl IntoResponse {
    let mut st = feedback_cell().write().await;
    st.auto_apply = req.enabled;
    drop(st);
    persist_feedback().await;
    ok(json!({}))
}
async fn feedback_reset_post() -> impl IntoResponse {
    let mut st = feedback_cell().write().await;
    st.signals.clear();
    st.suggestions.clear();
    drop(st);
    persist_feedback().await;
    ok(json!({}))
}

async fn apply_suggestion(s: &Suggestion, state: &AppState) -> bool {
    match s.action.as_str() {
        "hint" => {
            if let Some(v) = s.params.get("http_timeout_secs").and_then(|x| x.as_u64()) {
                let mut h = hints().write().await;
                h.http_timeout_secs = Some(v);
                true
            } else {
                false
            }
        }
        "profile" => {
            if let Some(name) = s.params.get("name").and_then(|x| x.as_str()) {
                let mut g = governor_profile().write().await;
                *g = name.to_string();
                state
                    .bus
                    .publish("Governor.Changed", &json!({"profile": name}));
                true
            } else {
                false
            }
        }
        "mem_limit" => {
            if let Some(new) = s.params.get("limit").and_then(|x| x.as_u64()) {
                let mut m = mem_limit().write().await;
                *m = (new as usize).max(1);
                true
            } else {
                false
            }
        }
        _ => false,
    }
}
// moved to chat module

// debug_ui implementation lives in ui.rs

/* === HTML (debug UI with Save/Load, self-tests, tools panel) ===
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
    .chip{display:inline-block;padding:2px 6px;border-radius:10px;background:#e5e7eb;color:#374151;font-size:12px;margin-left:6px}
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
      <h3>Chat <a href="#" class="help" data-doc="chat" data-tip="Chat UI with echo/reverse/time. Set ARW_LLAMA_URL or ARW_OPENAI_API_KEY for real backends." title="Chat docs">?</a></h3>
      <div class="row"><span class="key">backend</span> <code id="chatBackend">–</code> <button class="iconbtn" onclick="chatTest()">Test</button></div>
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
      <h3>Self‑Learning <a href="#" class="help" data-doc="orchestrator" data-tip="Signals → suggestions. Gently embedded feedback." title="Self‑Learning docs">?</a> <span class="key">ver</span> <code id="fbVer">–</code><span id="fbVerChip" class="chip" style="display:none" title="Newer snapshot available"></span></h3>
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
        <button onclick="fbRollback()">Rollback</button>
      </div>
      <pre id="fbOut">{"signals":[],"suggestions":[]}</pre>
      <h4>Suggestions (live)</h4>
      <div id="fbSugList"></div>
      <div class="row">
        <button onclick="fbVersions()">Refresh versions</button>
        <select id="fbVerSel" style="min-width:160px"></select>
        <button onclick="fbRollbackTo()">Rollback to</button>
      </div>
    </div>
    <div class="box">
      <h3>Models <a href="#" class="help" data-doc="orchestrator" data-tip="Add/delete, set default, and download models (sha256 required)." title="Models docs">?</a></h3>
      <div class="row">
        <input id="mId" placeholder="model id" style="width:220px;">
        <input id="mProv" placeholder="provider (local)" style="width:140px;">
      </div>
      <div class="row">
        <input id="mUrl" placeholder="download url (http/https)" style="width:50%">
        <input id="mSha" placeholder="sha256 (required)" style="width:220px;">
      </div>
      <div class="row">
        <button onclick="modelsList()">List</button>
        <button onclick="modelsAdd()">Add</button>
        <button onclick="modelsDelete()">Delete</button>
        <button onclick="modelsDefaultGet()">Get default</button>
        <button onclick="modelsDefaultSet()">Set default</button>
        <button onclick="modelsDownload()">Download</button>
        <button onclick="modelsCancel()">Cancel</button>
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
// Initial backend status
chatStatus(false);
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
async function modelsDownload(){
  const id=document.getElementById('mId').value.trim(); if(!id) return;
  const provider=(document.getElementById('mProv').value||'local').trim();
  const url=(document.getElementById('mUrl').value||'').trim(); if(!url){ showToast('Enter download url'); return; }
  const sha=(document.getElementById('mSha').value||'').trim();
  const body = {id, provider, url}; if(sha) body.sha256 = sha.toLowerCase();
  await req('POST','/models/download', body, 'modelsOut','rt-orch');
}
async function modelsCancel(){ const id=document.getElementById('mId').value.trim(); if(!id) return; await req('POST','/models/download/cancel',{id},'modelsOut','rt-orch'); }

// Chat UI
async function chatSend(){
  const message = (document.getElementById('chatInput').value||'').trim();
  if(!message) return;
  const model = document.getElementById('chatModel').value || 'echo';
  const t = parseFloat(document.getElementById('chatTemp').value||'');
  const body = { message, model };
  if (!Number.isNaN(t)) body.temperature = t;
  await req('POST','/chat/send', body, 'chatOut','rt-orch');
}
async function chatClear(){ await req('POST','/chat/clear', {}, 'chatOut','rt-orch'); }
async function chatStatus(probe){
  try{
    const r = await fetch(base + '/chat/status' + (probe?'?probe=1':''));
    const obj = await r.json();
    const el = document.getElementById('chatBackend'); if (el) el.textContent = (obj.backend || 'synthetic') + (obj.ok? '' : ' (error)');
    if (probe){ showToast('Chat '+(obj.backend||'synthetic')+': '+(obj.ok?'ok':'fail') + (obj.latency_ms? (' · '+obj.latency_ms+' ms') : '')); }
  }catch{}
}
async function chatTest(){ await chatStatus(true); }

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
['Service.Connected','Service.Health','Service.Test','Memory.Applied','Models.Refreshed','Tool.Ran','Models.DownloadProgress','Feedback.Suggested'].forEach(k => {
  es.addEventListener(k, (e)=>pushEvt(k, e.data));
});

// reflect download progress in Models box
es.addEventListener('Models.DownloadProgress', (e)=>{
  try{ document.getElementById('modelsOut').textContent = JSON.stringify(JSON.parse(e.data), null, 2); }catch{}
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
async function fbRollback(){ await req('POST','/feedback/rollback',{},'fbOut','rt-orch'); await fbFetchSuggestions(); }

// Live suggestions panel (feedback engine)
async function fbFetchSuggestions(){
  try{
    const r = await fetch(base + '/feedback/suggestions');
    if(!r.ok) return; const obj = await r.json();
    // show current version
    const v = (obj && obj.version) || 0; const vEl = document.getElementById('fbVer'); if (vEl) vEl.textContent = String(v);
    const list = (obj && obj.suggestions) || [];
    const el = document.getElementById('fbSugList');
    const items = list.map(s => {
      const btn = `<button class=\"iconbtn\" onclick=\"fbApplySingle('${s.id.replace(/'/g,'')}')\">Apply</button>`;
      return `<div class=\"row\"><code>${s.id}</code> ${btn}<div>${s.action} ${JSON.stringify(s.params)}</div><div style=\"color:#6b7280\">${(s.confidence||0).toFixed(2)} · ${s.rationale||''}</div></div>`;
    });
    el.innerHTML = items.join('') || '<div class=\"row\">No suggestions</div>';
  }catch{}
}
async function fbApplySingle(id){ await req('POST','/feedback/apply',{id},'fbOut','rt-orch'); }

// Refresh suggestions when engine publishes updates
es.addEventListener('Feedback.Suggested', ()=>{ fbFetchSuggestions(); fbVersions(); });
// Initial fetch
fbFetchSuggestions();
fbVersions();

// Versions list and rollback-to
async function fbVersions(){
  try{
    const r = await fetch(base + '/feedback/versions'); if(!r.ok) return;
    const obj = await r.json(); const list=(obj&&obj.versions)||[];
    const sel=document.getElementById('fbVerSel');
    if (sel){ sel.innerHTML=''; list.forEach(v=>{ const opt=document.createElement('option'); opt.value=String(v); opt.textContent=String(v); sel.appendChild(opt); }); }
    const vCur = parseInt((document.getElementById('fbVer')?.textContent||'0'),10)||0;
    const latest = list.length ? parseInt(list[0],10)||0 : vCur;
    const chip = document.getElementById('fbVerChip');
    if (chip){ if (latest > vCur) { chip.textContent = 'v'+latest; chip.style.display='inline-block'; } else { chip.style.display='none'; } }
  }catch{}
}
async function fbRollbackTo(){ const sel=document.getElementById('fbVerSel'); const val=(sel && sel.value) ? parseInt(sel.value,10) : NaN; if(!val){ showToast('Pick a version'); return; } await req('POST','/feedback/rollback?to='+encodeURIComponent(String(val)),{},'fbOut','rt-orch'); await fbFetchSuggestions(); }
// Docs wiring
let docsBase = null;
function docPath(key){
  switch(key){
    case 'memory': return '/memory_and_training/';
    case 'events': return '/guide/quickstart/';
    case 'tools': return '/api_and_schema/';
    case 'orchestrator': return '/guide/quickstart/';
    case 'chat': return '/guide/chat_backends/';
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
"##; */

// stats moved to stats.rs
