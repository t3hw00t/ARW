use axum::{extract::{State, Query}, Json};
use axum::http::HeaderMap;
use axum::response::IntoResponse;
use serde::Deserialize;
use serde_json::json;

use crate::{AppState, admin_ok};

pub async fn orchestrator_mini_agents() -> impl IntoResponse {
    Json(json!({"items": []}))
}

#[derive(Deserialize)]
pub(crate) struct OrchestratorStartReq { #[serde(default)] pub id: Option<String>, pub goal: String, #[serde(default)] pub data: Option<serde_json::Value>, #[serde(default)] pub budget: Option<serde_json::Value> }

pub async fn orchestrator_start_training(State(state): State<AppState>, headers: HeaderMap, Json(req): Json<OrchestratorStartReq>) -> impl IntoResponse {
    if !admin_ok(&headers) {
        return (axum::http::StatusCode::UNAUTHORIZED, Json(json!({"type":"about:blank","title":"Unauthorized","status":401})));
    }
    let goal = req.goal.clone();
    let id = match state.kernel.insert_orchestrator_job(&goal, req.data.as_ref()) {
        Ok(id) => id,
        Err(e) => return (axum::http::StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"type":"about:blank","title":"Error","status":500, "detail": e.to_string()})))
    };
    state.bus.publish("orchestrator.job.created", &json!({"id": id, "goal": goal}));
    let state2 = state.clone();
    let id_clone = id.clone();
    let goal_clone = goal.clone();
    tokio::spawn(async move {
        let steps = 5;
        for i in 1..=steps {
            let p = (i as f64) / (steps as f64);
            let _ = state2.kernel.update_orchestrator_job(&id_clone, Some(if i<steps {"running"} else {"completed"}), Some(p));
            state2.bus.publish("orchestrator.job.progress", &json!({"id": id_clone, "progress": p}));
            if i < steps { tokio::time::sleep(std::time::Duration::from_millis(500)).await; }
        }
        state2.bus.publish("orchestrator.job.completed", &json!({"id": id_clone, "ok": true}));
        // Suggest a Logic Unit manifest as an output of the training
        let lu_id = format!("lu-{}", id_clone);
        let manifest = json!({
            "id": lu_id,
            "kind": "config-only",
            "patches": [
                {"target": "governor.hints", "op": "merge", "value": {"goal": goal_clone}}
            ]
        });
        let _ = state2.kernel.insert_logic_unit(&lu_id, &manifest, "suggested");
        state2.bus.publish("logic.unit.suggested", &json!({"id": lu_id, "job_id": id_clone}));
    });
    (axum::http::StatusCode::ACCEPTED, Json(json!({"job_id": id, "ok": true})))
}

pub async fn state_orchestrator_jobs(State(state): State<AppState>, Query(q): Query<std::collections::HashMap<String, String>>) -> impl IntoResponse {
    let limit = q.get("limit").and_then(|s| s.parse::<i64>().ok()).unwrap_or(200);
    let items = state.kernel.list_orchestrator_jobs(limit).unwrap_or_default();
    Json(json!({"items": items}))
}

