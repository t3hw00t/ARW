use crate::AppState;
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
    state.bus.publish("Experiment.Started", &payload);
    super::ok(json!({ "id": id }))
}

pub async fn stop(State(state): State<AppState>, Json(req): Json<StopReq>) -> impl IntoResponse {
    let mut payload = json!({ "id": req.id });
    super::corr::ensure_corr(&mut payload);
    state.bus.publish("Experiment.Completed", &payload);
    super::ok(json!({ "stopped": true }))
}

pub async fn assign(State(state): State<AppState>, Json(req): Json<AssignReq>) -> impl IntoResponse {
    let mut payload = json!({ "id": req.id, "variant": req.variant, "agent": req.agent });
    super::corr::ensure_corr(&mut payload);
    state.bus.publish("Experiment.VariantChosen", &payload);
    super::ok(json!({ "assigned": true }))
}

