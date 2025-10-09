use axum::http::{HeaderMap, StatusCode};
use axum::response::IntoResponse;
use axum::Json;
use serde_json::json;

use crate::{admin_ok, crashguard, read_models};

/// Crash log snapshot from state_dir/crash and crash/archive.
#[utoipa::path(
    get,
    path = "/state/crashlog",
    tag = "State",
    responses(
        (status = 200, description = "Crash log", body = serde_json::Value),
        (status = 401, description = "Unauthorized", body = serde_json::Value)
    )
)]
pub async fn state_crashlog(headers: HeaderMap) -> impl IntoResponse {
    if !admin_ok(&headers).await {
        return (
            StatusCode::UNAUTHORIZED,
            Json(json!({"type":"about:blank","title":"Unauthorized","status":401})),
        )
            .into_response();
    }
    let value = read_models::crashlog_snapshot().await;
    Json(value).into_response()
}

/// Screenshots OCR index snapshot.
#[utoipa::path(
    get,
    path = "/state/screenshots",
    tag = "State",
    operation_id = "state_screenshots_doc",
    description = "Indexed OCR sidecars for captured screenshots, grouped by source path and language.",
    responses(
        (status = 200, description = "Screenshots index", body = serde_json::Value),
        (status = 401, description = "Unauthorized", body = serde_json::Value)
    )
)]
pub async fn state_screenshots(headers: HeaderMap) -> impl IntoResponse {
    if !admin_ok(&headers).await {
        return (
            StatusCode::UNAUTHORIZED,
            Json(json!({"type":"about:blank","title":"Unauthorized","status":401})),
        )
            .into_response();
    }
    let value = read_models::screenshots_snapshot().await;
    Json(value).into_response()
}

/// Aggregated service health (read-model built from service.health events).
#[utoipa::path(
    get,
    path = "/state/service_health",
    tag = "State",
    responses(
        (status = 200, description = "Service health", body = serde_json::Value),
        (status = 401, description = "Unauthorized", body = serde_json::Value)
    )
)]
pub async fn state_service_health(headers: HeaderMap) -> impl IntoResponse {
    if !admin_ok(&headers).await {
        return (
            StatusCode::UNAUTHORIZED,
            Json(json!({"type":"about:blank","title":"Unauthorized","status":401})),
        )
            .into_response();
    }
    let value = read_models::cached_read_model("service_health")
        .unwrap_or_else(|| json!({"history": [], "last": null}));
    Json(value).into_response()
}

/// Consolidated service status: safe-mode, last crash, and last health signal.
#[utoipa::path(
    get,
    path = "/state/service_status",
    tag = "State",
    responses(
        (status = 200, description = "Service status", body = serde_json::Value),
        (status = 401, description = "Unauthorized", body = serde_json::Value)
    )
)]
pub async fn state_service_status(headers: HeaderMap) -> impl IntoResponse {
    if !admin_ok(&headers).await {
        return (
            StatusCode::UNAUTHORIZED,
            Json(json!({"type":"about:blank","title":"Unauthorized","status":401})),
        )
            .into_response();
    }
    let until_ms = crashguard::safe_mode_until_ms();
    let safe_mode = if until_ms > 0 {
        json!({"active": true, "until_ms": until_ms})
    } else {
        json!({"active": false})
    };
    let crashlog = read_models::cached_read_model("crashlog")
        .unwrap_or_else(|| json!({"count": 0, "items": []}));
    let last_crash = crashlog
        .get("items")
        .and_then(|v| v.as_array())
        .and_then(|arr| arr.first())
        .cloned()
        .unwrap_or(serde_json::Value::Null);
    let service_health = read_models::cached_read_model("service_health")
        .unwrap_or_else(|| json!({"history": [], "last": null}));
    let last_health = service_health.get("last").cloned().unwrap_or(serde_json::Value::Null);
    Json(json!({
        "safe_mode": safe_mode,
        "last_crash": last_crash,
        "last_health": last_health,
    }))
    .into_response()
}
