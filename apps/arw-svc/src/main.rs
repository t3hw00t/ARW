use arw_macros::arw_tool;
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
use serde_json::json;
use std::convert::Infallible;
use std::net::SocketAddr;
use std::path::Path as FsPath;
use std::sync::{Mutex, OnceLock};
use std::time::Duration as StdDuration;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio_stream::wrappers::BroadcastStream;
use tokio_stream::StreamExt;
use tower_http::compression::CompressionLayer;
use tower_http::cors::{Any, CorsLayer};
use tower_http::limit::RequestBodyLimitLayer;
use tower_http::services::ServeDir;
use tower_http::timeout::TimeoutLayer;
use tower_http::trace::TraceLayer;
use tracing::info;
use utoipa::{OpenApi, ToSchema};
mod ext;
use arw_core::gating;

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

#[derive(Clone)]
pub(crate) struct AppState {
    pub(crate) bus: arw_events::Bus,
    pub(crate) stop_tx: tokio::sync::broadcast::Sender<()>,
    // Pluggable queue (Local by default; NATS when enabled)
    pub(crate) queue: std::sync::Arc<dyn arw_core::orchestrator::Queue>,
}

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
        return;
    }

    let (stop_tx, mut stop_rx) = tokio::sync::broadcast::channel::<()>(1);
    let bus = arw_events::Bus::new(256);
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
            let node_id = std::env::var("ARW_NODE_ID").ok()
                .or_else(|| cfg.as_ref().and_then(|c| c.cluster.node_id.clone()))
                .unwrap_or_else(|| "local".to_string());
            arw_events::attach_nats_incoming(&bus, &url, &node_id).await;
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
        stop_tx: stop_tx.clone(),
        queue,
    };

    // Load persisted orchestration/feedback state
    ext::load_persisted().await;

    // Emit a startup event so /events sees something if connected early.
    state
        .bus
        .publish("Service.Start", &json!({"msg":"arw-svc started"}));

    // Spawn stats aggregator (subscribes to bus and updates counters)
    {
        let mut rx = state.bus.subscribe();
        tokio::spawn(async move {
            while let Ok(env) = rx.recv().await {
                ext::stats::stats_on_event(&env.kind).await;
            }
        });
    }

    // Start lightweight feedback engine (near-live suggestions via bus)
    ext::feedback_engine::start_feedback_engine(state.clone());
    // Start local task worker to exercise the orchestrator MVP
    ext::start_local_task_worker(state.clone());

    let mut app = Router::new()
        .route("/healthz", get(healthz))
        .route("/introspect/tools", get(introspect_tools))
        .route("/introspect/schemas/:id", get(introspect_schema))
        // Serve generated specs when present
        .route("/spec/openapi.yaml", get(spec_openapi))
        .route("/spec/asyncapi.yaml", get(spec_asyncapi))
        .route("/spec/mcp-tools.json", get(spec_mcp))
        .route("/spec", get(spec_index))
        // Match paths before metrics/security to capture MatchedPath
        .route("/probe", get(probe))
        .route("/events", get(events))
        .route("/emit/test", get(emit_test))
        .route("/shutdown", get(shutdown))
        .merge(ext::extra_routes())
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
        .layer({
            let secs = std::env::var("ARW_HTTP_TIMEOUT_SECS")
                .ok()
                .and_then(|s| s.parse::<u64>().ok())
                .unwrap_or(20);
            TimeoutLayer::new(StdDuration::from_secs(secs))
        })
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

    let port: u16 = std::env::var("ARW_PORT")
        .ok()
        .and_then(|s| s.parse().ok())
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
    let is_sensitive = path.starts_with("/debug")
        || path.starts_with("/probe")
        || path.starts_with("/memory")
        || path.starts_with("/models")
        || path.starts_with("/governor")
        || path.starts_with("/introspect")
        || path.starts_with("/tasks")
        || path.starts_with("/hierarchy")
        || path.starts_with("/chat")
        || path.starts_with("/feedback");
    if !is_sensitive {
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

#[utoipa::path(
    get,
    path = "/introspect/tools",
    responses((status = 200, description = "List available tools"))
)]
async fn introspect_tools() -> impl IntoResponse {
    Json(serde_json::to_value(arw_core::introspect_tools()).unwrap())
}

#[utoipa::path(
    get,
    path = "/introspect/schemas/{id}",
    params(("id" = String, Path, description = "Tool id")),
    responses(
        (status = 200, description = "Schema JSON"),
        (status = 404, description = "Unknown tool id"),
    )
)]
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

// REPLACE your existing probe with this:
#[utoipa::path(
    get,
    path = "/probe",
    responses((status = 200, description = "Returns effective memory paths"))
)]
async fn probe(State(state): State<AppState>) -> impl IntoResponse {
    // Effective paths as JSON (serde_json::Value)
    let ep = arw_core::load_effective_paths();

    // Publish that JSON to the event bus
    state.bus.publish("Memory.Applied", &ep);

    // Return it to the client
    Json::<serde_json::Value>(ep)
}

#[utoipa::path(
    get,
    path = "/emit/test",
    responses((status = 200, description = "Emit test event", body = OkResponse))
)]
async fn emit_test(State(state): State<AppState>) -> impl IntoResponse {
    let now_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    state
        .bus
        .publish("Service.Test", &json!({"msg":"ping","t": now_ms}));
    Json(OkResponse { ok: true })
}

#[utoipa::path(
    get,
    path = "/shutdown",
    responses((status = 200, description = "Shutdown service", body = OkResponse))
)]
async fn shutdown(State(state): State<AppState>) -> impl IntoResponse {
    state
        .bus
        .publish("Service.Stop", &json!({"reason":"user request"}));
    let _ = state.stop_tx.send(());
    Json(OkResponse { ok: true })
}

#[utoipa::path(
    get,
    path = "/events",
    responses((status = 200, description = "SSE event stream"))
)]
async fn events(
    State(state): State<AppState>,
) -> Sse<impl tokio_stream::Stream<Item = Result<Event, Infallible>>> {
    let rx = state.bus.subscribe();
    let bstream = BroadcastStream::new(rx)
        .filter_map(|res| res.ok())
        .map(|env| {
            let data = serde_json::to_string(&env).unwrap_or_else(|_| "{}".to_string());
            // Use RFC3339 ms timestamp as SSE id for simple replay correlation
            Ok::<Event, Infallible>(
                Event::default()
                    .id(env.time.clone())
                    .event(env.kind.clone())
                    .data(data),
            )
        });

    let initial = tokio_stream::once(Ok::<Event, Infallible>(
        Event::default()
            .id("0")
            .event("Service.Connected")
            .data("{}"),
    ));
    let stream = initial.chain(bstream);

    Sse::new(stream).keep_alive(
        KeepAlive::new()
            .interval(Duration::from_secs(15))
            .text("hb"),
    )
}

// --- OpenAPI-only wrappers for feedback endpoints (for documentation) ---
#[allow(dead_code)]
#[utoipa::path(get, path = "/feedback/suggestions", responses((status=200, description="Versioned suggestions")))]
async fn feedback_suggestions_doc() -> impl IntoResponse {
    ext::feedback_engine_api::feedback_suggestions().await
}
#[allow(dead_code)]
#[utoipa::path(get, path = "/feedback/updates", params(("since" = Option<u64>, Query, description = "Return updates if newer than this version")), responses((status=200, description="Latest version"),(status=204, description="No change")))]
async fn feedback_updates_doc(
    Query(q): Query<ext::feedback_engine_api::UpdatesQs>,
) -> impl IntoResponse {
    ext::feedback_engine_api::feedback_updates(Query(q)).await
}
#[allow(dead_code)]
#[utoipa::path(get, path = "/feedback/policy", responses((status=200, description="Effective policy caps/bounds")))]
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
        feedback_suggestions_doc,
        feedback_updates_doc,
        feedback_policy_doc
    ),
    tags((name = "arw-svc"))
)]
struct ApiDoc;

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
        (StatusCode::NOT_FOUND, "missing spec/openapi.yaml").into_response()
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
        (StatusCode::NOT_FOUND, "missing spec/asyncapi.yaml").into_response()
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
        (StatusCode::NOT_FOUND, "missing spec/mcp-tools.json").into_response()
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
