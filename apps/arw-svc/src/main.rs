use axum::{routing::get, extract::State, response::IntoResponse, Json, Router};
use axum::response::sse::{Sse, Event, KeepAlive};
use serde::Serialize;
use serde_json::json;
use std::net::SocketAddr;
use std::convert::Infallible;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio_stream::wrappers::BroadcastStream;
use tokio_stream::StreamExt;
use tracing::info;

#[derive(Clone)]
struct AppState {
    bus: arw_events::Bus
}

#[tokio::main]
async fn main() {
    arw_otel::init();

    let state = AppState {
        bus: arw_events::Bus::new(256),
    };

    // Emit a startup event so /events sees something if connected early.
    state.bus.publish("Service.Start", &json!({"msg":"arw-svc started"}));

    let app = Router::new()
        .route("/healthz", get(healthz))
        .route("/introspect/tools", get(introspect_tools))
        .route("/probe", get(probe))
        .route("/events", get(events))
        .route("/emit/test", get(emit_test))
        .with_state(state.clone());

    let port: u16 = std::env::var("ARW_PORT").ok().and_then(|s| s.parse().ok()).unwrap_or(8090);
    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    info!("arw-svc listening on http://{}", addr);

    let listener = tokio::net::TcpListener::bind(addr).await.expect("bind");
    if let Err(e) = axum::serve(listener, app).await {
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

#[derive(Serialize)]
struct ProbeOut {
    portable: bool,
    state_dir: String,
    cache_dir: String,
    logs_dir: String,
    memory: serde_json::Value
}

async fn probe(State(state): State<AppState>) -> impl IntoResponse {
    let ep = arw_core::load_effective_paths();
    let out = ProbeOut {
        portable: ep.portable,
        state_dir: ep.state_dir.display().to_string(),
        cache_dir: ep.cache_dir.display().to_string(),
        logs_dir: ep.logs_dir.display().to_string(),
        memory: serde_json::json!({"ephemeral":[],"episodic":[],"semantic":[],"procedural":[]})
    };
    state.bus.publish("Memory.Applied", &out);
    Json(out)
}

async fn emit_test(State(state): State<AppState>) -> impl IntoResponse {
    let now_ms = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis()).unwrap_or(0);
    state.bus.publish("Service.Test", &json!({"msg":"ping","t": now_ms}));
    Json(json!({"ok": true}))
}

async fn events(State(state): State<AppState>) -> Sse<impl tokio_stream::Stream<Item = Result<Event, Infallible>>> {
    let rx = state.bus.subscribe();
    let bstream = BroadcastStream::new(rx)
        .filter_map(|res| res.ok())
        .map(|env| {
            let data = serde_json::to_string(&env).unwrap_or_else(|_| "{}".to_string());
            Ok::<Event, Infallible>(Event::default().event(env.kind.clone()).data(data))
        });

    // Hello event so clients see output immediately
    let initial = tokio_stream::once(
        Ok::<Event, Infallible>(Event::default().event("Service.Connected").data("{}"))
    );
    let stream = initial.chain(bstream);

    Sse::new(stream).keep_alive(
        KeepAlive::new().interval(std::time::Duration::from_secs(15)).text("hb")
    )
}
