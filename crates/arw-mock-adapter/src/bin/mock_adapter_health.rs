use axum::{routing::get, Json, Router};
use chrono::{SecondsFormat, Utc};
use serde_json::json;
use std::{env, net::SocketAddr};
use tracing_subscriber::{fmt, EnvFilter};

#[tokio::main]
async fn main() {
    let _ = fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .try_init();

    let app = Router::new().route(
        "/healthz",
        get(|| async move {
            let now = Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true);
            let body = json!({
                "status": {
                    "code": "ready",
                    "severity": "info",
                    "label": "Ready",
                    "detail": ["Mock adapter ok"],
                    "aria_hint": "Mock adapter healthy"
                },
                "generated": now,
                "http": {"avg_ewma_ms": 10.0, "errors": 0u64, "hits": 1u64}
            });
            Json(body)
        }),
    );

    let port = env::var("ARW_MOCK_ADAPTER_PORT")
        .ok()
        .and_then(|s| s.parse::<u16>().ok())
        .unwrap_or(8081);
    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    tracing::info!("mock adapter health server listening on {}", addr);
    axum::serve(tokio::net::TcpListener::bind(addr).await.unwrap(), app)
        .await
        .unwrap();
}
