use axum::{extract::State, response::IntoResponse, Json};
use serde::Deserialize;
use crate::AppState;

pub(crate) async fn chat_get() -> impl IntoResponse { super::chat_get().await }
pub(crate) async fn chat_clear() -> impl IntoResponse { super::chat_clear().await }

#[derive(Deserialize)]
struct ChatSendReq { message: String, #[serde(default)] model: Option<String> }
pub(crate) async fn chat_send(State(state): State<AppState>, Json(req): Json<ChatSendReq>) -> impl IntoResponse {
    let req2 = super::ChatSendReq { message: req.message, model: req.model };
    super::chat_send(State(state), Json(req2)).await
}

