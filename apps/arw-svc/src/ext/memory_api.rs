use crate::AppState;
use axum::{extract::State, response::IntoResponse, Json};
use arw_core::{gating, gating_keys as gk};

pub(crate) async fn memory_get() -> impl IntoResponse {
    if !gating::allowed(gk::MEMORY_GET) { return (axum::http::StatusCode::FORBIDDEN, "gated").into_response(); }
    super::memory_get().await.into_response()
}
pub(crate) async fn memory_save() -> impl IntoResponse {
    if !gating::allowed(gk::MEMORY_SAVE) { return (axum::http::StatusCode::FORBIDDEN, "gated").into_response(); }
    super::memory_save().await.into_response()
}
pub(crate) async fn memory_load() -> impl IntoResponse {
    if !gating::allowed(gk::MEMORY_LOAD) { return (axum::http::StatusCode::FORBIDDEN, "gated").into_response(); }
    super::memory_load().await.into_response()
}
pub(crate) async fn memory_apply(
    State(state): State<AppState>,
    Json(req): Json<super::ApplyMemory>,
) -> impl IntoResponse {
    if !gating::allowed(gk::MEMORY_APPLY) { return (axum::http::StatusCode::FORBIDDEN, "gated").into_response(); }
    super::memory_apply(State(state), Json(req)).await.into_response()
}
pub(crate) async fn memory_limit_get() -> impl IntoResponse {
    if !gating::allowed(gk::MEMORY_LIMIT_GET) { return (axum::http::StatusCode::FORBIDDEN, "gated").into_response(); }
    super::memory_limit_get().await.into_response()
}
pub(crate) async fn memory_limit_set(Json(req): Json<super::SetLimit>) -> impl IntoResponse {
    if !gating::allowed(gk::MEMORY_LIMIT_SET) { return (axum::http::StatusCode::FORBIDDEN, "gated").into_response(); }
    super::memory_limit_set(Json(req)).await.into_response()
}
