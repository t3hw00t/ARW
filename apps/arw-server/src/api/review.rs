use axum::http::{HeaderMap, StatusCode};
use axum::{extract::State, response::IntoResponse, Json};
use serde_json::json;

use crate::{review, AppState};

fn unauthorized() -> axum::response::Response {
    (
        StatusCode::UNAUTHORIZED,
        Json(json!({
            "type": "about:blank",
            "title": "Unauthorized",
            "status": 401
        })),
    )
        .into_response()
}

fn storage_error(detail: impl Into<String>) -> axum::response::Response {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(json!({
            "type": "about:blank",
            "title": "Review storage error",
            "status": 500,
            "detail": detail.into()
        })),
    )
        .into_response()
}

fn not_found(detail: impl Into<String>) -> axum::response::Response {
    (
        StatusCode::NOT_FOUND,
        Json(json!({
            "type": "about:blank",
            "title": "Not Found",
            "status": 404,
            "detail": detail.into()
        })),
    )
        .into_response()
}

#[utoipa::path(
    get,
    path = "/admin/memory/quarantine",
    tag = "Review",
    responses(
        (status = 200, description = "Memory quarantine entries", body = serde_json::Value),
        (status = 401, description = "Unauthorized")
    )
)]
pub async fn memory_quarantine_get(headers: HeaderMap) -> impl IntoResponse {
    if !crate::admin_ok(&headers) {
        return unauthorized();
    }
    let items = review::memory_quarantine_list().await;
    Json(items).into_response()
}

#[utoipa::path(
    post,
    path = "/admin/memory/quarantine",
    tag = "Review",
    request_body = review::MemoryQuarantineRequest,
    responses(
        (status = 200, description = "Queued for review", body = serde_json::Value),
        (status = 401, description = "Unauthorized"),
        (status = 500, description = "Storage error")
    )
)]
pub async fn memory_quarantine_queue(
    headers: HeaderMap,
    State(state): State<AppState>,
    Json(req): Json<review::MemoryQuarantineRequest>,
) -> impl IntoResponse {
    if !crate::admin_ok(&headers) {
        return unauthorized();
    }
    match review::memory_quarantine_queue(&state.bus(), req).await {
        Ok(_) => Json(json!({"ok": true})).into_response(),
        Err(err) => storage_error(err.to_string()),
    }
}

#[utoipa::path(
    post,
    path = "/admin/memory/quarantine/admit",
    tag = "Review",
    request_body = review::MemoryQuarantineAdmit,
    responses(
        (status = 200, description = "Entry removed", body = serde_json::Value),
        (status = 401, description = "Unauthorized"),
        (status = 500, description = "Storage error")
    )
)]
pub async fn memory_quarantine_admit(
    headers: HeaderMap,
    State(state): State<AppState>,
    Json(req): Json<review::MemoryQuarantineAdmit>,
) -> impl IntoResponse {
    if !crate::admin_ok(&headers) {
        return unauthorized();
    }
    match review::memory_quarantine_admit(&state.bus(), req).await {
        Ok((removed, _)) => Json(json!({"removed": removed})).into_response(),
        Err(err) => storage_error(err.to_string()),
    }
}

#[utoipa::path(
    get,
    path = "/admin/world_diffs",
    tag = "Review",
    responses(
        (status = 200, description = "Queued world diffs", body = serde_json::Value),
        (status = 401, description = "Unauthorized")
    )
)]
pub async fn world_diffs_get(headers: HeaderMap) -> impl IntoResponse {
    if !crate::admin_ok(&headers) {
        return unauthorized();
    }
    let items = review::world_diffs_list().await;
    Json(items).into_response()
}

#[utoipa::path(
    post,
    path = "/admin/world_diffs/queue",
    tag = "Review",
    request_body = review::WorldDiffQueueRequest,
    responses(
        (status = 200, description = "Diff queued", body = serde_json::Value),
        (status = 401, description = "Unauthorized"),
        (status = 500, description = "Storage error")
    )
)]
pub async fn world_diffs_queue(
    headers: HeaderMap,
    State(state): State<AppState>,
    Json(req): Json<review::WorldDiffQueueRequest>,
) -> impl IntoResponse {
    if !crate::admin_ok(&headers) {
        return unauthorized();
    }
    match review::world_diffs_queue(&state.bus(), req).await {
        Ok(_) => Json(json!({"ok": true})).into_response(),
        Err(err) => storage_error(err.to_string()),
    }
}

#[utoipa::path(
    post,
    path = "/admin/world_diffs/decision",
    tag = "Review",
    request_body = review::WorldDiffDecision,
    responses(
        (status = 200, description = "Decision recorded", body = serde_json::Value),
        (status = 401, description = "Unauthorized"),
        (status = 404, description = "Diff not found"),
        (status = 500, description = "Storage error")
    )
)]
pub async fn world_diffs_decision(
    headers: HeaderMap,
    State(state): State<AppState>,
    Json(req): Json<review::WorldDiffDecision>,
) -> impl IntoResponse {
    if !crate::admin_ok(&headers) {
        return unauthorized();
    }
    match review::world_diffs_decision(&state.bus(), req).await {
        Ok(Some(_)) => Json(json!({"ok": true})).into_response(),
        Ok(None) => not_found("diff not found"),
        Err(err) => storage_error(err.to_string()),
    }
}
