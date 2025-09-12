use crate::AppState;
use axum::{extract::State, response::IntoResponse, Json};
use serde::Deserialize;
use serde_json::{json, Value};

#[derive(Deserialize)]
pub struct DryRunReq {
    #[serde(default)]
    pub patches: Vec<Value>,
}

#[derive(Deserialize)]
pub struct ApplyPatchReq {
    #[serde(default)]
    pub unit_id: Option<String>,
    #[serde(default)]
    pub patches: Vec<Value>,
    #[serde(default)]
    pub scope: Option<String>,
}

#[derive(Deserialize)]
pub struct RevertPatchReq {
    #[serde(default)]
    pub unit_id: Option<String>,
    #[serde(default)]
    pub snapshot_id: Option<String>,
}

pub async fn dry_run(Json(req): Json<DryRunReq>) -> impl IntoResponse {
    let summary = json!({
        "patch_count": req.patches.len(),
        "targets": req
            .patches
            .iter()
            .filter_map(|p| p.get("target").and_then(|s| s.as_str()))
            .collect::<Vec<_>>()
    });
    super::ok(json!({ "diff": summary, "warnings": [] as [Value;0] }))
}

pub async fn apply(State(state): State<AppState>, Json(req): Json<ApplyPatchReq>) -> impl IntoResponse {
    // Stub: emit LogicUnit.Applied with patch_count; in future, validate+apply.
    let mut payload = json!({
        "unit_id": req.unit_id,
        "scope": req.scope,
        "patch_count": req.patches.len(),
    });
    super::corr::ensure_corr(&mut payload);
    state.bus.publish("LogicUnit.Applied", &payload);
    super::ok(json!({ "applied": true }))
}

pub async fn revert(State(state): State<AppState>, Json(req): Json<RevertPatchReq>) -> impl IntoResponse {
    let mut payload = json!({ "unit_id": req.unit_id, "snapshot_id": req.snapshot_id });
    super::corr::ensure_corr(&mut payload);
    state.bus.publish("LogicUnit.Reverted", &payload);
    super::ok(json!({ "reverted": true }))
}

