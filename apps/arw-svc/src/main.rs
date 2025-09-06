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

    // Emit a startup event so /events sees something if connected early.
    state
        .bus
        .publish("Service.Start", &json!({"msg":"arw-svc started"}));

    let app = Router::new()
        .route("/healthz", get(healthz))
        .route("/introspect/tools", get(introspect_tools))
        .route("/introspect/schemas/:id", get(introspect_schema))
        .route("/probe", get(probe))
        .route("/events", get(events))
        .route("/emit/test", get(emit_test))
        .route("/shutdown", get(shutdown))
        .with_state(state.clone());

    let port: u16 = std::env::var("ARW_PORT")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(8090);
    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    info!("arw-svc listening on http://{}", addr);

    let listener = tokio::net::TcpListener::bind(addr).await.expect("bind");
    let server = axum::serve(listener, app).with_graceful_shutdown(async move {
        let _ = stop_rx.recv().await;
    });
    if let Err(e) = server.await {
        eprintln!("server error: {}", e);
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
            Ok::<Event, Infallible>(Event::default().event(env.kind.clone()).data(data))
        });

    let initial = tokio_stream::once(Ok::<Event, Infallible>(
        Event::default().event("Service.Connected").data("{}"),
    ));
    let stream = initial.chain(bstream);

    Sse::new(stream).keep_alive(
        KeepAlive::new()
            .interval(Duration::from_secs(15))
            .text("hb"),
    )
}
