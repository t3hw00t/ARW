use axum::{extract::Query, response::IntoResponse, Json};
use serde::Deserialize;

pub async fn feedback_suggestions() -> impl IntoResponse {
    let (v, list) = super::feedback_engine::snapshot().await;
    Json(serde_json::json!({"version": v, "suggestions": list}))
}

#[derive(Deserialize)]
pub struct UpdatesQs { since: Option<u64> }
pub async fn feedback_updates(Query(q): Query<UpdatesQs>) -> impl IntoResponse {
    let since = q.since.unwrap_or(0);
    match super::feedback_engine::updates_since(since).await {
        Some((v, list)) => Json(serde_json::json!({"version": v, "suggestions": list})).into_response(),
        None => (axum::http::StatusCode::NO_CONTENT, "").into_response(),
    }
}

