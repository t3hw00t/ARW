use crate::AppState;
use arw_macros::arw_gate;
use axum::{extract::State, response::IntoResponse, Json};

#[arw_gate("memory:get")]
pub(crate) async fn memory_get() -> impl IntoResponse {
    super::memory_get().await.into_response()
}
#[arw_gate("memory:save")]
pub(crate) async fn memory_save() -> impl IntoResponse {
    super::memory_save().await.into_response()
}
#[arw_gate("memory:load")]
pub(crate) async fn memory_load() -> impl IntoResponse {
    super::memory_load().await.into_response()
}
#[arw_gate("memory:apply")]
pub(crate) async fn memory_apply(
    State(state): State<AppState>,
    Json(req): Json<super::ApplyMemory>,
) -> impl IntoResponse {
    super::memory_apply(State(state), Json(req))
        .await
        .into_response()
}
#[arw_gate("memory:limit:get")]
pub(crate) async fn memory_limit_get() -> impl IntoResponse {
    super::memory_limit_get().await.into_response()
}
#[arw_gate("memory:limit:set")]
pub(crate) async fn memory_limit_set(Json(req): Json<super::SetLimit>) -> impl IntoResponse {
    super::memory_limit_set(Json(req)).await.into_response()
}
