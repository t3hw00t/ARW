use crate::AppState;
use axum::{extract::State, response::IntoResponse, Json};
use arw_core::{gating, gating_keys as gk};
use serde::Deserialize;

// List tools from the single source of truth (inventory + defaults)
pub(crate) async fn list_tools() -> impl IntoResponse {
    if !gating::allowed(gk::TOOLS_LIST) { return (axum::http::StatusCode::FORBIDDEN, "gated").into_response(); }
    let list = arw_core::introspect_tools();
    Json(serde_json::to_value(list).unwrap_or(serde_json::json!([]))).into_response()
}

#[derive(Deserialize)]
pub(crate) struct ToolRunReq {
    id: String,
    input: serde_json::Value,
}
pub(crate) async fn run_tool_endpoint(
    State(state): State<AppState>,
    Json(req): Json<ToolRunReq>,
) -> impl IntoResponse {
    if !gating::allowed(gk::TOOLS_RUN) { return (axum::http::StatusCode::FORBIDDEN, "gated").into_response(); }
    let ingress_key = format!("io:ingress:tools.{}", req.id);
    if !gating::allowed(&ingress_key) { return (axum::http::StatusCode::FORBIDDEN, "gated:ingress").into_response(); }
    let req2 = super::ToolRunReq {
        id: req.id,
        input: req.input,
    };
    let id_for_egress = req2.id.clone();
    let resp = super::run_tool_endpoint(State(state), Json(req2)).await.into_response();
    // Egress gate (policy-level)
    let egress_key = format!("io:egress:tools.{}", id_for_egress);
    if !gating::allowed(&egress_key) { return (axum::http::StatusCode::FORBIDDEN, "gated:egress").into_response(); }
    resp
}
