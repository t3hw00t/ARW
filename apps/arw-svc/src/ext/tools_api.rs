use crate::AppState;
use arw_core::gating;
use arw_macros::{arw_gate, arw_admin};
use axum::{extract::State, response::IntoResponse, Json};
use serde::Deserialize;

// List tools from the single source of truth (inventory + defaults)
#[arw_admin(method="GET", path="/admin/tools", summary="List tools")]
#[arw_gate("tools:list")]
pub(crate) async fn list_tools() -> impl IntoResponse {
    let list = arw_core::introspect_tools();
    super::ok(serde_json::to_value(list).unwrap_or(serde_json::json!([]))).into_response()
}

#[derive(Deserialize, utoipa::ToSchema)]
pub(crate) struct ToolRunReq {
    id: String,
    input: serde_json::Value,
}
#[arw_admin(method="POST", path="/admin/tools/run", summary="Run a tool")]
#[arw_gate("tools:run")]
pub(crate) async fn run_tool_endpoint(
    State(state): State<AppState>,
    Json(req): Json<ToolRunReq>,
) -> impl IntoResponse {
    let ingress_key = format!("io:ingress:tools.{}", req.id);
    if !gating::allowed(&ingress_key) {
        return (axum::http::StatusCode::FORBIDDEN, "gated:ingress").into_response();
    }
    let req2 = super::ToolRunReq {
        id: req.id,
        input: req.input,
    };
    let id_for_egress = req2.id.clone();
    let resp = super::run_tool_endpoint(State(state), Json(req2))
        .await
        .into_response();
    // Egress gate (policy-level)
    let egress_key = format!("io:egress:tools.{}", id_for_egress);
    if !gating::allowed(&egress_key) {
        return (axum::http::StatusCode::FORBIDDEN, "gated:egress").into_response();
    }
    resp
}
