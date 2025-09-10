use crate::AppState;
use axum::{extract::State, response::IntoResponse, Json};
use arw_core::{gating, gating_keys as gk};
use serde::Deserialize;

pub(crate) async fn chat_get() -> impl IntoResponse {
    super::chat_get().await
}
pub(crate) async fn chat_clear() -> impl IntoResponse {
    if !gating::allowed(gk::CHAT_CLEAR) { return (axum::http::StatusCode::FORBIDDEN, "gated").into_response(); }
    super::chat_clear().await.into_response()
}

#[derive(Deserialize)]
pub(crate) struct ChatSendReq {
    message: String,
    #[serde(default)]
    model: Option<String>,
}
pub(crate) async fn chat_send(
    State(state): State<AppState>,
    Json(req): Json<ChatSendReq>,
) -> impl IntoResponse {
    if !gating::allowed(gk::CHAT_SEND) { return (axum::http::StatusCode::FORBIDDEN, "gated").into_response(); }
    let req2 = super::ChatSendReq {
        message: req.message,
        model: req.model,
    };
    super::chat_send(State(state), Json(req2)).await.into_response()
}
