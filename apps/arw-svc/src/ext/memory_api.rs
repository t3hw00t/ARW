use axum::{extract::State, response::IntoResponse, Json};
use serde_json::Value;
use crate::AppState;

pub(crate) async fn memory_get() -> impl IntoResponse { super::memory_get().await }
pub(crate) async fn memory_save() -> impl IntoResponse { super::memory_save().await }
pub(crate) async fn memory_load() -> impl IntoResponse { super::memory_load().await }
pub(crate) async fn memory_apply(State(state): State<AppState>, Json(req): Json<super::ApplyMemory>) -> impl IntoResponse {
    super::memory_apply(State(state), Json(req)).await
}
pub(crate) async fn memory_limit_get() -> impl IntoResponse { super::memory_limit_get().await }
pub(crate) async fn memory_limit_set(Json(req): Json<super::SetLimit>) -> impl IntoResponse {
    super::memory_limit_set(Json(req)).await
}

