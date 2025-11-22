use std::collections::BTreeSet;

use axum::http::HeaderMap;
use axum::response::IntoResponse;
use axum::response::Response;
use axum::{extract::State, Json};
use serde_json::json;

use arw_core::list_admin_endpoints;

/// Health probe.
#[utoipa::path(
    get,
    path = "/healthz",
    tag = "Meta",
    operation_id = "healthz_doc",
    description = "Service readiness probe.",
    responses(
        (status = 200, description = "Service healthy", body = crate::openapi::HealthOk)
    )
)]
pub async fn healthz() -> impl IntoResponse {
    crate::responses::json_ok(json!({"ok": true}))
}

/// Bus lag diagnostics (dev-only; hidden).
#[utoipa::path(
    get,
    path = "/dev/bus/lag",
    tag = "Meta",
    operation_id = "bus_lag_doc",
    description = "Aggregated bus lag per subscriber (dev only; admin)",
    responses(
        (status = 200, description = "Lag stats", body = serde_json::Value),
        (status = 401, description = "Unauthorized"),
    )
)]
pub async fn bus_lag(headers: HeaderMap) -> Response {
    if !crate::admin_ok(&headers).await {
        return crate::responses::unauthorized(None);
    }
    let stats = crate::util::bus_lag_stats();
    crate::responses::json_ok(json!({ "lag": stats })).into_response()
}

/// Bus counters + lag (dev-only; hidden).
#[utoipa::path(
    get,
    path = "/dev/bus/stats",
    tag = "Meta",
    operation_id = "bus_stats_doc",
    description = "Bus counters and lag totals (dev only; admin).",
    responses(
        (status = 200, description = "Bus stats", body = serde_json::Value),
        (status = 401, description = "Unauthorized"),
    )
)]
pub async fn bus_stats(headers: HeaderMap, State(state): State<crate::AppState>) -> Response {
    if !crate::admin_ok(&headers).await {
        return crate::responses::unauthorized(None);
    }
    let counters = state.bus().stats();
    let lag = crate::util::bus_lag_stats();
    crate::responses::json_ok(json!({
        "counters": counters,
        "lag": lag,
    }))
    .into_response()
}

/// Service metadata and endpoints index.
#[utoipa::path(
    get,
    path = "/about",
    tag = "Meta",
    operation_id = "about_doc",
    description = "Service metadata, endpoints index, and performance preset.",
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
    let endpoints = state.endpoints();
    let endpoints_meta = state.endpoints_meta();
    let admin_endpoints = list_admin_endpoints();

    let admin_paths: BTreeSet<String> = admin_endpoints
        .iter()
        .map(|ep| ep.path.to_string())
        .collect();
    let admin_entries: BTreeSet<String> = admin_endpoints
        .iter()
        .map(|ep| format!("{} {}", ep.method, ep.path))
        .collect();

    let mut public_set: BTreeSet<String> = endpoints.as_ref().clone().into_iter().collect();
    for entry in &admin_entries {
        public_set.remove(entry);
    }

    let public_count = public_set.len();
    let admin_count = admin_endpoints.len();

    let mut endpoints_meta_vec = endpoints_meta.as_ref().clone();
    endpoints_meta_vec.retain(|entry| {
        entry
            .get("path")
            .and_then(|v| v.as_str())
            .map(|path| !admin_paths.contains(path))
            .unwrap_or(true)
    });

    let mut endpoint_set = public_set.clone();
    for admin in admin_endpoints {
        let entry = format!("{} {}", admin.method, admin.path);
        endpoint_set.insert(entry.clone());
        endpoints_meta_vec.push(json!({
            "method": admin.method,
            "path": admin.path,
            "summary": admin.summary,
            "stability": "admin",
        }));
    }

    let endpoints_vec: Vec<String> = endpoint_set.into_iter().collect();
    let total_count = endpoints_vec.len();
    crate::responses::json_ok(json!({
        "service": name,
        "version": version,
        "http": {"bind": bind, "port": port},
        "docs_url": docs,
        "security_posture": posture,
        "counts": {"public": public_count, "admin": admin_count, "total": total_count},
        "endpoints": endpoints_vec,
        "endpoints_meta": endpoints_meta_vec,
        "perf_preset": {
            "tier": tier,
            "http_max_conc": http_max_conc,
            "actions_queue_max": actions_queue_max
        }
    }))
}

/// Graceful shutdown (admin/debug only). For development convenience.
#[utoipa::path(
    get,
    path = "/shutdown",
    tag = "Meta",
    responses((status = 200, description = "Exiting soon", body = serde_json::Value), (status = 401))
)]
pub async fn shutdown(headers: HeaderMap) -> impl IntoResponse {
    if !crate::admin_ok(&headers).await {
        return (
            axum::http::StatusCode::UNAUTHORIZED,
            Json(json!({"type":"about:blank","title":"Unauthorized","status":401})),
        )
            .into_response();
    }
    tokio::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        std::process::exit(0);
    });
    (axum::http::StatusCode::OK, Json(json!({"ok": true}))).into_response()
}
