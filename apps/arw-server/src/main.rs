use arw_policy::PolicyEngine;
use arw_wasi::ToolHost;
use axum::http::HeaderMap;
use axum::{
    routing::{get, post},
    Router,
};
use serde_json::json;
use std::net::SocketAddr;
use tower::limit::ConcurrencyLimitLayer;
use tower_http::{compression::CompressionLayer, trace::TraceLayer};
// jsonschema moved to modules
use sha2::Digest as _;
use tokio::sync::Mutex;

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

mod api_actions;
mod api_config;
mod api_connectors;
mod api_context;
mod api_egress;
mod api_egress_settings;
mod api_events;
mod api_leases;
mod api_logic_units;
mod api_memory;
mod api_meta;
mod api_orchestrator;
mod api_policy;
mod api_spec;
mod api_state;
mod context_loop;
mod coverage;
mod egress_proxy;
mod read_models;
mod util;
mod worker;
mod working_set;

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
    let kernel = arw_kernel::Kernel::open(&crate::util::state_dir()).expect("init kernel");
    // dual-write bus events to kernel and track DB ids for SSE
    let sse_id_map =
        std::sync::Arc::new(Mutex::new(std::collections::VecDeque::with_capacity(2048)));
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
                    if let Ok(pbytes) = serde_json::to_vec(&env.payload) {
                        hasher.update(&pbytes);
                    }
                    let digest = hasher.finalize();
                    let key = u64::from_le_bytes([
                        digest[0], digest[1], digest[2], digest[3], digest[4], digest[5],
                        digest[6], digest[7],
                    ]);
                    let mut dq = sse_ids.lock().await;
                    if dq.len() >= 2048 {
                        dq.pop_front();
                    }
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
            Err(_) => std::sync::Arc::new(arw_wasi::NoopHost),
        }
    };
    // Curated endpoints list recorded as routes are added (avoid drift)
    let mut endpoints_acc: Vec<String> = Vec::new();
    let mut endpoints_meta_acc: Vec<serde_json::Value> = Vec::new();
    let mut app = Router::new();
    app = route_get_tag!(
        app,
        endpoints_acc,
        endpoints_meta_acc,
        paths::HEALTHZ,
        api_meta::healthz,
        "stable"
    );
    app = route_get_tag!(
        app,
        endpoints_acc,
        endpoints_meta_acc,
        paths::ABOUT,
        api_meta::about,
        "stable"
    );
    app = route_post_tag!(
        app,
        endpoints_acc,
        endpoints_meta_acc,
        paths::ACTIONS,
        api_actions::actions_submit,
        "beta"
    );
    app = route_get_tag!(
        app,
        endpoints_acc,
        endpoints_meta_acc,
        paths::ACTIONS_ID,
        api_actions::actions_get,
        "beta"
    );
    app = route_post_tag!(
        app,
        endpoints_acc,
        endpoints_meta_acc,
        paths::ACTIONS_ID_STATE,
        api_actions::actions_state_set,
        "beta"
    );
    app = route_get_tag!(
        app,
        endpoints_acc,
        endpoints_meta_acc,
        paths::EVENTS,
        api_events::events_sse,
        "stable"
    );
    app = route_get_tag!(
        app,
        endpoints_acc,
        endpoints_meta_acc,
        paths::STATE_EPISODES,
        api_state::state_episodes,
        "beta"
    );
    app = route_get_tag!(
        app,
        endpoints_acc,
        endpoints_meta_acc,
        paths::STATE_ROUTE_STATS,
        api_state::state_route_stats,
        "beta"
    );
    app = route_get_tag!(
        app,
        endpoints_acc,
        endpoints_meta_acc,
        paths::STATE_ACTIONS,
        api_state::state_actions,
        "beta"
    );
    app = route_get_tag!(
        app,
        endpoints_acc,
        endpoints_meta_acc,
        paths::STATE_CONTRIBS,
        api_state::state_contributions,
        "beta"
    );
    app = route_post_tag!(
        app,
        endpoints_acc,
        endpoints_meta_acc,
        paths::LEASES,
        api_leases::leases_create,
        "experimental"
    );
    app = route_get_tag!(
        app,
        endpoints_acc,
        endpoints_meta_acc,
        paths::STATE_LEASES,
        api_leases::state_leases,
        "experimental"
    );
    app = route_get_tag!(
        app,
        endpoints_acc,
        endpoints_meta_acc,
        paths::STATE_EGRESS,
        api_state::state_egress,
        "beta"
    );
    app = route_get_tag!(
        app,
        endpoints_acc,
        endpoints_meta_acc,
        paths::STATE_EGRESS_SETTINGS,
        api_egress_settings::state_egress_settings,
        "beta"
    );
    app = route_post_tag!(
        app,
        endpoints_acc,
        endpoints_meta_acc,
        paths::EGRESS_SETTINGS,
        api_egress_settings::egress_settings_update,
        "beta"
    );
    app = route_post_tag!(
        app,
        endpoints_acc,
        endpoints_meta_acc,
        paths::EGRESS_PREVIEW,
        api_egress::egress_preview,
        "beta"
    );
    app = route_get_tag!(
        app,
        endpoints_acc,
        endpoints_meta_acc,
        paths::STATE_POLICY,
        api_policy::state_policy,
        "experimental"
    );
    app = route_post_tag!(
        app,
        endpoints_acc,
        endpoints_meta_acc,
        paths::POLICY_RELOAD,
        api_policy::policy_reload,
        "experimental"
    );
    app = route_post_tag!(
        app,
        endpoints_acc,
        endpoints_meta_acc,
        paths::POLICY_SIMULATE,
        api_policy::policy_simulate,
        "experimental"
    );
    app = route_get_tag!(
        app,
        endpoints_acc,
        endpoints_meta_acc,
        paths::STATE_MODELS,
        api_state::state_models,
        "beta"
    );
    // Specs
    app = route_get_tag!(
        app,
        endpoints_acc,
        endpoints_meta_acc,
        paths::SPEC_OPENAPI,
        api_spec::spec_openapi,
        "stable"
    );
    app = route_get_tag!(
        app,
        endpoints_acc,
        endpoints_meta_acc,
        paths::SPEC_ASYNCAPI,
        api_spec::spec_asyncapi,
        "stable"
    );
    app = route_get_tag!(
        app,
        endpoints_acc,
        endpoints_meta_acc,
        paths::SPEC_MCP,
        api_spec::spec_mcp,
        "stable"
    );
    app = route_get_tag!(
        app,
        endpoints_acc,
        endpoints_meta_acc,
        paths::SPEC_SCHEMA,
        api_spec::spec_schema,
        "stable"
    );
    app = route_get_tag!(
        app,
        endpoints_acc,
        endpoints_meta_acc,
        paths::SPEC_INDEX,
        api_spec::spec_index,
        "stable"
    );
    // Record internal routes as well (no stability tagging for these yet)
    app = route_get_rec!(
        app,
        endpoints_acc,
        "/logic-units",
        api_logic_units::logic_units_list
    );
    app = route_get_rec!(
        app,
        endpoints_acc,
        "/state/logic_units",
        api_logic_units::state_logic_units
    );
    app = route_post_rec!(
        app,
        endpoints_acc,
        "/logic-units/install",
        api_logic_units::logic_units_install
    );
    app = route_post_rec!(
        app,
        endpoints_acc,
        "/logic-units/apply",
        api_logic_units::logic_units_apply
    );
    app = route_post_rec!(
        app,
        endpoints_acc,
        "/logic-units/revert",
        api_logic_units::logic_units_revert
    );
    app = route_get_rec!(
        app,
        endpoints_acc,
        "/state/config",
        api_config::state_config
    );
    app = route_post_rec!(app, endpoints_acc, "/patch/apply", api_config::patch_apply);
    app = route_post_rec!(
        app,
        endpoints_acc,
        "/patch/revert",
        api_config::patch_revert
    );
    app = route_get_rec!(
        app,
        endpoints_acc,
        "/state/config/snapshots",
        api_config::state_config_snapshots
    );
    app = route_get_rec!(
        app,
        endpoints_acc,
        "/state/config/snapshots/:id",
        api_config::state_config_snapshot_get
    );
    app = route_post_rec!(
        app,
        endpoints_acc,
        "/patch/validate",
        api_config::patch_validate
    );
    app = route_get_rec!(
        app,
        endpoints_acc,
        "/state/schema_map",
        api_config::state_schema_map
    );
    app = route_post_rec!(
        app,
        endpoints_acc,
        "/patch/infer_schema",
        api_config::patch_infer_schema
    );
    app = route_get_rec!(
        app,
        endpoints_acc,
        "/state/self",
        api_state::state_self_list
    );
    app = route_get_rec!(
        app,
        endpoints_acc,
        "/state/self/:agent",
        api_state::state_self_get
    );
    app = route_post_rec!(
        app,
        endpoints_acc,
        "/context/assemble",
        api_context::context_assemble
    );
    app = route_post_rec!(
        app,
        endpoints_acc,
        "/context/rehydrate",
        api_context::context_rehydrate
    );
    app = route_get_rec!(
        app,
        endpoints_acc,
        "/state/connectors",
        api_connectors::state_connectors
    );
    app = route_post_rec!(
        app,
        endpoints_acc,
        "/connectors/register",
        api_connectors::connector_register
    );
    app = route_post_rec!(
        app,
        endpoints_acc,
        "/connectors/token",
        api_connectors::connector_token_set
    );
    app = route_post_rec!(app, endpoints_acc, "/memory/put", api_memory::memory_put);
    app = route_get_rec!(
        app,
        endpoints_acc,
        "/state/memory/select",
        api_memory::state_memory_select
    );
    app = route_post_rec!(
        app,
        endpoints_acc,
        "/memory/search_embed",
        api_memory::memory_search_embed
    );
    app = route_post_rec!(
        app,
        endpoints_acc,
        "/memory/link",
        api_memory::memory_link_put
    );
    app = route_get_rec!(
        app,
        endpoints_acc,
        "/state/memory/links",
        api_memory::state_memory_links
    );
    app = route_post_rec!(
        app,
        endpoints_acc,
        "/state/memory/select_hybrid",
        api_memory::memory_select_hybrid
    );
    app = route_post_rec!(
        app,
        endpoints_acc,
        "/memory/select_coherent",
        api_memory::memory_select_coherent
    );
    app = route_get_rec!(
        app,
        endpoints_acc,
        "/state/memory/recent",
        api_memory::state_memory_recent
    );
    app = route_post_rec!(
        app,
        endpoints_acc,
        "/state/memory/explain_coherent",
        api_memory::memory_explain_coherent
    );
    app = route_get_rec!(
        app,
        endpoints_acc,
        "/orchestrator/mini_agents",
        api_orchestrator::orchestrator_mini_agents
    );
    app = route_post_rec!(
        app,
        endpoints_acc,
        "/orchestrator/mini_agents/start_training",
        api_orchestrator::orchestrator_start_training
    );
    app = route_get_rec!(
        app,
        endpoints_acc,
        "/state/orchestrator/jobs",
        api_orchestrator::state_orchestrator_jobs
    );
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
    worker::start_local_worker(state.clone());
    // Start read-model publishers (logic units, orchestrator jobs)
    read_models::start_read_models(state.clone());
    // Start/stop egress proxy based on current settings
    egress_proxy::apply_current(state.clone()).await;
    let app = app.with_state(state);
    // HTTP layers: compression, tracing, and concurrency limit
    let conc: usize = std::env::var("ARW_HTTP_MAX_CONC")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(1024);
    let app = app
        .layer(CompressionLayer::new())
        .layer(TraceLayer::new_for_http())
        .layer(ConcurrencyLimitLayer::new(conc));
    // Bind address/port (env overrides)
    let bind = std::env::var("ARW_BIND").unwrap_or_else(|_| "127.0.0.1".into());
    let port: u16 = std::env::var("ARW_PORT")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(8091);
    let addr: SocketAddr = format!("{}:{}", bind, port).parse().unwrap();
    axum::serve(
        tokio::net::TcpListener::bind(addr).await.unwrap(),
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .await
    .unwrap();
}

pub(crate) fn admin_ok(headers: &HeaderMap) -> bool {
    // When ARW_ADMIN_TOKEN is set, require it in Authorization: Bearer or X-ARW-Admin
    let token = match std::env::var("ARW_ADMIN_TOKEN") {
        Ok(t) if !t.is_empty() => t,
        _ => return true,
    };
    if let Some(hv) = headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|h| h.to_str().ok())
    {
        if let Some(bearer) = hv.strip_prefix("Bearer ") {
            if bearer == token {
                return true;
            }
        }
    }
    if let Some(hv) = headers.get("X-ARW-Admin").and_then(|h| h.to_str().ok()) {
        if hv == token {
            return true;
        }
    }
    false
}

// ---------- Config Plane (moved to api_config) ----------
// moved to api_memory
// moved to api_config
