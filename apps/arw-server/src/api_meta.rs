use axum::response::IntoResponse;
use axum::{extract::State, Json};
use serde_json::json;

/// Health probe.
#[utoipa::path(
    get,
    path = "/healthz",
    tag = "Meta",
    responses(
        (status = 200, description = "Service healthy", body = crate::openapi::HealthOk)
    )
)]
pub async fn healthz() -> impl IntoResponse {
    Json(json!({"ok": true}))
}

/// Service metadata and endpoints index.
#[utoipa::path(
    get,
    path = "/about",
    tag = "Meta",
    responses(
        (status = 200, description = "Service metadata", body = crate::openapi::AboutResponse)
    )
)]
pub async fn about(State(state): State<crate::AppState>) -> impl IntoResponse {
    let version = env!("CARGO_PKG_VERSION");
    let name = env!("CARGO_PKG_NAME");
    let docs = std::env::var("ARW_DOCS_URL").ok();
    let bind = std::env::var("ARW_BIND").unwrap_or_else(|_| "127.0.0.1".into());
    let port: u16 = std::env::var("ARW_PORT")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(8091);
    let tier = std::env::var("ARW_PERF_PRESET_TIER").ok();
    let http_max_conc: Option<usize> = std::env::var("ARW_HTTP_MAX_CONC")
        .ok()
        .and_then(|s| s.parse().ok());
    let actions_queue_max: Option<i64> = std::env::var("ARW_ACTIONS_QUEUE_MAX")
        .ok()
        .and_then(|s| s.parse().ok());
    let posture = std::env::var("ARW_SECURITY_POSTURE").ok();
    let endpoints = state.endpoints.as_ref().clone();
    let endpoints_meta = state.endpoints_meta.as_ref().clone();
    Json(json!({
        "service": name,
        "version": version,
        "http": {"bind": bind, "port": port},
        "docs_url": docs,
        "security_posture": posture,
        "counts": {"public": endpoints.len(), "admin": 0, "total": endpoints.len()},
        "endpoints": endpoints,
        "endpoints_meta": endpoints_meta,
        "perf_preset": {
            "tier": tier,
            "http_max_conc": http_max_conc,
            "actions_queue_max": actions_queue_max
        }
    }))
}
