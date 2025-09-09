use axum::{extract::State, response::IntoResponse, Json};
use serde::Deserialize;
use crate::AppState;

pub(crate) async fn list_tools() -> impl IntoResponse { super::list_tools().await }

#[derive(Deserialize)]
pub(crate) struct ToolRunReq { id: String, input: serde_json::Value }
pub(crate) async fn run_tool_endpoint(State(state): State<AppState>, Json(req): Json<ToolRunReq>) -> impl IntoResponse {
    let req2 = super::ToolRunReq { id: req.id, input: req.input };
    super::run_tool_endpoint(State(state), Json(req2)).await
}
