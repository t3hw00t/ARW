use crate::AppState;
use arw_macros::{arw_admin, arw_gate};
use axum::{extract::State, response::IntoResponse, Json};
use serde::Deserialize;

#[arw_admin(method = "GET", path = "/admin/chat", summary = "Get chat history")]
#[arw_gate("io:egress:chat")]
pub(crate) async fn chat_get() -> impl IntoResponse {
    super::chat::chat_get().await.into_response()
}
#[arw_admin(
    method = "POST",
    path = "/admin/chat/clear",
    summary = "Clear chat history"
)]
#[arw_gate("chat:clear")]
pub(crate) async fn chat_clear() -> impl IntoResponse {
    super::chat::chat_clear().await.into_response()
}

#[derive(Deserialize, utoipa::ToSchema)]
pub(crate) struct ChatSendReq {
    message: String,
    #[serde(default)]
    model: Option<String>,
    #[serde(default)]
    temperature: Option<f64>,
}
#[arw_admin(
    method = "POST",
    path = "/admin/chat/send",
    summary = "Send chat message"
)]
#[arw_gate("chat:send")]
pub(crate) async fn chat_send(
    State(state): State<AppState>,
    Json(req): Json<ChatSendReq>,
) -> impl IntoResponse {
    let req2 = super::chat::ChatSendReq {
        message: req.message,
        model: req.model,
        temperature: req.temperature,
    };
    super::chat::chat_send(State(state), Json(req2))
        .await
        .into_response()
}
