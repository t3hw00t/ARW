use crate::AppState;
use arw_macros::arw_gate;
use axum::{extract::State, response::IntoResponse, Json};
use serde::Deserialize;

#[arw_gate("models:list")]
pub(crate) async fn list_models() -> impl IntoResponse {
    super::list_models().await.into_response()
}
#[arw_gate("models:refresh")]
pub(crate) async fn refresh_models(State(state): State<AppState>) -> impl IntoResponse {
    super::refresh_models(State(state)).await.into_response()
}
#[arw_gate("models:save")]
pub(crate) async fn models_save() -> impl IntoResponse {
    super::models_save().await.into_response()
}
#[arw_gate("models:load")]
pub(crate) async fn models_load() -> impl IntoResponse {
    super::models_load().await.into_response()
}

#[derive(Deserialize)]
pub(crate) struct ModelId {
    id: String,
    #[serde(default)]
    provider: Option<String>,
}
#[arw_gate("models:add")]
pub(crate) async fn models_add(
    State(state): State<AppState>,
    Json(req): Json<ModelId>,
) -> impl IntoResponse {
    let req2 = super::ModelId {
        id: req.id,
        provider: req.provider,
    };
    super::models_add(State(state), Json(req2))
        .await
        .into_response()
}
#[arw_gate("models:delete")]
pub(crate) async fn models_delete(
    State(state): State<AppState>,
    Json(req): Json<ModelId>,
) -> impl IntoResponse {
    let req2 = super::ModelId {
        id: req.id,
        provider: req.provider,
    };
    super::models_delete(State(state), Json(req2))
        .await
        .into_response()
}
#[arw_gate("models:default:get")]
pub(crate) async fn models_default_get() -> impl IntoResponse {
    super::models_default_get().await.into_response()
}
#[arw_gate("models:default:set")]
pub(crate) async fn models_default_set(
    State(state): State<AppState>,
    Json(req): Json<ModelId>,
) -> impl IntoResponse {
    let req2 = super::ModelId {
        id: req.id,
        provider: req.provider,
    };
    super::models_default_set(State(state), Json(req2))
        .await
        .into_response()
}

#[derive(Deserialize)]
pub(crate) struct DownloadReq {
    id: String,
    url: String,
    #[serde(default)]
    provider: Option<String>,
    #[serde(default)]
    sha256: Option<String>,
}
#[arw_gate("models:download")]
pub(crate) async fn models_download(
    State(state): State<AppState>,
    Json(req): Json<DownloadReq>,
) -> impl IntoResponse {
    let req2 = super::DownloadReq {
        id: req.id,
        url: req.url,
        provider: req.provider,
        sha256: req.sha256,
    };
    super::models_download(State(state), Json(req2))
        .await
        .into_response()
}

#[derive(Deserialize)]
pub(crate) struct CancelReq {
    id: String,
}

#[arw_gate("models:download")]
pub(crate) async fn models_download_cancel(
    State(state): State<AppState>,
    Json(req): Json<CancelReq>,
) -> impl IntoResponse {
    let req2 = super::CancelReq { id: req.id };
    super::models_download_cancel(State(state), Json(req2))
        .await
        .into_response()
}
