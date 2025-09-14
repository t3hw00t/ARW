use crate::AppState;
use arw_macros::{arw_admin, arw_gate};
use axum::{extract::State, response::IntoResponse, Json};
use serde::Deserialize;
use serde_json::{json, Value};

#[derive(Deserialize)]
pub struct StartReq {
    pub name: String,
    #[serde(default)]
    pub variants: Vec<String>,
    #[serde(default)]
    pub assignment: Option<Value>,
    #[serde(default)]
    pub budgets: Option<Value>,
}

#[derive(Deserialize)]
pub struct StopReq {
    pub id: String,
}

#[derive(Deserialize)]
pub struct AssignReq {
    pub id: String,
    pub variant: String,
    #[serde(default)]
    pub agent: Option<String>,
}

#[arw_admin(
    method = "POST",
    path = "/admin/experiments/start",
    summary = "Start an experiment"
)]
#[arw_gate("experiments:start")]
pub async fn start(State(state): State<AppState>, Json(req): Json<StartReq>) -> impl IntoResponse {
    let id = uuid::Uuid::new_v4().to_string();
    let mut payload = json!({
        "id": id,
        "name": req.name,
        "variants": req.variants,
        "assignment": req.assignment,
        "budgets": req.budgets,
    });
    super::corr::ensure_corr(&mut payload);
    state
        .bus
        .publish(crate::ext::topics::TOPIC_EXPERIMENT_STARTED, &payload);
    super::ok(json!({ "id": id })).into_response()
}

#[arw_admin(
    method = "POST",
    path = "/admin/experiments/stop",
    summary = "Stop an experiment"
)]
#[arw_gate("experiments:stop")]
pub async fn stop(State(state): State<AppState>, Json(req): Json<StopReq>) -> impl IntoResponse {
    let mut payload = json!({ "id": req.id });
    super::corr::ensure_corr(&mut payload);
    state
        .bus
        .publish(crate::ext::topics::TOPIC_EXPERIMENT_COMPLETED, &payload);
    super::ok(json!({ "stopped": true })).into_response()
}

#[arw_admin(
    method = "POST",
    path = "/admin/experiments/assign",
    summary = "Assign experiment variant"
)]
#[arw_gate("experiments:assign")]
pub async fn assign(
    State(state): State<AppState>,
    Json(req): Json<AssignReq>,
) -> impl IntoResponse {
    let mut payload = json!({ "id": req.id, "variant": req.variant, "agent": req.agent });
    super::corr::ensure_corr(&mut payload);
    state.bus.publish(
        crate::ext::topics::TOPIC_EXPERIMENT_VARIANT_CHOSEN,
        &payload,
    );
    super::ok(json!({ "assigned": true })).into_response()
}
