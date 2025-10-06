use axum::http::HeaderMap;
use axum::response::IntoResponse;
use axum::{
    extract::{Path, State},
    Json,
};
use serde::Deserialize;
use serde_json::json;
use utoipa::ToSchema;

use crate::{research_watcher, AppState};

#[derive(Debug, Deserialize, ToSchema)]
pub struct WatcherDecision {
    #[serde(default)]
    pub note: Option<String>,
}

#[utoipa::path(
    post,
    path = "/research_watcher/{id}/approve",
    tag = "Research",
    params(("id" = String, Path, description = "Watcher item id")),
    request_body = WatcherDecision,
    responses(
        (status = 200, description = "Updated item", body = serde_json::Value),
        (status = 401, description = "Unauthorized"),
        (status = 404, description = "Not found"),
        (status = 500, description = "Error")
    )
)]
pub async fn research_watcher_approve(
    headers: HeaderMap,
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<WatcherDecision>,
) -> impl IntoResponse {
    if !crate::admin_ok(&headers).await {
        return (
            axum::http::StatusCode::UNAUTHORIZED,
            Json(json!({"type":"about:blank","title":"Unauthorized","status":401})),
        )
            .into_response();
    }
    match research_watcher::update_status(&state, &id, "approved", body.note.clone()).await {
        Ok(Some(item)) => (
            axum::http::StatusCode::OK,
            Json(json!({"ok": true, "item": item})),
        )
            .into_response(),
        Ok(None) => (
            axum::http::StatusCode::NOT_FOUND,
            Json(json!({"type":"about:blank","title":"Not Found","status":404})),
        )
            .into_response(),
        Err(err) => (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({
                "type":"about:blank",
                "title":"Error",
                "status":500,
                "detail": err.to_string()
            })),
        )
            .into_response(),
    }
}

#[utoipa::path(
    post,
    path = "/research_watcher/{id}/archive",
    tag = "Research",
    params(("id" = String, Path, description = "Watcher item id")),
    request_body = WatcherDecision,
    responses(
        (status = 200, description = "Updated item", body = serde_json::Value),
        (status = 401, description = "Unauthorized"),
        (status = 404, description = "Not found"),
        (status = 500, description = "Error")
    )
)]
pub async fn research_watcher_archive(
    headers: HeaderMap,
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<WatcherDecision>,
) -> impl IntoResponse {
    if !crate::admin_ok(&headers).await {
        return (
            axum::http::StatusCode::UNAUTHORIZED,
            Json(json!({"type":"about:blank","title":"Unauthorized","status":401})),
        )
            .into_response();
    }
    match research_watcher::update_status(&state, &id, "archived", body.note.clone()).await {
        Ok(Some(item)) => (
            axum::http::StatusCode::OK,
            Json(json!({"ok": true, "item": item})),
        )
            .into_response(),
        Ok(None) => (
            axum::http::StatusCode::NOT_FOUND,
            Json(json!({"type":"about:blank","title":"Not Found","status":404})),
        )
            .into_response(),
        Err(err) => (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({
                "type":"about:blank",
                "title":"Error",
                "status":500,
                "detail": err.to_string()
            })),
        )
            .into_response(),
    }
}
