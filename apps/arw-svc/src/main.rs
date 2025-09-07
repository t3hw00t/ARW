use arw_macros::arw_tool;
use axum::http::StatusCode;
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::{
    extract::{Path, State},
    response::IntoResponse,
    routing::get,
    Json, Router,
};
use serde_json::json;
use std::convert::Infallible;
use std::net::SocketAddr;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio_stream::wrappers::BroadcastStream;
use tokio_stream::StreamExt;
use tracing::info;
use tower_http::trace::TraceLayer;
use tower_http::compression::CompressionLayer;
use tower_http::cors::{Any, CorsLayer};
use std::time::Duration as StdDuration;
use tower_http::timeout::TimeoutLayer;
use tower_http::services::ServeDir;
use std::path::Path as FsPath;
use axum::{http::Request, response::Response, middleware::{self, Next}};
use axum::http::HeaderMap;
mod ext;


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

#[derive(Clone)]
struct AppState {
    bus: arw_events::Bus,
    stop_tx: tokio::sync::broadcast::Sender<()>,
}

#[tokio::main]
async fn main() {
    arw_otel::init();

    let (stop_tx, mut stop_rx) = tokio::sync::broadcast::channel::<()>(1);
    let state = AppState {
        bus: arw_events::Bus::new(256),
        stop_tx: stop_tx.clone(),
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
                ext::stats_on_event(&env.kind).await;
            }
        });
    }

    let mut app = Router::new()
        .route("/healthz", get(healthz))
        .route("/introspect/tools", get(introspect_tools))
        .route("/introspect/schemas/:id", get(introspect_schema))
        // Match paths before metrics/security to capture MatchedPath
        .route("/probe", get(probe))
        .route("/events", get(events))
        .route("/emit/test", get(emit_test))
        .route("/shutdown", get(shutdown))
        .merge(ext::extra_routes())
        .layer(
            TraceLayer::new_for_http()
        )
        .layer(CompressionLayer::new())
        .layer(if std::env::var("ARW_CORS_ANY").ok().as_deref() == Some("1") || std::env::var("ARW_DEBUG").ok().as_deref() == Some("1") {
            CorsLayer::new()
                .allow_origin(Any)
                .allow_methods([axum::http::Method::GET, axum::http::Method::POST])
                .allow_headers(Any)
        } else { CorsLayer::new() })
        .layer({
            let secs = std::env::var("ARW_HTTP_TIMEOUT_SECS").ok().and_then(|s| s.parse::<u64>().ok()).unwrap_or(20);
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
        } else { None };
        if let Some(p) = doc_dir { app = app.nest_service("/docs", ServeDir::new(p)); }
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
    let path = req.extensions().get::<MatchedPath>().map(|m| m.as_str().to_string()).unwrap_or_else(|| req.uri().path().to_string());
    let t0 = std::time::Instant::now();
    let res = next.run(req).await;
    let dt = t0.elapsed().as_millis() as u64;
    let status = res.status().as_u16();
    ext::route_obs(&path, status, dt).await;
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
        || path.starts_with("/chat")
        || path.starts_with("/feedback");
    if !is_sensitive { return next.run(req).await; }

    let debug = std::env::var("ARW_DEBUG").ok().as_deref() == Some("1");
    let token = std::env::var("ARW_ADMIN_TOKEN").ok();
    let ok = if let Some(t) = token {
        if t.is_empty() { debug } else { header_token_matches(req.headers(), &t) }
    } else { debug };
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
    h.get("x-arw-admin").and_then(|v| v.to_str().ok()).map(|v| v == token).unwrap_or(false)
}

// ---- global rate limit (fixed window) ----
struct RateWin { count: u64, start: std::time::Instant }
static mut RL_START: Option<std::time::Instant> = None;
static mut RL_COUNT: u64 = 0;
fn rl_params() -> (u64, u64) {
    if let Ok(s) = std::env::var("ARW_ADMIN_RL") { if let Some((a,b)) = s.split_once('/') { if let (Ok(l), Ok(w)) = (a.parse::<u64>(), b.parse::<u64>()) { return (l.max(1), w.max(1)); } } }
    (60, 60)
}
fn rate_allow() -> bool {
    // safe for single-user local use; for multi-threaded strictness, switch to a Mutex/RwLock
    let (limit, win_secs) = rl_params();
    let now = std::time::Instant::now();
    unsafe {
        if RL_START.is_none() { RL_START = Some(now); RL_COUNT = 0; }
        if now.duration_since(RL_START.unwrap()).as_secs() >= win_secs { RL_START = Some(now); RL_COUNT = 0; }
        if RL_COUNT >= limit { return false; }
        RL_COUNT += 1; true
    }
}
async fn healthz(State(state): State<AppState>) -> impl IntoResponse {
    state.bus.publish("Service.Health", &json!({"ok": true}));
    Json(json!({"ok": true}))
}

async fn introspect_tools() -> impl IntoResponse {
    Json(serde_json::to_value(arw_core::introspect_tools()).unwrap())
}

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
async fn probe(State(state): State<AppState>) -> impl IntoResponse {
    // Effective paths as JSON (serde_json::Value)
    let ep = arw_core::load_effective_paths();

    // Publish that JSON to the event bus
    state.bus.publish("Memory.Applied", &ep);

    // Return it to the client
    Json::<serde_json::Value>(ep)
}

async fn emit_test(State(state): State<AppState>) -> impl IntoResponse {
    let now_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    state
        .bus
        .publish("Service.Test", &json!({"msg":"ping","t": now_ms}));
    Json(json!({"ok": true}))
}

async fn shutdown(State(state): State<AppState>) -> impl IntoResponse {
    state
        .bus
        .publish("Service.Stop", &json!({"reason":"user request"}));
    let _ = state.stop_tx.send(());
    Json(json!({"ok": true}))
}

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
                    .data(data)
            )
        });

    let initial = tokio_stream::once(Ok::<Event, Infallible>(
        Event::default().id("0").event("Service.Connected").data("{}"),
    ));
    let stream = initial.chain(bstream);

    Sse::new(stream).keep_alive(
        KeepAlive::new()
            .interval(Duration::from_secs(15))
            .text("hb"),
    )
}
