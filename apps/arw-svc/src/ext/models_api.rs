use axum::{extract::State, response::IntoResponse, Json};
use serde::Deserialize;
use crate::AppState;

pub(crate) async fn list_models() -> impl IntoResponse { super::list_models().await }
pub(crate) async fn refresh_models(State(state): State<AppState>) -> impl IntoResponse { super::refresh_models(State(state)).await }
pub(crate) async fn models_save() -> impl IntoResponse { super::models_save().await }
pub(crate) async fn models_load() -> impl IntoResponse { super::models_load().await }

#[derive(Deserialize)]
pub(crate) struct ModelId { id: String, #[serde(default)] provider: Option<String> }
pub(crate) async fn models_add(State(state): State<AppState>, Json(req): Json<ModelId>) -> impl IntoResponse {
    let req2 = super::ModelId { id: req.id, provider: req.provider };
    super::models_add(State(state), Json(req2)).await
}
pub(crate) async fn models_delete(State(state): State<AppState>, Json(req): Json<ModelId>) -> impl IntoResponse {
    let req2 = super::ModelId { id: req.id, provider: req.provider };
    super::models_delete(State(state), Json(req2)).await
}
pub(crate) async fn models_default_get() -> impl IntoResponse { super::models_default_get().await }
pub(crate) async fn models_default_set(State(state): State<AppState>, Json(req): Json<ModelId>) -> impl IntoResponse {
    let req2 = super::ModelId { id: req.id, provider: req.provider };
    super::models_default_set(State(state), Json(req2)).await
}

#[derive(Deserialize)]
pub(crate) struct DownloadReq { id: String, url: String, #[serde(default)] provider: Option<String> }
pub(crate) async fn models_download(State(state): State<AppState>, Json(req): Json<DownloadReq>) -> impl IntoResponse {
    let req2 = super::DownloadReq { id: req.id, url: req.url, provider: req.provider };
    super::models_download(State(state), Json(req2)).await
}
