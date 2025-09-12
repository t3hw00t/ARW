use arw_macros::arw_tool;
use arw_macros::{arw_admin, arw_gate};
pub use arw_svc::resources;
use arw_svc::resources::governor_service::GovernorService;
use arw_svc::resources::hierarchy_service::HierarchyService;
use arw_svc::resources::memory_service::MemoryService;
use arw_svc::resources::models_service::ModelsService;
use arw_svc::resources::Resources;
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
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio_stream::wrappers::errors::BroadcastStreamRecvError;
use tokio_stream::wrappers::BroadcastStream;
use tower_http::compression::CompressionLayer;
use tower_http::cors::{Any, CorsLayer};
use tower_http::limit::RequestBodyLimitLayer;
use tower_http::services::ServeDir;
// use tower_http::timeout::TimeoutLayer; // replaced with dynamic timeout layer
use tower_http::trace::TraceLayer;
use tracing::info;
use utoipa::{OpenApi, ToSchema};
mod dyn_timeout;
mod ext;
use arw_core::gating;
#[cfg(feature = "grpc")]
mod grpc;

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

#[tokio::main]
async fn main() {
    arw_otel::init();

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
    // Initialize gating from config/env
    gating::init_from_config("configs/gating.toml");
    let cfg = arw_core::load_config(
        &std::env::var("ARW_CONFIG").unwrap_or_else(|_| "configs/default.toml".to_string()),
    )
    .ok();
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
        bus,
        stop_tx: Some(stop_tx.clone()),
        queue,
        resources: Resources::new(),
    };
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
    state
        .bus
        .publish("Service.Start", &json!({"msg":"arw-svc started"}));

    // Spawn stats aggregator and observations read-model updater
    {
        let mut rx = state.bus.subscribe();
        tokio::spawn(async move {
            while let Ok(env) = rx.recv().await {
                ext::stats::stats_on_event(&env.kind).await;
                ext::state_api::on_event(&env).await;
            }
        });
    }

    // Start lightweight feedback engine (near-live suggestions via bus)
    ext::feedback_engine::start_feedback_engine(state.clone());
    // Start local task worker to exercise the orchestrator MVP
    ext::start_local_task_worker(state.clone());

    // Optionally start gRPC service when enabled and requested
    #[cfg(feature = "grpc")]
    {
        if std::env::var("ARW_GRPC").ok().as_deref() == Some("1") {
            let st = state.clone();
            tokio::spawn(async move { grpc::serve(st).await });
        }
    }

    let mut app = Router::new()
        // Public endpoints
        .route("/healthz", get(healthz))
        .route("/metrics", get(ext::stats::metrics_get))
        .route("/version", get(ext::version))
        .route("/about", get(ext::about))
        // Serve generated specs when present (public)
        .route("/spec/openapi.yaml", get(spec_openapi))
        .route("/spec/asyncapi.yaml", get(spec_asyncapi))
        .route("/spec/mcp-tools.json", get(spec_mcp))
        .route("/spec", get(spec_index))
        // Administrative endpoints are nested under /admin
        .nest(
            "/admin",
            Router::new()
                // Match paths before metrics/security to capture MatchedPath
                .route("/", get(admin_index_html))
                .route("/index.json", get(admin_index_json))
                .route("/probe", get(probe))
                .route("/probe/hw", get(probe_hw))
                .route("/events", get(events))
                .route("/emit/test", get(emit_test))
                .route("/shutdown", get(shutdown))
                .route("/introspect/tools", get(introspect_tools))
                .route("/introspect/schemas/:id", get(introspect_schema))
                // Bring in extra admin routes (memory/models/tools/etc.)
                .merge(ext::extra_routes()),
        )
        .layer(TraceLayer::new_for_http())
        .layer(CompressionLayer::new())
        .layer(RequestBodyLimitLayer::new(8 * 1024 * 1024))
        .layer(
            if std::env::var("ARW_CORS_ANY").ok().as_deref() == Some("1")
                || std::env::var("ARW_DEBUG").ok().as_deref() == Some("1")
            {
                CorsLayer::new()
                    .allow_origin(Any)
                    .allow_methods([axum::http::Method::GET, axum::http::Method::POST])
                    .allow_headers(Any)
            } else {
                CorsLayer::new()
            },
        )
        .layer(middleware::from_fn(dyn_timeout::dyn_timeout_mw))
        .layer(middleware::from_fn(security_mw))
        .layer(middleware::from_fn(metrics_mw))
        .with_state(state.clone());

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
    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    info!("arw-svc listening on http://{}", addr);

    let listener = match tokio::net::TcpListener::bind(addr).await {
        Ok(l) => l,
        Err(e) => {
            tracing::error!("failed to bind {}: {}", addr, e);
            return;
        }
    };
    let server = axum::serve(listener, app).with_graceful_shutdown(async move {
        let _ = stop_rx.recv().await;
    });
    if let Err(e) = server.await {
        eprintln!("server error: {}", e);
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
    let token = std::env::var("ARW_ADMIN_TOKEN").ok();
    let ok = if let Some(t) = token {
        if t.is_empty() {
            debug
        } else {
            header_token_matches(req.headers(), &t)
        }
    } else {
        debug
    };
    if ok {
        if !rate_allow() {
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
    let body = serde_json::json!({
        "type": "about:blank",
        "title": "Forbidden",
        "status": 403,
        "detail": "administrative endpoint; set ARW_DEBUG=1 or provide X-ARW-Admin token"
    });
    (SC::FORBIDDEN, axum::Json(body)).into_response()
}

fn header_token_matches(h: &HeaderMap, token: &str) -> bool {
    h.get("x-arw-admin")
        .and_then(|v| v.to_str().ok())
        .map(|v| ct_eq(v.as_bytes(), token.as_bytes()))
        .unwrap_or(false)
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

// ---- global rate limit (fixed window) ----
struct RateWin {
    count: u64,
    start: std::time::Instant,
}
static RL_STATE: OnceLock<Mutex<RateWin>> = OnceLock::new();
fn rl_params() -> (u64, u64) {
    if let Ok(s) = std::env::var("ARW_ADMIN_RL") {
        if let Some((a, b)) = s.split_once('/') {
            if let (Ok(l), Ok(w)) = (a.parse::<u64>(), b.parse::<u64>()) {
                return (l.max(1), w.max(1));
            }
        }
    }
    (60, 60)
}
fn rate_allow() -> bool {
    let (limit, win_secs) = rl_params();
    let now = std::time::Instant::now();
    let m = RL_STATE.get_or_init(|| {
        Mutex::new(RateWin {
            count: 0,
            start: now,
        })
    });
    let mut st = m.lock().unwrap();
    if now.duration_since(st.start).as_secs() >= win_secs {
        st.start = now;
        st.count = 0;
    }
    if st.count >= limit {
        return false;
    }
    st.count += 1;
    true
}
#[utoipa::path(
    get,
    path = "/healthz",
    responses((status = 200, description = "Service health", body = OkResponse))
)]
async fn healthz(State(state): State<AppState>) -> impl IntoResponse {
    state.bus.publish("Service.Health", &json!({"ok": true}));
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
    state.bus.publish("Memory.Applied", &ep);

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
        .get(0)
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

    // Disks (subset: state_dir and root free/total, best-effort)
    let state_dir = std::path::Path::new(".");
    let disks: Vec<serde_json::Value> = {
        let mut v = Vec::new();
        if let Ok(av) = fs2::available_space(state_dir) {
            if let Ok(md) = std::fs::metadata(state_dir) {
                if let Ok(dev) = nix_dev_from_md(&md) {
                    v.push(serde_json::json!({"mount": state_dir, "available": av, "dev": dev}));
                } else {
                    v.push(serde_json::json!({"mount": state_dir, "available": av}));
                }
            }
        }
        v
    };

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

    let out = serde_json::json!({
        "cpu": {"brand": cpu_brand, "logical": cpus_logical, "physical": cpus_physical},
        "memory": {"total": total_mem, "available": avail_mem},
        "os": {"name": os_name, "version": os_version, "kernel": kernel, "arch": arch},
        "disks": disks,
        "boot": boot,
        "virt": virt,
        "container": container,
        "wsl": wsl,
        "env": env,
        "gpus": gpus,
    });
    // Publish minimal event for observability
    state.bus.publish("Probe.HW", &serde_json::json!({"cpus": cpus_logical, "gpus": out["gpus"].as_array().map(|a| a.len()).unwrap_or(0)}));
    ext::ok::<serde_json::Value>(out).into_response()
}

#[cfg(target_os = "linux")]
fn probe_gpus_best_effort() -> Vec<serde_json::Value> {
    probe_gpus_linux()
}
#[cfg(not(target_os = "linux"))]
fn probe_gpus_best_effort() -> Vec<serde_json::Value> {
    // TODO: add Windows/macOS probes in future iterations (DXGI/Metal)
    Vec::new()
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

#[cfg(unix)]
fn nix_dev_from_md(md: &std::fs::Metadata) -> std::io::Result<u64> {
    use std::os::unix::fs::MetadataExt as _;
    Ok(md.dev())
}
#[cfg(not(unix))]
fn nix_dev_from_md(_md: &std::fs::Metadata) -> std::io::Result<u64> {
    Err(std::io::Error::new(
        std::io::ErrorKind::Other,
        "unsupported",
    ))
}

#[arw_admin(method = "GET", path = "/admin/emit/test", summary = "Emit test event")]
#[utoipa::path(
    get,
    path = "/admin/emit/test",
    tag = "Admin/Core",
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
    state
        .bus
        .publish("Service.Test", &json!({"msg":"ping","t": now_ms}));
    Json(OkResponse { ok: true }).into_response()
}

#[arw_admin(method = "GET", path = "/admin/shutdown", summary = "Shutdown service")]
#[utoipa::path(
    get,
    path = "/admin/shutdown",
    tag = "Admin/Core",
    responses(
        (status = 200, description = "Shutdown service", body = OkResponse),
        (status = 403, description = "Forbidden", body = arw_protocol::ProblemDetails)
    )
)]
#[arw_gate("admin:shutdown")]
async fn shutdown(State(state): State<AppState>) -> impl IntoResponse {
    state
        .bus
        .publish("Service.Stop", &json!({"reason":"user request"}));
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
    responses(
        (status = 200, description = "SSE event stream"),
        (status = 403, description = "Forbidden", body = arw_protocol::ProblemDetails)
    )
)]
async fn events(
    State(state): State<AppState>,
    Query(q): Query<EventsQs>,
) -> Sse<impl tokio_stream::Stream<Item = Result<Event, Infallible>>> {
    let rx = if !q.prefix.is_empty() {
        state.bus.subscribe_filtered(q.prefix.clone(), None)
    } else {
        state.bus.subscribe()
    };
    let bus_for_lag = state.bus.clone();
    let bstream = BroadcastStream::new(rx).flat_map(move |res| match res {
        Ok(env) => {
            let data = serde_json::to_string(&env).unwrap_or_else(|_| "{}".to_string());
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
                Event::default().id("gap").event("Bus.Gap").data(body),
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
    let replay_stream = tokio_stream::iter(replay_items.into_iter().map(|env| {
        let data = serde_json::to_string(&env).unwrap_or_else(|_| "{}".to_string());
        Ok::<Event, Infallible>(
            Event::default()
                .id(env.time.clone())
                .event(env.kind.clone())
                .data(data),
        )
    }));

    let initial = tokio_stream::once(Ok::<Event, Infallible>(
        Event::default()
            .id("0")
            .event("Service.Connected")
            .data("{}"),
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
        models_default_get_doc,
        models_refresh_doc,
        models_save_doc,
        models_load_doc,
        models_add_doc,
        models_delete_doc,
        models_default_set_doc,
        models_download_doc,
        models_download_cancel_doc,
        tools_list_doc,
        tools_run_doc,
        state_observations_doc,
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
        tasks_enqueue_doc
    ),
    tags((name = "arw-svc"))
)]
struct ApiDoc;

// --- OpenAPI-only wrappers for common admin endpoints ---
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

#[allow(dead_code)]
#[utoipa::path(post, path = "/admin/tools/run", tag = "Admin/Tools", request_body = ext::tools_api::ToolRunReq, responses(
    (status=200, description="Tool output"),
    (status=403, description="Forbidden", body = arw_protocol::ProblemDetails)
))]
async fn tools_run_doc(Json(_req): Json<ext::tools_api::ToolRunReq>) -> impl IntoResponse {
    Json(json!({}))
}

#[allow(dead_code)]
#[utoipa::path(post, path = "/admin/chat/send", tag = "Admin/Chat", request_body = ext::chat_api::ChatSendReq, responses(
    (status=200, description="Message sent", body = OkResponse),
    (status=403, description="Forbidden", body = arw_protocol::ProblemDetails)
))]
async fn chat_send_doc(Json(_req): Json<ext::chat_api::ChatSendReq>) -> impl IntoResponse {
    Json(json!({"ok": true}))
}
#[allow(dead_code)]
#[utoipa::path(post, path = "/admin/chat/clear", tag = "Admin/Chat", responses(
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
#[allow(dead_code)]
#[utoipa::path(get, path = "/admin/chat", tag = "Admin/Chat", responses(
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
    // Build simple HTML list
    let mut list = String::new();
    for e in &items {
        let line = format!(
            "<li><code>{}</code> <a href=\"{}\">{}</a> — {}</li>",
            e.method, e.path, e.path, e.summary
        );
        list.push_str(&line);
    }
    let body = format!(
        "<!doctype html><html><head><meta charset=\"utf-8\"><title>Admin Index</title></head><body><h1>Admin Endpoints</h1><ul>{}</ul><p><a href=\"/admin/index.json\">index.json</a></p></body></html>",
        list
    );
    (
        StatusCode::OK,
        [(axum::http::header::CONTENT_TYPE, "text/html; charset=utf-8")],
        body,
    )
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
#[utoipa::path(get, path = "/about", tag = "Public", responses((status=200, description="About service")))]
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
    let items: String = links
        .iter()
        .map(|(n, _ct)| format!("<li><a href=\"/spec/{}\">{}</a></li>", n, n))
        .collect();
    let body = format!(
        "<!doctype html><html><head><meta charset=\"utf-8\"><title>ARW Specs</title></head><body><h1>Specs</h1><ul>{}</ul></body></html>",
        items
    );
    (
        StatusCode::OK,
        [(axum::http::header::CONTENT_TYPE, "text/html; charset=utf-8")],
        body,
    )
}
