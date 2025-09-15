use arw_macros::arw_tool;
use arw_macros::{arw_admin, arw_gate};
pub use arw_svc::resources;
use arw_svc::resources::governor_service::GovernorService;
use arw_svc::resources::hierarchy_service::HierarchyService;
use arw_svc::resources::memory_service::MemoryService;
use arw_svc::resources::models_service::ModelsService;
use arw_svc::resources::Resources;
use axum::extract::ConnectInfo;
use axum::extract::Extension;
use axum::extract::Query;
use axum::http::HeaderMap;
use axum::http::StatusCode;
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::{
    extract::{Path, State},
    response::IntoResponse,
    routing::get,
    Json, Router,
};
use axum::{
    http::Request,
    middleware::{self, Next},
    response::Response,
};
use futures_util::StreamExt as _; // for flat_map on streams
use serde_json::json;
use std::convert::Infallible;
use std::net::SocketAddr;
use std::path::Path as FsPath;
use std::sync::OnceLock;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio_stream::wrappers::errors::BroadcastStreamRecvError;
use tokio_stream::wrappers::BroadcastStream;
use tower_http::compression::CompressionLayer;
use tower_http::cors::{Any, CorsLayer};
use tower_http::limit::RequestBodyLimitLayer;
use tower_http::services::ServeDir;
// use tower_http::timeout::TimeoutLayer; // replaced with dynamic timeout layer
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::RwLock as StdRwLock;
use tower_http::trace::TraceLayer;
use tracing::info;
use utoipa::{OpenApi, ToSchema};
mod dyn_timeout;
mod ext;
mod route_recorder;
use arw_core::gating;
use arw_svc::state_bridge;
#[cfg(feature = "grpc")]
mod grpc;

// Optional Windows DXCore interop for NPU detection (opt-in)
#[cfg(all(target_os = "windows", feature = "npu_dxcore"))]
mod win_npu_dxcore {
    #![allow(non_snake_case, non_camel_case_types, non_upper_case_globals)]
    use serde_json::json;
    use windows::Win32::Graphics::DxCore::*;

    pub fn probe() -> Vec<serde_json::Value> {
        unsafe {
            let mut out: Vec<serde_json::Value> = Vec::new();
            let Ok(factory) = CreateDXCoreAdapterFactory() else {
                return out;
            };
            let attrs = [DXCORE_ADAPTER_ATTRIBUTE_D3D12_CORE_COMPUTE];
            let Ok(list) = factory.CreateAdapterList(&attrs) else {
                return out;
            };
            let count = list.GetAdapterCount();
            for i in 0..count {
                if let Ok(adapter) = list.GetAdapter(i) {
                    if adapter.IsAttributeSupported(&DXCORE_ADAPTER_ATTRIBUTE_D3D12_CORE_COMPUTE) {
                        // vendor/device ids when available
                        let mut ven = 0u32;
                        let mut dev = 0u32;
                        let mut sz: usize = 0;
                        if adapter.IsPropertySupported(DXCoreAdapterProperty::HardwareID) {
                            if adapter
                                .GetPropertySize(DXCoreAdapterProperty::HardwareID, &mut sz)
                                .is_ok()
                                && sz >= core::mem::size_of::<DXCoreHardwareID>()
                            {
                                let mut hwid: DXCoreHardwareID = core::mem::zeroed();
                                if adapter
                                    .GetProperty(
                                        DXCoreAdapterProperty::HardwareID,
                                        &mut hwid as *mut _ as *mut core::ffi::c_void,
                                        core::mem::size_of::<DXCoreHardwareID>(),
                                    )
                                    .is_ok()
                                {
                                    ven = hwid.VendorID;
                                    dev = hwid.DeviceID;
                                }
                            }
                        }
                        let vendor_hex = format!("0x{:04x}", ven);
                        // description is optional; omit to keep code small/safe
                        let is_amd = ven == 0x1002;
                        out.push(json!({
                            "vendor_id": vendor_hex,
                            "device_id": format!("0x{:04x}", dev),
                            "dxcore": true,
                            "is_amd": is_amd,
                        }));
                    }
                }
            }
            out
        }
    }
}

#[arw_tool(
    id = "introspect.tools",
    version = "1.0.0",
    summary = "List available tools with metadata",
    stability = "experimental",
    capabilities("read-only")
)]
fn _register_introspect_tools() {}

#[arw_tool(
    id = "memory.probe",
    version = "1.0.0",
    summary = "Read-only memory probe (shows applied memories and paths)",
    stability = "experimental",
    capabilities("read-only")
)]
fn _register_memory_probe() {}

#[arw_tool(
    id = "feedback.evaluate",
    version = "1.0.0",
    summary = "Run heuristic evaluation and emit suggestions",
    stability = "experimental",
    capabilities("read-only")
)]
fn _register_feedback_evaluate() {}

#[arw_tool(
    id = "feedback.apply",
    version = "1.0.0",
    summary = "Apply a suggestion by id (policy-gated)",
    stability = "experimental",
    capabilities("admin")
)]
fn _register_feedback_apply() {}

pub(crate) use arw_svc::app_state::AppState;

#[derive(serde::Serialize, ToSchema)]
struct OkResponse {
    ok: bool,
}

#[derive(serde::Serialize, ToSchema)]
struct About {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    tagline: Option<String>,
    #[serde(default)]
    description: Option<String>,
    service: String,
    version: String,
    #[serde(default)]
    role: Option<String>,
    #[serde(default)]
    docs_url: Option<String>,
    endpoints: Vec<String>,
}

#[tokio::main]
async fn main() {
    arw_otel::init();

    // legacy/dual event kind modes removed; bus publishes normalized kinds

    if let Ok(path) = std::env::var("OPENAPI_OUT") {
        let doc = ApiDoc::openapi();
        if let Some(parent) = std::path::Path::new(&path).parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        std::fs::write(&path, doc.to_yaml().unwrap()).expect("write openapi spec");
        // Also emit gating schemas and keys listing for docs
        {
            use schemars::schema_for;
            let dir = std::path::Path::new("spec/schemas");
            let _ = std::fs::create_dir_all(dir);
            let contract_schema = schema_for!(arw_core::gating::ContractCfg);
            let capsule_schema = schema_for!(arw_protocol::GatingCapsule);
            std::fs::write(
                dir.join("gating_contract.json"),
                serde_json::to_string_pretty(&contract_schema).unwrap(),
            )
            .ok();
            std::fs::write(
                dir.join("gating_capsule.json"),
                serde_json::to_string_pretty(&capsule_schema).unwrap(),
            )
            .ok();
        }
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

    let (stop_tx, mut stop_rx) = tokio::sync::broadcast::channel::<()>(1);
    let bus_cap: usize = std::env::var("ARW_BUS_CAP")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(256);
    let bus_replay: usize = std::env::var("ARW_BUS_REPLAY")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(256);
    let bus = arw_events::Bus::new_with_replay(bus_cap, bus_replay);
    // Experimental: initialize SQLite kernel journal (on by default)
    let kernel_opt: Option<arw_kernel::Kernel> = {
        let enabled = std::env::var("ARW_KERNEL_ENABLE")
            .ok()
            .map(|s| s == "1" || s.eq_ignore_ascii_case("true"))
            .unwrap_or(true);
        if enabled {
            match arw_kernel::Kernel::open(&ext::paths::state_dir()) {
                Ok(k) => Some(k),
                Err(e) => {
                    tracing::warn!("kernel init failed: {}", e);
                    None
                }
            }
        } else {
            None
        }
    };
    // Initialize gating from config/env using resolved path (CWD-independent)
    if let Some(p) = arw_core::resolve_config_path("configs/gating.toml") {
        gating::init_from_config(p.to_string_lossy().as_ref());
    }
    let cfg = {
        if let Ok(p) = std::env::var("ARW_CONFIG") {
            arw_core::load_config(&p).ok()
        } else if let Some(p) = arw_core::resolve_config_path("configs/default.toml") {
            arw_core::load_config(p.to_string_lossy().as_ref()).ok()
        } else {
            None
        }
    };
    // Queue selection
    let queue: std::sync::Arc<dyn arw_core::orchestrator::Queue> = {
        let use_nats = cfg
            .as_ref()
            .and_then(|c| c.cluster.enabled)
            .unwrap_or(false)
            && cfg
                .as_ref()
                .and_then(|c| c.cluster.queue.as_deref())
                .unwrap_or("local")
                .eq_ignore_ascii_case("nats");
        if use_nats {
            #[cfg(feature = "nats")]
            {
                let url = cfg
                    .as_ref()
                    .and_then(|c| c.cluster.nats_url.clone())
                    .unwrap_or_else(|| "nats://127.0.0.1:4222".to_string());
                match arw_core::orchestrator_nats::NatsQueue::connect(&url).await {
                    Ok(nq) => std::sync::Arc::new(nq),
                    Err(e) => {
                        tracing::warn!("nats queue unavailable: {} — falling back to local", e);
                        std::sync::Arc::new(arw_core::orchestrator::LocalQueue::new())
                    }
                }
            }
            #[cfg(not(feature = "nats"))]
            {
                tracing::info!(
                    "cluster.queue=nats requested but arw-core built without 'nats' feature; using local"
                );
                std::sync::Arc::new(arw_core::orchestrator::LocalQueue::new())
            }
        } else {
            std::sync::Arc::new(arw_core::orchestrator::LocalQueue::new())
        }
    };
    // Event replication when configured (ingest from NATS into local bus to avoid loops)
    let use_nats_bus = cfg
        .as_ref()
        .and_then(|c| c.cluster.enabled)
        .unwrap_or(false)
        && cfg
            .as_ref()
            .and_then(|c| c.cluster.bus.as_deref())
            .unwrap_or("local")
            .eq_ignore_ascii_case("nats");
    if use_nats_bus {
        #[cfg(feature = "nats")]
        {
            let url = cfg
                .as_ref()
                .and_then(|c| c.cluster.nats_url.clone())
                .unwrap_or_else(|| "nats://127.0.0.1:4222".to_string());
            let node_id = std::env::var("ARW_NODE_ID")
                .ok()
                .or_else(|| cfg.as_ref().and_then(|c| c.cluster.node_id.clone()))
                .unwrap_or_else(|| "local".to_string());
            arw_events::attach_nats_incoming(&bus, &url, &node_id).await;
            if std::env::var("ARW_NATS_OUT").ok().as_deref() == Some("1") {
                arw_events::attach_nats_outgoing(&bus, &url, &node_id).await;
            }
        }
        #[cfg(not(feature = "nats"))]
        {
            tracing::info!(
                "cluster.bus=nats requested but arw-events built without 'nats' feature; using local"
            );
        }
    }

    let state = AppState {
        bus: bus.clone(),
        stop_tx: Some(stop_tx.clone()),
        queue,
        resources: Resources::new(),
    };
    // Share kernel with ext and persist events in background
    ext::set_kernel(kernel_opt.clone());
    if let Some(k) = kernel_opt.clone() {
        let mut rx = state.bus.subscribe();
        tokio::spawn(async move {
            while let Ok(env) = rx.recv().await {
                let _ = k.append_event(&env);
            }
        });
    }
    // Register typed services
    state
        .resources
        .insert(std::sync::Arc::new(ModelsService::new()));
    state
        .resources
        .insert(std::sync::Arc::new(MemoryService::new()));
    state
        .resources
        .insert(std::sync::Arc::new(GovernorService::new()));
    state
        .resources
        .insert(std::sync::Arc::new(HierarchyService::new()));

    // Load persisted orchestration/feedback state
    ext::load_persisted().await;

    // Emit a startup event so /events sees something if connected early.
    state.bus.publish(
        ext::topics::TOPIC_SERVICE_START,
        &json!({"msg":"arw-svc started"}),
    );
    // Pre‑warm hot lookups/caches for interactive I2F (snappy stream)
    ext::snappy::prewarm().await;

    // Spawn stats aggregator and observations read-model updater
    {
        let mut rx = state.bus.subscribe();
        let bus = state.bus.clone();
        let st2 = state.clone();
        tokio::spawn(async move {
            while let Ok(env) = rx.recv().await {
                ext::stats::stats_on_event(&env.kind).await;
                ext::state_api::on_event(&env).await;
                // Materialize world model (belief graph) from events
                ext::world::on_event(&bus, &env).await;
                // Update self‑model aggregates (competence, forecasts)
                ext::self_model_agg::on_event(&env).await;
                // Auto-apply safe feedback when enabled
                ext::auto_apply_from_event(&st2, &env).await;
            }
        });
    }

    // Start lightweight feedback engine (near-live suggestions via bus)
    ext::feedback_engine::start_feedback_engine(state.clone());
    // Start nightly distillation job (beliefs/playbooks/index hygiene)
    ext::distill::start_nightly(state.clone());
    // Start local task worker to exercise the orchestrator MVP
    ext::start_local_task_worker(state.clone());

    // Optional: CAS GC loop (models/by-hash) — off by default
    if std::env::var("ARW_MODELS_GC_ENABLE").ok().as_deref() == Some("1") {
        let bus = state.bus.clone();
        let ttl_days: u64 = std::env::var("ARW_MODELS_GC_TTL_DAYS")
            .ok()
            .and_then(|s| s.parse::<u64>().ok())
            .unwrap_or(30);
        let interval_secs: u64 = std::env::var("ARW_MODELS_GC_INTERVAL_SECS")
            .ok()
            .and_then(|s| s.parse::<u64>().ok())
            .unwrap_or(3600);
        tokio::spawn(async move {
            loop {
                arw_svc::resources::models_service::ModelsService::cas_gc_once(&bus, ttl_days)
                    .await;
                tokio::time::sleep(std::time::Duration::from_secs(interval_secs)).await;
            }
        });
    }

    // Background metrics emitter to SSE (low-frequency; avoids dashboard polling)
    {
        let st = state.clone();
        tokio::spawn(async move {
            let secs = std::env::var("ARW_METRICS_INTERVAL_SECS")
                .ok()
                .and_then(|s| s.parse::<u64>().ok())
                .unwrap_or(10)
                .max(2);
            let mut intv = tokio::time::interval(std::time::Duration::from_secs(secs));
            loop {
                intv.tick().await;
                let snap = collect_metrics_snapshot().await;
                st.bus.publish(
                    ext::topics::TOPIC_PROBE_METRICS,
                    &serde_json::json!({
                        "cpu": snap["cpu"]["avg"],
                        "mem": {"used": snap["memory"]["used"], "total": snap["memory"]["total"]},
                        "disk": snap["disk"],
                        "gpus": snap["gpus"],
                        "npus": snap["npus"],
                    }),
                );
            }
        });
    }

    // Interface catalog watcher -> publish catalog.updated on changes
    {
        let bus = state.bus.clone();
        tokio::spawn(async move {
            use sha2::{Digest as _, Sha256};
            use tokio::fs as afs;
            let files = [
                "interfaces/index.yaml",
                "spec/openapi.yaml",
                "spec/asyncapi.yaml",
                "spec/mcp-tools.json",
            ];
            let mut last_digest = String::new();
            let mut intv = tokio::time::interval(std::time::Duration::from_secs(10));
            loop {
                intv.tick().await;
                let mut any = false;
                let mut hasher = Sha256::new();
                for f in files.iter() {
                    if let Ok(bytes) = afs::read(f).await {
                        hasher.update(f.as_bytes());
                        hasher.update(&bytes);
                        any = true;
                    }
                }
                if !any {
                    continue;
                }
                let digest = format!("{:x}", hasher.finalize());
                if digest != last_digest {
                    last_digest = digest.clone();
                    let payload = serde_json::json!({ "digest": digest, "files": files });
                    bus.publish(ext::topics::TOPIC_CATALOG_UPDATED, &payload);
                    // bump catalog generation to refresh deprecation caches lazily
                    catalog_gen().fetch_add(1, Ordering::Relaxed);
                }
            }
        });
    }

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
                    let payload = serde_json::json!({ "count": count, "path": path, "ts_ms": arw_core::rpu::trust_last_reload_ms() });
                    bus.publish(ext::topics::TOPIC_RPU_TRUST_CHANGED, &payload);
                }
                tokio::time::sleep(Duration::from_secs(5)).await;
            }
        });
    }

    // Subscribe to catalog.updated to refresh deprecation caches immediately
    {
        let bus = state.bus.clone();
        tokio::spawn(async move {
            let mut rx = bus.subscribe();
            while let Ok(env) = rx.recv().await {
                if env.kind == ext::topics::TOPIC_CATALOG_UPDATED {
                    refresh_dep_cache();
                    // align seen_gen with current generation
                    dep_cache()
                        .seen_gen
                        .store(catalog_gen().load(Ordering::Relaxed), Ordering::Relaxed);
                }
            }
        });
    }

    // Start periodic self‑model aggregator (resource forecasts, etc.)
    tokio::spawn(async move { ext::self_model_agg::start_periodic().await });

    // Optionally start gRPC service when enabled and requested
    #[cfg(feature = "grpc")]
    {
        if std::env::var("ARW_GRPC").ok().as_deref() == Some("1") {
            let st = state.clone();
            tokio::spawn(async move { grpc::serve(st).await });
        }
    }

    let mut app = Router::new();
    // Record + attach public endpoints without changing route types
    arw_svc::route_recorder::note("GET", "/healthz");
    app = app.route("/healthz", get(healthz));
    arw_svc::route_recorder::note("GET", "/metrics");
    app = app.route("/metrics", get(ext::stats::metrics_get));
    arw_svc::route_recorder::note("GET", "/version");
    app = app.route("/version", get(ext::version));
    arw_svc::route_recorder::note("GET", "/about");
    app = app.route("/about", get(ext::about));
    // Public CAS blob by digest (GET/HEAD)
    arw_svc::route_recorder::note("GET", "/models/blob/:sha256");
    app = app.route(
        "/models/blob/:sha256",
        get(ext::models_api::models_blob_get),
    );
    arw_svc::route_recorder::note("HEAD", "/models/blob/:sha256");
    app = app.route(
        "/models/blob/:sha256",
        axum::routing::head(ext::models_api::models_blob_head),
    );
    // Interface catalog (index + health)
    arw_svc::route_recorder::note("GET", "/catalog/index");
    app = app.route("/catalog/index", get(catalog_index));
    arw_svc::route_recorder::note("GET", "/catalog/health");
    app = app.route("/catalog/health", get(catalog_health));
    arw_svc::route_recorder::note("GET", "/state/models");
    app = app.route("/state/models", get(ext::state_api::models_state_get));
    arw_svc::route_recorder::note("GET", "/state/self");
    app = app.route("/state/self", get(ext::self_model_api::self_state_list));
    arw_svc::route_recorder::note("GET", "/state/self/:agent");
    app = app.route(
        "/state/self/:agent",
        get(ext::self_model_api::self_state_get),
    );
    arw_svc::route_recorder::note("GET", "/state/models_hashes");
    app = app.route(
        "/state/models_hashes",
        get(ext::models_api::models_hashes_get),
    );
    // Serve generated specs when present (public)
    arw_svc::route_recorder::note("GET", "/spec/openapi.yaml");
    app = app.route("/spec/openapi.yaml", get(spec_openapi));
    arw_svc::route_recorder::note("GET", "/spec/asyncapi.yaml");
    app = app.route("/spec/asyncapi.yaml", get(spec_asyncapi));
    arw_svc::route_recorder::note("GET", "/spec/mcp-tools.json");
    app = app.route("/spec/mcp-tools.json", get(spec_mcp));
    arw_svc::route_recorder::note("GET", "/spec");
    app = app.route("/spec", get(spec_index));
    // Actions (triad)
    arw_svc::route_recorder::note("POST", "/actions");
    app = app.route(
        "/actions",
        axum::routing::post(ext::actions_api::actions_submit_post),
    );
    // Triad (experimental): public events with replay via kernel
    arw_svc::route_recorder::note("GET", "/triad/events");
    app = app.route("/triad/events", get(ext::triad_events_sse));
    // Friendly top-level landing page + quiet favicon
    app = app
        .route("/", get(index_landing))
        .route("/favicon.ico", get(favicon_empty));

    // Administrative endpoints are nested under /admin
    app = app.nest(
        "/admin",
        Router::new()
            // Match paths before metrics/security to capture MatchedPath
            .route("/", get(admin_index_html))
            .route("/index.json", get(admin_index_json))
            .route("/probe", get(probe))
            .route("/probe/hw", get(probe_hw))
            .route("/probe/metrics", get(probe_metrics))
            .route("/events", get(events))
            .route("/emit/test", get(emit_test))
            .route("/shutdown", get(shutdown))
            .route(
                "/self_model/propose",
                axum::routing::post(ext::self_model_api::self_model_propose),
            )
            .route(
                "/self_model/apply",
                axum::routing::post(ext::self_model_api::self_model_apply),
            )
            .route("/introspect/tools", get(introspect_tools))
            .route("/introspect/schemas/:id", get(introspect_schema))
            // Bring in extra admin routes (memory/models/tools/etc.)
            .merge(ext::extra_routes()),
    );
    // Friendly top-level alias for Debug UI in local debug builds
    if std::env::var("ARW_DEBUG").ok().as_deref() == Some("1") {
        arw_svc::route_recorder::note("GET", "/debug");
        app = app.route("/debug", get(ext::ui::debug_ui));
    }
    // Spec health (public)
    app = app.route("/spec/health", get(spec_health));
    // Trace with request-id and client ip in span
    let trace = TraceLayer::new_for_http().make_span_with(|req: &Request<_>| {
        let rid = req
            .extensions()
            .get::<String>()
            .cloned()
            .unwrap_or_else(|| "".to_string());
        let ip = client_ip_key_from_request(req);
        tracing::info_span!(
            "http",
            method = %req.method(),
            uri = %req.uri(),
            version = ?req.version(),
            request_id = %rid,
            ip = %ip
        )
    });
    app = app
        .layer(trace)
        .layer(CompressionLayer::new())
        .layer(RequestBodyLimitLayer::new(8 * 1024 * 1024))
        .layer(
            if std::env::var("ARW_CORS_ANY").ok().as_deref() == Some("1")
                || std::env::var("ARW_DEBUG").ok().as_deref() == Some("1")
            {
                CorsLayer::new()
                    .allow_origin(Any)
                    .allow_methods([
                        axum::http::Method::GET,
                        axum::http::Method::POST,
                        axum::http::Method::HEAD,
                    ])
                    .allow_headers(Any)
            } else {
                CorsLayer::new()
            },
        )
        .layer(middleware::from_fn(dyn_timeout::dyn_timeout_mw))
        .layer(middleware::from_fn(access_log_mw))
        .layer(middleware::from_fn(request_id_mw))
        .layer(middleware::from_fn(security_headers_mw))
        .layer(middleware::from_fn(deprecation_headers_mw))
        .layer(middleware::from_fn(security_mw))
        .layer(middleware::from_fn(metrics_mw));

    // Optionally serve local docs at /docs when in debug mode and site exists
    if std::env::var("ARW_DEBUG").ok().as_deref() == Some("1") {
        let doc_dir = if FsPath::new("docs-site").exists() {
            Some("docs-site")
        } else if FsPath::new("site").exists() {
            Some("site")
        } else {
            None
        };
        if let Some(p) = doc_dir {
            app = app.nest_service("/docs", ServeDir::new(p));
        }
    }

    let port: u16 = cfg
        .as_ref()
        .and_then(|c| c.runtime.port)
        .or_else(|| std::env::var("ARW_PORT").ok().and_then(|s| s.parse().ok()))
        .unwrap_or(8090);
    let bind = std::env::var("ARW_BIND").unwrap_or_else(|_| "127.0.0.1".to_string());
    let addr = format!("{}:{}", bind, port)
        .parse::<SocketAddr>()
        .unwrap_or_else(|_| SocketAddr::from(([127, 0, 0, 1], port)));
    info!("arw-svc listening on http://{}", addr);

    let listener = match tokio::net::TcpListener::bind(addr).await {
        Ok(l) => l,
        Err(e) => {
            tracing::error!("failed to bind {}: {}", addr, e);
            return;
        }
    };
    // Start periodic publishers for read-model patches
    {
        let bus_clone = bus.clone();
        tokio::spawn(async move {
            ext::stats::start_route_stats_publisher(bus_clone).await;
        });
    }
    {
        let bus_clone = bus.clone();
        tokio::spawn(async move {
            arw_svc::resources::models_service::start_models_metrics_publisher(bus_clone).await;
        });
    }
    {
        let bus_clone = bus.clone();
        tokio::spawn(async move {
            ext::snappy::start_snappy_publisher(bus_clone).await;
        });
    }
    {
        let bus_clone = bus.clone();
        tokio::spawn(async move {
            ext::stats::start_probe_metrics_collector(bus_clone).await;
        });
    }

    // Serve router (Axum 0.7)
    // Keep handlers typed with `State<AppState>`, but run unit-state router.
    // A FromRef bridge provides access to the global state.
    state_bridge::set_global_state(state.clone());
    let app_unit = app.with_state(state);
    let server = axum::serve(
        listener,
        app_unit.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .with_graceful_shutdown(async move {
        let _ = stop_rx.recv().await;
    });
    if let Err(e) = server.await {
        eprintln!("server error: {}", e);
    }
}

// Test helpers for bin tests to satisfy paths used in ext/* tests
#[cfg(test)]
mod test_support {
    /// Set `ARW_STATE_DIR` to a unique temp dir for the scope of the returned guard.
    /// Drops reset the env var to its previous value.
    pub fn scoped_state_dir() -> (tempfile::TempDir, ScopedEnv) {
        let dir = tempfile::tempdir().expect("tempdir");
        let prev = std::env::var("ARW_STATE_DIR").ok();
        std::env::set_var("ARW_STATE_DIR", dir.path().to_string_lossy().to_string());
        (
            dir,
            ScopedEnv {
                key: "ARW_STATE_DIR",
                prev,
            },
        )
    }
    pub struct ScopedEnv {
        key: &'static str,
        prev: Option<String>,
    }
    impl Drop for ScopedEnv {
        fn drop(&mut self) {
            match &self.prev {
                Some(v) => std::env::set_var(self.key, v),
                None => std::env::remove_var(self.key),
            }
        }
    }
}

async fn metrics_mw(req: Request<axum::body::Body>, next: Next) -> Response {
    use axum::extract::MatchedPath;
    let path = req
        .extensions()
        .get::<MatchedPath>()
        .map(|m| m.as_str().to_string())
        .unwrap_or_else(|| req.uri().path().to_string());
    let t0 = std::time::Instant::now();
    let res = next.run(req).await;
    let dt = t0.elapsed().as_millis() as u64;
    let status = res.status().as_u16();
    ext::stats::route_obs(&path, status, dt).await;
    // Add basic Server-Timing for easy client-side perf inspection
    let mut res = res;
    let h = res.headers_mut();
    if !h.contains_key("server-timing") {
        let _ = h.insert(
            axum::http::header::HeaderName::from_static("server-timing"),
            axum::http::HeaderValue::from_str(&format!("total;dur={}", dt))
                .unwrap_or_else(|_| axum::http::HeaderValue::from_static("total")),
        );
    }
    res
}

async fn security_mw(req: Request<axum::body::Body>, next: Next) -> Response {
    use axum::http::StatusCode as SC;
    use axum::response::IntoResponse;
    let path = req.uri().path();
    // Only paths under /admin are considered sensitive and require token/debug access.
    let is_admin = path.starts_with("/admin/") || path == "/admin";
    if !is_admin {
        return next.run(req).await;
    }

    let debug = std::env::var("ARW_DEBUG").ok().as_deref() == Some("1");
    let tok_provided = extract_admin_token(req.headers());
    // Accept logic: prefer hashed token, else plain token, else debug
    let ok = if let Some(tok) = tok_provided.as_deref() {
        if let Ok(h) = std::env::var("ARW_ADMIN_TOKEN_SHA256") {
            token_sha256_matches(tok, &h)
        } else if let Ok(t) = std::env::var("ARW_ADMIN_TOKEN") {
            if t.is_empty() {
                debug
            } else {
                ct_eq(tok.as_bytes(), t.as_bytes())
            }
        } else {
            debug
        }
    } else {
        // No token supplied; allow only if debug
        debug
    };
    if ok {
        // Per-token/IP token-bucket rate limit (sliding) for admin endpoints
        let key = tok_provided.unwrap_or_else(|| client_ip_key_req(&req));
        if !rate_allow_keyed(&key).await {
            let body = serde_json::json!({
                "type": "about:blank",
                "title": "Too Many Requests",
                "status": 429,
                "detail": "rate limit exceeded for administrative endpoints"
            });
            return (SC::TOO_MANY_REQUESTS, axum::Json(body)).into_response();
        }
        // Optional gating capsule via header (JSON in x-arw-gate). Apply after rate check.
        if let Some(h) = req.headers().get("x-arw-gate") {
            if let Ok(s) = h.to_str() {
                if s.len() <= 4096 {
                    let _ = arw_core::rpu::adopt_from_header_json(s);
                } else {
                    tracing::warn!("x-arw-gate header too large; ignoring");
                }
            }
        }
        return next.run(req).await;
    }
    // Unauthorized: advertise Bearer to guide clients
    let body = serde_json::json!({
        "type": "about:blank",
        "title": "Unauthorized",
        "status": 401,
        "detail": "administrative endpoint; provide Bearer token or X-ARW-Admin"
    });
    (
        [(axum::http::header::WWW_AUTHENTICATE, "Bearer")],
        (SC::UNAUTHORIZED, axum::Json(body)),
    )
        .into_response()
}

// ---- Request ID middleware ----
// Ensures each response carries x-request-id; adopts incoming if present.
async fn request_id_mw(mut req: Request<axum::body::Body>, next: Next) -> Response {
    let rid = req
        .headers()
        .get("x-request-id")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string())
        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    req.extensions_mut().insert(rid.clone());
    let mut res = next.run(req).await;
    let h = res.headers_mut();
    if !h.contains_key("x-request-id") {
        let _ = h.insert(
            axum::http::header::HeaderName::from_static("x-request-id"),
            axum::http::HeaderValue::from_str(&rid)
                .unwrap_or_else(|_| axum::http::HeaderValue::from_static("invalid-request-id")),
        );
    }
    res
}

// ---- Security headers middleware ----
// Adds conservative security headers that play well with our endpoints.
async fn security_headers_mw(req: Request<axum::body::Body>, next: Next) -> Response {
    let mut res = next.run(req).await;
    let h = res.headers_mut();
    h.entry(axum::http::header::HeaderName::from_static(
        "x-content-type-options",
    ))
    .or_insert(axum::http::HeaderValue::from_static("nosniff"));
    h.entry(axum::http::header::HeaderName::from_static(
        "referrer-policy",
    ))
    .or_insert(axum::http::HeaderValue::from_static("no-referrer"));
    h.entry(axum::http::header::HeaderName::from_static(
        "x-frame-options",
    ))
    .or_insert(axum::http::HeaderValue::from_static("DENY"));
    h.entry(axum::http::header::HeaderName::from_static(
        "permissions-policy",
    ))
    .or_insert(axum::http::HeaderValue::from_static(
        "geolocation=(), microphone=(), camera=()",
    ));
    // Optionally add CSP automatically for HTML responses (debug/docs/static). Can be overridden by ARW_CSP.
    if csp_auto_enabled() && !h.contains_key("content-security-policy") {
        if let Some(ct) = h
            .get(axum::http::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
        {
            if ct.starts_with("text/html") {
                if let Some(csp) = csp_header_value() {
                    let _ = h.insert(
                        axum::http::HeaderName::from_static("content-security-policy"),
                        csp,
                    );
                }
            }
        }
    }
    res
}

fn csp_header_value() -> Option<axum::http::HeaderValue> {
    // Allow override or opt-out
    if let Ok(v) = std::env::var("ARW_CSP") {
        let s = v.trim();
        if s.is_empty() || s == "0" || s.eq_ignore_ascii_case("off") {
            return None;
        }
        return axum::http::HeaderValue::from_str(s).ok();
    }
    // Preset: relaxed (default) or strict (disables inline scripts/styles used by tiny HTML pages)
    let preset = std::env::var("ARW_CSP_PRESET").unwrap_or_else(|_| "relaxed".into());
    let def = if preset.eq_ignore_ascii_case("strict") {
        // Hardened: disable inline scripts/styles; small HTML pages may lose minor UX features.
        "default-src 'self'; img-src 'self' data:; style-src 'self'; script-src 'self'"
    } else {
        // Relaxed: permit inline for small admin/spec/landing pages without nonces.
        "default-src 'self'; img-src 'self' data:; style-src 'self' 'unsafe-inline'; script-src 'self' 'unsafe-inline'"
    };
    axum::http::HeaderValue::from_str(def).ok()
}

fn csp_auto_enabled() -> bool {
    std::env::var("ARW_CSP_AUTO")
        .ok()
        .map(|v| v == "1")
        .unwrap_or(true)
}

// ---- Access log (opt-in via ARW_ACCESS_LOG=1) ----
static ACCESS_ENABLED: OnceLock<bool> = OnceLock::new();
static ACCESS_SAMPLE_N: OnceLock<u64> = OnceLock::new();
static ACCESS_SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
fn access_log_enabled() -> bool {
    *ACCESS_ENABLED.get_or_init(|| std::env::var("ARW_ACCESS_LOG").ok().as_deref() == Some("1"))
}
fn access_sample_n() -> u64 {
    *ACCESS_SAMPLE_N.get_or_init(|| {
        std::env::var("ARW_ACCESS_SAMPLE_N")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(1)
            .max(1)
    })
}
fn access_log_ua() -> bool {
    std::env::var("ARW_ACCESS_UA").ok().as_deref() == Some("1")
}
fn access_log_ua_hash() -> bool {
    std::env::var("ARW_ACCESS_UA_HASH").ok().as_deref() == Some("1")
}
fn access_log_ref() -> bool {
    std::env::var("ARW_ACCESS_REF").ok().as_deref() == Some("1")
}
fn access_log_ref_strip_qs() -> bool {
    std::env::var("ARW_ACCESS_REF_STRIP_QS")
        .ok()
        .map(|v| v == "1")
        .unwrap_or(true)
}
async fn access_log_mw(req: Request<axum::body::Body>, next: Next) -> Response {
    if !access_log_enabled() {
        return next.run(req).await;
    }
    use axum::extract::MatchedPath;
    let t0 = std::time::Instant::now();
    let method = req.method().clone();
    let path_only = req.uri().path().to_string();
    let rid = req
        .headers()
        .get("x-request-id")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string())
        .unwrap_or_else(|| {
            req.extensions()
                .get::<String>()
                .cloned()
                .unwrap_or_else(|| uuid::Uuid::new_v4().to_string())
        });
    let ip = client_ip_key_req(&req);
    let matched = req
        .extensions()
        .get::<MatchedPath>()
        .map(|m| m.as_str().to_string());
    let req_len = req
        .headers()
        .get(axum::http::header::CONTENT_LENGTH)
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(0);
    // Capture UA/Referer before handing off
    let user_agent: String = if access_log_ua() {
        let raw = req
            .headers()
            .get(axum::http::header::USER_AGENT)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");
        if access_log_ua_hash() {
            use sha2::Digest as _;
            let mut hasher = sha2::Sha256::new();
            hasher.update(raw.as_bytes());
            format!("sha256:{}", hex::encode(hasher.finalize()))
        } else {
            raw.to_string()
        }
    } else {
        String::new()
    };
    let referer_out: String = if access_log_ref() {
        let raw = req
            .headers()
            .get(axum::http::header::REFERER)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");
        if access_log_ref_strip_qs() {
            if let Ok(u) = url::Url::parse(raw) {
                let mut out = format!(
                    "{}://{}{}",
                    u.scheme(),
                    u.host_str().unwrap_or(""),
                    u.path()
                );
                if let Some(p) = u.port() {
                    out = format!("{}:{}", out, p);
                }
                out
            } else {
                raw.split('?').next().unwrap_or("").to_string()
            }
        } else {
            raw.to_string()
        }
    } else {
        String::new()
    };
    let res = next.run(req).await;
    let dt = t0.elapsed().as_millis() as u64;
    let status = res.status().as_u16();
    let resp_len = res
        .headers()
        .get(axum::http::header::CONTENT_LENGTH)
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(0);
    let n = ACCESS_SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
    if n % access_sample_n() == 0 {
        tracing::info!(
            target: "http.access",
            %method,
            path = %path_only,
            matched = matched.as_deref().unwrap_or(""),
            status = status,
            dt_ms = dt,
            ip = %ip,
            request_id = %rid,
            req_len = req_len,
            resp_len = resp_len,
            ua = %user_agent,
            referer = %referer_out,
            "request"
        );
    }
    res
}

// ---- Deprecation/Sunset headers middleware ----
// If the OpenAPI marks an operation as deprecated, emit Deprecation: true.
// If the interface descriptor has a sunset date, also emit Sunset and Link: rel="deprecation".
async fn deprecation_headers_mw(req: Request<axum::body::Body>, next: Next) -> Response {
    use axum::extract::MatchedPath;
    let method = req.method().clone();
    let path_pat = req
        .extensions()
        .get::<MatchedPath>()
        .map(|m| m.as_str().to_string())
        .unwrap_or_else(|| req.uri().path().to_string());
    let mut res = next.run(req).await;
    if is_deprecated_operation(method.as_str(), &path_pat) {
        let h = res.headers_mut();
        h.insert("Deprecation", axum::http::HeaderValue::from_static("true"));
        if let Some(sunset) = spec_sunset_for(method.as_str(), &path_pat).or_else(descriptor_sunset)
        {
            if let Ok(v) = axum::http::HeaderValue::from_str(&sunset) {
                h.insert("Sunset", v);
            }
        }
        if let Some(doc) = descriptor_docs_url() {
            if let Ok(v) =
                axum::http::HeaderValue::from_str(&format!("<{}>; rel=\"deprecation\"", doc))
            {
                h.append("Link", v);
            }
        }
    }
    res
}

// ---- Catalog generation & deprecation caches (refreshable) ----
fn catalog_gen() -> &'static AtomicU64 {
    static GEN: once_cell::sync::OnceCell<AtomicU64> = once_cell::sync::OnceCell::new();
    GEN.get_or_init(|| AtomicU64::new(1))
}

struct DepCache {
    seen_gen: AtomicU64,
    deprecated: StdRwLock<HashSet<(String, String)>>,
    sunsets: StdRwLock<HashMap<(String, String), String>>,
    desc_sunset: StdRwLock<Option<String>>,
    doc_url: StdRwLock<Option<String>>,
}

fn dep_cache() -> &'static DepCache {
    static CACHE: once_cell::sync::OnceCell<DepCache> = once_cell::sync::OnceCell::new();
    CACHE.get_or_init(|| DepCache {
        seen_gen: AtomicU64::new(0),
        deprecated: StdRwLock::new(HashSet::new()),
        sunsets: StdRwLock::new(HashMap::new()),
        desc_sunset: StdRwLock::new(None),
        doc_url: StdRwLock::new(None),
    })
}

fn refresh_dep_cache() {
    use serde_yaml as yaml;
    // OpenAPI: deprecated + x-sunset
    let mut dep: HashSet<(String, String)> = HashSet::new();
    let mut suns: HashMap<(String, String), String> = HashMap::new();
    if let Ok(bytes) = std::fs::read("spec/openapi.yaml") {
        if let Ok(doc) = yaml::from_slice::<yaml::Value>(&bytes) {
            if let Some(paths) = doc.get("paths").and_then(|v| v.as_mapping()) {
                for (pkey, pval) in paths.iter() {
                    let pstr = pkey.as_str().unwrap_or_default().to_string();
                    if let Some(ops) = pval.as_mapping() {
                        for (mkey, oval) in ops.iter() {
                            let m = mkey.as_str().unwrap_or_default().to_lowercase();
                            if [
                                "get", "post", "put", "delete", "patch", "options", "head", "trace",
                            ]
                            .contains(&m.as_str())
                            {
                                let deprecated = oval
                                    .get("deprecated")
                                    .and_then(|v| v.as_bool())
                                    .unwrap_or(false);
                                if deprecated {
                                    dep.insert((m.to_uppercase(), pstr.clone()));
                                }
                                if let Some(s) = oval.get("x-sunset").and_then(|v| v.as_str()) {
                                    suns.insert((m.to_uppercase(), pstr.clone()), s.to_string());
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    let cache = dep_cache();
    if let Ok(mut w) = cache.deprecated.write() {
        *w = dep;
    }
    if let Ok(mut w) = cache.sunsets.write() {
        *w = suns;
    }
    // Descriptor sunset/docs
    #[derive(serde::Deserialize)]
    struct Docs {
        human: Option<String>,
    }
    #[derive(serde::Deserialize)]
    struct Desc {
        sunset: Option<String>,
        docs: Option<Docs>,
    }
    if let Ok(s) = std::fs::read_to_string("interfaces/http/arw-svc/descriptor.yaml") {
        if let Ok(d) = serde_yaml::from_str::<Desc>(&s) {
            if let Ok(mut w) = cache.desc_sunset.write() {
                *w = d.sunset;
            }
            if let Ok(mut w) = cache.doc_url.write() {
                *w = d.docs.and_then(|dd| dd.human);
            }
        }
    }
}

fn refresh_dep_cache_if_needed() {
    let gen = catalog_gen().load(Ordering::Relaxed);
    let cache = dep_cache();
    if cache.seen_gen.load(Ordering::Relaxed) != gen {
        refresh_dep_cache();
        cache.seen_gen.store(gen, Ordering::Relaxed);
    }
}

fn is_deprecated_operation(method: &str, path_pat: &str) -> bool {
    refresh_dep_cache_if_needed();
    dep_cache()
        .deprecated
        .read()
        .map(|m| m.contains(&(method.to_uppercase(), path_pat.to_string())))
        .unwrap_or(false)
}

fn descriptor_sunset() -> Option<String> {
    refresh_dep_cache_if_needed();
    dep_cache().desc_sunset.read().ok().and_then(|o| o.clone())
}

fn descriptor_docs_url() -> Option<String> {
    refresh_dep_cache_if_needed();
    dep_cache().doc_url.read().ok().and_then(|o| o.clone())
}

fn spec_sunset_for(method: &str, path_pat: &str) -> Option<String> {
    refresh_dep_cache_if_needed();
    dep_cache().sunsets.read().ok().and_then(|m| {
        m.get(&(method.to_uppercase(), path_pat.to_string()))
            .cloned()
    })
}

fn extract_admin_token(h: &HeaderMap) -> Option<String> {
    // Prefer Authorization: Bearer <token>
    if let Some(v) = h
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
    {
        if let Some(bearer) = v.strip_prefix("Bearer ") {
            return Some(bearer.to_string());
        }
    }
    // Back-compat: X-ARW-Admin: <token>
    if let Some(v) = h.get("x-arw-admin").and_then(|v| v.to_str().ok()) {
        return Some(v.to_string());
    }
    None
}

fn client_ip_from_headers(h: &HeaderMap) -> Option<String> {
    if let Some(v) = h.get("x-forwarded-for").and_then(|v| v.to_str().ok()) {
        if let Some(first) = v.split(',').next() {
            return Some(first.trim().to_string());
        }
    }
    if let Some(v) = h.get("x-real-ip").and_then(|v| v.to_str().ok()) {
        return Some(v.trim().to_string());
    }
    if let Some(v) = h.get("cf-connecting-ip").and_then(|v| v.to_str().ok()) {
        return Some(v.trim().to_string());
    }
    None
}

// Back-compat helper used by a unit test; keep behavior trusting headers.
#[allow(dead_code)]
fn client_ip_key(h: &HeaderMap) -> String {
    client_ip_from_headers(h).unwrap_or_else(|| "unknown".to_string())
}

// Prefer remote addr unless explicitly trusting forward headers.
fn client_ip_key_req(req: &Request<axum::body::Body>) -> String {
    let trust = std::env::var("ARW_TRUST_FORWARD_HEADERS").ok().as_deref() == Some("1");
    if trust {
        if let Some(s) = client_ip_from_headers(req.headers()) {
            return s;
        }
    }
    if let Some(ConnectInfo(addr)) = req.extensions().get::<ConnectInfo<SocketAddr>>().cloned() {
        return addr.ip().to_string();
    }
    // Fallback to headers if no connect info was available
    client_ip_from_headers(req.headers()).unwrap_or_else(|| "unknown".to_string())
}

// Generic variant for TraceLayer (any Body type)
fn client_ip_key_from_request<B>(req: &Request<B>) -> String {
    let trust = std::env::var("ARW_TRUST_FORWARD_HEADERS").ok().as_deref() == Some("1");
    if trust {
        if let Some(s) = client_ip_from_headers(req.headers()) {
            return s;
        }
    }
    if let Some(ConnectInfo(addr)) = req.extensions().get::<ConnectInfo<SocketAddr>>().cloned() {
        return addr.ip().to_string();
    }
    client_ip_from_headers(req.headers()).unwrap_or_else(|| "unknown".to_string())
}

fn token_sha256_matches(token: &str, expected_hex: &str) -> bool {
    use sha2::Digest as _;
    let mut hasher = sha2::Sha256::new();
    hasher.update(token.as_bytes());
    let dig = hasher.finalize();
    let got = hex::encode(dig);
    ct_eq(got.as_bytes(), expected_hex.as_bytes())
}

#[inline]
fn ct_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff: u8 = 0;
    for (&x, &y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

// ---- per-key token-bucket rate limit (sliding) ----
struct Bucket {
    tokens: f64,
    last: std::time::Instant,
}
static RL_MAP: OnceLock<tokio::sync::Mutex<std::collections::HashMap<String, Bucket>>> =
    OnceLock::new();
fn rl_params() -> (f64, f64) {
    if let Ok(s) = std::env::var("ARW_ADMIN_RL") {
        if let Some((a, b)) = s.split_once('/') {
            if let (Ok(l), Ok(w)) = (a.parse::<f64>(), b.parse::<f64>()) {
                return (l.max(1.0), w.max(1.0));
            }
        }
    }
    (60.0, 60.0)
}
async fn rate_allow_keyed(key: &str) -> bool {
    let (limit, win_secs) = rl_params();
    let cap = limit; // bucket capacity
    let refill_per_sec = limit / win_secs;
    let now = std::time::Instant::now();
    let m = RL_MAP.get_or_init(|| tokio::sync::Mutex::new(std::collections::HashMap::new()));
    let mut map = m.lock().await;
    let b = map.entry(key.to_string()).or_insert(Bucket {
        tokens: cap,
        last: now,
    });
    let elapsed = now.duration_since(b.last).as_secs_f64();
    b.last = now;
    // refill
    b.tokens = (b.tokens + elapsed * refill_per_sec).min(cap);
    if b.tokens >= 1.0 {
        b.tokens -= 1.0;
        true
    } else {
        false
    }
}
#[utoipa::path(
    get,
    path = "/healthz",
    operation_id = "healthz_doc",
    responses((status = 200, description = "Service health", body = OkResponse))
)]
async fn healthz(State(state): State<AppState>) -> impl IntoResponse {
    state
        .bus
        .publish(ext::topics::TOPIC_SERVICE_HEALTH, &json!({"ok": true}));
    Json(OkResponse { ok: true })
}

#[arw_admin(
    method = "GET",
    path = "/admin/introspect/tools",
    summary = "List available tools"
)]
#[utoipa::path(
    get,
    path = "/admin/introspect/tools",
    tag = "Admin/Introspect",
    operation_id = "introspect_tools_doc",
    responses(
        (status = 200, description = "List available tools"),
        (status = 403, description = "Forbidden", body = arw_protocol::ProblemDetails)
    )
)]
#[arw_gate("introspect:tools")]
async fn introspect_tools() -> impl IntoResponse {
    ext::ok(serde_json::to_value(arw_core::introspect_tools()).unwrap()).into_response()
}

#[arw_admin(
    method = "GET",
    path = "/admin/introspect/schemas/{id}",
    summary = "Get tool schema"
)]
#[utoipa::path(
    get,
    path = "/admin/introspect/schemas/{id}",
    tag = "Admin/Introspect",
    operation_id = "introspect_schema_doc",
    params(("id" = String, Path, description = "Tool id")),
    responses(
        (status = 200, description = "Schema JSON"),
        (status = 404, description = "Unknown tool id"),
        (status = 403, description = "Forbidden", body = arw_protocol::ProblemDetails)
    )
)]
#[arw_gate("introspect:schema")]
async fn introspect_schema(Path(id): Path<String>) -> impl IntoResponse {
    match arw_core::introspect_schema(&id) {
        Some(s) => Json::<serde_json::Value>(s).into_response(),
        None => {
            let body = json!({
                "type":   "about:blank",
                "title":  "Not Found",
                "status": 404,
                "detail": format!("unknown tool id: {}", id)
            });
            (StatusCode::NOT_FOUND, Json(body)).into_response()
        }
    }
}

#[arw_admin(
    method = "GET",
    path = "/admin/probe",
    summary = "Effective paths and memory"
)]
#[utoipa::path(
    get,
    path = "/admin/probe",
    tag = "Admin/Introspect",
    operation_id = "probe_doc",
    responses(
        (status = 200, description = "Returns effective memory paths"),
        (status = 403, description = "Forbidden", body = arw_protocol::ProblemDetails)
    )
)]
#[arw_gate("introspect:probe")]
async fn probe(State(state): State<AppState>) -> impl IntoResponse {
    // Effective paths as JSON (serde_json::Value)
    let ep = arw_core::load_effective_paths();

    // Publish that JSON to the event bus
    state.bus.publish(ext::topics::TOPIC_MEMORY_APPLIED, &ep);

    // Return it to the client
    ext::ok::<serde_json::Value>(ep).into_response()
}

#[arw_admin(
    method = "GET",
    path = "/admin/probe/hw",
    summary = "Hardware/Software probe (CPU/OS/Disks/GPUs)"
)]
#[utoipa::path(
    get,
    path = "/admin/probe/hw",
    tag = "Admin/Introspect",
    responses(
        (status = 200, description = "Hardware and software info (best-effort)"),
        (status = 403, description = "Forbidden", body = arw_protocol::ProblemDetails)
    )
)]
#[arw_gate("introspect:probe")]
async fn probe_hw(State(state): State<AppState>) -> impl IntoResponse {
    use sysinfo::System;
    let mut sys = System::new_all();
    sys.refresh_all();

    // CPU
    let cpus_logical = sys.cpus().len() as u64;
    let cpus_physical = sys.physical_core_count().unwrap_or(0) as u64;
    let cpu_brand = sys
        .cpus()
        .first()
        .map(|c| c.brand().to_string())
        .unwrap_or_default();

    // Memory (bytes)
    let total_mem = sys.total_memory();
    let avail_mem = sys.available_memory();

    // OS
    let info = os_info::get();
    let os_name = info.os_type().to_string();
    let os_version = info.version().to_string();
    let kernel = sysinfo::System::kernel_version().unwrap_or_default();
    let arch = std::env::consts::ARCH.to_string();

    // Disks (system view, not just app paths)
    let disks: Vec<serde_json::Value> = probe_disks_best_effort();

    // Boot/virt/container hints (Linux-only paths are best-effort)
    let mut boot = serde_json::Map::new();
    boot.insert(
        "uefi".into(),
        serde_json::Value::Bool(std::path::Path::new("/sys/firmware/efi").exists()),
    );
    let mut virt = serde_json::Map::new();
    virt.insert(
        "hypervisor_flag".into(),
        serde_json::Value::Bool(read_cpuinfo_has_flag("hypervisor")),
    );
    if let Some(pname) = read_small("/sys/devices/virtual/dmi/id/product_name") {
        virt.insert("product_name".into(), serde_json::Value::String(pname));
    }
    let mut container = serde_json::Map::new();
    container.insert(
        "dockerenv".into(),
        serde_json::Value::Bool(std::path::Path::new("/.dockerenv").exists()),
    );
    container.insert(
        "containerenv".into(),
        serde_json::Value::Bool(std::path::Path::new("/run/.containerenv").exists()),
    );
    if let Ok(v) = std::env::var("container") {
        container.insert("env".into(), serde_json::Value::String(v));
    }
    let wsl = read_small("/proc/sys/kernel/osrelease")
        .map(|s| s.to_ascii_lowercase().contains("microsoft"))
        .unwrap_or(false);

    // Env hints
    let mut env = serde_json::Map::new();
    for k in [
        "CUDA_VISIBLE_DEVICES",
        "NVIDIA_VISIBLE_DEVICES",
        "ROCR_VISIBLE_DEVICES",
        "HSA_VISIBLE_DEVICES",
    ] {
        if let Ok(v) = std::env::var(k) {
            env.insert(k.to_string(), serde_json::Value::String(v));
        }
    }

    // GPUs (best-effort)
    let gpus = probe_gpus_best_effort();
    // NPUs (best-effort)
    let npus = probe_npus_best_effort();
    #[cfg(feature = "gpu_wgpu")]
    let gpus_wgpu = probe_gpus_wgpu();
    #[cfg(not(feature = "gpu_wgpu"))]
    let gpus_wgpu: Vec<serde_json::Value> = Vec::new();
    #[cfg(feature = "gpu_nvml")]
    let gpus_nvml = probe_gpu_nvml();
    #[cfg(not(feature = "gpu_nvml"))]
    let gpus_nvml: Vec<serde_json::Value> = Vec::new();

    let out = serde_json::json!({
        "cpu": {"brand": cpu_brand, "logical": cpus_logical, "physical": cpus_physical, "features": cpu_features()},
        "memory": {"total": total_mem, "available": avail_mem},
        "os": {"name": os_name, "version": os_version, "kernel": kernel, "arch": arch},
        "disks": disks,
        "boot": boot,
        "virt": virt,
        "container": container,
        "wsl": wsl,
        "env": env,
        "gpus": gpus,
        "gpus_wgpu": gpus_wgpu,
        "gpus_nvml": gpus_nvml,
        "npus": npus,
    });
    // Publish minimal event for observability
    state.bus.publish(
        ext::topics::TOPIC_PROBE_HW,
        &serde_json::json!({"cpus": cpus_logical, "gpus": out["gpus"].as_array().map(|a| a.len()).unwrap_or(0)}),
    );
    ext::ok::<serde_json::Value>(out).into_response()
}

#[cfg(target_os = "linux")]
fn probe_gpus_best_effort() -> Vec<serde_json::Value> {
    probe_gpus_linux()
}
#[cfg(target_os = "windows")]
fn probe_gpus_best_effort() -> Vec<serde_json::Value> {
    use serde_json::json;
    use std::os::windows::ffi::OsStringExt as _;
    use windows::Win32::Graphics::Dxgi::{
        CreateDXGIFactory1, IDXGIFactory1, DXGI_ADAPTER_DESC1, DXGI_ERROR_NOT_FOUND,
    };
    unsafe {
        let factory: IDXGIFactory1 = match CreateDXGIFactory1::<IDXGIFactory1>() {
            Ok(f) => f,
            Err(_) => return Vec::new(),
        };
        let mut out = Vec::new();
        let mut i: u32 = 0;
        loop {
            match factory.EnumAdapters1(i) {
                Ok(adapter) => {
                    let mut desc: DXGI_ADAPTER_DESC1 = std::mem::zeroed();
                    if adapter.GetDesc1(&mut desc).is_ok() {
                        let wname = &desc.Description;
                        let len = wname.iter().position(|&c| c == 0).unwrap_or(wname.len());
                        let name = std::ffi::OsString::from_wide(&wname[..len])
                            .to_string_lossy()
                            .to_string();
                        out.push(json!({
                            "name": name,
                            "vendor_id": format!("0x{:04x}", desc.VendorId),
                            "device_id": format!("0x{:04x}", desc.DeviceId),
                            "dedicated_vram": (desc.DedicatedVideoMemory as u64),
                        }));
                    }
                    i += 1;
                }
                Err(e) => {
                    if e.code() == DXGI_ERROR_NOT_FOUND {
                        break;
                    } else {
                        break;
                    }
                }
            }
        }
        out
    }
}
#[cfg(not(any(target_os = "linux", target_os = "windows")))]
fn probe_gpus_best_effort() -> Vec<serde_json::Value> {
    Vec::new()
}

#[cfg(feature = "gpu_wgpu")]
fn probe_gpus_wgpu() -> Vec<serde_json::Value> {
    use serde_json::json;
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::default());
    let mut out = Vec::new();
    for adapter in instance.enumerate_adapters(wgpu::Backends::all()) {
        let info = adapter.get_info();
        let backend = match info.backend {
            wgpu::Backend::Empty => "empty",
            wgpu::Backend::Vulkan => "vulkan",
            wgpu::Backend::Metal => "metal",
            wgpu::Backend::Dx12 => "dx12",
            wgpu::Backend::Gl => "gl",
            wgpu::Backend::BrowserWebGpu => "webgpu",
        };
        out.push(json!({
            "name": info.name,
            "vendor": format!("0x{:04x}", info.vendor),
            "device": format!("0x{:04x}", info.device),
            "device_type": format!("{:?}", info.device_type).to_lowercase(),
            "backend": backend,
        }));
    }
    out
}

#[cfg(target_os = "linux")]
fn probe_gpus_linux() -> Vec<serde_json::Value> {
    use std::fs;
    use std::path::Path;
    let mut out: Vec<serde_json::Value> = Vec::new();
    let drm = Path::new("/sys/class/drm");
    if let Ok(entries) = fs::read_dir(drm) {
        for ent in entries.flatten() {
            let name = ent.file_name().to_string_lossy().into_owned();
            if !name.starts_with("card") || name.contains('-') {
                continue; // skip renderD* and control* symlinks
            }
            let path = ent.path();
            if !path.is_dir() {
                continue;
            }
            let dev = path.join("device");
            let vendor = fs::read_to_string(dev.join("vendor")).unwrap_or_default();
            let device = fs::read_to_string(dev.join("device")).unwrap_or_default();
            let vendor = vendor.trim().to_string();
            let device = device.trim().to_string();
            let vendor_name = match vendor.as_str() {
                "0x10de" => "NVIDIA",
                "0x1002" => "AMD",
                "0x8086" => "Intel",
                _ => "Unknown",
            };
            // PCI bus id from uevent
            let mut pci_bus = String::new();
            if let Ok(ue) = fs::read_to_string(dev.join("uevent")) {
                for line in ue.lines() {
                    if let Some(val) = line.strip_prefix("PCI_SLOT_NAME=") {
                        pci_bus = val.trim().to_string();
                        break;
                    }
                }
            }
            // driver name
            let mut driver = String::new();
            if let Ok(link) = fs::read_link(dev.join("driver")) {
                if let Some(b) = link.file_name() {
                    driver = b.to_string_lossy().to_string();
                }
            }
            // Extra per-vendor hints
            let mut model = String::new();
            let mut vram_total: Option<u64> = None;
            // NVIDIA: parse /proc/driver/nvidia/gpus/<pci>/information
            if vendor == "0x10de" && !pci_bus.is_empty() {
                let info_path = format!("/proc/driver/nvidia/gpus/{}/information", pci_bus);
                if let Ok(body) = fs::read_to_string(&info_path) {
                    for line in body.lines() {
                        if let Some(val) = line.strip_prefix("Model:") {
                            model = val.trim().to_string();
                        }
                        if let Some(val) = line.strip_prefix("FB Memory Total:") {
                            // e.g., " 16384 MiB"
                            let txt = val.trim();
                            let parts: Vec<&str> = txt.split_whitespace().collect();
                            if parts.len() >= 2 {
                                if let Ok(num) = parts[0].parse::<u64>() {
                                    let bytes = match parts[1].to_ascii_lowercase().as_str() {
                                        "mib" => num * 1024 * 1024,
                                        "gib" => num * 1024 * 1024 * 1024,
                                        _ => 0,
                                    };
                                    if bytes > 0 {
                                        vram_total = Some(bytes);
                                    }
                                }
                            }
                        }
                    }
                }
            }
            // AMD: try mem_info_vram_total
            if vendor == "0x1002" {
                let vpath = dev.join("mem_info_vram_total");
                if let Ok(s) = fs::read_to_string(&vpath) {
                    if let Ok(num) = s.trim().parse::<u64>() {
                        vram_total = Some(num);
                    }
                }
                // Expose product name when available
                let name_path = dev.join("product_name");
                if model.is_empty() {
                    if let Ok(s) = fs::read_to_string(&name_path) {
                        model = s.trim().to_string();
                    }
                }
            }
            out.push(serde_json::json!({
                "index": name,
                "vendor_id": vendor,
                "vendor": vendor_name,
                "device_id": device,
                "pci_bus": pci_bus,
                "driver": driver,
                "model": model,
                "vram_total": vram_total,
            }));
        }
    }
    out
}

fn read_small(p: &str) -> Option<String> {
    std::fs::read_to_string(p)
        .ok()
        .map(|s| s.trim().to_string())
}

fn read_cpuinfo_has_flag(flag: &str) -> bool {
    if let Ok(body) = std::fs::read_to_string("/proc/cpuinfo") {
        for line in body.lines() {
            if let Some(rest) = line.strip_prefix("flags") {
                if rest.contains(flag) {
                    return true;
                }
            }
        }
    }
    false
}

fn cpu_features() -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    // x86_64 common features
    #[cfg(target_arch = "x86_64")]
    {
        if std::is_x86_feature_detected!("sse4.2") {
            out.push("sse4.2".into());
        }
        if std::is_x86_feature_detected!("avx") {
            out.push("avx".into());
        }
        if std::is_x86_feature_detected!("avx2") {
            out.push("avx2".into());
        }
        if std::is_x86_feature_detected!("fma") {
            out.push("fma".into());
        }
        if std::is_x86_feature_detected!("aes") {
            out.push("aes".into());
        }
    }
    // aarch64 common features
    #[cfg(target_arch = "aarch64")]
    {
        if std::arch::is_aarch64_feature_detected!("neon") {
            out.push("neon".into());
        }
        if std::arch::is_aarch64_feature_detected!("asimd") {
            out.push("asimd".into());
        }
        if std::arch::is_aarch64_feature_detected!("pmull") {
            out.push("pmull".into());
        }
        if std::arch::is_aarch64_feature_detected!("aes") {
            out.push("aes".into());
        }
        if std::arch::is_aarch64_feature_detected!("sha2") {
            out.push("sha2".into());
        }
        if std::arch::is_aarch64_feature_detected!("sha3") {
            out.push("sha3".into());
        }
    }
    out
}

#[cfg(unix)]
#[allow(dead_code)]
fn nix_dev_from_md(md: &std::fs::Metadata) -> std::io::Result<u64> {
    use std::os::unix::fs::MetadataExt as _;
    Ok(md.dev())
}
#[cfg(not(unix))]
#[allow(dead_code)]
fn nix_dev_from_md(_md: &std::fs::Metadata) -> std::io::Result<u64> {
    Err(std::io::Error::new(
        std::io::ErrorKind::Other,
        "unsupported",
    ))
}

// ---- Disk probes (best-effort) ----
fn probe_disks_best_effort() -> Vec<serde_json::Value> {
    #[cfg(target_os = "linux")]
    {
        return probe_disks_linux();
    }
    #[cfg(target_os = "macos")]
    {
        return probe_disks_macos();
    }
    #[cfg(target_os = "windows")]
    {
        return probe_disks_windows();
    }
    #[allow(unreachable_code)]
    {
        Vec::new()
    }
}

#[cfg(target_os = "linux")]
fn probe_disks_linux() -> Vec<serde_json::Value> {
    use std::collections::HashSet;
    let mounts = std::fs::read_to_string("/proc/mounts").unwrap_or_default();
    let mut out: Vec<serde_json::Value> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    let allowed_fs: HashSet<&str> = [
        "ext4", "ext3", "ext2", "xfs", "btrfs", "zfs", "f2fs", "reiserfs", "ntfs", "vfat", "exfat",
        "overlay",
    ]
    .into_iter()
    .collect();
    for line in mounts.lines() {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() < 3 {
            continue;
        }
        let mount = parts[1];
        let fstype = parts[2];
        if !allowed_fs.contains(fstype) && mount != "/" {
            continue;
        }
        if seen.contains(mount) {
            continue;
        }
        seen.insert(mount.to_string());
        let p = std::path::Path::new(mount);
        let (total, avail) = (
            fs2::total_space(p).unwrap_or(0),
            fs2::available_space(p).unwrap_or(0),
        );
        out.push(
            serde_json::json!({"mount": mount, "fs": fstype, "total": total, "available": avail}),
        );
    }
    // Prefer a small set sorted by mount path length then name
    out.sort_by(|a, b| {
        a["mount"]
            .as_str()
            .unwrap_or("")
            .cmp(b["mount"].as_str().unwrap_or(""))
    });
    out
}

#[cfg(target_os = "macos")]
fn probe_disks_macos() -> Vec<serde_json::Value> {
    let mut paths: Vec<std::path::PathBuf> = vec![std::path::PathBuf::from("/")];
    // Add volumes
    if let Ok(rd) = std::fs::read_dir("/Volumes") {
        for ent in rd.flatten() {
            let p = ent.path();
            if p.is_dir() {
                paths.push(p);
            }
        }
    }
    let mut out = Vec::new();
    for p in paths {
        let (total, avail) = (
            fs2::total_space(&p).unwrap_or(0),
            fs2::available_space(&p).unwrap_or(0),
        );
        out.push(serde_json::json!({"mount": p, "total": total, "available": avail}));
    }
    out
}

#[cfg(target_os = "windows")]
fn probe_disks_windows() -> Vec<serde_json::Value> {
    let mut out = Vec::new();
    for letter in b'A'..=b'Z' {
        let root = format!("{}:\\", letter as char);
        let p = std::path::Path::new(&root);
        if std::fs::metadata(&p).is_ok() {
            let total = fs2::total_space(&p).unwrap_or(0);
            let avail = fs2::available_space(&p).unwrap_or(0);
            if total > 0 {
                out.push(serde_json::json!({"mount": root, "total": total, "available": avail}));
            }
        }
    }
    out
}

#[arw_admin(
    method = "GET",
    path = "/admin/probe/metrics",
    summary = "System metrics snapshot (CPU/mem/disk/GPU)"
)]
#[utoipa::path(
    get,
    path = "/admin/probe/metrics",
    tag = "Admin/Introspect",
    responses(
        (status = 200, description = "System metrics"),
        (status = 403, description = "Forbidden", body = arw_protocol::ProblemDetails)
    )
)]
#[arw_gate("introspect:probe")]
async fn probe_metrics(State(state): State<AppState>) -> impl IntoResponse {
    let out = collect_metrics_snapshot().await;
    // publish a compact event for dashboards (no polling needed)
    state.bus.publish(
        ext::topics::TOPIC_PROBE_METRICS,
        &serde_json::json!({
            "cpu": out["cpu"]["avg"],
            "mem": {"used": out["memory"]["used"], "total": out["memory"]["total"]},
            "disk": out["disk"],
            "gpus": out["gpus"],
            "npus": out["npus"],
        }),
    );
    ext::ok::<serde_json::Value>(out).into_response()
}

// Shared collector used by HTTP and background emitter
async fn collect_metrics_snapshot() -> serde_json::Value {
    use sysinfo::System;
    let mut sys = System::new();
    sys.refresh_memory();
    sys.refresh_cpu();
    tokio::time::sleep(std::time::Duration::from_millis(180)).await;
    sys.refresh_cpu();
    let per_core: Vec<f64> = sys.cpus().iter().map(|c| c.cpu_usage() as f64).collect();
    let avg = if per_core.is_empty() {
        0.0
    } else {
        per_core.iter().sum::<f64>() / (per_core.len() as f64)
    };
    let total_mem = sys.total_memory();
    let avail_mem = sys.available_memory();
    let used_mem = total_mem.saturating_sub(avail_mem);
    let swap_total = sys.total_swap();
    let swap_used = sys.used_swap();
    let sdir = crate::ext::paths::state_dir();
    let (disk_total, disk_avail) = (
        fs2::total_space(&sdir).unwrap_or(0),
        fs2::available_space(&sdir).unwrap_or(0),
    );
    let gpus = probe_gpu_metrics_best_effort_async().await;
    let npus = probe_npus_best_effort();
    serde_json::json!({
        "cpu": {"avg": avg, "per_core": per_core},
        "memory": {"total": total_mem, "used": used_mem, "available": avail_mem, "swap_total": swap_total, "swap_used": swap_used},
        "disk": {"state_dir": sdir, "total": disk_total, "available": disk_avail},
        "gpus": gpus,
        "npus": npus,
    })
}

#[cfg(target_os = "linux")]
fn probe_gpu_metrics_best_effort() -> Vec<serde_json::Value> {
    use std::fs;
    use std::path::Path;
    let mut out: Vec<serde_json::Value> = Vec::new();
    let drm = Path::new("/sys/class/drm");
    if let Ok(entries) = fs::read_dir(drm) {
        for ent in entries.flatten() {
            let name = ent.file_name().to_string_lossy().into_owned();
            if !name.starts_with("card") || name.contains('-') {
                continue;
            }
            let dev = ent.path().join("device");
            let vendor = fs::read_to_string(dev.join("vendor")).unwrap_or_default();
            let vendor = vendor.trim().to_string();
            let vendor_name = match vendor.as_str() {
                "0x10de" => "NVIDIA",
                "0x1002" => "AMD",
                "0x8086" => "Intel",
                _ => "Unknown",
            };
            let mut mem_used = None;
            let mut mem_total = None;
            let mut busy = None;
            if vendor == "0x1002" {
                if let Ok(s) = fs::read_to_string(dev.join("mem_info_vram_used")) {
                    if let Ok(n) = s.trim().parse::<u64>() {
                        mem_used = Some(n);
                    }
                }
                if let Ok(s) = fs::read_to_string(dev.join("mem_info_vram_total")) {
                    if let Ok(n) = s.trim().parse::<u64>() {
                        mem_total = Some(n);
                    }
                }
                if let Ok(s) = fs::read_to_string(dev.join("gpu_busy_percent")) {
                    if let Ok(n) = s.trim().parse::<u64>() {
                        busy = Some(n);
                    }
                }
            }
            out.push(serde_json::json!({
                "index": name,
                "vendor": vendor_name,
                "vendor_id": vendor,
                "mem_used": mem_used,
                "mem_total": mem_total,
                "busy_percent": busy,
            }));
        }
    }
    out
}
#[cfg(target_os = "windows")]
fn probe_gpu_metrics_best_effort() -> Vec<serde_json::Value> {
    use serde_json::json;
    use std::os::windows::ffi::OsStringExt as _;
    use windows::core::Interface as _;
    use windows::Win32::Graphics::Dxgi::{
        CreateDXGIFactory1, IDXGIAdapter3, IDXGIFactory1, DXGI_ADAPTER_DESC1, DXGI_ERROR_NOT_FOUND,
        DXGI_MEMORY_SEGMENT_GROUP_LOCAL, DXGI_MEMORY_SEGMENT_GROUP_NON_LOCAL,
        DXGI_QUERY_VIDEO_MEMORY_INFO,
    };
    unsafe {
        let factory: IDXGIFactory1 = match CreateDXGIFactory1::<IDXGIFactory1>() {
            Ok(f) => f,
            Err(_) => return Vec::new(),
        };
        let mut out = Vec::new();
        let mut i: u32 = 0;
        loop {
            match factory.EnumAdapters1(i) {
                Ok(adapter) => {
                    let mut desc: DXGI_ADAPTER_DESC1 = std::mem::zeroed();
                    if adapter.GetDesc1(&mut desc).is_ok() {
                        let wname = &desc.Description;
                        let len = wname.iter().position(|&c| c == 0).unwrap_or(wname.len());
                        let name = std::ffi::OsString::from_wide(&wname[..len])
                            .to_string_lossy()
                            .to_string();
                        // Best-effort current usage (requires IDXGIAdapter3)
                        let mut used_local: Option<u64> = None;
                        // QueryVideoMemoryInfo is advisory; ignore errors
                        if let Ok(adapter3) = adapter.cast::<IDXGIAdapter3>() {
                            let mut info: DXGI_QUERY_VIDEO_MEMORY_INFO = std::mem::zeroed();
                            if adapter3
                                .QueryVideoMemoryInfo(0, DXGI_MEMORY_SEGMENT_GROUP_LOCAL, &mut info)
                                .is_ok()
                            {
                                used_local = Some(info.CurrentUsage as u64);
                            }
                            // Optionally check non-local as a fallback
                            if used_local.is_none() {
                                let mut info2: DXGI_QUERY_VIDEO_MEMORY_INFO = std::mem::zeroed();
                                if adapter3
                                    .QueryVideoMemoryInfo(
                                        0,
                                        DXGI_MEMORY_SEGMENT_GROUP_NON_LOCAL,
                                        &mut info2,
                                    )
                                    .is_ok()
                                {
                                    used_local = Some(info2.CurrentUsage as u64);
                                }
                            }
                        }
                        out.push(json!({
                            "index": format!("adapter{}", i),
                            "vendor": "windows",
                            "vendor_id": format!("0x{:04x}", desc.VendorId),
                            "name": name,
                            "mem_total": (desc.DedicatedVideoMemory as u64),
                            "mem_used": used_local,
                        }));
                    }
                    i += 1;
                }
                Err(e) => {
                    if e.code() == DXGI_ERROR_NOT_FOUND {
                        break;
                    } else {
                        break;
                    }
                }
            }
        }
        out
    }
}
#[cfg(not(any(target_os = "linux", target_os = "windows")))]
fn probe_gpu_metrics_best_effort() -> Vec<serde_json::Value> {
    Vec::new()
}

// Async facade (Linux optionally uses ROCm SMI)
#[cfg(target_os = "linux")]
async fn probe_gpu_metrics_best_effort_async() -> Vec<serde_json::Value> {
    let mut base = probe_gpu_metrics_best_effort();
    if std::env::var("ARW_ROCM_SMI").ok().as_deref() == Some("1") {
        if let Some(extra) = rocm_smi_json().await {
            if let Some(obj) = extra.as_object() {
                for (k, v) in obj.iter() {
                    if !k.starts_with("card") {
                        continue;
                    }
                    if let Some(gpu) = base.iter_mut().find(|g| g["index"].as_str() == Some(k)) {
                        if let Some(map) = v.as_object() {
                            if gpu["busy_percent"].is_null() {
                                if let Some(bp) = pick_number(
                                    map,
                                    &["GPU use (%)", "GPU Utilization (%)", "GPU_Util"],
                                ) {
                                    gpu["busy_percent"] = serde_json::json!(bp as u64);
                                }
                            }
                            if gpu["mem_total"].is_null() {
                                if let Some(mt) =
                                    pick_number(map, &["VRAM Total (B)", "VRAM_Total_Bytes"])
                                {
                                    gpu["mem_total"] = serde_json::json!(mt as u64);
                                }
                            }
                            if gpu["mem_used"].is_null() {
                                if let Some(mu) =
                                    pick_number(map, &["VRAM Used (B)", "VRAM_Used_Bytes"])
                                {
                                    gpu["mem_used"] = serde_json::json!(mu as u64);
                                }
                            }
                            gpu["extra"]["rocm_smi"] = v.clone();
                        }
                    } else {
                        base.push(serde_json::json!({"index": k, "vendor":"AMD","vendor_id":"0x1002","extra": {"rocm_smi": v}}));
                    }
                }
            }
        }
    }
    base
}
#[cfg(not(target_os = "linux"))]
async fn probe_gpu_metrics_best_effort_async() -> Vec<serde_json::Value> {
    probe_gpu_metrics_best_effort()
}

#[cfg(target_os = "linux")]
async fn rocm_smi_json() -> Option<serde_json::Value> {
    use tokio::process::Command;
    use tokio::time::{timeout, Duration};
    let mut cmd = Command::new("rocm-smi");
    cmd.arg("--showuse")
        .arg("--showmeminfo")
        .arg("vram")
        .arg("--showtemp")
        .arg("--showclocks")
        .arg("--showpower")
        .arg("--json");
    match timeout(Duration::from_millis(1200), cmd.output()).await {
        Ok(Ok(out)) if out.status.success() => {
            let txt = String::from_utf8_lossy(&out.stdout);
            serde_json::from_str::<serde_json::Value>(&txt).ok()
        }
        _ => None,
    }
}
#[cfg(target_os = "linux")]
fn pick_number(map: &serde_json::Map<String, serde_json::Value>, keys: &[&str]) -> Option<f64> {
    for k in keys {
        if let Some(v) = map.get(*k) {
            if v.is_number() {
                return v.as_f64();
            }
            if let Some(s) = v.as_str() {
                let s = s.trim_end_matches('%');
                if let Ok(x) = s.parse::<f64>() {
                    return Some(x);
                }
            }
        }
    }
    None
}

// ---- NPU probes (best-effort) ----
#[cfg(target_os = "linux")]
fn probe_npus_best_effort() -> Vec<serde_json::Value> {
    use std::fs;
    use std::path::Path;
    let mut out: Vec<serde_json::Value> = Vec::new();
    let accel = Path::new("/sys/class/accel");
    if let Ok(entries) = fs::read_dir(accel) {
        for ent in entries.flatten() {
            let name = ent.file_name().to_string_lossy().into_owned();
            let dev = ent.path().join("device");
            let vendor = fs::read_to_string(dev.join("vendor")).unwrap_or_default();
            let device = fs::read_to_string(dev.join("device")).unwrap_or_default();
            let vendor = vendor.trim().to_string();
            let device = device.trim().to_string();
            let mut driver = String::new();
            if let Ok(link) = fs::read_link(dev.join("driver")) {
                if let Some(b) = link.file_name() {
                    driver = b.to_string_lossy().to_string();
                }
            }
            let mut pci_bus = String::new();
            if let Ok(ue) = fs::read_to_string(dev.join("uevent")) {
                for line in ue.lines() {
                    if let Some(val) = line.strip_prefix("PCI_SLOT_NAME=") {
                        pci_bus = val.trim().to_string();
                        break;
                    }
                }
            }
            out.push(serde_json::json!({
                "index": name,
                "vendor_id": vendor,
                "device_id": device,
                "driver": driver,
                "pci_bus": pci_bus,
            }));
        }
    }
    // Kernel module hints
    if let Ok(mods) = std::fs::read_to_string("/proc/modules") {
        let has_intel_vpu = mods.lines().any(|l| l.starts_with("intel_vpu "));
        let has_amd_xdna = mods.lines().any(|l| l.starts_with("amdxdna "));
        if has_intel_vpu || has_amd_xdna {
            out.push(serde_json::json!({"modules": {"intel_vpu": has_intel_vpu, "amdxdna": has_amd_xdna}}));
        }
    }
    out
}

#[cfg(target_os = "macos")]
fn probe_npus_best_effort() -> Vec<serde_json::Value> {
    let mut out = Vec::new();
    if std::env::consts::ARCH == "aarch64" {
        out.push(serde_json::json!({
            "vendor": "Apple",
            "name": "Neural Engine",
            "present": true
        }));
    }
    out
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
fn probe_npus_best_effort() -> Vec<serde_json::Value> {
    #[cfg(all(target_os = "windows", feature = "npu_dxcore"))]
    {
        if std::env::var("ARW_DXCORE_NPU").ok().as_deref() == Some("1") {
            return crate::win_npu_dxcore::probe();
        }
    }
    Vec::new()
}

#[arw_admin(method = "GET", path = "/admin/emit/test", summary = "Emit test event")]
#[utoipa::path(
    get,
    path = "/admin/emit/test",
    tag = "Admin/Core",
    operation_id = "emit_test_doc",
    description = "Emit a test event onto the internal event bus (for verification).",
    responses(
        (status = 200, description = "Emit test event", body = OkResponse),
        (status = 403, description = "Forbidden", body = arw_protocol::ProblemDetails)
    )
)]
#[arw_gate("admin:emit")]
async fn emit_test(State(state): State<AppState>) -> impl IntoResponse {
    let now_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    state.bus.publish(
        ext::topics::TOPIC_SERVICE_TEST,
        &json!({"msg":"ping","t": now_ms}),
    );
    // audit
    crate::ext::io::audit_event("admin.emit.test", &json!({"t": now_ms})).await;
    Json(OkResponse { ok: true }).into_response()
}

#[arw_admin(method = "GET", path = "/admin/shutdown", summary = "Shutdown service")]
#[utoipa::path(
    get,
    path = "/admin/shutdown",
    tag = "Admin/Core",
    operation_id = "shutdown_doc",
    responses(
        (status = 200, description = "Shutdown service", body = OkResponse),
        (status = 403, description = "Forbidden", body = arw_protocol::ProblemDetails)
    )
)]
#[arw_gate("admin:shutdown")]
async fn shutdown(State(state): State<AppState>) -> impl IntoResponse {
    state.bus.publish(
        ext::topics::TOPIC_SERVICE_STOP,
        &json!({"reason":"user request"}),
    );
    // audit
    crate::ext::io::audit_event("admin.shutdown", &json!({"reason": "user request"})).await;
    if let Some(tx) = &state.stop_tx {
        let _ = tx.send(());
    }
    Json(OkResponse { ok: true }).into_response()
}

#[derive(serde::Deserialize)]
struct EventsQs {
    #[serde(default)]
    replay: Option<usize>,
    #[serde(default)]
    prefix: Vec<String>,
}
#[arw_admin(
    method = "GET",
    path = "/admin/events",
    summary = "Server-sent events stream"
)]
#[utoipa::path(
    get,
    path = "/admin/events",
    tag = "Admin/Core",
    operation_id = "events_doc",
    description = "Server-Sent Events stream; emits JSON envelopes with CloudEvents metadata; supports Last-Event-ID resume and ?replay=N.",
    responses(
        (status = 200, description = "SSE event stream"),
        (status = 403, description = "Forbidden", body = arw_protocol::ProblemDetails)
    )
)]
async fn events(
    State(state): State<AppState>,
    Query(q): Query<EventsQs>,
    headers: HeaderMap,
    Extension(rid): Extension<String>,
) -> Sse<impl tokio_stream::Stream<Item = Result<Event, Infallible>>> {
    // Helper: format SSE data as either ARW envelope JSON (default) or CloudEvents structured JSON (opt-in)
    let ce_mode = std::env::var("ARW_EVENTS_SSE_MODE")
        .ok()
        .unwrap_or_else(|| "envelope".into());
    let decorate = std::env::var("ARW_EVENTS_SSE_DECORATE").ok().as_deref() == Some("1");
    let rid_sse = rid.clone();
    fn sse_data_for2(mode: &str, env: &arw_events::Envelope, decorate: bool, rid: &str) -> String {
        if mode.eq_ignore_ascii_case("ce-structured") {
            // CloudEvents structured content; fall back to env fields when ce meta is absent
            let (specversion, type_name, source, id, time, dct) = match &env.ce {
                Some(ce) => (
                    ce.specversion.clone(),
                    ce.type_name.clone(),
                    ce.source.clone(),
                    ce.id.clone(),
                    ce.time.clone(),
                    ce.datacontenttype
                        .clone()
                        .unwrap_or_else(|| "application/json".into()),
                ),
                None => (
                    "1.0".into(),
                    env.kind.clone(),
                    std::env::var("ARW_EVENT_SOURCE").unwrap_or_else(|_| "arw-svc".into()),
                    env.time.clone(),
                    env.time.clone(),
                    "application/json".into(),
                ),
            };
            let mut v = serde_json::json!({
                "specversion": specversion,
                "type": type_name,
                "source": source,
                "id": id,
                "time": time,
                "datacontenttype": dct,
                "data": env.payload.clone(),
            });
            if decorate {
                if let Some(obj) = v.as_object_mut() {
                    obj.insert(
                        "arw_request_id".into(),
                        serde_json::Value::String(rid.to_string()),
                    );
                }
            }
            v.to_string()
        } else {
            let mut v = serde_json::to_value(env).unwrap_or_else(|_| serde_json::json!({}));
            if decorate {
                if let Some(obj) = v.as_object_mut() {
                    obj.insert(
                        "request_id".into(),
                        serde_json::Value::String(rid.to_string()),
                    );
                }
            }
            serde_json::to_string(&v).unwrap_or_else(|_| "{}".to_string())
        }
    }
    // audit subscription
    crate::ext::io::audit_event(
        "admin.events.subscribe",
        &json!({"prefix": q.prefix, "replay": q.replay.unwrap_or(0)}),
    )
    .await;
    let rx = if !q.prefix.is_empty() {
        state.bus.subscribe_filtered(q.prefix.clone(), None)
    } else {
        state.bus.subscribe()
    };
    let bus_for_lag = state.bus.clone();
    let ce_mode_b = ce_mode.clone();
    let rid_b = rid_sse.clone();
    let bstream = BroadcastStream::new(rx).flat_map(move |res| match res {
        Ok(env) => {
            let data = sse_data_for2(&ce_mode_b, &env, decorate, &rid_b);
            tokio_stream::once(Ok::<Event, Infallible>(
                Event::default()
                    .id(env.time.clone())
                    .event(env.kind.clone())
                    .data(data),
            ))
        }
        Err(BroadcastStreamRecvError::Lagged(n)) => {
            bus_for_lag.note_lag(n);
            let body = format!("{{\"skipped\":{}}}", n);
            tokio_stream::once(Ok::<Event, Infallible>(
                Event::default().id("gap").event("bus.gap").data(body),
            ))
        }
    });

    // optional replay of last N bus events (best-effort)
    let rcount = q.replay.unwrap_or(0).min(1000);
    let replay_items = if rcount > 0 {
        state.bus.replay(rcount)
    } else {
        Vec::new()
    };
    let ce_mode_r = ce_mode.clone();
    let rid_r = rid_sse.clone();
    let replay_stream = tokio_stream::iter(replay_items.into_iter().map(move |env| {
        let data = sse_data_for2(&ce_mode_r, &env, decorate, &rid_r);
        Ok::<Event, Infallible>(
            Event::default()
                .id(env.time.clone())
                .event(env.kind.clone())
                .data(data),
        )
    }));

    let resume_from = headers
        .get("Last-Event-ID")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());
    let init_json = serde_json::json!({
        "resume_from": resume_from,
        "request_id": rid,
    });
    let init_body = serde_json::to_string(&init_json).unwrap_or_else(|_| "{}".to_string());
    let initial = tokio_stream::once(Ok::<Event, Infallible>(
        Event::default()
            .id("0")
            .event("service.connected")
            .data(init_body),
    ));
    // merge: connected -> replay -> live
    let stream = tokio_stream::StreamExt::chain(initial, replay_stream);
    let stream = tokio_stream::StreamExt::chain(stream, bstream);

    Sse::new(stream).keep_alive(
        KeepAlive::new()
            .interval(Duration::from_secs(15))
            .text("hb"),
    )
}

// --- OpenAPI-only wrappers for feedback endpoints (for documentation) ---
#[allow(dead_code)]
#[utoipa::path(get, path = "/admin/feedback/suggestions", tag = "Admin/Feedback", responses(
    (status=200, description="Versioned suggestions"),
    (status=403, description="Forbidden", body = arw_protocol::ProblemDetails)
))]
async fn feedback_suggestions_doc() -> impl IntoResponse {
    ext::feedback_engine_api::feedback_suggestions().await
}
#[allow(dead_code)]
#[utoipa::path(get, path = "/admin/feedback/updates", tag = "Admin/Feedback", params(("since" = Option<u64>, Query, description = "Return updates if newer than this version")), responses(
    (status=200, description="Latest version"),
    (status=204, description="No change"),
    (status=403, description="Forbidden", body = arw_protocol::ProblemDetails)
))]
async fn feedback_updates_doc(
    Query(q): Query<ext::feedback_engine_api::UpdatesQs>,
) -> impl IntoResponse {
    ext::feedback_engine_api::feedback_updates(Query(q)).await
}
#[allow(dead_code)]
#[utoipa::path(get, path = "/admin/feedback/policy", tag = "Admin/Feedback", responses(
    (status=200, description="Effective policy caps/bounds"),
    (status=403, description="Forbidden", body = arw_protocol::ProblemDetails)
))]
async fn feedback_policy_doc() -> impl IntoResponse {
    ext::feedback_engine_api::feedback_policy_get().await
}
#[derive(OpenApi)]
#[openapi(
    paths(
        healthz,
        introspect_tools,
        introspect_schema,
        probe,
        emit_test,
        shutdown,
        events,
        metrics_doc,
        spec_openapi_doc,
        spec_asyncapi_doc,
        spec_mcp_doc,
        spec_index_doc,
        catalog_index_doc,
        catalog_health_doc,
        version_doc,
        about_doc,
        feedback_suggestions_doc,
        feedback_updates_doc,
        feedback_policy_doc,
        memory_get_doc,
        memory_limit_get_doc,
        memory_save_doc,
        memory_load_doc,
        memory_apply_doc,
        memory_limit_set_doc,
        models_list_doc,
        models_summary_doc,
        models_default_get_doc,
        models_refresh_doc,
        models_save_doc,
        models_load_doc,
        models_add_doc,
        models_delete_doc,
        models_default_set_doc,
        models_download_doc,
        models_download_cancel_doc,
        // state_models_hashes_doc,
        models_jobs_doc,
        models_concurrency_get_doc,
        models_concurrency_set_doc,
        tools_list_doc,
        tools_run_doc,
        state_observations_doc,
        models_public_list_doc,
        state_beliefs_doc,
        state_intents_doc,
        state_actions_doc,
        chat_get_doc,
        chat_send_doc,
        chat_clear_doc,
        governor_profile_get_doc,
        governor_hints_get_doc,
        governor_profile_set_doc,
        governor_hints_set_doc,
        hierarchy_state_doc,
        hierarchy_role_set_doc,
        hierarchy_hello_doc,
        hierarchy_offer_doc,
        hierarchy_accept_doc,
        projects_list_doc,
        projects_create_doc,
        projects_notes_set_doc,
        feedback_state_get_doc,
        feedback_signal_post_doc,
        feedback_analyze_post_doc,
        feedback_apply_post_doc,
        feedback_auto_post_doc,
        feedback_reset_post_doc,
        tasks_enqueue_doc,
        // Public blob-by-digest endpoint (not wrapped)
        ext::models_api::models_blob_get,
        // new read-model and tools endpoints
        // state_models_metrics_doc,
        // state_route_stats_doc,
        // tools_cache_stats_doc
    ),
    info(
        title = "Agent Hub (ARW) Service API",
        description = "Your private AI control room that can scale and share when you choose.\n\nIn plain terms: Agent Hub (ARW) lets you run your own team of AI \u{201C}helpers\u{201D} on your computer to research, plan, write, and build—while you stay in charge. It is local\u{2011}first and privacy\u{2011}first by default, with the option to securely pool computing power with trusted peers when a project needs more muscle.\n",
        license(name = ""),
        version = "0.1.0"
    ),
    tags(
        (name = "Admin/Chat", description = "Admin/Chat endpoints"),
        (name = "Admin/Core", description = "Admin/Core endpoints"),
        (name = "Admin/Feedback", description = "Admin/Feedback endpoints"),
        (name = "Admin/Governor", description = "Admin/Governor endpoints"),
        (name = "Admin/Hierarchy", description = "Admin/Hierarchy endpoints"),
        (name = "Admin/Introspect", description = "Admin/Introspect endpoints"),
        (name = "Admin/Memory", description = "Admin/Memory endpoints"),
        (name = "Admin/Models", description = "Admin/Models endpoints"),
        (name = "Admin/Projects", description = "Admin/Projects endpoints"),
        (name = "Admin/State", description = "Admin/State endpoints"),
        (name = "Admin/Tasks", description = "Admin/Tasks endpoints"),
        (name = "Admin/Tools", description = "Admin/Tools endpoints"),
        (name = "Public", description = "Public endpoints"),
        (name = "Public/Specs", description = "Public/Specs endpoints")
    )
)]
struct ApiDoc;

// --- OpenAPI-only wrappers for common admin endpoints ---
#[allow(dead_code)]
#[utoipa::path(get, path = "/admin/models/summary", tag = "Admin/Models", responses(
    (status=200, description="Models summary", body = ext::models_api::ModelsSummary)
))]
async fn models_summary_doc() -> impl IntoResponse {
    ext::models_api::models_summary(State(AppState {
        bus: arw_events::Bus::new_with_replay(1, 1),
        stop_tx: None,
        queue: std::sync::Arc::new(arw_core::orchestrator::LocalQueue::new()),
        resources: Resources::new(),
    }))
    .await
}

#[allow(dead_code)]
#[utoipa::path(get, path = "/admin/memory", tag = "Admin/Memory", responses(
    (status=200, description="Memory snapshot"),
    (status=403, description="Forbidden", body = arw_protocol::ProblemDetails)
))]
async fn memory_get_doc() -> impl IntoResponse {
    ext::memory_api::memory_get(State(AppState {
        bus: arw_events::Bus::new_with_replay(1, 1),
        stop_tx: None,
        queue: std::sync::Arc::new(arw_core::orchestrator::LocalQueue::new()),
        resources: Resources::new(),
    }))
    .await
}
#[allow(dead_code)]
#[utoipa::path(get, path = "/admin/memory/limit", tag = "Admin/Memory", responses(
    (status=200, description="Memory limit"),
    (status=403, description="Forbidden", body = arw_protocol::ProblemDetails)
))]
async fn memory_limit_get_doc() -> impl IntoResponse {
    ext::memory_api::memory_limit_get(State(AppState {
        bus: arw_events::Bus::new_with_replay(1, 1),
        stop_tx: None,
        queue: std::sync::Arc::new(arw_core::orchestrator::LocalQueue::new()),
        resources: Resources::new(),
    }))
    .await
}
#[allow(dead_code)]
#[utoipa::path(get, path = "/admin/models", tag = "Admin/Models", responses(
    (status=200, description="Models list"),
    (status=403, description="Forbidden", body = arw_protocol::ProblemDetails)
))]
async fn models_list_doc() -> impl IntoResponse {
    ext::models_api::list_models(State(AppState {
        bus: arw_events::Bus::new_with_replay(1, 1),
        stop_tx: None,
        queue: std::sync::Arc::new(arw_core::orchestrator::LocalQueue::new()),
        resources: Resources::new(),
    }))
    .await
}
#[allow(dead_code)]
#[utoipa::path(get, path = "/admin/models/default", tag = "Admin/Models", responses(
    (status=200, description="Default model"),
    (status=403, description="Forbidden", body = arw_protocol::ProblemDetails)
))]
async fn models_default_get_doc() -> impl IntoResponse {
    ext::models_api::models_default_get(State(AppState {
        bus: arw_events::Bus::new_with_replay(1, 1),
        stop_tx: None,
        queue: std::sync::Arc::new(arw_core::orchestrator::LocalQueue::new()),
        resources: Resources::new(),
    }))
    .await
}
#[allow(dead_code)]
#[utoipa::path(get, path = "/admin/governor/profile", tag = "Admin/Governor", responses(
    (status=200, description="Governor profile"),
    (status=403, description="Forbidden", body = arw_protocol::ProblemDetails)
))]
async fn governor_profile_get_doc() -> impl IntoResponse {
    ext::governor_api::governor_get(State(AppState {
        bus: arw_events::Bus::new_with_replay(1, 1),
        stop_tx: None,
        queue: std::sync::Arc::new(arw_core::orchestrator::LocalQueue::new()),
        resources: Resources::new(),
    }))
    .await
}
#[allow(dead_code)]
#[utoipa::path(get, path = "/admin/governor/hints", tag = "Admin/Governor", responses(
    (status=200, description="Governor hints"),
    (status=403, description="Forbidden", body = arw_protocol::ProblemDetails)
))]
async fn governor_hints_get_doc() -> impl IntoResponse {
    ext::governor_api::governor_hints_get(State(AppState {
        bus: arw_events::Bus::new_with_replay(1, 1),
        stop_tx: None,
        queue: std::sync::Arc::new(arw_core::orchestrator::LocalQueue::new()),
        resources: Resources::new(),
    }))
    .await
}
#[allow(dead_code)]
#[utoipa::path(get, path = "/admin/tools", tag = "Admin/Tools", responses(
    (status=200, description="Tools list"),
    (status=403, description="Forbidden", body = arw_protocol::ProblemDetails)
))]
async fn tools_list_doc() -> impl IntoResponse {
    ext::tools_api::list_tools().await
}

// --- POST admin doc wrappers (no-op bodies; for OpenAPI only) ---
#[allow(dead_code)]
#[utoipa::path(post, path = "/admin/memory/save", tag = "Admin/Memory", responses(
    (status=200, description="Saved", body = OkResponse),
    (status=403, description="Forbidden", body = arw_protocol::ProblemDetails),
    (status=500, description="Error", body = arw_protocol::ProblemDetails)
))]
async fn memory_save_doc() -> impl IntoResponse {
    Json(json!({"ok": true}))
}
#[allow(dead_code)]
#[utoipa::path(post, path = "/admin/memory/load", tag = "Admin/Memory", responses(
    (status=200, description="Loaded"),
    (status=404, description="Not Found", body = arw_protocol::ProblemDetails),
    (status=403, description="Forbidden", body = arw_protocol::ProblemDetails)
))]
async fn memory_load_doc() -> impl IntoResponse {
    Json(json!({"ok": true}))
}
#[allow(dead_code)]
#[utoipa::path(post, path = "/admin/memory/apply", tag = "Admin/Memory", request_body = ext::ApplyMemory, responses(
    (status=202, description="Accepted", body = OkResponse),
    (status=400, description="Bad Request", body = arw_protocol::ProblemDetails),
    (status=403, description="Forbidden", body = arw_protocol::ProblemDetails)
))]
async fn memory_apply_doc(Json(_req): Json<ext::ApplyMemory>) -> impl IntoResponse {
    (StatusCode::ACCEPTED, Json(json!({"ok": true}))).into_response()
}
#[allow(dead_code)]
#[utoipa::path(post, path = "/admin/memory/limit", tag = "Admin/Memory", request_body = ext::SetLimit, responses(
    (status=200, description="Set", body = OkResponse),
    (status=403, description="Forbidden", body = arw_protocol::ProblemDetails)
))]
async fn memory_limit_set_doc(Json(_req): Json<ext::SetLimit>) -> impl IntoResponse {
    Json(json!({"ok": true}))
}

#[allow(dead_code)]
#[utoipa::path(post, path = "/admin/models/refresh", tag = "Admin/Models", responses(
    (status=200, description="Refreshed", body = OkResponse),
    (status=403, description="Forbidden", body = arw_protocol::ProblemDetails)
))]
async fn models_refresh_doc() -> impl IntoResponse {
    Json(json!({"ok": true}))
}
#[allow(dead_code)]
#[utoipa::path(post, path = "/admin/models/save", tag = "Admin/Models", responses(
    (status=200, description="Saved", body = OkResponse),
    (status=403, description="Forbidden", body = arw_protocol::ProblemDetails)
))]
async fn models_save_doc() -> impl IntoResponse {
    Json(json!({"ok": true}))
}
#[allow(dead_code)]
#[utoipa::path(post, path = "/admin/models/load", tag = "Admin/Models", responses(
    (status=200, description="Loaded", body = OkResponse),
    (status=403, description="Forbidden", body = arw_protocol::ProblemDetails)
))]
async fn models_load_doc() -> impl IntoResponse {
    Json(json!({"ok": true}))
}
#[allow(dead_code)]
#[utoipa::path(post, path = "/admin/models/add", tag = "Admin/Models", request_body = ext::models_api::ModelId, responses(
    (status=200, description="Added", body = OkResponse),
    (status=400, description="Bad Request", body = arw_protocol::ProblemDetails),
    (status=403, description="Forbidden", body = arw_protocol::ProblemDetails)
))]
async fn models_add_doc(Json(_req): Json<ext::models_api::ModelId>) -> impl IntoResponse {
    Json(json!({"ok": true}))
}
#[allow(dead_code)]
#[utoipa::path(post, path = "/admin/models/delete", tag = "Admin/Models", request_body = ext::models_api::ModelId, responses(
    (status=200, description="Deleted", body = OkResponse),
    (status=403, description="Forbidden", body = arw_protocol::ProblemDetails)
))]
async fn models_delete_doc(Json(_req): Json<ext::models_api::ModelId>) -> impl IntoResponse {
    Json(json!({"ok": true}))
}
#[allow(dead_code)]
#[utoipa::path(post, path = "/admin/models/default", tag = "Admin/Models", request_body = ext::models_api::ModelId, responses(
    (status=200, description="Default set", body = OkResponse),
    (status=403, description="Forbidden", body = arw_protocol::ProblemDetails)
))]
async fn models_default_set_doc(Json(_req): Json<ext::models_api::ModelId>) -> impl IntoResponse {
    Json(json!({"ok": true}))
}
#[allow(dead_code)]
#[utoipa::path(post, path = "/admin/models/download", tag = "Admin/Models", request_body = ext::models_api::DownloadReq, responses(
    (status=200, description="Started", body = OkResponse),
    (status=400, description="Bad Request", body = arw_protocol::ProblemDetails),
    (status=403, description="Forbidden", body = arw_protocol::ProblemDetails),
    (status=500, description="Error", body = arw_protocol::ProblemDetails)
))]
async fn models_download_doc(Json(_req): Json<ext::models_api::DownloadReq>) -> impl IntoResponse {
    Json(json!({"ok": true}))
}
#[allow(dead_code)]
#[utoipa::path(post, path = "/admin/models/download/cancel", tag = "Admin/Models", request_body = ext::models_api::CancelReq, responses(
    (status=200, description="Canceled", body = OkResponse),
    (status=403, description="Forbidden", body = arw_protocol::ProblemDetails)
))]
async fn models_download_cancel_doc(
    Json(_req): Json<ext::models_api::CancelReq>,
) -> impl IntoResponse {
    Json(json!({"ok": true}))
}

// Public: models hashes with pagination/filtering/sorting
#[allow(dead_code)]
#[utoipa::path(
    get,
    path = "/state/models_hashes",
    tag = "Public/State",
    params(
        ("limit" = Option<usize>, Query, description = "Max items to return (default 200, max 10000)"),
        ("offset" = Option<usize>, Query, description = "Starting index (default 0)"),
        ("provider" = Option<String>, Query, description = "Filter by provider id") ,
        ("sort" = Option<String>, Query, description = "Sort key: bytes|sha256|path|providers_count (default bytes)"),
        ("order" = Option<String>, Query, description = "Sort order: asc|desc (default desc for bytes, asc otherwise)")
    ),
    responses(
        (status = 200, description = "Paginated hashes list", body = ext::models_api::HashesPage),
        (status = 403, description = "Forbidden", body = arw_protocol::ProblemDetails)
    )
)]
async fn state_models_hashes_doc(
    Query(_q): Query<ext::models_api::ModelsHashesQs>,
) -> impl IntoResponse {
    Json(json!({}))
}

// Public: models metrics snapshot (counters + EWMA)
#[allow(dead_code)]
#[utoipa::path(
    get,
    path = "/state/models_metrics",
    tag = "Public/State",
    responses(
        (status = 200, description = "Download metrics", body = ext::models_api::ModelsMetrics),
        (status = 403, description = "Forbidden", body = arw_protocol::ProblemDetails)
    )
)]
async fn state_models_metrics_public_doc() -> impl IntoResponse {
    Json(json!({}))
}

// --- OpenAPI-only wrappers for models jobs/concurrency ---
#[allow(dead_code)]
#[utoipa::path(get, path = "/admin/models/jobs", tag = "Admin/Models", responses(
    (status=200, description="Jobs status", body = ext::models_api::ModelsJobs),
    (status=403, description="Forbidden", body = arw_protocol::ProblemDetails)
))]
async fn models_jobs_doc() -> impl IntoResponse {
    ext::models_api::models_jobs(State(AppState {
        bus: arw_events::Bus::new_with_replay(1, 1),
        stop_tx: None,
        queue: std::sync::Arc::new(arw_core::orchestrator::LocalQueue::new()),
        resources: Resources::new(),
    }))
    .await
}
#[allow(dead_code)]
#[utoipa::path(get, path = "/admin/models/concurrency", tag = "Admin/Models", responses(
    (status=200, description="Concurrency settings", body = ext::models_api::ModelsConcurrency),
    (status=403, description="Forbidden", body = arw_protocol::ProblemDetails)
))]
async fn models_concurrency_get_doc() -> impl IntoResponse {
    ext::models_api::models_concurrency_get(State(AppState {
        bus: arw_events::Bus::new_with_replay(1, 1),
        stop_tx: None,
        queue: std::sync::Arc::new(arw_core::orchestrator::LocalQueue::new()),
        resources: Resources::new(),
    }))
    .await
}
#[allow(dead_code)]
#[utoipa::path(post, path = "/admin/models/concurrency", tag = "Admin/Models", request_body = ext::models_api::ConcurrencySetReq, responses(
    (status=200, description="Set", body = OkResponse),
    (status=403, description="Forbidden", body = arw_protocol::ProblemDetails)
))]
async fn models_concurrency_set_doc(
    Json(_req): Json<ext::models_api::ConcurrencySetReq>,
) -> impl IntoResponse {
    Json(json!({"ok": true}))
}

#[allow(dead_code)]
#[utoipa::path(post, path = "/admin/tools/run", tag = "Admin/Tools", request_body = ext::tools_api::ToolRunReq, responses(
    (status=200, description="Tool output"),
    (status=403, description="Forbidden", body = arw_protocol::ProblemDetails)
))]
async fn tools_run_doc(Json(_req): Json<ext::tools_api::ToolRunReq>) -> impl IntoResponse {
    Json(json!({}))
}

#[allow(dead_code)]
#[utoipa::path(post, path = "/admin/chat/send", tag = "Admin/Chat", summary = "Deprecated: Send chat message", description = "Deprecated dev helper to send a message to the synthetic chat backend.", request_body = ext::chat_api::ChatSendReq, responses(
    (status=200, description="Message sent", body = OkResponse),
    (status=403, description="Forbidden", body = arw_protocol::ProblemDetails)
))]
async fn chat_send_doc(Json(_req): Json<ext::chat_api::ChatSendReq>) -> impl IntoResponse {
    Json(json!({"ok": true}))
}
#[allow(dead_code)]
#[utoipa::path(post, path = "/admin/chat/clear", tag = "Admin/Chat", summary = "Deprecated: Clear chat history", description = "Deprecated dev helper to clear in-memory chat history.", responses(
    (status=200, description="Cleared", body = OkResponse),
    (status=403, description="Forbidden", body = arw_protocol::ProblemDetails)
))]
async fn chat_clear_doc() -> impl IntoResponse {
    Json(json!({"ok": true}))
}

#[allow(dead_code)]
#[utoipa::path(post, path = "/admin/governor/profile", tag = "Admin/Governor", request_body = ext::governor_api::SetProfile, responses(
    (status=200, description="Set", body = OkResponse),
    (status=403, description="Forbidden", body = arw_protocol::ProblemDetails)
))]
async fn governor_profile_set_doc(
    Json(_req): Json<ext::governor_api::SetProfile>,
) -> impl IntoResponse {
    Json(json!({"ok": true}))
}
#[allow(dead_code)]
#[utoipa::path(post, path = "/admin/governor/hints", tag = "Admin/Governor", request_body = ext::governor_api::Hints, responses(
    (status=200, description="Set", body = OkResponse),
    (status=403, description="Forbidden", body = arw_protocol::ProblemDetails)
))]
async fn governor_hints_set_doc(Json(_req): Json<ext::governor_api::Hints>) -> impl IntoResponse {
    Json(json!({"ok": true}))
}

#[derive(serde::Deserialize, utoipa::ToSchema)]
#[allow(dead_code)]
struct RoleSetDoc {
    role: String,
}
#[allow(dead_code)]
#[utoipa::path(post, path = "/admin/hierarchy/role", tag = "Admin/Hierarchy", request_body = RoleSetDoc, responses(
    (status=200, description="Set", body = OkResponse),
    (status=403, description="Forbidden", body = arw_protocol::ProblemDetails)
))]
async fn hierarchy_role_set_doc(Json(_req): Json<RoleSetDoc>) -> impl IntoResponse {
    Json(json!({"ok": true}))
}
#[allow(dead_code)]
#[utoipa::path(post, path = "/admin/hierarchy/hello", tag = "Admin/Hierarchy", request_body = arw_protocol::CoreHello, responses(
    (status=200, description="Hello", body = OkResponse),
    (status=403, description="Forbidden", body = arw_protocol::ProblemDetails)
))]
async fn hierarchy_hello_doc(Json(_req): Json<arw_protocol::CoreHello>) -> impl IntoResponse {
    Json(json!({"ok": true}))
}
#[allow(dead_code)]
#[utoipa::path(post, path = "/admin/hierarchy/offer", tag = "Admin/Hierarchy", request_body = arw_protocol::CoreOffer, responses(
    (status=200, description="Offer", body = OkResponse),
    (status=403, description="Forbidden", body = arw_protocol::ProblemDetails)
))]
async fn hierarchy_offer_doc(Json(_req): Json<arw_protocol::CoreOffer>) -> impl IntoResponse {
    Json(json!({"ok": true}))
}
#[allow(dead_code)]
#[utoipa::path(post, path = "/admin/hierarchy/accept", tag = "Admin/Hierarchy", request_body = arw_protocol::CoreAccept, responses(
    (status=200, description="Accept", body = OkResponse),
    (status=403, description="Forbidden", body = arw_protocol::ProblemDetails)
))]
async fn hierarchy_accept_doc(Json(_req): Json<arw_protocol::CoreAccept>) -> impl IntoResponse {
    Json(json!({"ok": true}))
}

#[allow(dead_code)]
#[utoipa::path(post, path = "/admin/projects/create", tag = "Admin/Projects", request_body = ext::projects::ProjCreateReq, responses(
    (status=200, description="Created", body = OkResponse),
    (status=400, description="Bad Request", body = arw_protocol::ProblemDetails),
    (status=403, description="Forbidden", body = arw_protocol::ProblemDetails)
))]
async fn projects_create_doc(Json(_req): Json<ext::projects::ProjCreateReq>) -> impl IntoResponse {
    Json(json!({"ok": true}))
}
#[derive(serde::Deserialize, utoipa::ToSchema)]
#[allow(dead_code)]
struct NotesSetDoc {
    #[serde(default)]
    proj: String,
    #[serde(default)]
    body: String,
}
#[allow(dead_code)]
#[utoipa::path(
    post,
    path = "/admin/projects/notes",
    tag = "Admin/Projects",
    params(("proj" = String, Query, description = "Project name")),
    request_body = String,
    responses(
        (status=200, description="Saved", body = OkResponse),
        (status=403, description="Forbidden", body = arw_protocol::ProblemDetails)
    )
)]
async fn projects_notes_set_doc() -> impl IntoResponse {
    Json(json!({"ok": true}))
}

#[allow(dead_code)]
#[utoipa::path(get, path = "/admin/feedback/state", tag = "Admin/Feedback", responses(
    (status=200, description="Feedback state"),
    (status=403, description="Forbidden", body = arw_protocol::ProblemDetails)
))]
async fn feedback_state_get_doc() -> impl IntoResponse {
    ext::feedback_api::feedback_state_get().await
}
#[allow(dead_code)]
#[utoipa::path(post, path = "/admin/feedback/signal", tag = "Admin/Feedback", request_body = ext::feedback_api::FeedbackSignalPost, responses(
    (status=200, description="OK", body = OkResponse),
    (status=403, description="Forbidden", body = arw_protocol::ProblemDetails)
))]
async fn feedback_signal_post_doc(
    Json(_req): Json<ext::feedback_api::FeedbackSignalPost>,
) -> impl IntoResponse {
    Json(json!({"ok": true}))
}
#[allow(dead_code)]
#[utoipa::path(post, path = "/admin/feedback/analyze", tag = "Admin/Feedback", responses(
    (status=200, description="OK", body = OkResponse),
    (status=403, description="Forbidden", body = arw_protocol::ProblemDetails)
))]
async fn feedback_analyze_post_doc() -> impl IntoResponse {
    Json(json!({"ok": true}))
}
#[allow(dead_code)]
#[utoipa::path(post, path = "/admin/feedback/apply", tag = "Admin/Feedback", request_body = ext::feedback_api::ApplyReq, responses(
    (status=200, description="OK", body = OkResponse),
    (status=403, description="Forbidden", body = arw_protocol::ProblemDetails)
))]
async fn feedback_apply_post_doc(
    Json(_req): Json<ext::feedback_api::ApplyReq>,
) -> impl IntoResponse {
    Json(json!({"ok": true}))
}
#[allow(dead_code)]
#[utoipa::path(post, path = "/admin/feedback/auto", tag = "Admin/Feedback", request_body = ext::feedback_api::AutoReq, responses(
    (status=200, description="OK", body = OkResponse),
    (status=403, description="Forbidden", body = arw_protocol::ProblemDetails)
))]
async fn feedback_auto_post_doc(Json(_req): Json<ext::feedback_api::AutoReq>) -> impl IntoResponse {
    Json(json!({"ok": true}))
}
#[allow(dead_code)]
#[utoipa::path(post, path = "/admin/feedback/reset", tag = "Admin/Feedback", responses(
    (status=200, description="OK", body = OkResponse),
    (status=403, description="Forbidden", body = arw_protocol::ProblemDetails)
))]
async fn feedback_reset_post_doc() -> impl IntoResponse {
    Json(json!({"ok": true}))
}

#[allow(dead_code)]
#[utoipa::path(post, path = "/admin/tasks/enqueue", tag = "Admin/Tasks", request_body = ext::EnqueueReq, responses(
    (status=200, description="OK", body = OkResponse),
    (status=403, description="Forbidden", body = arw_protocol::ProblemDetails)
))]
async fn tasks_enqueue_doc(Json(_req): Json<ext::EnqueueReq>) -> impl IntoResponse {
    Json(json!({"ok": true}))
}
#[allow(dead_code)]
#[utoipa::path(get, path = "/admin/state/observations", tag = "Admin/State", responses(
    (status=200, description="Recent observations"),
    (status=403, description="Forbidden", body = arw_protocol::ProblemDetails)
))]
async fn state_observations_doc() -> impl IntoResponse {
    ext::state_api::observations_get().await
}
#[allow(dead_code)]
#[utoipa::path(get, path = "/admin/state/beliefs", tag = "Admin/State", responses(
    (status=200, description="Beliefs"),
    (status=403, description="Forbidden", body = arw_protocol::ProblemDetails)
))]
async fn state_beliefs_doc() -> impl IntoResponse {
    ext::state_api::beliefs_get().await
}
#[allow(dead_code)]
#[utoipa::path(get, path = "/admin/state/world", tag = "Admin/State", responses(
    (status=200, description="Scoped world model (Project Map)"),
    (status=403, description="Forbidden", body = arw_protocol::ProblemDetails)
))]
async fn state_world_doc() -> impl IntoResponse {
    ext::world::world_get(axum::extract::Query(ext::world::WorldQs { proj: None })).await
}
#[allow(dead_code)]
#[utoipa::path(get, path = "/admin/state/world/select", tag = "Admin/State", responses(
    (status=200, description="Top‑K beliefs"),
    (status=403, description="Forbidden", body = arw_protocol::ProblemDetails)
))]
async fn state_world_select_doc() -> impl IntoResponse {
    ext::world::world_select_get(axum::extract::Query(ext::world::WorldSelectQs {
        proj: None,
        q: None,
        k: Some(8),
    }))
    .await
}
#[allow(dead_code)]
#[utoipa::path(get, path = "/admin/context/assemble", tag = "Admin/Context", responses(
    (status=200, description="Assembled context"),
    (status=403, description="Forbidden", body = arw_protocol::ProblemDetails)
))]
async fn context_assemble_doc() -> impl IntoResponse {
    ext::context_api::assemble_get(
        axum::extract::State(AppState {
            bus: arw_events::Bus::new_with_replay(1, 1),
            stop_tx: None,
            queue: std::sync::Arc::new(arw_core::orchestrator::LocalQueue::new()),
            resources: Resources::new(),
        }),
        axum::extract::Query(ext::context_api::AssembleQs {
            proj: None,
            q: None,
            k: Some(8),
            evidence_k: None,
            div: None,
            s_inst: None,
            s_plan: None,
            s_policy: None,
            s_evid: None,
            s_nice: None,
            s_intents: None,
            s_actions: None,
            s_files: None,
            s_total: None,
            context_format: None,
            include_provenance: None,
            context_item_template: None,
            context_header: None,
            context_footer: None,
            joiner: None,
            context_budget_tokens: None,
            context_item_budget_tokens: None,
        }),
    )
    .await
}
#[allow(dead_code)]
#[utoipa::path(get, path = "/admin/state/intents", tag = "Admin/State", responses(
    (status=200, description="Intents"),
    (status=403, description="Forbidden", body = arw_protocol::ProblemDetails)
))]
async fn state_intents_doc() -> impl IntoResponse {
    ext::state_api::intents_get().await
}
#[allow(dead_code)]
#[utoipa::path(get, path = "/admin/state/actions", tag = "Admin/State", responses(
    (status=200, description="Actions"),
    (status=403, description="Forbidden", body = arw_protocol::ProblemDetails)
))]
async fn state_actions_doc() -> impl IntoResponse {
    ext::state_api::actions_get().await
}

// --- OpenAPI-only wrappers for new read-model and tools endpoints ---
#[allow(dead_code)]
#[utoipa::path(get, path = "/admin/state/models_metrics", tag = "Admin/State", responses(
    (status=200, description="Models download metrics (counters + EWMA)", body = ext::models_api::ModelsMetrics),
    (status=403, description="Forbidden", body = arw_protocol::ProblemDetails)
))]
async fn state_models_metrics_doc(State(state): State<AppState>) -> impl IntoResponse {
    ext::models_api::models_metrics_get(State(state)).await
}

#[allow(dead_code)]
#[utoipa::path(get, path = "/admin/state/route_stats", tag = "Admin/State", responses(
    (status=200, description="HTTP route stats (p95/ewma/hits/errors)"),
    (status=403, description="Forbidden", body = arw_protocol::ProblemDetails)
))]
async fn state_route_stats_doc() -> impl IntoResponse {
    ext::stats::route_stats_get().await
}

#[allow(dead_code)]
#[utoipa::path(get, path = "/admin/tools/cache_stats", tag = "Admin/Tools", responses(
    (status=200, description="Tool Action Cache stats (hit/miss/coalesced)")
))]
async fn tools_cache_stats_doc() -> impl IntoResponse {
    ext::tools_api::tools_cache_stats().await
}
#[allow(dead_code)]
#[utoipa::path(get, path = "/admin/chat", tag = "Admin/Chat", summary = "Deprecated: Chat history", description = "Deprecated dev chat history used by the debug UI; scheduled for removal after sunset.", responses(
    (status=200, description="Chat history"),
    (status=403, description="Forbidden", body = arw_protocol::ProblemDetails)
))]
async fn chat_get_doc() -> impl IntoResponse {
    ext::chat_api::chat_get().await
}
// (moved doc wrappers earlier)
#[allow(dead_code)]
#[utoipa::path(get, path = "/admin/hierarchy/state", tag = "Admin/Hierarchy", responses(
    (status=200, description="Hierarchy state"),
    (status=403, description="Forbidden", body = arw_protocol::ProblemDetails)
))]
async fn hierarchy_state_doc() -> impl IntoResponse {
    Json(json!({}))
}
#[allow(dead_code)]
#[utoipa::path(get, path = "/admin/projects/list", tag = "Admin/Projects", responses(
    (status=200, description="Projects list"),
    (status=403, description="Forbidden", body = arw_protocol::ProblemDetails)
))]
async fn projects_list_doc() -> impl IntoResponse {
    ext::projects::projects_list().await
}

#[arw_admin(method = "GET", path = "/admin", summary = "Admin index (HTML)")]
async fn admin_index_html() -> impl IntoResponse {
    let items = arw_core::list_admin_endpoints();
    let docs_url = descriptor_docs_url();
    // Build items
    let mut list_html = String::new();
    for e in &items {
        use std::fmt::Write as _;
        let _ = writeln!(
            &mut list_html,
            "<li data-q=\"{} {} {}\"><code class=method>{}</code> <a href=\"{}\">{}</a><span class=summary>{}</span></li>",
            e.method.to_lowercase(), e.path.to_lowercase(), e.summary.to_lowercase(),
            e.method, e.path, e.path, e.summary
        );
    }
    // Compose HTML
    let mut body = String::new();
    body.push_str(r#"<!doctype html><html><head><meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1"><title>Admin Index</title>
<style>:root{--bg:#ffffff;--fg:#222;--muted:#777;--btn-border:#ddd;--accent:#0366d6}
@media (prefers-color-scheme: dark){:root{--bg:#0b0b0f;--fg:#ddd;--muted:#889;--btn-border:#333;--accent:#4da3ff}}
[data-theme="dark"]{--bg:#0b0b0f;--fg:#ddd;--muted:#889;--btn-border:#333;--accent:#4da3ff}
*{box-sizing:border-box} body{font:14px/1.5 -apple-system,BlinkMacSystemFont,Segoe UI,Roboto,Helvetica,Arial,sans-serif;margin:24px;background:var(--bg);color:var(--fg)}
h1{font-size:20px;margin:0 0 12px} .bar{display:flex;gap:8px;align-items:center;margin:8px 0 16px} input[type=text]{flex:1;padding:8px;border:1px solid var(--btn-border);border-radius:6px}
.btn{display:inline-block;padding:6px 10px;border:1px solid var(--btn-border);border-radius:6px;text-decoration:none;color:var(--accent);background:transparent}
ul{list-style:none;padding:0;margin:0} li{padding:6px 0;border-bottom:1px solid #f0f0f0} code.method{display:inline-block;min-width:48px;margin-right:8px;color:#555} .summary{color:#666;margin-left:8px}
.links{margin-left:auto;display:flex;gap:8px;align-items:center} .top{display:flex;gap:12px;align-items:center;justify-content:space-between} .muted{color:var(--muted)} .toggle{cursor:pointer}</style>
<script>(function(){function f(){const q=(document.getElementById('q').value||'').toLowerCase();document.querySelectorAll('#list li').forEach(function(li){const k=li.getAttribute('data-q')||'';li.style.display=k.indexOf(q)>=0?'':'none';});}
window.addEventListener('DOMContentLoaded',function(){var el=document.getElementById('q'); if(el){ el.addEventListener('input',f); }
// theme toggle
var root=document.documentElement; var KEY='arw-theme'; function apply(t){ if(t==='dark'){ root.setAttribute('data-theme','dark'); } else { root.removeAttribute('data-theme'); } }
function cur(){ return localStorage.getItem(KEY)||''; } function tog(){ var t=cur()==='dark'?'light':'dark'; localStorage.setItem(KEY,t); apply(t); }
apply(cur()); var btn=document.getElementById('themeToggle'); if(btn){ btn.addEventListener('click', tog); }
});})();</script></head><body>
"#);
    body.push_str("<div class=top><h1>Admin Endpoints</h1><div class=links><a class=btn href=\\\"/\\\">Home</a>");
    if std::env::var("ARW_DEBUG").ok().as_deref() == Some("1") {
        body.push_str("<a class=btn href=\"/debug\">Open Debug UI</a>");
    }
    if let Some(u) = docs_url {
        body.push_str(&format!(
            "<a class=btn href=\"{}\" target=\"_blank\">Docs</a>",
            u
        ));
    }
    body.push_str(" <a class=btn href=\"/admin/index.json\">index.json</a> <button id=\"themeToggle\" class=\"btn toggle\" type=\"button\" aria-label=\"Toggle dark mode\">Theme</button></div></div>");
    body.push_str(&format!("<div class=bar><input id=q type=text placeholder=\"Filter routes (method, path, summary)\"><span class=muted>{} endpoints</span></div>", items.len()));
    body.push_str(&format!("<ul id=list>{}</ul>", list_html));
    body.push_str("</body></html>");
    let mut h = HeaderMap::new();
    h.insert(
        axum::http::header::CONTENT_TYPE,
        axum::http::HeaderValue::from_static("text/html; charset=utf-8"),
    );
    if let Some(csp) = csp_header_value() {
        h.insert(
            axum::http::HeaderName::from_static("content-security-policy"),
            csp,
        );
    }
    (StatusCode::OK, h, body)
}

#[arw_admin(
    method = "GET",
    path = "/admin/index.json",
    summary = "Admin endpoints (JSON)"
)]
async fn admin_index_json() -> impl IntoResponse {
    let list = arw_core::list_admin_endpoints();
    Json(serde_json::to_value(list).unwrap_or_else(|_| json!([]))).into_response()
}

// ---- Public endpoints: metrics & specs ----
#[allow(dead_code)]
#[utoipa::path(get, path = "/metrics", tag = "Public", responses((status=200, description="Prometheus metrics")))]
async fn metrics_doc() -> impl IntoResponse {
    ext::stats::metrics_get(State(AppState {
        bus: arw_events::Bus::new_with_replay(1, 1),
        stop_tx: Some(tokio::sync::broadcast::channel(1).0),
        queue: std::sync::Arc::new(arw_core::orchestrator::LocalQueue::new()),
        resources: Resources::new(),
    }))
    .await
}

#[allow(dead_code)]
#[utoipa::path(get, path = "/spec/openapi.yaml", tag = "Public/Specs", responses(
    (status=200, description="OpenAPI YAML"),
    (status=404, description="Missing")
))]
async fn spec_openapi_doc() -> impl IntoResponse {
    spec_openapi().await
}

#[allow(dead_code)]
#[utoipa::path(get, path = "/spec/asyncapi.yaml", tag = "Public/Specs", responses(
    (status=200, description="AsyncAPI YAML"),
    (status=404, description="Missing")
))]
async fn spec_asyncapi_doc() -> impl IntoResponse {
    spec_asyncapi().await
}

#[allow(dead_code)]
#[utoipa::path(get, path = "/spec/mcp-tools.json", tag = "Public/Specs", responses(
    (status=200, description="MCP tools JSON"),
    (status=404, description="Missing")
))]
async fn spec_mcp_doc() -> impl IntoResponse {
    spec_mcp().await
}

#[allow(dead_code)]
#[utoipa::path(get, path = "/spec", tag = "Public/Specs", responses((status=200, description="Spec index")))]
async fn spec_index_doc() -> impl IntoResponse {
    spec_index().await
}

#[allow(dead_code)]
#[utoipa::path(get, path = "/version", tag = "Public", responses((status=200, description="Service version")))]
async fn version_doc() -> impl IntoResponse {
    ext::version().await
}

#[allow(dead_code)]
#[utoipa::path(
    get,
    path = "/about",
    tag = "Public",
    description = "Return service metadata and branding for the running instance.",
    responses((status=200, description="About service", body = About))
)]
async fn about_doc() -> impl IntoResponse {
    ext::about().await
}

async fn spec_openapi() -> impl IntoResponse {
    let p = std::path::Path::new("spec/openapi.yaml");
    if let Ok(bytes) = tokio::fs::read(p).await {
        let mut h = HeaderMap::new();
        h.insert(
            axum::http::header::CONTENT_TYPE,
            axum::http::HeaderValue::from_static("application/yaml"),
        );
        (StatusCode::OK, h, bytes).into_response()
    } else {
        let pd = arw_protocol::ProblemDetails {
            r#type: "about:blank".into(),
            title: "Not Found".into(),
            status: StatusCode::NOT_FOUND.as_u16(),
            detail: Some("missing spec/openapi.yaml".into()),
            instance: None,
            trace_id: None,
            code: None,
        };
        (StatusCode::NOT_FOUND, Json(pd)).into_response()
    }
}
async fn spec_asyncapi() -> impl IntoResponse {
    let p = std::path::Path::new("spec/asyncapi.yaml");
    if let Ok(bytes) = tokio::fs::read(p).await {
        let mut h = HeaderMap::new();
        h.insert(
            axum::http::header::CONTENT_TYPE,
            axum::http::HeaderValue::from_static("application/yaml"),
        );
        (StatusCode::OK, h, bytes).into_response()
    } else {
        let pd = arw_protocol::ProblemDetails {
            r#type: "about:blank".into(),
            title: "Not Found".into(),
            status: StatusCode::NOT_FOUND.as_u16(),
            detail: Some("missing spec/asyncapi.yaml".into()),
            instance: None,
            trace_id: None,
            code: None,
        };
        (StatusCode::NOT_FOUND, Json(pd)).into_response()
    }
}
async fn spec_mcp() -> impl IntoResponse {
    let p = std::path::Path::new("spec/mcp-tools.json");
    if let Ok(bytes) = tokio::fs::read(p).await {
        let mut h = HeaderMap::new();
        h.insert(
            axum::http::header::CONTENT_TYPE,
            axum::http::HeaderValue::from_static("application/json"),
        );
        (StatusCode::OK, h, bytes).into_response()
    } else {
        let pd = arw_protocol::ProblemDetails {
            r#type: "about:blank".into(),
            title: "Not Found".into(),
            status: StatusCode::NOT_FOUND.as_u16(),
            detail: Some("missing spec/mcp-tools.json".into()),
            instance: None,
            trace_id: None,
            code: None,
        };
        (StatusCode::NOT_FOUND, Json(pd)).into_response()
    }
}

async fn spec_index() -> impl IntoResponse {
    let mut links: Vec<(String, &'static str)> = Vec::new();
    for (name, ct) in [
        ("openapi.yaml", "application/yaml"),
        ("asyncapi.yaml", "application/yaml"),
        ("mcp-tools.json", "application/json"),
    ] {
        let p = std::path::Path::new("spec").join(name);
        if p.exists() {
            links.push((name.to_string(), ct));
        }
    }
    let mut items_html = String::new();
    for (n, _ct) in &links {
        items_html.push_str(&format!("<li><a href=\"/spec/{}\">{}</a></li>", n, n));
    }
    items_html.push_str("<li><a href=\"/spec/health\">health</a></li>");
    let mut body = String::new();
    body.push_str(r#"<!doctype html><html><head><meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1"><title>ARW Specs</title>
<style>:root{--bg:#ffffff;--fg:#222;--muted:#777;--btn-border:#ddd;--accent:#0366d6}
@media (prefers-color-scheme: dark){:root{--bg:#0b0b0f;--fg:#ddd;--muted:#889;--btn-border:#333;--accent:#4da3ff}}
[data-theme="dark"]{--bg:#0b0b0f;--fg:#ddd;--muted:#889;--btn-border:#333;--accent:#4da3ff}
*{box-sizing:border-box} body{font:14px/1.5 -apple-system,BlinkMacSystemFont,Segoe UI,Roboto,Helvetica,Arial,sans-serif;margin:24px;background:var(--bg);color:var(--fg)}
h1{font-size:20px;margin:0 0 12px} .bar{display:flex;gap:8px;align-items:center;margin:8px 0 16px}
.btn{display:inline-block;padding:6px 10px;border:1px solid var(--btn-border);border-radius:6px;text-decoration:none;color:var(--accent);background:transparent}
ul{list-style:none;padding:0;margin:0} li{padding:6px 0;border-bottom:1px solid #f0f0f0} .toggle{cursor:pointer}
</style>
<script>(function(){var root=document.documentElement,KEY='arw-theme';function apply(t){if(t==='dark'){root.setAttribute('data-theme','dark');}else{root.removeAttribute('data-theme');}}function cur(){return localStorage.getItem(KEY)||'';}function tog(){var t=cur()==='dark'?'light':'dark';localStorage.setItem(KEY,t);apply(t);}window.addEventListener('DOMContentLoaded',function(){apply(cur());var b=document.getElementById('themeToggle'); if(b){b.addEventListener('click',tog);} });})();</script>
</head><body>
<div class=bar><h1>Specs</h1><span style="flex:1"></span><a class="btn" href="/">Home</a> <button id="themeToggle" class="btn toggle" type="button" aria-label="Toggle dark mode">Theme</button></div>
<ul>"#);
    body.push_str(&items_html);
    body.push_str("</ul></body></html>");
    let mut h = HeaderMap::new();
    h.insert(
        axum::http::header::CONTENT_TYPE,
        axum::http::HeaderValue::from_static("text/html; charset=utf-8"),
    );
    if let Some(csp) = csp_header_value() {
        h.insert(
            axum::http::HeaderName::from_static("content-security-policy"),
            csp,
        );
    }
    (StatusCode::OK, h, body)
}

// Public landing page
async fn index_landing() -> impl IntoResponse {
    let debug = std::env::var("ARW_DEBUG").ok().as_deref() == Some("1");
    let docs = descriptor_docs_url();
    let mut links = vec![
        "<a class=btn href=\"/about\">About</a>".to_string(),
        "<a class=btn href=\"/spec\">Specs</a>".to_string(),
    ];
    if debug {
        links.push("<a class=btn href=\"/debug\">Debug UI</a>".to_string());
        links.push("<a class=btn href=\"/admin\">Admin</a>".to_string());
    }
    if let Some(u) = docs {
        links.push(format!(
            "<a class=btn href=\"{}\" target=\"_blank\">Docs</a>",
            u
        ));
    }
    let mut body = String::new();
    body.push_str(r#"<!doctype html><html><head><meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1"><title>Agent Hub</title>
<style>:root{--bg:#ffffff;--fg:#222;--muted:#666;--btn-border:#ddd;--accent:#0366d6;--ok:#2da44e;--err:#d1242f}
@media (prefers-color-scheme: dark){:root{--bg:#0b0b0f;--fg:#ddd;--muted:#889;--btn-border:#333;--accent:#4da3ff}}
[data-theme="dark"]{--bg:#0b0b0f;--fg:#ddd;--muted:#889;--btn-border:#333;--accent:#4da3ff}
*{box-sizing:border-box} body{margin:24px;background:var(--bg);color:var(--fg);font:16px/1.5 -apple-system,BlinkMacSystemFont,Segoe UI,Roboto,Helvetica,Arial,sans-serif}
h1{font-size:22px;margin:0 0 12px} .bar{display:flex;gap:8px;flex-wrap:wrap;align-items:center}
.btn{display:inline-block;padding:8px 12px;border:1px solid var(--btn-border);border-radius:8px;text-decoration:none;color:var(--accent);background:transparent}
.muted{color:var(--muted)} .spacer{flex:1}
.badge{display:inline-flex;align-items:center;gap:6px;padding:6px 10px;border:1px solid var(--btn-border);border-radius:999px}
.badge .dot{width:10px;height:10px;border-radius:50%;background:var(--muted)}
.badge.ok .dot{background:var(--ok)} .badge.err .dot{background:var(--err)} .toggle{cursor:pointer}
</style>
</head><body>"#);
    body.push_str(r#"<div class=bar><h1>Agent Hub (ARW)</h1><span class="spacer"></span><span id="status" class="badge"><span class=dot></span><span id="statusText">Checking…</span></span> <span id="version" class="badge">v…</span> <button id="themeToggle" class="btn toggle" type="button" aria-label="Toggle dark mode">Theme</button></div>"#);
    body.push_str(r#"<p class=muted>Your private AI control room.</p><div class=bar>"#);
    body.push_str(&links.join(" "));
    body.push_str("</div>");
    body.push_str(r#"<script>(function(){var root=document.documentElement,KEY='arw-theme';function apply(t){if(t==='dark'){root.setAttribute('data-theme','dark');}else{root.removeAttribute('data-theme');}}
function cur(){return localStorage.getItem(KEY)||'';} function tog(){var t=cur()==='dark'?'light':'dark';localStorage.setItem(KEY,t);apply(t);} apply(cur()); var btn=document.getElementById('themeToggle'); if(btn){btn.addEventListener('click',tog);} 
var badge=document.getElementById('status'), text=document.getElementById('statusText'); async function poll(){try{var r=await fetch('/healthz',{cache:'no-store'}); if(r.ok){badge.classList.add('ok');badge.classList.remove('err'); if(text){text.textContent='Online';}} else {throw new Error('bad');}} catch(e){badge.classList.add('err');badge.classList.remove('ok'); if(text){text.textContent='Offline';}} finally {setTimeout(poll,4000);}} poll(); async function about(){try{var r=await fetch('/about',{cache:'no-store'}); if(r.ok){var j=await r.json(); var v=(j&&j.version)||(j&&j.data&&j.data.version); var el=document.getElementById('version'); if(el && v){ el.textContent='v'+v; }}}catch(e){} } about();})();</script>"#);
    body.push_str("</body></html>");
    let mut h = HeaderMap::new();
    h.insert(
        axum::http::header::CONTENT_TYPE,
        axum::http::HeaderValue::from_static("text/html; charset=utf-8"),
    );
    if let Some(csp) = csp_header_value() {
        h.insert(
            axum::http::HeaderName::from_static("content-security-policy"),
            csp,
        );
    }
    (StatusCode::OK, h, body)
}

async fn favicon_empty() -> impl IntoResponse {
    StatusCode::NO_CONTENT
}

#[allow(dead_code)]
#[utoipa::path(get, path = "/spec/health", tag = "Public/Specs", responses((status=200, description="Spec health JSON")))]
async fn spec_health() -> impl IntoResponse {
    let checks = [
        ("openapi.yaml", "application/yaml"),
        ("asyncapi.yaml", "application/yaml"),
        ("mcp-tools.json", "application/json"),
    ];
    let mut items = Vec::new();
    for (name, ct) in checks.iter() {
        let p = std::path::Path::new("spec").join(name);
        let (exists, size) = match std::fs::metadata(&p) {
            Ok(md) => (true, md.len()),
            Err(_) => (false, 0),
        };
        items.push(serde_json::json!({"name": name, "content_type": ct, "path": p.to_string_lossy(), "exists": exists, "size": size }));
    }
    Json(serde_json::json!({"items": items}))
}

// ---- Tests for specs and security helpers ----
#[cfg(test)]
mod tests_specs_sec {
    use super::*;
    use axum::{routing::get, Router};
    use tower::ServiceExt as _;

    #[tokio::test]
    async fn spec_endpoints_return_ok() {
        // Ensure local spec files exist under current CWD for the handlers
        let _ = std::fs::create_dir_all("spec");
        let _ = std::fs::write(
            "spec/openapi.yaml",
            b"openapi: 3.1.0\ninfo: {title: test, version: 0.0.0}\n",
        );
        let _ = std::fs::write(
            "spec/asyncapi.yaml",
            b"asyncapi: 2.6.0\ninfo: {title: test, version: 0.0.0}\n",
        );
        let _ = std::fs::write("spec/mcp-tools.json", b"[]\n");

        let app = Router::new()
            .route("/spec/openapi.yaml", get(spec_openapi))
            .route("/spec/asyncapi.yaml", get(spec_asyncapi))
            .route("/spec/mcp-tools.json", get(spec_mcp))
            .route("/spec", get(spec_index))
            .route("/spec/health", get(spec_health));

        for (path, ct) in [
            ("/spec/openapi.yaml", Some("application/yaml")),
            ("/spec/asyncapi.yaml", Some("application/yaml")),
            ("/spec/mcp-tools.json", Some("application/json")),
            ("/spec", Some("text/html; charset=utf-8")),
            ("/spec/health", Some("application/json")),
        ] {
            let resp = app
                .clone()
                .oneshot(
                    axum::http::Request::builder()
                        .uri(path)
                        .body(axum::body::Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert!(resp.status().is_success(), "{}", path);
            if let Some(expect_ct) = ct {
                let got = resp
                    .headers()
                    .get(axum::http::header::CONTENT_TYPE)
                    .and_then(|v| v.to_str().ok())
                    .unwrap_or("");
                assert_eq!(got, expect_ct, "{}", path);
            }
        }
    }

    #[test]
    fn extract_admin_token_prefers_bearer() {
        let mut h = HeaderMap::new();
        h.insert(
            axum::http::header::AUTHORIZATION,
            axum::http::HeaderValue::from_static("Bearer abc"),
        );
        h.insert(
            axum::http::HeaderName::from_static("x-arw-admin"),
            axum::http::HeaderValue::from_static("xyz"),
        );
        let tok = extract_admin_token(&h).unwrap();
        assert_eq!(tok, "abc");
    }

    #[test]
    fn client_ip_key_picks_forwarded() {
        let mut h = HeaderMap::new();
        h.insert(
            axum::http::HeaderName::from_static("x-forwarded-for"),
            axum::http::HeaderValue::from_static("1.2.3.4, 5.6.7.8"),
        );
        let k = client_ip_key(&h);
        assert_eq!(k, "1.2.3.4");
    }

    #[test]
    fn token_sha256_matches_works() {
        let tok = "s3cr3t";
        use sha2::Digest as _;
        let mut hasher = sha2::Sha256::new();
        hasher.update(tok.as_bytes());
        let hex = hex::encode(hasher.finalize());
        assert!(token_sha256_matches(tok, &hex));
        assert!(!token_sha256_matches("nope", &hex));
    }

    #[tokio::test]
    async fn security_rate_limit_per_token() {
        // Configure rate limit: 1 per 60 seconds
        std::env::set_var("ARW_ADMIN_RL", "1/60");
        std::env::set_var("ARW_ADMIN_TOKEN", "t0k");

        let app = Router::new()
            .route(
                "/admin/ping",
                get(|| async { (StatusCode::OK, Json(serde_json::json!({"ok": true}))) }),
            )
            .layer(middleware::from_fn(security_mw));

        let mk = |auth: &str| {
            axum::http::Request::builder()
                .uri("/admin/ping")
                .header(axum::http::header::AUTHORIZATION, auth)
                .body(axum::body::Body::empty())
                .unwrap()
        };
        // First should pass
        let r1 = app.clone().oneshot(mk("Bearer t0k")).await.unwrap();
        assert_eq!(r1.status(), StatusCode::OK);
        // Second should rate-limit
        let r2 = app.oneshot(mk("Bearer t0k")).await.unwrap();
        assert_eq!(r2.status(), StatusCode::TOO_MANY_REQUESTS);
    }
}

// ---- Interface catalog (debug) ----
#[allow(dead_code)]
#[utoipa::path(get, path = "/catalog/index", tag = "Public/Specs", responses(
    (status=200, description="Interface catalog index (YAML)"),
    (status=404, description="Missing")
))]
async fn catalog_index_doc() -> impl IntoResponse {
    catalog_index().await
}

async fn catalog_index() -> impl IntoResponse {
    let p = std::path::Path::new("interfaces/index.yaml");
    if let Ok(bytes) = tokio::fs::read(p).await {
        let mut h = HeaderMap::new();
        h.insert(
            axum::http::header::CONTENT_TYPE,
            axum::http::HeaderValue::from_static("application/yaml"),
        );
        (StatusCode::OK, h, bytes).into_response()
    } else {
        let pd = arw_protocol::ProblemDetails {
            r#type: "about:blank".into(),
            title: "Not Found".into(),
            status: StatusCode::NOT_FOUND.as_u16(),
            detail: Some("missing interfaces/index.yaml".into()),
            instance: None,
            trace_id: None,
            code: None,
        };
        (StatusCode::NOT_FOUND, Json(pd)).into_response()
    }
}

#[allow(dead_code)]
#[utoipa::path(get, path = "/catalog/health", tag = "Public/Specs", responses((status=200, description="Catalog health")))]
async fn catalog_health_doc() -> impl IntoResponse {
    catalog_health().await
}

async fn catalog_health() -> impl IntoResponse {
    use tokio::fs as afs;
    let (idx_m, oa_m, aa_m, mcp_m) = tokio::join!(
        afs::metadata("interfaces/index.yaml"),
        afs::metadata("spec/openapi.yaml"),
        afs::metadata("spec/asyncapi.yaml"),
        afs::metadata("spec/mcp-tools.json"),
    );
    let (idx, oa, aa, mcp) = (idx_m.is_ok(), oa_m.is_ok(), aa_m.is_ok(), mcp_m.is_ok());
    let out = serde_json::json!({
        "ok": idx && oa,
        "index_present": idx,
        "specs": {"openapi": oa, "asyncapi": aa, "mcp": mcp},
    });
    (StatusCode::OK, Json(out))
}
#[allow(dead_code)]
#[utoipa::path(get, path = "/state/models", tag = "Public/State", operation_id = "state_models_doc", responses(
    (status=200, description="Models list", body = [ext::models_api::ModelItem])
))]
async fn models_public_list_doc() -> impl IntoResponse {
    ext::state_api::models_state_get().await
}
