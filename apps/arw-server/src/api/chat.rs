use axum::http::HeaderMap;
use axum::response::IntoResponse;
use axum::{extract::State, Json};
use serde::{Deserialize, Serialize};
use serde_json::json;
use utoipa::ToSchema;

use crate::{admin_ok, chat, AppState};

#[utoipa::path(
    get,
    path = "/admin/chat",
    tag = "Admin/Chat",
    responses(
        (status = 200, description = "Current chat history", body = ChatHistory),
        (status = 401, description = "Unauthorized")
    )
)]
pub async fn chat_history(headers: HeaderMap, State(state): State<AppState>) -> impl IntoResponse {
    if !admin_ok(&headers) {
        return (
            axum::http::StatusCode::UNAUTHORIZED,
            Json(json!({"type":"about:blank","title":"Unauthorized","status":401})),
        )
            .into_response();
    }
    let history = state.chat().history().await;
    Json(ChatHistory { items: history }).into_response()
}

#[derive(Serialize, ToSchema)]
pub struct ChatHistory {
    pub items: Vec<chat::ChatMessage>,
}

#[derive(Deserialize, ToSchema)]
pub struct ChatSendReq {
    pub prompt: String,
    #[serde(default)]
    pub temperature: Option<f64>,
    #[serde(default)]
    pub vote_k: Option<u64>,
}

#[utoipa::path(
    post,
    path = "/admin/chat/send",
    tag = "Admin/Chat",
    request_body = ChatSendReq,
    responses(
        (status = 200, description = "Synthetic reply", body = ChatSendResp),
        (status = 401, description = "Unauthorized")
    )
)]
pub async fn chat_send(
    headers: HeaderMap,
    State(state): State<AppState>,
    Json(req): Json<ChatSendReq>,
) -> impl IntoResponse {
    if !admin_ok(&headers) {
        return (
            axum::http::StatusCode::UNAUTHORIZED,
            Json(json!({"type":"about:blank","title":"Unauthorized","status":401})),
        )
            .into_response();
    }
    let options = chat::ChatSendOptions {
        temperature: req.temperature,
        vote_k: req.vote_k.map(|v| v as usize),
    };
    let outcome = state.chat().send(&state, &req.prompt, options).await;
    Json(ChatSendResp {
        ok: true,
        backend: outcome.backend,
        reply: outcome.reply,
        history: outcome.history,
    })
    .into_response()
}

#[derive(Serialize, ToSchema)]
pub struct ChatSendResp {
    pub ok: bool,
    pub backend: String,
    pub reply: chat::ChatMessage,
    pub history: Vec<chat::ChatMessage>,
}

#[utoipa::path(
    post,
    path = "/admin/chat/clear",
    tag = "Admin/Chat",
    responses(
        (status = 200, description = "Cleared", body = serde_json::Value),
        (status = 401, description = "Unauthorized")
    )
)]
pub async fn chat_clear(headers: HeaderMap, State(state): State<AppState>) -> impl IntoResponse {
    if !admin_ok(&headers) {
        return (
            axum::http::StatusCode::UNAUTHORIZED,
            Json(json!({"type":"about:blank","title":"Unauthorized","status":401})),
        )
            .into_response();
    }
    state.chat().clear().await;
    Json(json!({"ok": true})).into_response()
}

#[utoipa::path(
    get,
    path = "/admin/chat/status",
    tag = "Admin/Chat",
    params(("probe" = Option<bool>, Query, description = "Trigger latency probe")),
    responses(
        (status = 200, description = "Backend status", body = ChatStatusResp),
        (status = 401, description = "Unauthorized")
    )
)]
pub async fn chat_status(
    headers: HeaderMap,
    State(state): State<AppState>,
    axum::extract::Query(params): axum::extract::Query<std::collections::HashMap<String, String>>,
) -> impl IntoResponse {
    if !admin_ok(&headers) {
        return (
            axum::http::StatusCode::UNAUTHORIZED,
            Json(json!({"type":"about:blank","title":"Unauthorized","status":401})),
        )
            .into_response();
    }
    let probe = params
        .get("probe")
        .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
        .unwrap_or(false);
    let status = state.chat().status(probe).await;
    Json(ChatStatusResp {
        ok: status.ok,
        backend: status.backend,
        messages: status.messages,
        latency_ms: status.latency_ms,
    })
    .into_response()
}

#[derive(Serialize, ToSchema)]
pub struct ChatStatusResp {
    pub ok: bool,
    pub backend: String,
    pub messages: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub latency_ms: Option<u64>,
}
