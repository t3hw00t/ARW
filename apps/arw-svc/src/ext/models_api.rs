use crate::AppState;
use axum::{extract::State, response::IntoResponse, Json};
use arw_core::{gating, gating_keys as gk};
use serde::Deserialize;

pub(crate) async fn list_models() -> impl IntoResponse {
    if !gating::allowed(gk::MODELS_LIST) { return (axum::http::StatusCode::FORBIDDEN, "gated").into_response(); }
    super::list_models().await.into_response()
}
pub(crate) async fn refresh_models(State(state): State<AppState>) -> impl IntoResponse {
    if !gating::allowed(gk::MODELS_REFRESH) { return (axum::http::StatusCode::FORBIDDEN, "gated").into_response(); }
    super::refresh_models(State(state)).await.into_response()
}
pub(crate) async fn models_save() -> impl IntoResponse {
    if !gating::allowed(gk::MODELS_SAVE) { return (axum::http::StatusCode::FORBIDDEN, "gated").into_response(); }
    super::models_save().await.into_response()
}
pub(crate) async fn models_load() -> impl IntoResponse {
    if !gating::allowed(gk::MODELS_LOAD) { return (axum::http::StatusCode::FORBIDDEN, "gated").into_response(); }
    super::models_load().await.into_response()
}

#[derive(Deserialize)]
pub(crate) struct ModelId {
    id: String,
    #[serde(default)]
    provider: Option<String>,
}
pub(crate) async fn models_add(
    State(state): State<AppState>,
    Json(req): Json<ModelId>,
) -> impl IntoResponse {
    if !gating::allowed(gk::MODELS_ADD) { return (axum::http::StatusCode::FORBIDDEN, "gated").into_response(); }
    let req2 = super::ModelId {
        id: req.id,
        provider: req.provider,
    };
    super::models_add(State(state), Json(req2)).await.into_response()
}
pub(crate) async fn models_delete(
    State(state): State<AppState>,
    Json(req): Json<ModelId>,
) -> impl IntoResponse {
    if !gating::allowed(gk::MODELS_DELETE) { return (axum::http::StatusCode::FORBIDDEN, "gated").into_response(); }
    let req2 = super::ModelId {
        id: req.id,
        provider: req.provider,
    };
    super::models_delete(State(state), Json(req2)).await.into_response()
}
pub(crate) async fn models_default_get() -> impl IntoResponse {
    if !gating::allowed(gk::MODELS_DEFAULT_GET) { return (axum::http::StatusCode::FORBIDDEN, "gated").into_response(); }
    super::models_default_get().await.into_response()
}
pub(crate) async fn models_default_set(
    State(state): State<AppState>,
    Json(req): Json<ModelId>,
) -> impl IntoResponse {
    if !gating::allowed(gk::MODELS_DEFAULT_SET) { return (axum::http::StatusCode::FORBIDDEN, "gated").into_response(); }
    let req2 = super::ModelId {
        id: req.id,
        provider: req.provider,
    };
    super::models_default_set(State(state), Json(req2)).await.into_response()
}

#[derive(Deserialize)]
pub(crate) struct DownloadReq {
    id: String,
    url: String,
    #[serde(default)]
    provider: Option<String>,
}
pub(crate) async fn models_download(
    State(state): State<AppState>,
    Json(req): Json<DownloadReq>,
) -> impl IntoResponse {
    if !gating::allowed(gk::MODELS_DOWNLOAD) { return (axum::http::StatusCode::FORBIDDEN, "gated").into_response(); }
    let req2 = super::DownloadReq {
        id: req.id,
        url: req.url,
        provider: req.provider,
    };
    super::models_download(State(state), Json(req2)).await.into_response()
}
