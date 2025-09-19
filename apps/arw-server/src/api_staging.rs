use axum::http::HeaderMap;
use axum::response::IntoResponse;
use axum::{
    extract::{Path, State},
    Json,
};
use serde::Deserialize;
use serde_json::json;
use utoipa::ToSchema;

use crate::{staging, AppState};

#[derive(Debug, Deserialize, ToSchema)]
pub struct StagingDecision {
    #[serde(default)]
    pub reason: Option<String>,
    #[serde(default)]
    pub decided_by: Option<String>,
}

fn map_error(err: &anyhow::Error) -> axum::http::StatusCode {
    let msg = err.to_string();
    if msg.contains("not found") {
        axum::http::StatusCode::NOT_FOUND
    } else if msg.contains("pending") {
        axum::http::StatusCode::BAD_REQUEST
    } else {
        axum::http::StatusCode::INTERNAL_SERVER_ERROR
    }
}

#[utoipa::path(
    post,
    path = "/staging/actions/{id}/approve",
    tag = "Staging",
    params(("id" = String, Path, description = "Staging entry id")),
    request_body = StagingDecision,
    responses(
        (status = 200, description = "Action queued", body = serde_json::Value),
        (status = 401, description = "Unauthorized"),
        (status = 404, description = "Not found"),
        (status = 500, description = "Error")
    )
)]
pub async fn staging_action_approve(
    headers: HeaderMap,
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<StagingDecision>,
) -> impl IntoResponse {
    if !crate::admin_ok(&headers) {
        return (
            axum::http::StatusCode::UNAUTHORIZED,
            Json(json!({"type":"about:blank","title":"Unauthorized","status":401})),
        )
            .into_response();
    }
    match staging::approve_action(&state, &id, body.decided_by.clone()).await {
        Ok(action_id) => (
            axum::http::StatusCode::OK,
            Json(json!({"ok": true, "action_id": action_id})),
        )
            .into_response(),
        Err(err) => {
            let status = map_error(&err);
            (
                status,
                Json(json!({
                    "type":"about:blank",
                    "title": if status == axum::http::StatusCode::NOT_FOUND {"Not Found"} else {"Error"},
                    "status": status.as_u16(),
                    "detail": err.to_string()
                })),
            )
                .into_response()
        }
    }
}

#[utoipa::path(
    post,
    path = "/staging/actions/{id}/deny",
    tag = "Staging",
    params(("id" = String, Path, description = "Staging entry id")),
    request_body = StagingDecision,
    responses(
        (status = 200, description = "Action denied", body = serde_json::Value),
        (status = 401, description = "Unauthorized"),
        (status = 404, description = "Not found"),
        (status = 500, description = "Error")
    )
)]
pub async fn staging_action_deny(
    headers: HeaderMap,
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<StagingDecision>,
) -> impl IntoResponse {
    if !crate::admin_ok(&headers) {
        return (
            axum::http::StatusCode::UNAUTHORIZED,
            Json(json!({"type":"about:blank","title":"Unauthorized","status":401})),
        )
            .into_response();
    }
    match staging::deny_action(&state, &id, body.reason.clone(), body.decided_by.clone()).await {
        Ok(()) => (axum::http::StatusCode::OK, Json(json!({"ok": true}))).into_response(),
        Err(err) => {
            let status = map_error(&err);
            (
                status,
                Json(json!({
                    "type":"about:blank",
                    "title": if status == axum::http::StatusCode::NOT_FOUND {"Not Found"} else {"Error"},
                    "status": status.as_u16(),
                    "detail": err.to_string()
                })),
            )
                .into_response()
        }
    }
}
