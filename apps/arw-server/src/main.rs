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
use utoipa::OpenApi;

// Route path constants (single source to reduce drift)
mod paths {
    pub const HEALTHZ: &str = "/healthz";
    pub const ABOUT: &str = "/about";
    pub const EVENTS: &str = "/events";
    pub const METRICS: &str = "/metrics";
    pub const ACTIONS: &str = "/actions";
    pub const ACTIONS_ID: &str = "/actions/:id";
    pub const ACTIONS_ID_STATE: &str = "/actions/:id/state";
    pub const STATE_EPISODES: &str = "/state/episodes";
    pub const STATE_ROUTE_STATS: &str = "/state/route_stats";
    pub const STATE_ACTIONS: &str = "/state/actions";
    pub const STATE_CONTRIBS: &str = "/state/contributions";
    pub const STATE_RESEARCH_WATCHER: &str = "/state/research_watcher";
    pub const STATE_STAGING_ACTIONS: &str = "/state/staging/actions";
    pub const STATE_TRAINING_TELEMETRY: &str = "/state/training/telemetry";
    pub const STATE_RUNTIME_MATRIX: &str = "/state/runtime_matrix";
    pub const STATE_SELF: &str = "/state/self";
    pub const STATE_SELF_AGENT: &str = "/state/self/:agent";
    pub const STATE_EXPERIMENTS: &str = "/state/experiments";
    pub const LEASES: &str = "/leases";
    pub const STATE_LEASES: &str = "/state/leases";
    pub const STATE_EGRESS: &str = "/state/egress";
    pub const STATE_EGRESS_SETTINGS: &str = "/state/egress/settings";
    pub const EGRESS_SETTINGS: &str = "/egress/settings";
    pub const EGRESS_PREVIEW: &str = "/egress/preview";
    pub const STATE_POLICY: &str = "/state/policy";
    pub const STATE_POLICY_CAPSULES: &str = "/state/policy/capsules";
    pub const POLICY_RELOAD: &str = "/policy/reload";
    pub const POLICY_SIMULATE: &str = "/policy/simulate";
    pub const STATE_MODELS: &str = "/state/models";
    pub const SPEC_OPENAPI: &str = "/spec/openapi.yaml";
    pub const SPEC_ASYNCAPI: &str = "/spec/asyncapi.yaml";
    pub const SPEC_MCP: &str = "/spec/mcp-tools.json";
    pub const SPEC_SCHEMA: &str = "/spec/schemas/:file";
    pub const SPEC_INDEX: &str = "/spec/index.json";
    pub const ADMIN_DEBUG: &str = "/admin/debug";
    pub const ADMIN_EVENTS: &str = "/admin/events";
    pub const ADMIN_STATE_ROUTE_STATS: &str = "/admin/state/route_stats";
    pub const ADMIN_MODELS: &str = "/admin/models";
    pub const ADMIN_MODELS_SUMMARY: &str = "/admin/models/summary";
    pub const ADMIN_MODELS_REFRESH: &str = "/admin/models/refresh";
    pub const ADMIN_MODELS_SAVE: &str = "/admin/models/save";
    pub const ADMIN_MODELS_LOAD: &str = "/admin/models/load";
    pub const ADMIN_MODELS_ADD: &str = "/admin/models/add";
    pub const ADMIN_MODELS_REMOVE: &str = "/admin/models/remove";
    pub const ADMIN_MODELS_DEFAULT: &str = "/admin/models/default";
    pub const ADMIN_MODELS_CONCURRENCY: &str = "/admin/models/concurrency";
    pub const ADMIN_MODELS_DOWNLOAD: &str = "/admin/models/download";
    pub const ADMIN_MODELS_DOWNLOAD_CANCEL: &str = "/admin/models/download/cancel";
    pub const ADMIN_MODELS_CAS_GC: &str = "/admin/models/cas_gc";
    pub const ADMIN_MODELS_JOBS: &str = "/admin/models/jobs";
    pub const ADMIN_STATE_MODELS_METRICS: &str = "/admin/state/models_metrics";
    pub const ADMIN_STATE_MODELS_HASHES: &str = "/admin/state/models_hashes";
    pub const ADMIN_STATE_GUARDRAILS_METRICS: &str = "/admin/state/guardrails_metrics";
    pub const ADMIN_TOOLS: &str = "/admin/tools";
    pub const ADMIN_TOOLS_RUN: &str = "/admin/tools/run";
    pub const ADMIN_TOOLS_CACHE_STATS: &str = "/admin/tools/cache_stats";
    pub const ADMIN_GOVERNOR_PROFILE: &str = "/admin/governor/profile";
    pub const ADMIN_GOVERNOR_HINTS: &str = "/admin/governor/hints";
    pub const ADMIN_STATE_MEMORY_QUARANTINE: &str = "/admin/state/memory/quarantine";
    pub const ADMIN_MEMORY_QUARANTINE: &str = "/admin/memory/quarantine";
    pub const ADMIN_MEMORY_QUARANTINE_ADMIT: &str = "/admin/memory/quarantine/admit";
    pub const ADMIN_STATE_WORLD_DIFFS: &str = "/admin/state/world_diffs";
    pub const ADMIN_WORLD_DIFFS_QUEUE: &str = "/admin/world_diffs/queue";
    pub const ADMIN_WORLD_DIFFS_DECISION: &str = "/admin/world_diffs/decision";
    pub const ADMIN_PROBE: &str = "/admin/probe";
    pub const ADMIN_PROBE_HW: &str = "/admin/probe/hw";
    pub const ADMIN_PROBE_METRICS: &str = "/admin/probe/metrics";
    pub const ADMIN_INTROSPECT_STATS: &str = "/admin/introspect/stats";
    pub const ADMIN_HIERARCHY_STATE: &str = "/admin/hierarchy/state";
    pub const ADMIN_HIERARCHY_ROLE: &str = "/admin/hierarchy/role";
    pub const ADMIN_HIERARCHY_HELLO: &str = "/admin/hierarchy/hello";
    pub const ADMIN_HIERARCHY_OFFER: &str = "/admin/hierarchy/offer";
    pub const ADMIN_HIERARCHY_ACCEPT: &str = "/admin/hierarchy/accept";
    pub const ADMIN_SELF_MODEL_PROPOSE: &str = "/admin/self_model/propose";
    pub const ADMIN_SELF_MODEL_APPLY: &str = "/admin/self_model/apply";
    pub const ADMIN_STATE_OBSERVATIONS: &str = "/admin/state/observations";
    pub const ADMIN_STATE_BELIEFS: &str = "/admin/state/beliefs";
    pub const ADMIN_STATE_INTENTS: &str = "/admin/state/intents";
    pub const ADMIN_STATE_ACTIONS: &str = "/admin/state/actions";
    pub const ADMIN_STATE_CLUSTER: &str = "/admin/state/cluster";
    pub const ADMIN_STATE_WORLD: &str = "/admin/state/world";
    pub const ADMIN_STATE_WORLD_SELECT: &str = "/admin/state/world/select";
    pub const ADMIN_PROJECTS_LIST: &str = "/admin/projects/list";
    pub const ADMIN_PROJECTS_CREATE: &str = "/admin/projects/create";
    pub const ADMIN_PROJECTS_TREE: &str = "/admin/projects/tree";
    pub const ADMIN_PROJECTS_NOTES: &str = "/admin/projects/notes";
    pub const ADMIN_PROJECTS_FILE: &str = "/admin/projects/file";
    pub const ADMIN_PROJECTS_PATCH: &str = "/admin/projects/patch";
    pub const ADMIN_PROJECTS_IMPORT: &str = "/admin/projects/import";
    pub const ADMIN_UI_MODELS: &str = "/admin/ui/models";
    pub const ADMIN_UI_AGENTS: &str = "/admin/ui/agents";
    pub const ADMIN_UI_PROJECTS: &str = "/admin/ui/projects";
    pub const ADMIN_UI_FLOWS: &str = "/admin/ui/flows";
    pub const ADMIN_UI_TOKENS: &str = "/admin/ui/assets/tokens.css";
    pub const ADMIN_UI_KIT: &str = "/admin/ui/assets/ui-kit.css";
    pub const CATALOG_INDEX: &str = "/catalog/index";
    pub const CATALOG_HEALTH: &str = "/catalog/health";
    pub const ADMIN_RPU_TRUST: &str = "/admin/rpu/trust";
    pub const ADMIN_RPU_RELOAD: &str = "/admin/rpu/reload";
    pub const ADMIN_FEEDBACK_STATE: &str = "/admin/feedback/state";
    pub const ADMIN_FEEDBACK_SIGNAL: &str = "/admin/feedback/signal";
    pub const ADMIN_FEEDBACK_ANALYZE: &str = "/admin/feedback/analyze";
    pub const ADMIN_FEEDBACK_APPLY: &str = "/admin/feedback/apply";
    pub const ADMIN_FEEDBACK_AUTO: &str = "/admin/feedback/auto";
    pub const ADMIN_FEEDBACK_RESET: &str = "/admin/feedback/reset";
    pub const ADMIN_DISTILL: &str = "/admin/distill";
    pub const ADMIN_FEEDBACK_SUGGESTIONS: &str = "/admin/feedback/suggestions";
    pub const ADMIN_FEEDBACK_UPDATES: &str = "/admin/feedback/updates";
    pub const ADMIN_FEEDBACK_POLICY: &str = "/admin/feedback/policy";
    pub const ADMIN_FEEDBACK_VERSIONS: &str = "/admin/feedback/versions";
    pub const ADMIN_FEEDBACK_ROLLBACK: &str = "/admin/feedback/rollback";
    pub const ADMIN_EXPERIMENTS_DEFINE: &str = "/admin/experiments/define";
    pub const ADMIN_EXPERIMENTS_RUN: &str = "/admin/experiments/run";
    pub const ADMIN_EXPERIMENTS_ACTIVATE: &str = "/admin/experiments/activate";
    pub const ADMIN_EXPERIMENTS_LIST: &str = "/admin/experiments/list";
    pub const ADMIN_EXPERIMENTS_SCOREBOARD: &str = "/admin/experiments/scoreboard";
    pub const ADMIN_EXPERIMENTS_WINNERS: &str = "/admin/experiments/winners";
    pub const ADMIN_EXPERIMENTS_START: &str = "/admin/experiments/start";
    pub const ADMIN_EXPERIMENTS_STOP: &str = "/admin/experiments/stop";
    pub const ADMIN_EXPERIMENTS_ASSIGN: &str = "/admin/experiments/assign";
    pub const ADMIN_GOLDENS_LIST: &str = "/admin/goldens/list";
    pub const ADMIN_GOLDENS_ADD: &str = "/admin/goldens/add";
    pub const ADMIN_GOLDENS_RUN: &str = "/admin/goldens/run";
    pub const RESEARCH_WATCHER_APPROVE: &str = "/research_watcher/:id/approve";
    pub const RESEARCH_WATCHER_ARCHIVE: &str = "/research_watcher/:id/archive";
    pub const STAGING_ACTION_APPROVE: &str = "/staging/actions/:id/approve";
    pub const STAGING_ACTION_DENY: &str = "/staging/actions/:id/deny";
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

mod access_log;
mod api_actions;
mod api_config;
mod api_connectors;
mod api_context;
mod api_distill;
mod api_egress;
mod api_egress_settings;
mod api_events;
mod api_experiments;
mod api_feedback;
mod api_goldens;
mod api_governor;
mod api_hierarchy;
mod api_leases;
mod api_logic_units;
mod api_memory;
mod api_meta;
mod api_metrics;
mod api_models;
mod api_orchestrator;
mod api_policy;
mod api_probe;
mod api_projects;
mod api_research_watcher;
mod api_review;
mod api_rpu;
mod api_self_model;
mod api_spec;
mod api_staging;
mod api_state;
mod api_tools;
mod api_ui;
mod capsule_guard;
mod cluster;
pub mod config;
mod context_loop;
mod coverage;
mod distill;
mod egress_policy;
mod egress_proxy;
mod experiments;
mod feedback;
mod goldens;
mod governor;
#[cfg(feature = "grpc")]
mod grpc;
mod http_timeout;
mod metrics;
mod models;
mod openapi;
mod patch_guard;
mod read_models;
mod research_watcher;
mod responses;
mod review;
mod runtime_matrix;
mod security;
mod self_model;
mod sse_cache;
mod staging;
mod state_observer;
mod tool_cache;
mod tools;
mod training;
mod util;
mod worker;
mod working_set;
mod world;

#[derive(Clone)]
pub(crate) struct AppState {
    bus: arw_events::Bus,
    kernel: arw_kernel::Kernel,
    policy: std::sync::Arc<Mutex<Policy>>, // hot‑reloadable
    host: std::sync::Arc<dyn ToolHost>,
    config_state: std::sync::Arc<Mutex<serde_json::Value>>, // effective config (demo)
    config_history: std::sync::Arc<Mutex<Vec<(String, serde_json::Value)>>>, // snapshots
    sse_id_map: std::sync::Arc<Mutex<sse_cache::SseIdCache>>,
    endpoints: std::sync::Arc<Vec<String>>,
    endpoints_meta: std::sync::Arc<Vec<serde_json::Value>>,
    metrics: std::sync::Arc<metrics::Metrics>,
    kernel_enabled: bool,
    models: std::sync::Arc<models::ModelStore>,
    tool_cache: std::sync::Arc<tool_cache::ToolCache>,
    governor: std::sync::Arc<governor::GovernorState>,
    feedback: std::sync::Arc<feedback::FeedbackHub>,
    cluster: std::sync::Arc<cluster::ClusterRegistry>,
    experiments: std::sync::Arc<experiments::Experiments>,
    capsules: std::sync::Arc<capsule_guard::CapsuleStore>,
}

type Policy = PolicyEngine;

impl AppState {
    pub fn kernel_enabled(&self) -> bool {
        self.kernel_enabled
    }

    pub fn kernel(&self) -> &arw_kernel::Kernel {
        &self.kernel
    }

    pub fn kernel_if_enabled(&self) -> Option<&arw_kernel::Kernel> {
        if self.kernel_enabled {
            Some(&self.kernel)
        } else {
            None
        }
    }

    pub fn models(&self) -> std::sync::Arc<models::ModelStore> {
        self.models.clone()
    }

    pub fn tool_cache(&self) -> std::sync::Arc<tool_cache::ToolCache> {
        self.tool_cache.clone()
    }

    pub fn host(&self) -> std::sync::Arc<dyn ToolHost> {
        self.host.clone()
    }

    pub fn metrics(&self) -> std::sync::Arc<metrics::Metrics> {
        self.metrics.clone()
    }

    pub fn bus(&self) -> arw_events::Bus {
        self.bus.clone()
    }

    pub fn capsules(&self) -> std::sync::Arc<capsule_guard::CapsuleStore> {
        self.capsules.clone()
    }

    pub fn sse_cache(&self) -> std::sync::Arc<Mutex<sse_cache::SseIdCache>> {
        self.sse_id_map.clone()
    }

    pub fn governor(&self) -> std::sync::Arc<governor::GovernorState> {
        self.governor.clone()
    }

    pub fn feedback(&self) -> std::sync::Arc<feedback::FeedbackHub> {
        self.feedback.clone()
    }

    pub fn cluster(&self) -> std::sync::Arc<cluster::ClusterRegistry> {
        self.cluster.clone()
    }

    pub fn experiments(&self) -> std::sync::Arc<experiments::Experiments> {
        self.experiments.clone()
    }
}

#[tokio::main]
async fn main() {
    // OpenAPI/spec export mode for CI/docs sync (no server startup).
    if let Ok(path) = std::env::var("OPENAPI_OUT") {
        // If OPENAPI_GEN=1, emit generated OpenAPI from annotations; otherwise copy curated spec.
        let gen = std::env::var("OPENAPI_GEN").ok().as_deref() == Some("1");
        if let Some(parent) = std::path::Path::new(&path).parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if gen {
            let yaml = crate::openapi::ApiDoc::openapi()
                .to_yaml()
                .unwrap_or_else(|_| "openapi: 3.0.3".into());
            if let Err(e) = std::fs::write(&path, yaml) {
                eprintln!(
                    "error: failed to write generated OPENAPI_OUT ({}): {}",
                    path, e
                );
                std::process::exit(2);
            }
        } else {
            // Write curated OpenAPI from spec/ to the requested path to keep
            // CI's codegen-vs-curated comparison stable while we migrate.
            let src = std::path::Path::new("spec").join("openapi.yaml");
            match std::fs::read(&src) {
                Ok(bytes) => {
                    if let Err(e) = std::fs::write(&path, bytes) {
                        eprintln!("error: failed to write OPENAPI_OUT ({}): {}", path, e);
                        std::process::exit(2);
                    }
                }
                Err(e) => {
                    eprintln!("error: missing spec/openapi.yaml: {}", e);
                    std::process::exit(2);
                }
            }
        }
        // Emit selected schemas used in docs (gating contract & capsule)
        {
            use schemars::schema_for;
            let dir = std::path::Path::new("spec/schemas");
            let _ = std::fs::create_dir_all(dir);
            let contract_schema = schema_for!(arw_core::gating::ContractCfg);
            let capsule_schema = schema_for!(arw_protocol::GatingCapsule);
            let _ = std::fs::write(
                dir.join("gating_contract.json"),
                serde_json::to_string_pretty(&contract_schema).unwrap(),
            );
            let _ = std::fs::write(
                dir.join("gating_capsule.json"),
                serde_json::to_string_pretty(&capsule_schema).unwrap(),
            );
        }
        // Gating keys index for docs convenience
        {
            let keys_path = std::path::Path::new("docs/GATING_KEYS.md");
            let mut out = String::from("# Gating Keys\n\nGenerated from code.\n\n");
            for k in arw_core::gating_keys::list() {
                out.push_str(&format!("- `{}`\n", k));
            }
            let _ = std::fs::write(keys_path, out);
        }
        return;
    }

    arw_otel::init();
    // Apply performance presets early so env-based tunables pick up sensible defaults.
    // Explicit env vars still take precedence over these seeded values.
    let _tier = arw_core::perf::apply_performance_preset();
    http_timeout::init_from_env();
    let bus = arw_events::Bus::new_with_replay(256, 256);
    let kernel = arw_kernel::Kernel::open(&crate::util::state_dir()).expect("init kernel");
    let kernel_enabled = config::kernel_enabled_from_env();
    // dual-write bus events to kernel and track DB ids for SSE when enabled
    let sse_id_map = std::sync::Arc::new(Mutex::new(sse_cache::SseIdCache::with_capacity(2048)));
    let metrics = std::sync::Arc::new(metrics::Metrics::default());
    if kernel_enabled {
        let mut rx = bus.subscribe();
        let k2 = kernel.clone();
        let sse_ids = sse_id_map.clone();
        let metrics_clone = metrics.clone();
        tokio::spawn(async move {
            while let Ok(env) = rx.recv().await {
                metrics_clone.record_event(&env.kind);
                if let Ok(row_id) = k2.append_event_async(&env).await {
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
                    let mut cache = sse_ids.lock().await;
                    cache.insert(key, row_id);
                }
            }
        });
    } else {
        let mut rx = bus.subscribe();
        let metrics_clone = metrics.clone();
        tokio::spawn(async move {
            while let Ok(env) = rx.recv().await {
                metrics_clone.record_event(&env.kind);
            }
        });
    }
    let policy = PolicyEngine::load_from_env();
    let policy_arc = std::sync::Arc::new(Mutex::new(policy));
    // Initialize simple WASI host with http.fetch support
    let host: std::sync::Arc<dyn ToolHost> = {
        match arw_wasi::LocalHost::new() {
            Ok(h) => std::sync::Arc::new(h),
            Err(_) => std::sync::Arc::new(arw_wasi::NoopHost),
        }
    };
    let models_store = std::sync::Arc::new(models::ModelStore::new(
        bus.clone(),
        if kernel_enabled {
            Some(kernel.clone())
        } else {
            None
        },
    ));
    let governor_state = governor::GovernorState::new().await;
    models_store.bootstrap().await;
    let tool_cache = std::sync::Arc::new(tool_cache::ToolCache::new());
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
    app = route_get_tag!(
        app,
        endpoints_acc,
        endpoints_meta_acc,
        "/shutdown",
        api_meta::shutdown,
        "experimental"
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
        paths::METRICS,
        api_metrics::metrics_prometheus,
        "stable"
    );
    app = route_get_tag!(
        app,
        endpoints_acc,
        endpoints_meta_acc,
        paths::ADMIN_EVENTS,
        api_events::events_sse,
        "legacy"
    );
    app = route_get_tag!(
        app,
        endpoints_acc,
        endpoints_meta_acc,
        paths::ADMIN_DEBUG,
        api_ui::debug_ui,
        "beta"
    );
    app = route_get_tag!(
        app,
        endpoints_acc,
        endpoints_meta_acc,
        paths::ADMIN_UI_MODELS,
        api_ui::models_ui,
        "beta"
    );
    app = route_get_tag!(
        app,
        endpoints_acc,
        endpoints_meta_acc,
        paths::ADMIN_UI_AGENTS,
        api_ui::agents_ui,
        "beta"
    );
    app = route_get_tag!(
        app,
        endpoints_acc,
        endpoints_meta_acc,
        paths::ADMIN_UI_PROJECTS,
        api_ui::projects_ui,
        "beta"
    );
    app = route_get_tag!(
        app,
        endpoints_acc,
        endpoints_meta_acc,
        paths::ADMIN_UI_FLOWS,
        api_ui::flows_ui,
        "beta"
    );
    app = route_get_tag!(
        app,
        endpoints_acc,
        endpoints_meta_acc,
        paths::ADMIN_UI_TOKENS,
        api_ui::ui_tokens_css,
        "beta"
    );
    app = route_get_tag!(
        app,
        endpoints_acc,
        endpoints_meta_acc,
        paths::ADMIN_UI_KIT,
        api_ui::ui_kit_css,
        "beta"
    );
    app = route_get_tag!(
        app,
        endpoints_acc,
        endpoints_meta_acc,
        paths::ADMIN_MODELS_SUMMARY,
        api_models::models_summary,
        "beta"
    );
    app = route_get_tag!(
        app,
        endpoints_acc,
        endpoints_meta_acc,
        paths::ADMIN_MODELS,
        api_models::models_list,
        "beta"
    );
    app = route_post_tag!(
        app,
        endpoints_acc,
        endpoints_meta_acc,
        paths::ADMIN_MODELS_REFRESH,
        api_models::models_refresh,
        "beta"
    );
    app = route_post_tag!(
        app,
        endpoints_acc,
        endpoints_meta_acc,
        paths::ADMIN_MODELS_SAVE,
        api_models::models_save,
        "beta"
    );
    app = route_post_tag!(
        app,
        endpoints_acc,
        endpoints_meta_acc,
        paths::ADMIN_MODELS_LOAD,
        api_models::models_load,
        "beta"
    );
    app = route_post_tag!(
        app,
        endpoints_acc,
        endpoints_meta_acc,
        paths::ADMIN_MODELS_ADD,
        api_models::models_add,
        "beta"
    );
    app = route_post_tag!(
        app,
        endpoints_acc,
        endpoints_meta_acc,
        paths::ADMIN_MODELS_REMOVE,
        api_models::models_remove,
        "beta"
    );
    app = route_get_tag!(
        app,
        endpoints_acc,
        endpoints_meta_acc,
        paths::ADMIN_MODELS_DEFAULT,
        api_models::models_default_get,
        "beta"
    );
    app = route_post_tag!(
        app,
        endpoints_acc,
        endpoints_meta_acc,
        paths::ADMIN_MODELS_DEFAULT,
        api_models::models_default_set,
        "beta"
    );
    app = route_get_tag!(
        app,
        endpoints_acc,
        endpoints_meta_acc,
        paths::ADMIN_MODELS_CONCURRENCY,
        api_models::models_concurrency_get,
        "beta"
    );
    app = route_post_tag!(
        app,
        endpoints_acc,
        endpoints_meta_acc,
        paths::ADMIN_MODELS_CONCURRENCY,
        api_models::models_concurrency_set,
        "beta"
    );
    app = route_get_tag!(
        app,
        endpoints_acc,
        endpoints_meta_acc,
        paths::ADMIN_MODELS_JOBS,
        api_models::models_jobs,
        "beta"
    );
    app = route_get_tag!(
        app,
        endpoints_acc,
        endpoints_meta_acc,
        paths::ADMIN_STATE_MODELS_METRICS,
        api_models::models_metrics,
        "beta"
    );
    app = route_get_tag!(
        app,
        endpoints_acc,
        endpoints_meta_acc,
        paths::ADMIN_STATE_MODELS_HASHES,
        api_models::models_hashes,
        "beta"
    );
    app = route_get_tag!(
        app,
        endpoints_acc,
        endpoints_meta_acc,
        paths::ADMIN_TOOLS,
        api_tools::tools_list,
        "beta"
    );
    app = route_post_tag!(
        app,
        endpoints_acc,
        endpoints_meta_acc,
        paths::ADMIN_TOOLS_RUN,
        api_tools::tools_run,
        "beta"
    );
    app = route_get_tag!(
        app,
        endpoints_acc,
        endpoints_meta_acc,
        paths::ADMIN_TOOLS_CACHE_STATS,
        api_tools::tools_cache_stats,
        "beta"
    );
    app = route_get_tag!(
        app,
        endpoints_acc,
        endpoints_meta_acc,
        paths::ADMIN_STATE_MEMORY_QUARANTINE,
        api_review::memory_quarantine_get,
        "experimental"
    );
    app = route_post_tag!(
        app,
        endpoints_acc,
        endpoints_meta_acc,
        paths::ADMIN_MEMORY_QUARANTINE,
        api_review::memory_quarantine_queue,
        "experimental"
    );
    app = route_post_tag!(
        app,
        endpoints_acc,
        endpoints_meta_acc,
        paths::ADMIN_MEMORY_QUARANTINE_ADMIT,
        api_review::memory_quarantine_admit,
        "experimental"
    );
    app = route_get_tag!(
        app,
        endpoints_acc,
        endpoints_meta_acc,
        paths::ADMIN_STATE_WORLD_DIFFS,
        api_review::world_diffs_get,
        "experimental"
    );
    app = route_post_tag!(
        app,
        endpoints_acc,
        endpoints_meta_acc,
        paths::ADMIN_WORLD_DIFFS_QUEUE,
        api_review::world_diffs_queue,
        "experimental"
    );
    app = route_post_tag!(
        app,
        endpoints_acc,
        endpoints_meta_acc,
        paths::ADMIN_WORLD_DIFFS_DECISION,
        api_review::world_diffs_decision,
        "experimental"
    );
    app = route_get_tag!(
        app,
        endpoints_acc,
        endpoints_meta_acc,
        paths::ADMIN_GOVERNOR_PROFILE,
        api_governor::governor_profile_get,
        "experimental"
    );
    app = route_post_tag!(
        app,
        endpoints_acc,
        endpoints_meta_acc,
        paths::ADMIN_GOVERNOR_PROFILE,
        api_governor::governor_profile_set,
        "experimental"
    );
    app = route_get_tag!(
        app,
        endpoints_acc,
        endpoints_meta_acc,
        paths::ADMIN_GOVERNOR_HINTS,
        api_governor::governor_hints_get,
        "experimental"
    );
    app = route_post_tag!(
        app,
        endpoints_acc,
        endpoints_meta_acc,
        paths::ADMIN_GOVERNOR_HINTS,
        api_governor::governor_hints_set,
        "experimental"
    );
    app = route_get_tag!(
        app,
        endpoints_acc,
        endpoints_meta_acc,
        paths::ADMIN_FEEDBACK_STATE,
        api_feedback::feedback_state,
        "experimental"
    );
    app = route_post_tag!(
        app,
        endpoints_acc,
        endpoints_meta_acc,
        paths::ADMIN_FEEDBACK_SIGNAL,
        api_feedback::feedback_signal,
        "experimental"
    );
    app = route_post_tag!(
        app,
        endpoints_acc,
        endpoints_meta_acc,
        paths::ADMIN_FEEDBACK_ANALYZE,
        api_feedback::feedback_analyze,
        "experimental"
    );
    app = route_post_tag!(
        app,
        endpoints_acc,
        endpoints_meta_acc,
        paths::ADMIN_FEEDBACK_APPLY,
        api_feedback::feedback_apply,
        "experimental"
    );
    app = route_post_tag!(
        app,
        endpoints_acc,
        endpoints_meta_acc,
        paths::ADMIN_FEEDBACK_AUTO,
        api_feedback::feedback_auto,
        "experimental"
    );
    app = route_post_tag!(
        app,
        endpoints_acc,
        endpoints_meta_acc,
        paths::ADMIN_FEEDBACK_RESET,
        api_feedback::feedback_reset,
        "experimental"
    );
    app = route_post_tag!(
        app,
        endpoints_acc,
        endpoints_meta_acc,
        paths::ADMIN_DISTILL,
        api_distill::distill_run,
        "experimental"
    );
    app = route_get_tag!(
        app,
        endpoints_acc,
        endpoints_meta_acc,
        paths::ADMIN_FEEDBACK_SUGGESTIONS,
        api_feedback::feedback_suggestions,
        "experimental"
    );
    app = route_get_tag!(
        app,
        endpoints_acc,
        endpoints_meta_acc,
        paths::ADMIN_FEEDBACK_UPDATES,
        api_feedback::feedback_updates,
        "experimental"
    );
    app = route_get_tag!(
        app,
        endpoints_acc,
        endpoints_meta_acc,
        paths::ADMIN_FEEDBACK_POLICY,
        api_feedback::feedback_policy,
        "experimental"
    );
    app = route_get_tag!(
        app,
        endpoints_acc,
        endpoints_meta_acc,
        paths::ADMIN_FEEDBACK_VERSIONS,
        api_feedback::feedback_versions,
        "experimental"
    );
    app = route_post_tag!(
        app,
        endpoints_acc,
        endpoints_meta_acc,
        paths::ADMIN_FEEDBACK_ROLLBACK,
        api_feedback::feedback_rollback,
        "experimental"
    );
    app = route_post_tag!(
        app,
        endpoints_acc,
        endpoints_meta_acc,
        paths::ADMIN_EXPERIMENTS_DEFINE,
        api_experiments::experiments_define,
        "experimental"
    );
    app = route_post_tag!(
        app,
        endpoints_acc,
        endpoints_meta_acc,
        paths::ADMIN_EXPERIMENTS_RUN,
        api_experiments::experiments_run,
        "experimental"
    );
    app = route_post_tag!(
        app,
        endpoints_acc,
        endpoints_meta_acc,
        paths::ADMIN_EXPERIMENTS_ACTIVATE,
        api_experiments::experiments_activate,
        "experimental"
    );
    app = route_get_tag!(
        app,
        endpoints_acc,
        endpoints_meta_acc,
        paths::ADMIN_EXPERIMENTS_LIST,
        api_experiments::experiments_list,
        "experimental"
    );
    app = route_get_tag!(
        app,
        endpoints_acc,
        endpoints_meta_acc,
        paths::ADMIN_EXPERIMENTS_SCOREBOARD,
        api_experiments::experiments_scoreboard,
        "experimental"
    );
    app = route_get_tag!(
        app,
        endpoints_acc,
        endpoints_meta_acc,
        paths::ADMIN_EXPERIMENTS_WINNERS,
        api_experiments::experiments_winners,
        "experimental"
    );
    app = route_post_tag!(
        app,
        endpoints_acc,
        endpoints_meta_acc,
        paths::ADMIN_EXPERIMENTS_START,
        api_experiments::experiments_start,
        "experimental"
    );
    app = route_post_tag!(
        app,
        endpoints_acc,
        endpoints_meta_acc,
        paths::ADMIN_EXPERIMENTS_STOP,
        api_experiments::experiments_stop,
        "experimental"
    );
    app = route_post_tag!(
        app,
        endpoints_acc,
        endpoints_meta_acc,
        paths::ADMIN_EXPERIMENTS_ASSIGN,
        api_experiments::experiments_assign,
        "experimental"
    );
    app = route_get_tag!(
        app,
        endpoints_acc,
        endpoints_meta_acc,
        paths::ADMIN_GOLDENS_LIST,
        api_goldens::goldens_list,
        "experimental"
    );
    app = route_post_tag!(
        app,
        endpoints_acc,
        endpoints_meta_acc,
        paths::ADMIN_GOLDENS_ADD,
        api_goldens::goldens_add,
        "experimental"
    );
    app = route_post_tag!(
        app,
        endpoints_acc,
        endpoints_meta_acc,
        paths::ADMIN_GOLDENS_RUN,
        api_goldens::goldens_run,
        "experimental"
    );
    app = route_get_tag!(
        app,
        endpoints_acc,
        endpoints_meta_acc,
        paths::ADMIN_HIERARCHY_STATE,
        api_hierarchy::hierarchy_state,
        "experimental"
    );
    app = route_post_tag!(
        app,
        endpoints_acc,
        endpoints_meta_acc,
        paths::ADMIN_HIERARCHY_ROLE,
        api_hierarchy::hierarchy_role_set,
        "experimental"
    );
    app = route_post_tag!(
        app,
        endpoints_acc,
        endpoints_meta_acc,
        paths::ADMIN_HIERARCHY_HELLO,
        api_hierarchy::hierarchy_hello,
        "experimental"
    );
    app = route_post_tag!(
        app,
        endpoints_acc,
        endpoints_meta_acc,
        paths::ADMIN_HIERARCHY_OFFER,
        api_hierarchy::hierarchy_offer,
        "experimental"
    );
    app = route_post_tag!(
        app,
        endpoints_acc,
        endpoints_meta_acc,
        paths::ADMIN_HIERARCHY_ACCEPT,
        api_hierarchy::hierarchy_accept,
        "experimental"
    );
    app = route_get_tag!(
        app,
        endpoints_acc,
        endpoints_meta_acc,
        paths::ADMIN_PROBE,
        api_probe::probe_effective_paths,
        "experimental"
    );
    app = route_get_tag!(
        app,
        endpoints_acc,
        endpoints_meta_acc,
        paths::ADMIN_PROBE_HW,
        api_probe::probe_hw,
        "experimental"
    );
    app = route_get_tag!(
        app,
        endpoints_acc,
        endpoints_meta_acc,
        paths::ADMIN_PROBE_METRICS,
        api_probe::probe_metrics,
        "experimental"
    );
    app = route_get_tag!(
        app,
        endpoints_acc,
        endpoints_meta_acc,
        paths::ADMIN_INTROSPECT_STATS,
        api_metrics::metrics_overview,
        "legacy"
    );
    app = route_post_tag!(
        app,
        endpoints_acc,
        endpoints_meta_acc,
        paths::ADMIN_MODELS_DOWNLOAD,
        api_models::models_download,
        "experimental"
    );
    app = route_post_tag!(
        app,
        endpoints_acc,
        endpoints_meta_acc,
        paths::ADMIN_MODELS_DOWNLOAD_CANCEL,
        api_models::models_download_cancel,
        "experimental"
    );
    app = route_post_tag!(
        app,
        endpoints_acc,
        endpoints_meta_acc,
        paths::ADMIN_MODELS_CAS_GC,
        api_models::models_cas_gc,
        "experimental"
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
        paths::ADMIN_STATE_ROUTE_STATS,
        api_state::state_route_stats_admin,
        "legacy"
    );
    app = route_get_tag!(
        app,
        endpoints_acc,
        endpoints_meta_acc,
        paths::ADMIN_STATE_OBSERVATIONS,
        api_state::admin_state_observations,
        "legacy"
    );
    app = route_get_tag!(
        app,
        endpoints_acc,
        endpoints_meta_acc,
        paths::ADMIN_STATE_BELIEFS,
        api_state::admin_state_beliefs,
        "legacy"
    );
    app = route_get_tag!(
        app,
        endpoints_acc,
        endpoints_meta_acc,
        paths::ADMIN_STATE_INTENTS,
        api_state::admin_state_intents,
        "legacy"
    );
    app = route_get_tag!(
        app,
        endpoints_acc,
        endpoints_meta_acc,
        paths::ADMIN_STATE_ACTIONS,
        api_state::admin_state_actions,
        "legacy"
    );
    app = route_get_tag!(
        app,
        endpoints_acc,
        endpoints_meta_acc,
        paths::ADMIN_STATE_GUARDRAILS_METRICS,
        api_state::admin_state_guardrails_metrics,
        "beta"
    );
    app = route_get_tag!(
        app,
        endpoints_acc,
        endpoints_meta_acc,
        paths::ADMIN_STATE_CLUSTER,
        api_state::admin_state_cluster,
        "legacy"
    );
    app = route_get_tag!(
        app,
        endpoints_acc,
        endpoints_meta_acc,
        paths::ADMIN_STATE_WORLD,
        api_state::admin_state_world,
        "legacy"
    );
    app = route_get_tag!(
        app,
        endpoints_acc,
        endpoints_meta_acc,
        paths::ADMIN_STATE_WORLD_SELECT,
        api_state::admin_state_world_select,
        "legacy"
    );
    app = route_get_tag!(
        app,
        endpoints_acc,
        endpoints_meta_acc,
        paths::ADMIN_PROJECTS_LIST,
        api_projects::projects_list,
        "legacy"
    );
    app = route_post_tag!(
        app,
        endpoints_acc,
        endpoints_meta_acc,
        paths::ADMIN_PROJECTS_CREATE,
        api_projects::projects_create,
        "legacy"
    );
    app = route_get_tag!(
        app,
        endpoints_acc,
        endpoints_meta_acc,
        paths::ADMIN_PROJECTS_TREE,
        api_projects::projects_tree,
        "legacy"
    );
    app = route_get_tag!(
        app,
        endpoints_acc,
        endpoints_meta_acc,
        paths::ADMIN_PROJECTS_NOTES,
        api_projects::projects_notes_get,
        "legacy"
    );
    app = route_post_tag!(
        app,
        endpoints_acc,
        endpoints_meta_acc,
        paths::ADMIN_PROJECTS_NOTES,
        api_projects::projects_notes_set,
        "legacy"
    );
    app = route_get_tag!(
        app,
        endpoints_acc,
        endpoints_meta_acc,
        paths::ADMIN_PROJECTS_FILE,
        api_projects::projects_file_get,
        "legacy"
    );
    app = route_post_tag!(
        app,
        endpoints_acc,
        endpoints_meta_acc,
        paths::ADMIN_PROJECTS_FILE,
        api_projects::projects_file_set,
        "legacy"
    );
    app = route_post_tag!(
        app,
        endpoints_acc,
        endpoints_meta_acc,
        paths::ADMIN_PROJECTS_PATCH,
        api_projects::projects_file_patch,
        "legacy"
    );
    app = route_post_tag!(
        app,
        endpoints_acc,
        endpoints_meta_acc,
        paths::ADMIN_PROJECTS_IMPORT,
        api_projects::projects_import,
        "legacy"
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
    app = route_get_tag!(
        app,
        endpoints_acc,
        endpoints_meta_acc,
        paths::STATE_RESEARCH_WATCHER,
        api_state::state_research_watcher,
        "experimental"
    );
    app = route_get_tag!(
        app,
        endpoints_acc,
        endpoints_meta_acc,
        paths::STATE_STAGING_ACTIONS,
        api_state::state_staging_actions,
        "experimental"
    );
    app = route_get_tag!(
        app,
        endpoints_acc,
        endpoints_meta_acc,
        paths::STATE_TRAINING_TELEMETRY,
        api_state::state_training_telemetry,
        "experimental"
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
    app = route_get_tag!(
        app,
        endpoints_acc,
        endpoints_meta_acc,
        paths::STATE_POLICY_CAPSULES,
        api_state::state_policy_capsules,
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
    // Generated OpenAPI (experimental)
    app = route_get_tag!(
        app,
        endpoints_acc,
        endpoints_meta_acc,
        "/spec/openapi.gen.yaml",
        api_spec::spec_openapi_gen,
        "experimental"
    );
    app = route_get_tag!(
        app,
        endpoints_acc,
        endpoints_meta_acc,
        paths::CATALOG_INDEX,
        api_spec::catalog_index,
        "stable"
    );
    app = route_get_tag!(
        app,
        endpoints_acc,
        endpoints_meta_acc,
        paths::CATALOG_HEALTH,
        api_spec::catalog_health,
        "stable"
    );
    // Admin: RPU trust endpoints (admin token required)
    app = route_get_tag!(
        app,
        endpoints_acc,
        endpoints_meta_acc,
        paths::ADMIN_RPU_TRUST,
        api_rpu::rpu_trust_get,
        "experimental"
    );
    app = route_post_tag!(
        app,
        endpoints_acc,
        endpoints_meta_acc,
        paths::ADMIN_RPU_RELOAD,
        api_rpu::rpu_reload_post,
        "experimental"
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
    app = route_get_tag!(
        app,
        endpoints_acc,
        endpoints_meta_acc,
        paths::STATE_RUNTIME_MATRIX,
        api_state::state_runtime_matrix,
        "beta"
    );
    app = route_get_tag!(
        app,
        endpoints_acc,
        endpoints_meta_acc,
        paths::STATE_EXPERIMENTS,
        api_state::state_experiments,
        "beta"
    );
    app = route_get_tag!(
        app,
        endpoints_acc,
        endpoints_meta_acc,
        paths::STATE_SELF,
        api_state::state_self_list,
        "beta"
    );
    app = route_get_tag!(
        app,
        endpoints_acc,
        endpoints_meta_acc,
        paths::STATE_SELF_AGENT,
        api_state::state_self_get,
        "beta"
    );
    app = route_post_tag!(
        app,
        endpoints_acc,
        endpoints_meta_acc,
        paths::ADMIN_SELF_MODEL_PROPOSE,
        api_self_model::self_model_propose,
        "beta"
    );
    app = route_post_tag!(
        app,
        endpoints_acc,
        endpoints_meta_acc,
        paths::ADMIN_SELF_MODEL_APPLY,
        api_self_model::self_model_apply,
        "beta"
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
    app = route_post_tag!(
        app,
        endpoints_acc,
        endpoints_meta_acc,
        paths::RESEARCH_WATCHER_APPROVE,
        api_research_watcher::research_watcher_approve,
        "experimental"
    );
    app = route_post_tag!(
        app,
        endpoints_acc,
        endpoints_meta_acc,
        paths::RESEARCH_WATCHER_ARCHIVE,
        api_research_watcher::research_watcher_archive,
        "experimental"
    );
    app = route_post_tag!(
        app,
        endpoints_acc,
        endpoints_meta_acc,
        paths::STAGING_ACTION_APPROVE,
        api_staging::staging_action_approve,
        "experimental"
    );
    app = route_post_tag!(
        app,
        endpoints_acc,
        endpoints_meta_acc,
        paths::STAGING_ACTION_DENY,
        api_staging::staging_action_deny,
        "experimental"
    );
    let cluster_state = cluster::ClusterRegistry::new(bus.clone());
    let feedback_hub =
        feedback::FeedbackHub::new(bus.clone(), metrics.clone(), governor_state.clone()).await;
    let experiments_state =
        experiments::Experiments::new(bus.clone(), governor_state.clone()).await;
    let capsules_store = std::sync::Arc::new(capsule_guard::CapsuleStore::new());
    let state = AppState {
        bus,
        kernel,
        policy: policy_arc.clone(),
        host,
        config_state: std::sync::Arc::new(Mutex::new(json!({}))),
        config_history: std::sync::Arc::new(Mutex::new(Vec::new())),
        sse_id_map,
        endpoints: std::sync::Arc::new(endpoints_acc),
        endpoints_meta: std::sync::Arc::new(endpoints_meta_acc),
        metrics: metrics.clone(),
        kernel_enabled,
        models: models_store.clone(),
        tool_cache: tool_cache.clone(),
        governor: governor_state.clone(),
        feedback: feedback_hub.clone(),
        cluster: cluster_state.clone(),
        experiments: experiments_state.clone(),
        capsules: capsules_store.clone(),
    };
    read_models::publish_read_model_patch(
        &state.bus(),
        "policy_capsules",
        &json!({"items": [], "count": 0}),
    );
    world::load_persisted().await;
    // Start a simple local action worker (demo)
    if state.kernel_enabled() {
        worker::start_local_worker(state.clone());
    }
    // Start read-model publishers (logic units, orchestrator jobs)
    read_models::start_read_models(state.clone());
    cluster::start(state.clone());
    runtime_matrix::start(state.clone());
    state_observer::start(state.clone());
    world::start(state.clone());
    distill::start(state.clone());
    self_model::start_aggregators(state.clone());
    research_watcher::start(state.clone());
    // Start/stop egress proxy based on current settings
    egress_proxy::apply_current(state.clone()).await;
    // Watch trust store file and publish rpu.trust.changed on reloads
    {
        let bus = state.bus.clone();
        tokio::spawn(async move {
            use std::time::Duration;
            let path = std::env::var("ARW_TRUST_CAPSULES")
                .ok()
                .unwrap_or_else(|| "configs/trust_capsules.json".to_string());
            let mut last_mtime: Option<std::time::SystemTime> = None;
            loop {
                let mut changed = false;
                if let Ok(md) = std::fs::metadata(&path) {
                    if let Ok(mt) = md.modified() {
                        if last_mtime.map(|t| t < mt).unwrap_or(true) {
                            last_mtime = Some(mt);
                            changed = true;
                        }
                    }
                }
                if changed {
                    arw_core::rpu::reload_trust();
                    let count = arw_core::rpu::trust_snapshot().len();
                    let payload = serde_json::json!({
                        "count": count,
                        "path": path,
                        "ts_ms": arw_core::rpu::trust_last_reload_ms()
                    });
                    bus.publish(arw_topics::TOPIC_RPU_TRUST_CHANGED, &payload);
                }
                tokio::time::sleep(Duration::from_secs(5)).await;
            }
        });
    }
    #[cfg(feature = "grpc")]
    let _grpc_task = crate::grpc::spawn(state.clone());
    let capsule_mw_state = state.clone();
    let app = app.with_state(state);
    let app = app.layer(axum::middleware::from_fn(move |req, next| {
        let st = capsule_mw_state.clone();
        async move { capsule_guard::capsule_mw(st, req, next).await }
    }));
    let metrics_layer = metrics.clone();
    let app = app.layer(axum::middleware::from_fn(move |req, next| {
        let metrics = metrics_layer.clone();
        async move { metrics::track_http(metrics, req, next).await }
    }));
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
    // Security: refuse public bind without an admin token
    let token_set = std::env::var("ARW_ADMIN_TOKEN")
        .ok()
        .is_some_and(|v| !v.is_empty())
        || std::env::var("ARW_ADMIN_TOKEN_SHA256")
            .ok()
            .is_some_and(|v| !v.is_empty());
    let is_loopback = {
        let b = bind.trim().to_ascii_lowercase();
        b == "127.0.0.1" || b == "::1" || b == "[::1]" || b == "localhost"
    };
    if !is_loopback && !token_set {
        eprintln!(
            "error: ARW_BIND={} is public and ARW_ADMIN_TOKEN/ARW_ADMIN_TOKEN_SHA256 not set; refusing to start",
            bind
        );
        std::process::exit(2);
    }
    let addr: SocketAddr = format!("{}:{}", bind, port).parse().unwrap();
    // Global middleware: security headers, optional access log, then app
    let app = app
        .layer(axum::middleware::from_fn(security::headers_mw))
        .layer(axum::middleware::from_fn(access_log::access_log_mw));
    axum::serve(
        tokio::net::TcpListener::bind(addr).await.unwrap(),
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .await
    .unwrap();
}

#[cfg(test)]
#[allow(clippy::items_after_test_module)]
mod http_tests {
    use super::*;
    use axum::{
        body::Body,
        http::{Request, StatusCode},
        routing::{get, post},
        Router,
    };
    use http_body_util::BodyExt;
    use serde_json::{json, Value};
    use std::{path::Path, sync::Arc, time::Duration};
    use tempfile::tempdir;
    use tower::util::ServiceExt;

    async fn build_state(dir: &Path) -> AppState {
        std::env::set_var("ARW_STATE_DIR", dir.display().to_string());
        let bus = arw_events::Bus::new_with_replay(64, 64);
        let kernel = arw_kernel::Kernel::open(dir).expect("init kernel for tests");
        let policy = PolicyEngine::load_from_env();
        let policy_arc = Arc::new(Mutex::new(policy));
        let host: Arc<dyn ToolHost> = Arc::new(arw_wasi::NoopHost);
        let models_store = Arc::new(models::ModelStore::new(bus.clone(), Some(kernel.clone())));
        models_store.bootstrap().await;
        let tool_cache = Arc::new(tool_cache::ToolCache::new());
        let governor_state = governor::GovernorState::new().await;
        let metrics = Arc::new(metrics::Metrics::default());
        let cluster_state = cluster::ClusterRegistry::new(bus.clone());
        let feedback_hub =
            feedback::FeedbackHub::new(bus.clone(), metrics.clone(), governor_state.clone()).await;
        let experiments_state =
            experiments::Experiments::new(bus.clone(), governor_state.clone()).await;
        let capsules_store = Arc::new(capsule_guard::CapsuleStore::new());
        AppState {
            bus,
            kernel,
            policy: policy_arc,
            host,
            config_state: Arc::new(Mutex::new(json!({}))),
            config_history: Arc::new(Mutex::new(Vec::new())),
            sse_id_map: Arc::new(Mutex::new(sse_cache::SseIdCache::with_capacity(64))),
            endpoints: Arc::new(Vec::new()),
            endpoints_meta: Arc::new(Vec::new()),
            metrics,
            kernel_enabled: true,
            models: models_store,
            tool_cache,
            governor: governor_state,
            feedback: feedback_hub,
            cluster: cluster_state,
            experiments: experiments_state,
            capsules: capsules_store,
        }
    }

    fn router_with_actions(state: AppState) -> Router {
        Router::new()
            .route(paths::ACTIONS, post(api_actions::actions_submit))
            .route(paths::ACTIONS_ID, get(api_actions::actions_get))
            .with_state(state)
    }

    #[tokio::test]
    async fn http_action_roundtrip_completes() {
        let temp = tempdir().expect("tempdir");
        let state_dir = temp.path().to_path_buf();

        let state = build_state(&state_dir).await;
        worker::start_local_worker(state.clone());
        let app = router_with_actions(state);

        let submit_body = json!({
            "kind": "demo.echo",
            "input": { "msg": "hello-roundtrip" }
        });
        let submit_req = Request::builder()
            .method("POST")
            .uri(paths::ACTIONS)
            .header(axum::http::header::CONTENT_TYPE, "application/json")
            .body(Body::from(submit_body.to_string()))
            .expect("submit request");
        let submit_resp = app
            .clone()
            .oneshot(submit_req)
            .await
            .expect("submit response");
        assert_eq!(submit_resp.status(), StatusCode::ACCEPTED);
        let submit_bytes = submit_resp
            .into_body()
            .collect()
            .await
            .expect("submit body collect")
            .to_bytes();
        let submit_json: Value = serde_json::from_slice(&submit_bytes).expect("submit body json");
        let action_id = submit_json["id"].as_str().expect("action id").to_string();

        let mut completed: Option<Value> = None;
        for _ in 0..30 {
            let get_req = Request::builder()
                .method("GET")
                .uri(format!("{}/{}", paths::ACTIONS, action_id))
                .body(Body::empty())
                .expect("get request");
            let get_resp = app.clone().oneshot(get_req).await.expect("get response");
            assert_eq!(get_resp.status(), StatusCode::OK);
            let body_bytes = get_resp
                .into_body()
                .collect()
                .await
                .expect("get body collect")
                .to_bytes();
            let payload: Value = serde_json::from_slice(&body_bytes).expect("get body json");
            if payload["state"] == "completed" {
                completed = Some(payload);
                break;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }

        let payload = completed.expect("action completed");
        assert_eq!(payload["state"], "completed");
        assert_eq!(payload["output"]["echo"]["msg"], json!("hello-roundtrip"));
    }
}

pub(crate) fn admin_ok(headers: &HeaderMap) -> bool {
    // When ARW_ADMIN_TOKEN or ARW_ADMIN_TOKEN_SHA256 is set, require it in Authorization: Bearer or X-ARW-Admin
    let token_plain = std::env::var("ARW_ADMIN_TOKEN")
        .ok()
        .filter(|t| !t.is_empty());
    let token_hash = std::env::var("ARW_ADMIN_TOKEN_SHA256")
        .ok()
        .filter(|t| !t.is_empty());
    if token_plain.is_none() && token_hash.is_none() {
        return true;
    }
    // Extract presented token
    let mut presented: Option<String> = None;
    if let Some(hv) = headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|h| h.to_str().ok())
    {
        if let Some(bearer) = hv.strip_prefix("Bearer ") {
            presented = Some(bearer.to_string());
        }
    }
    if presented.is_none() {
        if let Some(hv) = headers.get("X-ARW-Admin").and_then(|h| h.to_str().ok()) {
            presented = Some(hv.to_string());
        }
    }
    let Some(ptok) = presented else { return false };
    // Constant-time eq helper
    fn ct_eq(a: &[u8], b: &[u8]) -> bool {
        if a.len() != b.len() {
            return false;
        }
        let mut diff: u8 = 0;
        for i in 0..a.len() {
            diff |= a[i] ^ b[i];
        }
        diff == 0
    }
    if let Some(ref hpref) = token_hash {
        let want = hpref.trim().to_ascii_lowercase();
        let got_hex = {
            let mut hasher = sha2::Sha256::new();
            hasher.update(ptok.as_bytes());
            let digest = hasher.finalize();
            hex::encode(digest)
        };
        return ct_eq(want.as_bytes(), got_hex.as_bytes())
            || token_plain
                .as_ref()
                .map(|p| ct_eq(p.as_bytes(), ptok.as_bytes()))
                .unwrap_or(false);
    }
    if let Some(ref p) = token_plain {
        return ct_eq(p.as_bytes(), ptok.as_bytes());
    }
    false
}

// ---------- Config Plane (moved to api_config) ----------
// moved to api_memory
// moved to api_config
