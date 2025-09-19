use axum::http::{HeaderMap, StatusCode};
use axum::response::IntoResponse;
use axum::{extract::State, Json};
use serde_json::json;

use crate::{admin_ok, AppState};

/// Trigger a manual distillation pass.
#[utoipa::path(
    post,
    path = "/admin/distill",
    tag = "Distill",
    responses(
        (status = 200, body = serde_json::Value, description = "Distillation summary"),
        (status = 401, description = "Unauthorized")
    )
)]
pub async fn distill_run(State(state): State<AppState>, headers: HeaderMap) -> impl IntoResponse {
    if !admin_ok(&headers) {
        return (
            StatusCode::UNAUTHORIZED,
            Json(json!({"type":"about:blank","title":"Unauthorized","status":401})),
        )
            .into_response();
    }
    let result = crate::distill::run_once(&state).await;
    (StatusCode::OK, Json(result)).into_response()
}
