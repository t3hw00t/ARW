use crate::AppState;
use arw_macros::{arw_gate, arw_admin};
use axum::{extract::State, response::IntoResponse, Json};
use serde::Deserialize;

#[arw_admin(method="GET", path="/admin/models", summary="List models")]
#[arw_gate("models:list")]
pub(crate) async fn list_models() -> impl IntoResponse {
    super::list_models().await.into_response()
}
#[arw_admin(method="POST", path="/admin/models/refresh", summary="Refresh model list")]
#[arw_gate("models:refresh")]
pub(crate) async fn refresh_models(State(state): State<AppState>) -> impl IntoResponse {
    super::refresh_models(State(state)).await.into_response()
}
#[arw_admin(method="POST", path="/admin/models/save", summary="Save models to disk")]
#[arw_gate("models:save")]
pub(crate) async fn models_save() -> impl IntoResponse {
    super::models_save().await.into_response()
}
#[arw_admin(method="POST", path="/admin/models/load", summary="Load models from disk")]
#[arw_gate("models:load")]
pub(crate) async fn models_load() -> impl IntoResponse {
    super::models_load().await.into_response()
}

#[derive(Deserialize, utoipa::ToSchema)]
pub(crate) struct ModelId {
    id: String,
    #[serde(default)]
    provider: Option<String>,
}
#[arw_admin(method="POST", path="/admin/models/add", summary="Add model entry")]
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
#[arw_admin(method="POST", path="/admin/models/delete", summary="Delete model entry")]
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
#[arw_admin(method="GET", path="/admin/models/default", summary="Get default model")]
#[arw_gate("models:default:get")]
pub(crate) async fn models_default_get() -> impl IntoResponse {
    super::models_default_get().await.into_response()
}
#[arw_admin(method="POST", path="/admin/models/default", summary="Set default model")]
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

#[derive(Deserialize, utoipa::ToSchema)]
pub(crate) struct DownloadReq {
    id: String,
    url: String,
    #[serde(default)]
    provider: Option<String>,
    #[serde(default)]
    sha256: Option<String>,
}
#[arw_admin(method="POST", path="/admin/models/download", summary="Download model file")]
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

#[derive(Deserialize, utoipa::ToSchema)]
pub(crate) struct CancelReq {
    id: String,
}

#[arw_admin(method="POST", path="/admin/models/download/cancel", summary="Cancel model download")]
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
