use axum::{extract::Query, response::IntoResponse, Json};
use serde::Deserialize;

pub async fn feedback_suggestions() -> impl IntoResponse {
    let (v, list) = super::feedback_engine::snapshot().await;
    Json(serde_json::json!({"version": v, "suggestions": list}))
}

#[derive(Deserialize)]
pub struct UpdatesQs {
    since: Option<u64>,
}
pub async fn feedback_updates(Query(q): Query<UpdatesQs>) -> impl IntoResponse {
    let since = q.since.unwrap_or(0);
    match super::feedback_engine::updates_since(since).await {
        Some((v, list)) => {
            Json(serde_json::json!({"version": v, "suggestions": list})).into_response()
        }
        None => (axum::http::StatusCode::NO_CONTENT, "").into_response(),
    }
}

pub async fn feedback_policy_get() -> impl IntoResponse {
    let cfg = super::policy::super_effective_policy();
    Json(cfg)
}

#[derive(Deserialize)]
pub struct RbQs {
    pub to: Option<u64>,
}
pub async fn feedback_rollback(Query(q): Query<RbQs>) -> impl IntoResponse {
    match super::feedback_engine::rollback_to(q.to).await {
        Some((v, list)) => {
            Json(serde_json::json!({"ok": true, "version": v, "suggestions": list})).into_response()
        }
        None => (axum::http::StatusCode::NOT_FOUND, "no backup").into_response(),
    }
}

pub async fn feedback_versions() -> impl IntoResponse {
    let list = super::feedback_engine::list_versions().await;
    Json(serde_json::json!({"versions": list}))
}
