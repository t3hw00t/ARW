use crate::AppState;
use arw_macros::{arw_admin, arw_gate};
use axum::{extract::State, response::IntoResponse, Json};
use serde::Deserialize;
use serde_json::{json, Value};

#[derive(Deserialize)]
pub struct InstallReq {
    #[serde(default)]
    pub manifest: Value,
}

#[derive(Deserialize)]
pub struct ApplyReq {
    #[serde(default)]
    pub unit_id: Option<String>,
    #[serde(default)]
    pub patches: Vec<Value>,
    #[serde(default)]
    pub scope: Option<String>,
    #[serde(default)]
    pub params: Option<Value>,
    #[serde(default)]
    pub dry_run: Option<bool>,
}

#[derive(Deserialize)]
pub struct RevertReq {
    #[serde(default)]
    pub unit_id: Option<String>,
    #[serde(default)]
    pub snapshot_id: Option<String>,
    #[serde(default)]
    pub scope: Option<String>,
}

#[arw_admin(
    method = "POST",
    path = "/admin/logic-units/install",
    summary = "Install logic unit"
)]
#[arw_gate("logic_units:install")]
pub async fn install(
    State(state): State<AppState>,
    Json(req): Json<InstallReq>,
) -> impl IntoResponse {
    let id = req
        .manifest
        .get("id")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let mut payload = json!({ "id": id, "manifest": req.manifest });
    super::corr::ensure_corr(&mut payload);
    state
        .bus
        .publish(crate::ext::topics::TOPIC_LOGICUNIT_INSTALLED, &payload);
    super::ok(json!({ "installed": true, "id": id })).into_response()
}

#[arw_admin(
    method = "POST",
    path = "/admin/logic-units/apply",
    summary = "Apply logic unit"
)]
#[arw_gate("logic_units:apply")]
pub async fn apply(State(state): State<AppState>, Json(req): Json<ApplyReq>) -> impl IntoResponse {
    if req.dry_run.unwrap_or(false) {
        let diff = json!({
            "patch_count": req.patches.len(),
            "scope": req.scope,
            "unit_id": req.unit_id,
        });
        return super::ok(json!({ "dry_run": true, "diff": diff })).into_response();
    }
    let mut payload = json!({
        "unit_id": req.unit_id,
        "scope": req.scope,
        "params": req.params,
        "patch_count": req.patches.len(),
    });
    super::corr::ensure_corr(&mut payload);
    state
        .bus
        .publish(crate::ext::topics::TOPIC_LOGICUNIT_APPLIED, &payload);
    super::ok(json!({ "applied": true })).into_response()
}

#[arw_admin(
    method = "POST",
    path = "/admin/logic-units/revert",
    summary = "Revert logic unit"
)]
#[arw_gate("logic_units:revert")]
pub async fn revert(
    State(state): State<AppState>,
    Json(req): Json<RevertReq>,
) -> impl IntoResponse {
    let mut payload = json!({
        "unit_id": req.unit_id,
        "snapshot_id": req.snapshot_id,
        "scope": req.scope,
    });
    super::corr::ensure_corr(&mut payload);
    state
        .bus
        .publish(crate::ext::topics::TOPIC_LOGICUNIT_REVERTED, &payload);
    super::ok(json!({ "reverted": true })).into_response()
}
