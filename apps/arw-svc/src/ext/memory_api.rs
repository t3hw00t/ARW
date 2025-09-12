use crate::AppState;
use crate::resources::memory_service::MemoryService;
use arw_macros::{arw_gate, arw_admin};
use axum::{extract::State, response::IntoResponse, Json};

#[arw_admin(method="GET", path="/admin/memory", summary="Get memory snapshot")]
#[arw_gate("memory:get")]
pub(crate) async fn memory_get(State(state): State<AppState>) -> impl IntoResponse {
    if let Some(svc) = state.resources.get::<MemoryService>() {
        let v = svc.snapshot().await;
        return super::ok::<serde_json::Value>(v).into_response();
    }
    super::memory_get().await.into_response()
}
#[arw_admin(method="POST", path="/admin/memory/save", summary="Save memory to disk")]
#[arw_gate("memory:save")]
pub(crate) async fn memory_save(State(state): State<AppState>) -> impl IntoResponse {
    if let Some(svc) = state.resources.get::<MemoryService>() {
        match svc.save().await { Ok(()) => return super::ok(serde_json::json!({})).into_response(), Err(e) => return super::ApiError::internal(&e).into_response() }
    }
    super::memory_save().await.into_response()
}
#[arw_admin(method="POST", path="/admin/memory/load", summary="Load memory from disk")]
#[arw_gate("memory:load")]
pub(crate) async fn memory_load(State(state): State<AppState>) -> impl IntoResponse {
    if let Some(svc) = state.resources.get::<MemoryService>() {
        match svc.load().await { Ok(v) => return super::ok::<serde_json::Value>(v).into_response(), Err(e) => return super::ApiError::not_found(&e).into_response() }
    }
    super::memory_load().await.into_response()
}
#[arw_admin(method="POST", path="/admin/memory/apply", summary="Apply memory delta")]
#[arw_gate("memory:apply")]
pub(crate) async fn memory_apply(
    State(state): State<AppState>,
    Json(req): Json<super::ApplyMemory>,
) -> impl IntoResponse {
    super::memory_apply(State(state), Json(req))
        .await
        .into_response()
}
#[arw_admin(method="GET", path="/admin/memory/limit", summary="Get memory limit")]
#[arw_gate("memory:limit:get")]
pub(crate) async fn memory_limit_get(State(state): State<AppState>) -> impl IntoResponse {
    if let Some(svc) = state.resources.get::<MemoryService>() { return super::ok(serde_json::json!({ "limit": svc.get_limit().await })).into_response(); }
    super::memory_limit_get().await.into_response()
}
#[arw_admin(method="POST", path="/admin/memory/limit", summary="Set memory limit")]
#[arw_gate("memory:limit:set")]
pub(crate) async fn memory_limit_set(State(state): State<AppState>, Json(req): Json<super::SetLimit>) -> impl IntoResponse {
    if let Some(svc) = state.resources.get::<MemoryService>() { svc.set_limit(req.limit).await; return super::ok(serde_json::json!({})).into_response(); }
    super::memory_limit_set(Json(req)).await.into_response()
}
