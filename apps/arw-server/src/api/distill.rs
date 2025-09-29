use axum::http::{HeaderMap, StatusCode};
use axum::response::IntoResponse;
use axum::{extract::State, Json};

use crate::AppState;

/// Trigger a manual distillation pass.
#[utoipa::path(
    post,
    path = "/admin/distill",
    tag = "Distill",
    responses(
        (status = 200, body = serde_json::Value, description = "Distillation summary"),
        (status = 401, description = "Unauthorized", body = arw_protocol::ProblemDetails)
    )
)]
pub async fn distill_run(State(state): State<AppState>, headers: HeaderMap) -> impl IntoResponse {
    if let Err(resp) = crate::responses::require_admin(&headers) {
        return *resp;
    }
    let result = crate::distill::run_once(&state).await;
    (StatusCode::OK, Json(result)).into_response()
}
