use axum::http::HeaderMap;
use axum::response::IntoResponse;
use axum::{
    extract::{Query, State},
    Json,
};
use serde::Deserialize;
use serde_json::json;
use utoipa::{IntoParams, ToSchema};

use crate::{admin_ok, feedback, AppState};

fn unauthorized() -> axum::response::Response {
    (
        axum::http::StatusCode::UNAUTHORIZED,
        Json(json!({"type":"about:blank","title":"Unauthorized","status":401})),
    )
        .into_response()
}

fn map_error(err: feedback::FeedbackError) -> axum::response::Response {
    match err {
        feedback::FeedbackError::NotFound => (
            axum::http::StatusCode::NOT_FOUND,
            Json(json!({
                "type": "about:blank",
                "title": "Not Found",
                "status": 404,
                "detail": "unknown suggestion id"
            })),
        )
            .into_response(),
        feedback::FeedbackError::PolicyDenied(detail) => (
            axum::http::StatusCode::FORBIDDEN,
            Json(json!({
                "type": "about:blank",
                "title": "Forbidden",
                "status": 403,
                "detail": detail
            })),
        )
            .into_response(),
        feedback::FeedbackError::Invalid(detail) => (
            axum::http::StatusCode::BAD_REQUEST,
            Json(json!({
                "type": "about:blank",
                "title": "Invalid feedback action",
                "status": 400,
                "detail": detail
            })),
        )
            .into_response(),
    }
}

#[utoipa::path(
    get,
    path = "/admin/feedback/state",
    tag = "Admin/Feedback",
    responses(
        (status = 200, description = "Feedback engine state", body = feedback::FeedbackState),
        (status = 401, description = "Unauthorized")
    )
)]
pub async fn feedback_state(
    headers: HeaderMap,
    State(state): State<AppState>,
) -> impl IntoResponse {
    if !admin_ok(&headers).await {
        return unauthorized();
    }
    let snapshot = state.feedback().snapshot().await;
    Json(snapshot).into_response()
}

#[derive(Deserialize, ToSchema)]
pub struct FeedbackSignalRequest {
    pub kind: String,
    pub target: String,
    pub confidence: f64,
    pub severity: u8,
    #[serde(default)]
    pub note: Option<String>,
}

#[utoipa::path(
    post,
    path = "/admin/feedback/signal",
    tag = "Admin/Feedback",
    request_body = FeedbackSignalRequest,
    responses(
        (status = 200, description = "Signal recorded", body = feedback::FeedbackState),
        (status = 401, description = "Unauthorized")
    )
)]
pub async fn feedback_signal(
    headers: HeaderMap,
    State(state): State<AppState>,
    Json(req): Json<FeedbackSignalRequest>,
) -> impl IntoResponse {
    if !admin_ok(&headers).await {
        return unauthorized();
    }
    let snapshot = state
        .feedback()
        .submit_signal(req.kind, req.target, req.confidence, req.severity, req.note)
        .await;
    Json(snapshot).into_response()
}

#[utoipa::path(
    post,
    path = "/admin/feedback/analyze",
    tag = "Admin/Feedback",
    responses(
        (status = 200, description = "Recomputed suggestions", body = feedback::FeedbackState),
        (status = 401, description = "Unauthorized")
    )
)]
pub async fn feedback_analyze(
    headers: HeaderMap,
    State(state): State<AppState>,
) -> impl IntoResponse {
    if !admin_ok(&headers).await {
        return unauthorized();
    }
    let snapshot = state.feedback().analyze().await;
    Json(snapshot).into_response()
}

#[derive(Deserialize, ToSchema)]
pub struct FeedbackApplyRequest {
    pub id: String,
}

#[utoipa::path(
    post,
    path = "/admin/feedback/apply",
    tag = "Admin/Feedback",
    request_body = FeedbackApplyRequest,
    responses(
        (status = 200, description = "Suggestion applied", body = serde_json::Value),
        (status = 401, description = "Unauthorized"),
        (status = 403, description = "Policy denied"),
        (status = 404, description = "Unknown suggestion"),
        (status = 400, description = "Invalid suggestion payload")
    )
)]
pub async fn feedback_apply(
    headers: HeaderMap,
    State(state): State<AppState>,
    Json(req): Json<FeedbackApplyRequest>,
) -> impl IntoResponse {
    if !admin_ok(&headers).await {
        return unauthorized();
    }
    match state.feedback().apply(&req.id, "feedback.apply").await {
        Ok(()) => Json(json!({"ok": true})).into_response(),
        Err(err) => map_error(err),
    }
}

#[derive(Deserialize, ToSchema)]
pub struct FeedbackAutoRequest {
    pub enabled: bool,
}

#[utoipa::path(
    post,
    path = "/admin/feedback/auto",
    tag = "Admin/Feedback",
    request_body = FeedbackAutoRequest,
    responses(
        (status = 200, description = "Auto-apply updated", body = serde_json::Value),
        (status = 401, description = "Unauthorized")
    )
)]
pub async fn feedback_auto(
    headers: HeaderMap,
    State(state): State<AppState>,
    Json(req): Json<FeedbackAutoRequest>,
) -> impl IntoResponse {
    if !admin_ok(&headers).await {
        return unauthorized();
    }
    let snapshot = state.feedback().set_auto_apply(req.enabled).await;
    Json(json!({"ok": true, "auto_apply": snapshot.auto_apply})).into_response()
}

#[utoipa::path(
    post,
    path = "/admin/feedback/reset",
    tag = "Admin/Feedback",
    responses(
        (status = 200, description = "Feedback state cleared", body = serde_json::Value),
        (status = 401, description = "Unauthorized")
    )
)]
pub async fn feedback_reset(
    headers: HeaderMap,
    State(state): State<AppState>,
) -> impl IntoResponse {
    if !admin_ok(&headers).await {
        return unauthorized();
    }
    state.feedback().reset().await;
    Json(json!({"ok": true})).into_response()
}

#[utoipa::path(
    get,
    path = "/admin/feedback/suggestions",
    tag = "Admin/Feedback",
    responses(
        (status = 200, description = "Current suggestions", body = serde_json::Value),
        (status = 401, description = "Unauthorized")
    )
)]
pub async fn feedback_suggestions(
    headers: HeaderMap,
    State(state): State<AppState>,
) -> impl IntoResponse {
    if !admin_ok(&headers).await {
        return unauthorized();
    }
    let (version, list) = state.feedback().suggestions_snapshot().await;
    Json(json!({"version": version, "suggestions": list})).into_response()
}

#[derive(Debug, Default, Deserialize, ToSchema, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct FeedbackUpdatesQuery {
    #[serde(default)]
    pub since: Option<u64>,
}

#[utoipa::path(
    get,
    path = "/admin/feedback/updates",
    tag = "Admin/Feedback",
    params(FeedbackUpdatesQuery),
    responses(
        (status = 200, description = "Suggestions updated", body = serde_json::Value),
        (status = 204, description = "No changes since provided version"),
        (status = 401, description = "Unauthorized")
    )
)]
pub async fn feedback_updates(
    headers: HeaderMap,
    State(state): State<AppState>,
    Query(q): Query<FeedbackUpdatesQuery>,
) -> impl IntoResponse {
    if !admin_ok(&headers).await {
        return unauthorized();
    }
    let since = q.since.unwrap_or(0);
    match state.feedback().updates_since(since).await {
        Some((version, list)) => {
            Json(json!({"version": version, "suggestions": list})).into_response()
        }
        None => axum::http::StatusCode::NO_CONTENT.into_response(),
    }
}

#[utoipa::path(
    get,
    path = "/admin/feedback/policy",
    tag = "Admin/Feedback",
    responses(
        (status = 200, description = "Effective policy", body = serde_json::Value),
        (status = 401, description = "Unauthorized")
    )
)]
pub async fn feedback_policy(
    headers: HeaderMap,
    State(state): State<AppState>,
) -> impl IntoResponse {
    if !admin_ok(&headers).await {
        return unauthorized();
    }
    Json(state.feedback().effective_policy()).into_response()
}

#[utoipa::path(
    get,
    path = "/admin/feedback/versions",
    tag = "Admin/Feedback",
    responses(
        (status = 200, description = "Available snapshots", body = serde_json::Value),
        (status = 401, description = "Unauthorized")
    )
)]
pub async fn feedback_versions(
    headers: HeaderMap,
    State(state): State<AppState>,
) -> impl IntoResponse {
    if !admin_ok(&headers).await {
        return unauthorized();
    }
    let versions = state.feedback().list_versions().await;
    Json(json!({"versions": versions})).into_response()
}

#[derive(Debug, Default, Deserialize, ToSchema, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct FeedbackRollbackQuery {
    #[serde(default)]
    pub to: Option<u64>,
}

#[utoipa::path(
    post,
    path = "/admin/feedback/rollback",
    tag = "Admin/Feedback",
    params(FeedbackRollbackQuery),
    responses(
        (status = 200, description = "Snapshot restored", body = serde_json::Value),
        (status = 401, description = "Unauthorized"),
        (status = 404, description = "Snapshot not found")
    )
)]
pub async fn feedback_rollback(
    headers: HeaderMap,
    State(state): State<AppState>,
    Query(q): Query<FeedbackRollbackQuery>,
) -> impl IntoResponse {
    if !admin_ok(&headers).await {
        return unauthorized();
    }
    match state.feedback().rollback(q.to).await {
        Some((version, list)) => {
            Json(json!({"version": version, "suggestions": list})).into_response()
        }
        None => (
            axum::http::StatusCode::NOT_FOUND,
            Json(json!({
                "type": "about:blank",
                "title": "Not Found",
                "status": 404,
                "detail": "snapshot not available"
            })),
        )
            .into_response(),
    }
}
