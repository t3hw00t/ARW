use axum::{
    routing::{get, post},
    Router,
};
use chrono::SecondsFormat;
use arw_policy::PolicyEngine;
use arw_wasi::{ToolHost};
use serde_json::{json, Value};
use std::net::SocketAddr;
use tower_http::{trace::TraceLayer, compression::CompressionLayer};
use tower::limit::ConcurrencyLimitLayer;
use axum::http::HeaderMap;
// jsonschema moved to modules
use tokio::sync::Mutex;
use sha2::Digest as _;

// Route path constants (single source to reduce drift)
mod paths {
    pub const HEALTHZ: &str = "/healthz";
    pub const ABOUT: &str = "/about";
    pub const EVENTS: &str = "/events";
    pub const ACTIONS: &str = "/actions";
    pub const ACTIONS_ID: &str = "/actions/:id";
    pub const ACTIONS_ID_STATE: &str = "/actions/:id/state";
    pub const STATE_EPISODES: &str = "/state/episodes";
    pub const STATE_ROUTE_STATS: &str = "/state/route_stats";
    pub const STATE_ACTIONS: &str = "/state/actions";
    pub const STATE_CONTRIBS: &str = "/state/contributions";
    pub const LEASES: &str = "/leases";
    pub const STATE_LEASES: &str = "/state/leases";
    pub const STATE_EGRESS: &str = "/state/egress";
    pub const STATE_EGRESS_SETTINGS: &str = "/state/egress/settings";
    pub const EGRESS_SETTINGS: &str = "/egress/settings";
    pub const EGRESS_PREVIEW: &str = "/egress/preview";
    pub const STATE_POLICY: &str = "/state/policy";
    pub const POLICY_RELOAD: &str = "/policy/reload";
    pub const POLICY_SIMULATE: &str = "/policy/simulate";
    pub const STATE_MODELS: &str = "/state/models";
    pub const SPEC_OPENAPI: &str = "/spec/openapi.yaml";
    pub const SPEC_ASYNCAPI: &str = "/spec/asyncapi.yaml";
    pub const SPEC_MCP: &str = "/spec/mcp-tools.json";
    pub const SPEC_SCHEMA: &str = "/spec/schemas/:file";
    pub const SPEC_INDEX: &str = "/spec/index.json";
}

// Macros to add routes and record them in the endpoints list (avoid drift)
macro_rules! route_get_rec {
    ($router:expr, $endpoints:expr, $path:expr, $handler:path) => {{
        $endpoints.push(format!("GET {}", $path));
        $router.route($path, get($handler))
    }};
}
macro_rules! route_post_rec {
    ($router:expr, $endpoints:expr, $path:expr, $handler:path) => {{
        $endpoints.push(format!("POST {}", $path));
        $router.route($path, post($handler))
    }};
}

macro_rules! route_get_tag {
    ($router:expr, $endpoints:expr, $meta:expr, $path:expr, $handler:path, $stability:expr) => {{
        $endpoints.push(format!("GET {}", $path));
        $meta.push(serde_json::json!({"method":"GET","path":$path,"stability":$stability}));
        $router.route($path, get($handler))
    }};
}
macro_rules! route_post_tag {
    ($router:expr, $endpoints:expr, $meta:expr, $path:expr, $handler:path, $stability:expr) => {{
        $endpoints.push(format!("POST {}", $path));
        $meta.push(serde_json::json!({"method":"POST","path":$path,"stability":$stability}));
        $router.route($path, post($handler))
    }};
}

mod api_policy;
mod api_events;
mod api_context;
mod api_actions;
mod api_memory;
mod api_connectors;
mod api_state;
mod api_config;
mod api_logic_units;
mod api_leases;
mod api_orchestrator;
mod util;
mod api_meta;
mod egress_proxy;
mod api_egress_settings;
mod api_egress;
mod api_spec;

#[derive(Clone)]
pub(crate) struct AppState {
    bus: arw_events::Bus,
    kernel: arw_kernel::Kernel,
    policy: std::sync::Arc<Mutex<Policy>>, // hot‑reloadable
    host: std::sync::Arc<dyn ToolHost>,
    config_state: std::sync::Arc<Mutex<serde_json::Value>>, // effective config (demo)
    config_history: std::sync::Arc<Mutex<Vec<(String, serde_json::Value)>>>, // snapshots
    sse_id_map: std::sync::Arc<Mutex<std::collections::VecDeque<(u64, i64)>>>,
    endpoints: std::sync::Arc<Vec<String>>,
    endpoints_meta: std::sync::Arc<Vec<serde_json::Value>>,
}

type Policy = PolicyEngine;

#[tokio::main]
async fn main() {
    arw_otel::init();
    // Apply performance presets early so env-based tunables pick up sensible defaults.
    // Explicit env vars still take precedence over these seeded values.
    let _tier = arw_core::perf::apply_performance_preset();
    let bus = arw_events::Bus::new_with_replay(256, 256);
    let kernel = arw_kernel::Kernel::open(std::path::Path::new(
        &std::env::var("ARW_STATE_DIR").unwrap_or_else(|_| "state".into()),
    ))
    .expect("init kernel");
    // dual-write bus events to kernel and track DB ids for SSE
    let sse_id_map = std::sync::Arc::new(Mutex::new(std::collections::VecDeque::with_capacity(2048)));
    {
        let mut rx = bus.subscribe();
        let k2 = kernel.clone();
        let sse_ids = sse_id_map.clone();
        tokio::spawn(async move {
            while let Ok(env) = rx.recv().await {
                if let Ok(row_id) = k2.append_event(&env) {
                    let mut hasher = sha2::Sha256::new();
                    hasher.update(env.time.as_bytes());
                    hasher.update(env.kind.as_bytes());
                    if let Ok(pbytes) = serde_json::to_vec(&env.payload) { hasher.update(&pbytes); }
                    let digest = hasher.finalize();
                    let key = u64::from_le_bytes([digest[0],digest[1],digest[2],digest[3],digest[4],digest[5],digest[6],digest[7]]);
                    let mut dq = sse_ids.lock().await;
                    if dq.len() >= 2048 { dq.pop_front(); }
                    dq.push_back((key, row_id));
                }
            }
        });
    }
    let policy = PolicyEngine::load_from_env();
    // Initialize simple WASI host with http.fetch support
    let host: std::sync::Arc<dyn ToolHost> = {
        match arw_wasi::LocalHost::new() {
            Ok(h) => std::sync::Arc::new(h),
            Err(_) => std::sync::Arc::new(arw_wasi::NoopHost::default()),
        }
    };
    // Curated endpoints list recorded as routes are added (avoid drift)
    let mut endpoints_acc: Vec<String> = Vec::new();
    let mut endpoints_meta_acc: Vec<serde_json::Value> = Vec::new();
    let mut app = Router::new();
    app = route_get_tag!(app, endpoints_acc, endpoints_meta_acc, paths::HEALTHZ, api_meta::healthz, "stable");
    app = route_get_tag!(app, endpoints_acc, endpoints_meta_acc, paths::ABOUT, api_meta::about, "stable");
    app = route_post_tag!(app, endpoints_acc, endpoints_meta_acc, paths::ACTIONS, api_actions::actions_submit, "beta");
    app = route_get_tag!(app, endpoints_acc, endpoints_meta_acc, paths::ACTIONS_ID, api_actions::actions_get, "beta");
    app = route_post_tag!(app, endpoints_acc, endpoints_meta_acc, paths::ACTIONS_ID_STATE, api_actions::actions_state_set, "beta");
    app = route_get_tag!(app, endpoints_acc, endpoints_meta_acc, paths::EVENTS, api_events::events_sse, "stable");
    app = route_get_tag!(app, endpoints_acc, endpoints_meta_acc, paths::STATE_EPISODES, api_state::state_episodes, "beta");
    app = route_get_tag!(app, endpoints_acc, endpoints_meta_acc, paths::STATE_ROUTE_STATS, api_state::state_route_stats, "beta");
    app = route_get_tag!(app, endpoints_acc, endpoints_meta_acc, paths::STATE_ACTIONS, api_state::state_actions, "beta");
    app = route_get_tag!(app, endpoints_acc, endpoints_meta_acc, paths::STATE_CONTRIBS, api_state::state_contributions, "beta");
    app = route_post_tag!(app, endpoints_acc, endpoints_meta_acc, paths::LEASES, api_leases::leases_create, "experimental");
    app = route_get_tag!(app, endpoints_acc, endpoints_meta_acc, paths::STATE_LEASES, api_leases::state_leases, "experimental");
    app = route_get_tag!(app, endpoints_acc, endpoints_meta_acc, paths::STATE_EGRESS, api_state::state_egress, "beta");
    app = route_get_tag!(app, endpoints_acc, endpoints_meta_acc, paths::STATE_EGRESS_SETTINGS, api_egress_settings::state_egress_settings, "beta");
    app = route_post_tag!(app, endpoints_acc, endpoints_meta_acc, paths::EGRESS_SETTINGS, api_egress_settings::egress_settings_update, "beta");
    app = route_post_tag!(app, endpoints_acc, endpoints_meta_acc, paths::EGRESS_PREVIEW, api_egress::egress_preview, "beta");
    app = route_get_tag!(app, endpoints_acc, endpoints_meta_acc, paths::STATE_POLICY, api_policy::state_policy, "experimental");
    app = route_post_tag!(app, endpoints_acc, endpoints_meta_acc, paths::POLICY_RELOAD, api_policy::policy_reload, "experimental");
    app = route_post_tag!(app, endpoints_acc, endpoints_meta_acc, paths::POLICY_SIMULATE, api_policy::policy_simulate, "experimental");
    app = route_get_tag!(app, endpoints_acc, endpoints_meta_acc, paths::STATE_MODELS, api_state::state_models, "beta");
    // Specs
    app = route_get_tag!(app, endpoints_acc, endpoints_meta_acc, paths::SPEC_OPENAPI, api_spec::spec_openapi, "stable");
    app = route_get_tag!(app, endpoints_acc, endpoints_meta_acc, paths::SPEC_ASYNCAPI, api_spec::spec_asyncapi, "stable");
    app = route_get_tag!(app, endpoints_acc, endpoints_meta_acc, paths::SPEC_MCP, api_spec::spec_mcp, "stable");
    app = route_get_tag!(app, endpoints_acc, endpoints_meta_acc, paths::SPEC_SCHEMA, api_spec::spec_schema, "stable");
    app = route_get_tag!(app, endpoints_acc, endpoints_meta_acc, paths::SPEC_INDEX, api_spec::spec_index, "stable");
    // Record internal routes as well (no stability tagging for these yet)
    app = route_get_rec!(app, endpoints_acc, "/logic-units", api_logic_units::logic_units_list);
    app = route_get_rec!(app, endpoints_acc, "/state/logic_units", api_logic_units::state_logic_units);
    app = route_post_rec!(app, endpoints_acc, "/logic-units/install", api_logic_units::logic_units_install);
    app = route_post_rec!(app, endpoints_acc, "/logic-units/apply", api_logic_units::logic_units_apply);
    app = route_post_rec!(app, endpoints_acc, "/logic-units/revert", api_logic_units::logic_units_revert);
    app = route_get_rec!(app, endpoints_acc, "/state/config", api_config::state_config);
    app = route_post_rec!(app, endpoints_acc, "/patch/apply", api_config::patch_apply);
    app = route_post_rec!(app, endpoints_acc, "/patch/revert", api_config::patch_revert);
    app = route_get_rec!(app, endpoints_acc, "/state/config/snapshots", api_config::state_config_snapshots);
    app = route_get_rec!(app, endpoints_acc, "/state/config/snapshots/:id", api_config::state_config_snapshot_get);
    app = route_post_rec!(app, endpoints_acc, "/patch/validate", api_config::patch_validate);
    app = route_get_rec!(app, endpoints_acc, "/state/schema_map", api_config::state_schema_map);
    app = route_post_rec!(app, endpoints_acc, "/patch/infer_schema", api_config::patch_infer_schema);
    app = route_get_rec!(app, endpoints_acc, "/state/self", api_state::state_self_list);
    app = route_get_rec!(app, endpoints_acc, "/state/self/:agent", api_state::state_self_get);
    app = route_post_rec!(app, endpoints_acc, "/context/assemble", api_context::context_assemble);
    app = route_post_rec!(app, endpoints_acc, "/context/rehydrate", api_context::context_rehydrate);
    app = route_get_rec!(app, endpoints_acc, "/state/connectors", api_connectors::state_connectors);
    app = route_post_rec!(app, endpoints_acc, "/connectors/register", api_connectors::connector_register);
    app = route_post_rec!(app, endpoints_acc, "/connectors/token", api_connectors::connector_token_set);
    app = route_post_rec!(app, endpoints_acc, "/memory/put", api_memory::memory_put);
    app = route_get_rec!(app, endpoints_acc, "/state/memory/select", api_memory::state_memory_select);
    app = route_post_rec!(app, endpoints_acc, "/memory/search_embed", api_memory::memory_search_embed);
    app = route_post_rec!(app, endpoints_acc, "/memory/link", api_memory::memory_link_put);
    app = route_get_rec!(app, endpoints_acc, "/state/memory/links", api_memory::state_memory_links);
    app = route_post_rec!(app, endpoints_acc, "/state/memory/select_hybrid", api_memory::memory_select_hybrid);
    app = route_post_rec!(app, endpoints_acc, "/memory/select_coherent", api_memory::memory_select_coherent);
    app = route_get_rec!(app, endpoints_acc, "/state/memory/recent", api_memory::state_memory_recent);
    app = route_post_rec!(app, endpoints_acc, "/state/memory/explain_coherent", api_memory::memory_explain_coherent);
    app = route_get_rec!(app, endpoints_acc, "/orchestrator/mini_agents", api_orchestrator::orchestrator_mini_agents);
    app = route_post_rec!(app, endpoints_acc, "/orchestrator/mini_agents/start_training", api_orchestrator::orchestrator_start_training);
    app = route_get_rec!(app, endpoints_acc, "/state/orchestrator/jobs", api_orchestrator::state_orchestrator_jobs);
    let state = AppState {
        bus,
        kernel,
        policy: std::sync::Arc::new(Mutex::new(policy)),
        host,
        config_state: std::sync::Arc::new(Mutex::new(json!({}))),
        config_history: std::sync::Arc::new(Mutex::new(Vec::new())),
        sse_id_map,
        endpoints: std::sync::Arc::new(endpoints_acc),
        endpoints_meta: std::sync::Arc::new(endpoints_meta_acc),
    };
    // Start a simple local action worker (demo)
    start_local_worker(state.clone());
    // Start read-model publishers (logic units, orchestrator jobs)
    start_read_models(state.clone());
    // Start/stop egress proxy based on current settings
    egress_proxy::apply_current(state.clone()).await;
    let app = app.with_state(state);
    // HTTP layers: compression, tracing, and concurrency limit
    let conc: usize = std::env::var("ARW_HTTP_MAX_CONC").ok().and_then(|s| s.parse().ok()).unwrap_or(1024);
    let app = app
        .layer(CompressionLayer::new())
        .layer(TraceLayer::new_for_http())
        .layer(ConcurrencyLimitLayer::new(conc));
    // Bind address/port (env overrides)
    let bind = std::env::var("ARW_BIND").unwrap_or_else(|_| "127.0.0.1".into());
    let port: u16 = std::env::var("ARW_PORT").ok().and_then(|s| s.parse().ok()).unwrap_or(8091);
    let addr: SocketAddr = format!("{}:{}", bind, port).parse().unwrap();
    axum::serve(
        tokio::net::TcpListener::bind(addr).await.unwrap(),
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .await
    .unwrap();
}

// moved to api_meta

// moved to api_actions module

// moved to api_events module
// state read‑models moved to api_state

// moved to api_state

// moved to api_state

// moved to api_state

// Leases moved to api_leases

// moved to api_policy/api_state modules

// ---------- Helpers for local state dir ----------
pub(crate) fn state_dir() -> std::path::PathBuf {
    std::path::PathBuf::from(std::env::var("ARW_STATE_DIR").unwrap_or_else(|_| "state".into()))
}

// ---------- /state/models ----------
// moved to api_state

// ---------- Connectors (cloud/local) ----------
// moved to api_connectors

// connectors helpers moved to api_connectors

// moved to api_connectors

// moved to api_connectors

// moved to api_connectors

// ---------- Memory Abstraction Layer ----------
/* #[derive(Deserialize)]
struct MemPutReq { lane: String, #[serde(default)] kind: Option<String>, #[serde(default)] key: Option<String>, value: Value, #[serde(default)] embed: Option<Vec<f32>>, #[serde(default)] tags: Option<Vec<String>>, #[serde(default)] score: Option<f64>, #[serde(default)] prob: Option<f64> }
async fn memory_put(State(state): State<AppState>, Json(req): Json<MemPutReq>) -> impl IntoResponse {
    match state.kernel.insert_memory(None, &req.lane, req.kind.as_deref(), req.key.as_deref(), &req.value, req.embed.as_deref().map(|v| v.as_ref()), req.tags.as_deref(), req.score, req.prob) {
        Ok(id) => {
            state.bus.publish("memory.record.put", &json!({"id": id, "lane": req.lane, "kind": req.kind, "key": req.key}));
            (axum::http::StatusCode::CREATED, Json(json!({"id": id })))
        }
        Err(e) => (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"type":"about:blank","title":"Error","status":500, "detail": e.to_string()})),
        )
    }
}

//
/* moved: state_memory_select */
    let query = q.get("q").cloned().unwrap_or_default();
    let lane = q.get("lane").map(|s| s.as_str());
    let limit = q.get("limit").and_then(|s| s.parse::<i64>().ok()).unwrap_or(50);
    let mode = q.get("mode").map(|s| s.as_str()).unwrap_or("like");
    let res = if mode == "fts" {
        state.kernel.fts_search_memory(&query, lane, limit)
    } else {
        state.kernel.search_memory(&query, lane, limit)
    };
    match res {
        Ok(items) => Json(json!({"items": items, "mode": mode})),
        Err(e) => Json(json!({"items": [], "error": e.to_string()})),
    }
}

#[derive(Deserialize)]
struct MemEmbedReq { embed: Vec<f32>, #[serde(default)] lane: Option<String>, #[serde(default)] limit: Option<i64> }
/* moved: memory_search_embed */
    let lane_opt = req.lane.as_deref();
    let limit = req.limit.unwrap_or(20);
    let res = state.kernel.search_memory_by_embedding(&req.embed, lane_opt, limit);
    match res {
        Ok(items) => (axum::http::StatusCode::OK, Json(json!({"items": items, "mode": "embed"}))).into_response(),
        Err(e) => (axum::http::StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"type":"about:blank","title":"Error","status":500, "detail": e.to_string()}))).into_response(),
    }
}

#[derive(Deserialize)]
struct MemHybridReq { #[serde(default)] q: Option<String>, #[serde(default)] embed: Option<Vec<f32>>, #[serde(default)] lane: Option<String>, #[serde(default)] limit: Option<i64> }
/* moved: memory_select_hybrid */
    let lane_opt = req.lane.as_deref();
    let limit = req.limit.unwrap_or(20);
    let res = state.kernel.select_memory_hybrid(req.q.as_deref(), req.embed.as_deref().map(|v| v.as_ref()), lane_opt, limit);
    match res {
        Ok(items) => (axum::http::StatusCode::OK, Json(json!({"items": items, "mode": "hybrid"}))).into_response(),
        Err(e) => (axum::http::StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"type":"about:blank","title":"Error","status":500, "detail": e.to_string()}))).into_response(),
    }
}

#[derive(Deserialize)]
struct MemCoherentReq { #[serde(default)] q: Option<String>, #[serde(default)] embed: Option<Vec<f32>>, #[serde(default)] lane: Option<String>, #[serde(default)] limit: Option<i64>, #[serde(default)] expand_per_seed: Option<i64> }
/* moved: memory_select_coherent */
    let lane_opt = req.lane.as_deref();
    let limit = req.limit.unwrap_or(30);
    let expand_n = req.expand_per_seed.unwrap_or(3).max(0).min(10);
    let seeds = match state.kernel.select_memory_hybrid(req.q.as_deref(), req.embed.as_deref().map(|v| v.as_ref()), lane_opt, (limit/2).max(1)) {
        Ok(items) => items,
        Err(e) => return (axum::http::StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"type":"about:blank","title":"Error","status":500, "detail": e.to_string()}))).into_response(),
    };
    use std::collections::HashSet;
    let mut seen: HashSet<String> = HashSet::new();
    let mut scored: Vec<(f32, Value)> = Vec::new();
    // Seed scores
    for it in seeds.iter() {
        let id = it.get("id").and_then(|v| v.as_str()).unwrap_or("").to_string();
        if id.is_empty() { continue; }
        seen.insert(id.clone());
        let sc = it.get("cscore").and_then(|v| v.as_f64()).unwrap_or(0.5) as f32;
        scored.push((sc, it.clone()));
        // Expand links
        if expand_n > 0 {
            if let Ok(links) = state.kernel.list_memory_links(&id, expand_n) {
                for lk in links {
                    let dst_id = lk.get("dst_id").and_then(|v| v.as_str()).unwrap_or("");
                    if dst_id.is_empty() { continue; }
                    if seen.contains(dst_id) { continue; }
                    if let Ok(Some(mut rec)) = state.kernel.get_memory(dst_id) {
                        let weight = lk.get("weight").and_then(|v| v.as_f64()).unwrap_or(1.0) as f32;
                        // recency score (same as hybrid's component)
                        let now = chrono::Utc::now();
                        let recency = rec.get("updated").and_then(|v| v.as_str()).and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok()).map(|t| {
                            let age = now.signed_duration_since(t.with_timezone(&chrono::Utc)).num_seconds().max(0) as f64;
                            let hl = 3600f64 * 6f64;
                            ((-age/hl).exp()) as f32
                        }).unwrap_or(0.5);
                        let cscore = 0.5*sc + 0.3*weight + 0.2*recency;
                        if let Some(obj) = rec.as_object_mut() { obj.insert("cscore".into(), json!(cscore)); }
                        seen.insert(dst_id.to_string());
                        scored.push((cscore, rec));
                    }
                }
            }
        }
    }
    // Sort and take top limit with light diversity filter (MMR-lite)
    scored.sort_by(|a,b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
    let mut items: Vec<Value> = Vec::new();
    for (_, v) in scored.into_iter() {
        if (items.len() as i64) >= limit { break; }
        let k_new = v.get("key").and_then(|x| x.as_str()).unwrap_or("");
        let tags_new: std::collections::HashSet<&str> = v.get("tags").and_then(|x| x.as_str()).unwrap_or("").split(',').filter(|s| !s.is_empty()).collect();
        let too_similar = items.iter().any(|e| {
            let k_old = e.get("key").and_then(|x| x.as_str()).unwrap_or("");
            if !k_old.is_empty() && k_old == k_new { return true; }
            let tags_old: std::collections::HashSet<&str> = e.get("tags").and_then(|x| x.as_str()).unwrap_or("").split(',').filter(|s| !s.is_empty()).collect();
            let inter = tags_old.intersection(&tags_new).count();
            inter >= 3 // simple overlap threshold
        });
        if !too_similar { items.push(v); }
    }
    // If still short, we may have filtered too much; fill from remaining tail without filtering
    if (items.len() as i64) < limit {
        // no-op: items already the best effort
    }
    (axum::http::StatusCode::OK, Json(json!({"items": items, "mode": "coherent"}))).into_response()
}

/* moved: state_memory_recent */
    let lane = q.get("lane").map(|s| s.as_str());
    let limit = q.get("limit").and_then(|s| s.parse::<i64>().ok()).unwrap_or(100);
    match state.kernel.list_recent_memory(lane, limit) {
        Ok(items) => (axum::http::StatusCode::OK, Json(json!({"items": items}))).into_response(),
        Err(e) => (axum::http::StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"type":"about:blank","title":"Error","status":500, "detail": e.to_string()}))).into_response(),
    }
}

/* moved: memory_explain_coherent */
    let lane_opt = req.lane.as_deref();
    let limit = req.limit.unwrap_or(30);
    let expand_n = req.expand_per_seed.unwrap_or(3).max(0).min(10);
    let seeds = match state.kernel.select_memory_hybrid(req.q.as_deref(), req.embed.as_deref().map(|v| v.as_ref()), lane_opt, (limit/2).max(1)) {
        Ok(items) => items,
        Err(e) => return (axum::http::StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"type":"about:blank","title":"Error","status":500, "detail": e.to_string()}))).into_response(),
    };
    use std::collections::HashSet;
    let mut seen: HashSet<String> = HashSet::new();
    let mut scored: Vec<(f32, Value)> = Vec::new();
    let now = chrono::Utc::now();
    // Seeds with explain
    for mut it in seeds.clone() {
        let id = it.get("id").and_then(|v| v.as_str()).unwrap_or("").to_string();
        if id.is_empty() { continue; }
        seen.insert(id.clone());
        let sim = it.get("sim").and_then(|v| v.as_f64()).unwrap_or(0.0) as f32;
        let fts = it.get("_fts_hit").and_then(|v| v.as_bool()).unwrap_or(false);
        let recency = it.get("updated").and_then(|v| v.as_str()).and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok()).map(|t| {
            let age = now.signed_duration_since(t.with_timezone(&chrono::Utc)).num_seconds().max(0) as f64;
            let hl = 3600f64 * 6f64;
            ((-age/hl).exp()) as f32
        }).unwrap_or(0.5);
        let util = it.get("score").and_then(|v| v.as_f64()).map(|s| s.max(0.0).min(1.0) as f32).unwrap_or(0.0);
        let cscore = 0.5*sim + 0.2*(if fts {1.0} else {0.0}) + 0.2*recency + 0.1*util;
        if let Some(obj) = it.as_object_mut() {
            obj.insert("cscore".into(), json!(cscore));
            obj.insert("explain".into(), json!({
                "kind":"seed",
                "components": {"sim": sim, "fts": fts, "recency": recency, "utility": util},
                "cscore": cscore
            }));
        }
        scored.push((cscore, it));
        // Expand links
        if expand_n > 0 {
            if let Ok(links) = state.kernel.list_memory_links(&id, expand_n) {
                for lk in links {
                    let dst_id = lk.get("dst_id").and_then(|v| v.as_str()).unwrap_or("");
                    if dst_id.is_empty() { continue; }
                    if seen.contains(dst_id) { continue; }
                    if let Ok(Some(mut rec)) = state.kernel.get_memory(dst_id) {
                        let weight = lk.get("weight").and_then(|v| v.as_f64()).unwrap_or(1.0) as f32;
                        let recency = rec.get("updated").and_then(|v| v.as_str()).and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok()).map(|t| {
                            let age = now.signed_duration_since(t.with_timezone(&chrono::Utc)).num_seconds().max(0) as f64;
                            let hl = 3600f64 * 6f64;
                            ((-age/hl).exp()) as f32
                        }).unwrap_or(0.5);
                        let seed_score = cscore;
                        let c2 = 0.5*seed_score + 0.3*weight + 0.2*recency;
                        if let Some(obj) = rec.as_object_mut() {
                            obj.insert("cscore".into(), json!(c2));
                            obj.insert("explain".into(), json!({
                                "kind":"expanded",
                                "components": {"seed_score": seed_score, "link_weight": weight, "recency": recency},
                                "path": {"from": id, "rel": lk.get("rel"), "weight": lk.get("weight")},
                                "cscore": c2
                            }));
                        }
                        seen.insert(dst_id.to_string());
                        scored.push((c2, rec));
                    }
                }
            }
        }
    }
    scored.sort_by(|a,b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
    let items: Vec<Value> = scored.into_iter().take(limit as usize).map(|(_,v)| v).collect();
    (axum::http::StatusCode::OK, Json(json!({"items": items, "mode": "coherent_explain"}))).into_response()
}

#[derive(Deserialize)]
struct MemLinkReq { src_id: String, dst_id: String, #[serde(default)] rel: Option<String>, #[serde(default)] weight: Option<f64> }
/* moved: memory_link_put */
    match state.kernel.insert_memory_link(&req.src_id, &req.dst_id, req.rel.as_deref(), req.weight) {
        Ok(()) => {
            state.bus.publish("memory.link.put", &json!({"src_id": req.src_id, "dst_id": req.dst_id, "rel": req.rel, "weight": req.weight}));
            (axum::http::StatusCode::CREATED, Json(json!({"ok": true}))).into_response()
        }
        Err(e) => (axum::http::StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"type":"about:blank","title":"Error","status":500, "detail": e.to_string()}))).into_response(),
    }
}

/* moved: state_memory_links */
    let src_id = match q.get("id").cloned() { Some(v) => v, None => return (axum::http::StatusCode::BAD_REQUEST, Json(json!({"type":"about:blank","title":"Bad Request","status":400, "detail":"missing id"}))).into_response() };
    let limit = q.get("limit").and_then(|s| s.parse::<i64>().ok()).unwrap_or(50);
    match state.kernel.list_memory_links(&src_id, limit) {
        Ok(items) => (axum::http::StatusCode::OK, Json(json!({"items": items}))).into_response(),
        Err(e) => (axum::http::StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"type":"about:blank","title":"Error","status":500, "detail": e.to_string()}))).into_response(),
    }
}

*/
// Orchestrator moved to api_orchestrator

// model defaults moved to util::default_models

// ---------- /state/self and /state/self/:agent ----------
// moved to api_state

// ---------- Local worker (demo) ----------
fn start_local_worker(state: AppState) {
    let bus = state.bus.clone();
    let kernel = state.kernel.clone();
    let policy = state.policy.clone();
    tokio::spawn(async move {
        loop {
            match kernel.dequeue_one_queued() {
                Ok(Some((id, kind, input))) => {
                    // publish running
                    let now = chrono::Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true);
                    let env = arw_events::Envelope { time: now, kind: "actions.running".into(), payload: json!({"id": id}), policy: None, ce: None };
                    bus.publish(&env.kind, &env.payload);
                    // simulate or run lightweight actions (policy-aware for net/http)
                    let out = if kind == "net.http.get" {
                        // Prepare input with correlation headers so proxy can tag rows/events
                        let mut input2 = input.clone();
                        if let Some(obj) = input2.as_object_mut() {
                            let hdrs = obj.entry("headers").or_insert_with(|| json!({}));
                            if let Some(hmap) = hdrs.as_object_mut() {
                                hmap.insert("X-ARW-Corr".to_string(), json!(id.clone()));
                                if let Ok(p) = std::env::var("ARW_PROJECT_ID") { hmap.insert("X-ARW-Project".to_string(), json!(p)); }
                            }
                        }
                        // Lease check for net:http when allow_all=false
                        if !policy.lock().await.evaluate_action("net.http.").allow {
                            if kernel.find_valid_lease("local", "net:http").ok().flatten().is_none() &&
                               kernel.find_valid_lease("local", "io:egress").ok().flatten().is_none() {
                                let _ = kernel.append_egress("deny", Some("no_lease"), None, None, Some("http"), None, None, None, None, None);
                                json!({"error":"lease required: net:http or io:egress"})
                            } else {
                                // perform http fetch via WASI host; pass through input2 (url, connector_id, headers+correlation)
                                match state.host.run_tool("http.fetch", &input2).await {
                                    Ok(v) => {
                                        let host = v.get("dest_host").and_then(|x| x.as_str());
                                        let port = v.get("dest_port").and_then(|x| x.as_i64());
                                        let proto = v.get("protocol").and_then(|x| x.as_str());
                                        let bin = v.get("bytes_in").and_then(|x| x.as_i64());
                                        let mut eid: Option<i64> = None;
                                        if std::env::var("ARW_EGRESS_LEDGER_ENABLE").ok().as_deref() == Some("1") {
                                            let posture = crate::util::effective_posture();
                                            if let Ok(id) = kernel.append_egress("allow", Some("ok"), host, port, proto, bin, Some(0), Some(&id), None, Some(&posture)) { eid = Some(id); }
                                        }
                                        // publish egress ledger event with id (if any)
                                        let posture = crate::util::effective_posture();
                                        bus.publish("egress.ledger.appended", &json!({"id": eid, "decision":"allow","dest_host":host,"dest_port":port,"protocol":proto,"bytes_in":bin, "corr_id": id, "posture": posture }));
                                        v
                                    }
                                    Err(arw_wasi::WasiError::Denied{ reason, dest_host, dest_port, protocol, .. }) => {
                                        let mut eid: Option<i64> = None;
                                        if std::env::var("ARW_EGRESS_LEDGER_ENABLE").ok().as_deref() == Some("1") {
                                            let posture = crate::util::effective_posture();
                                            if let Ok(id) = kernel.append_egress("deny", Some(reason.as_str()), dest_host.as_deref(), dest_port, protocol.as_deref(), None, None, Some(&id), None, Some(&posture)) { eid = Some(id); }
                                        }
                                        let posture = crate::util::effective_posture();
                                        bus.publish("egress.ledger.appended", &json!({"id": eid, "decision":"deny","reason":reason,"dest_host":dest_host,"dest_port":dest_port,"protocol":protocol, "corr_id": id, "posture": posture }));
                                        json!({"error":"denied","reason": reason})
                                    }
                                    Err(e) => json!({"error":"runtime","detail": e.to_string()}),
                                }
                            }
                        } else {
                            // allow_all path: still do the fetch for utility; pass through input2
                            match state.host.run_tool("http.fetch", &input2).await {
                                Ok(v) => {
                                    let host = v.get("dest_host").and_then(|x| x.as_str());
                                    let port = v.get("dest_port").and_then(|x| x.as_i64());
                                    let proto = v.get("protocol").and_then(|x| x.as_str());
                                    let bin = v.get("bytes_in").and_then(|x| x.as_i64());
                                    let mut eid: Option<i64> = None;
                                    if std::env::var("ARW_EGRESS_LEDGER_ENABLE").ok().as_deref() == Some("1") {
                                        let posture = crate::util::effective_posture();
                                        if let Ok(id) = kernel.append_egress("allow", Some("ok"), host, port, proto, bin, Some(0), Some(&id), None, Some(&posture)) { eid = Some(id); }
                                    }
                                    let posture = crate::util::effective_posture();
                                    bus.publish("egress.ledger.appended", &json!({"id": eid, "decision":"allow","dest_host":host,"dest_port":port,"protocol":proto,"bytes_in":bin, "corr_id": id, "posture": posture }));
                                    v
                                }
                                Err(e) => json!({"error":"runtime","detail": e.to_string()}),
                            }
                        }
                    } else if kind == "fs.patch" {
                        let allowed = if !policy.lock().await.evaluate_action("fs.patch").allow {
                            kernel.find_valid_lease("local", "fs").ok().flatten().is_some() ||
                            kernel.find_valid_lease("local", "fs:patch").ok().flatten().is_some()
                        } else { true };
                        if !allowed {
                            bus.publish(
                                "policy.decision",
                                &json!({
                                    "action": "fs.patch",
                                    "allow": false,
                                    "require_capability": "fs|fs:patch",
                                    "explain": {"reason":"lease_required"}
                                }),
                            );
                            json!({"error":"lease required: fs or fs:patch"})
                        } else {
                            match state.host.run_tool("fs.patch", &input).await {
                                Ok(v) => {
                                    // Publish projects.file.written
                                    let path_s = v.get("path").and_then(|x| x.as_str()).unwrap_or("");
                                    bus.publish("projects.file.written", &json!({"path": path_s, "sha256": v.get("sha256") }));
                                    v
                                }
                                Err(e) => json!({"error":"runtime","detail": e.to_string()}),
                            }
                        }
                    } else if kind == "app.vscode.open" {
                        // Lease gate for local app invocation
                        let allowed = if !policy.lock().await.evaluate_action("app.vscode.open").allow {
                            kernel.find_valid_lease("local", "io:app:vscode").ok().flatten().is_some() ||
                            kernel.find_valid_lease("local", "io:app").ok().flatten().is_some()
                        } else { true };
                        if !allowed {
                            bus.publish(
                                "policy.decision",
                                &json!({
                                    "action": "app.vscode.open",
                                    "allow": false,
                                    "require_capability": "io:app:vscode|io:app",
                                    "explain": {"reason":"lease_required"}
                                }),
                            );
                            json!({"error":"lease required: io:app:vscode or io:app"})
                        } else {
                            match state.host.run_tool("app.vscode.open", &input).await {
                                Ok(v) => {
                                    let path_s = input.get("path").and_then(|x| x.as_str()).unwrap_or("");
                                    bus.publish("apps.vscode.opened", &json!({"path": path_s }));
                                    v
                                }
                                Err(e) => json!({"error":"runtime","detail": e.to_string()}),
                            }
                        }
                    } else {
                        simulate_action(&kind, &input).unwrap_or(json!({"ok": true}))
                    };
                    let _ = kernel.update_action_result(&env.payload["id"].as_str().unwrap_or("").to_string(), Some(&out), None);
                    let _ = kernel.set_action_state(env.payload["id"].as_str().unwrap_or(""), "completed");
                    // publish completed
                    let now2 = chrono::Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true);
                    let env2 = arw_events::Envelope { time: now2, kind: "actions.completed".into(), payload: json!({"id": env.payload["id"], "output": out}), policy: None, ce: None };
                    bus.publish(&env2.kind, &env2.payload);
                    let _ = kernel.append_contribution("local", "task.complete", 1.0, "task", None, None, None);
                }
                Ok(None) => tokio::time::sleep(std::time::Duration::from_millis(200)).await,
                Err(_) => tokio::time::sleep(std::time::Duration::from_millis(500)).await,
            }
        }
    });
}

fn simulate_action(kind: &str, input: &Value) -> Result<Value, String> {
    match kind {
        "demo.echo" => Ok(json!({"echo": input})),
        _ => Ok(json!({"ok": true})),
    }
}

// ---------- Read-model publishers ----------
fn start_read_models(state: AppState) {
    let s1 = state.clone();
    tokio::spawn(async move {
        let mut last_hash: Option<[u8; 32]> = None;
        let mut tick = tokio::time::interval(std::time::Duration::from_millis(1500));
        loop {
            tick.tick().await;
            let items = s1.kernel.list_logic_units(200).unwrap_or_default();
            let snap = json!({"id":"logic_units","items": items});
            if let Ok(bytes) = serde_json::to_vec(&snap) {
                let mut hasher = sha2::Sha256::new();
                hasher.update(&bytes);
                let digest = hasher.finalize();
                let mut arr = [0u8;32]; arr.copy_from_slice(&digest);
                if last_hash.as_ref().map(|h| h!=&arr).unwrap_or(true) {
                    last_hash = Some(arr);
                    s1.bus.publish("state.read.model.patch", &json!({
                        "id":"logic_units",
                        "patch": [ {"op":"replace", "path":"/", "value": {"items": snap["items"]} } ]
                    }));
                }
            }
        }
    });
    let s2 = state.clone();
    tokio::spawn(async move {
        let mut last_hash: Option<[u8; 32]> = None;
        let mut tick = tokio::time::interval(std::time::Duration::from_millis(2000));
        loop {
            tick.tick().await;
            let items = s2.kernel.list_orchestrator_jobs(200).unwrap_or_default();
            let snap = json!({"id":"orchestrator_jobs","items": items});
            if let Ok(bytes) = serde_json::to_vec(&snap) {
                let mut hasher = sha2::Sha256::new();
                hasher.update(&bytes);
                let digest = hasher.finalize();
                let mut arr = [0u8;32]; arr.copy_from_slice(&digest);
                if last_hash.as_ref().map(|h| h!=&arr).unwrap_or(true) {
                    last_hash = Some(arr);
                    s2.bus.publish("state.read.model.patch", &json!({
                        "id":"orchestrator_jobs",
                        "patch": [ {"op":"replace", "path":"/", "value": {"items": snap["items"]} } ]
                    }));
                }
            }
        }
    });

    let s3 = state.clone();
    tokio::spawn(async move {
        let mut last_hash: Option<[u8; 32]> = None;
        let mut tick = tokio::time::interval(std::time::Duration::from_millis(2500));
        loop {
            tick.tick().await;
            let items = s3.kernel.list_recent_memory(None, 200).unwrap_or_default();
            let snap = json!({"id":"memory_recent","items": items});
            if let Ok(bytes) = serde_json::to_vec(&snap) {
                let mut hasher = sha2::Sha256::new();
                hasher.update(&bytes);
                let digest = hasher.finalize();
                let mut arr = [0u8;32]; arr.copy_from_slice(&digest);
                if last_hash.as_ref().map(|h| h!=&arr).unwrap_or(true) {
                    last_hash = Some(arr);
                    s3.bus.publish("state.read.model.patch", &json!({
                        "id":"memory_recent",
                        "patch": [ {"op":"replace", "path":"/", "value": {"items": snap["items"]} } ]
                    }));
                }
            }
        }
    });

    // Route stats read-model
    let s4 = state.clone();
    tokio::spawn(async move {
        let mut last_hash: Option<[u8; 32]> = None;
        let mut tick = tokio::time::interval(std::time::Duration::from_millis(2000));
        loop {
            tick.tick().await;
            let stats = s4.bus.stats();
            let snap = json!({"id":"route_stats","published": stats.published, "delivered": stats.delivered, "receivers": stats.receivers, "lagged": stats.lagged, "no_receivers": stats.no_receivers});
            if let Ok(bytes) = serde_json::to_vec(&snap) {
                let mut hasher = sha2::Sha256::new();
                hasher.update(&bytes);
                let digest = hasher.finalize();
                let mut arr = [0u8;32]; arr.copy_from_slice(&digest);
                if last_hash.as_ref().map(|h| h!=&arr).unwrap_or(true) {
                    last_hash = Some(arr);
                    s4.bus.publish("state.read.model.patch", &json!({
                        "id":"route_stats",
                        "patch": [ {"op":"replace", "path":"/", "value": {"published": stats.published, "delivered": stats.delivered, "receivers": stats.receivers, "lagged": stats.lagged, "no_receivers": stats.no_receivers} } ]
                    }));
                }
            }
        }
    });
}

// ---------- Context: assemble & rehydrate ----------
// moved to api_context module
// Logic Units moved to api_logic_units

pub(crate) fn admin_ok(headers: &HeaderMap) -> bool {
    // When ARW_ADMIN_TOKEN is set, require it in Authorization: Bearer or X-ARW-Admin
    let token = match std::env::var("ARW_ADMIN_TOKEN") {
        Ok(t) if !t.is_empty() => t,
        _ => return true,
    };
    if let Some(hv) = headers.get(axum::http::header::AUTHORIZATION).and_then(|h| h.to_str().ok()) {
        if let Some(bearer) = hv.strip_prefix("Bearer ") {
            if bearer == token { return true; }
        }
    }
    if let Some(hv) = headers.get("X-ARW-Admin").and_then(|h| h.to_str().ok()) {
        if hv == token { return true; }
    }
    false
}

// ---------- Config Plane (moved to api_config) ----------
    // moved to api_memory
    // moved to api_config
