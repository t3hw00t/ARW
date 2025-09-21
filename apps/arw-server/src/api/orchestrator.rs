use axum::http::HeaderMap;
use axum::response::IntoResponse;
use axum::{
    extract::{Query, State},
    Json,
};
use serde::Deserialize;
use serde_json::json;
use utoipa::ToSchema;

use crate::{admin_ok, AppState};
use arw_topics as topics;

/// List available mini-agents (placeholder).
#[utoipa::path(
    get,
    path = "/orchestrator/mini_agents",
    tag = "Orchestrator",
    responses(
        (status = 200, body = serde_json::Value),
        (status = 501, description = "Kernel disabled", body = serde_json::Value)
    )
)]
pub async fn orchestrator_mini_agents() -> impl IntoResponse {
    Json(json!({"items": []}))
}

#[derive(Deserialize, ToSchema)]
pub(crate) struct OrchestratorStartReq {
    #[serde(default)]
    #[allow(dead_code)]
    pub id: Option<String>,
    pub goal: String,
    #[serde(default)]
    pub data: Option<serde_json::Value>,
    #[serde(default)]
    #[allow(dead_code)]
    pub budget: Option<serde_json::Value>,
}

/// Start a training job that results in a suggested Logic Unit (admin).
#[utoipa::path(
    post,
    path = "/orchestrator/mini_agents/start_training",
    tag = "Orchestrator",
    request_body = OrchestratorStartReq,
    responses(
        (status = 202, body = serde_json::Value),
        (status = 401),
        (status = 501, description = "Kernel disabled", body = serde_json::Value)
    )
)]
pub async fn orchestrator_start_training(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<OrchestratorStartReq>,
) -> axum::response::Response {
    if !admin_ok(&headers) {
        return (
            axum::http::StatusCode::UNAUTHORIZED,
            Json(json!({"type":"about:blank","title":"Unauthorized","status":401})),
        )
            .into_response();
    }
    if !state.kernel_enabled() {
        return crate::responses::kernel_disabled();
    }
    let goal = req.goal.clone();
    let id = match state
        .kernel()
        .insert_orchestrator_job_async(req.goal.as_str(), req.data.as_ref())
        .await
    {
        Ok(id) => id,
        Err(e) => return (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            Json(
                json!({"type":"about:blank","title":"Error","status":500, "detail": e.to_string()}),
            ),
        )
            .into_response(),
    };
    state.bus.publish(
        topics::TOPIC_ORCHESTRATOR_JOB_CREATED,
        &json!({"id": id, "goal": goal}),
    );
    let state2 = state.clone();
    let id_clone = id.clone();
    let goal_clone = goal.clone();
    tokio::spawn(async move {
        let steps = 5;
        for i in 1..=steps {
            let p = (i as f64) / (steps as f64);
            let _ = state2
                .kernel()
                .update_orchestrator_job_async(
                    id_clone.clone(),
                    Some(if i < steps { "running" } else { "completed" }.to_string()),
                    Some(p),
                )
                .await;
            state2.bus.publish(
                topics::TOPIC_ORCHESTRATOR_JOB_PROGRESS,
                &json!({"id": id_clone, "progress": p}),
            );
            if i < steps {
                tokio::time::sleep(std::time::Duration::from_millis(500)).await;
            }
        }
        state2.bus.publish(
            topics::TOPIC_ORCHESTRATOR_JOB_COMPLETED,
            &json!({"id": id_clone, "ok": true}),
        );
        // Suggest a Logic Unit manifest as an output of the training
        let lu_id = format!("lu-{}", id_clone);
        let manifest = json!({
            "id": lu_id,
            "kind": "config-only",
            "patches": [
                {"target": "governor.hints", "op": "merge", "value": {"goal": goal_clone}}
            ]
        });
        let _ = state2
            .kernel()
            .insert_logic_unit_async(lu_id.clone(), manifest.clone(), "suggested".to_string())
            .await;
        state2.bus.publish(
            topics::TOPIC_LOGICUNIT_SUGGESTED,
            &json!({"id": lu_id, "job_id": id_clone}),
        );
    });
    (
        axum::http::StatusCode::ACCEPTED,
        Json(json!({"job_id": id, "ok": true})),
    )
        .into_response()
}

/// Orchestrator jobs snapshot.
#[utoipa::path(
    get,
    path = "/state/orchestrator/jobs",
    tag = "Orchestrator",
    params(("limit" = Option<i64>, Query)),
    responses(
        (status = 200, body = serde_json::Value),
        (status = 501, description = "Kernel disabled", body = serde_json::Value)
    )
)]
pub async fn state_orchestrator_jobs(
    State(state): State<AppState>,
    Query(q): Query<std::collections::HashMap<String, String>>,
) -> impl IntoResponse {
    if !state.kernel_enabled() {
        return crate::responses::kernel_disabled();
    }
    let limit = q
        .get("limit")
        .and_then(|s| s.parse::<i64>().ok())
        .unwrap_or(200);
    match state.kernel().list_orchestrator_jobs_async(limit).await {
        Ok(items) => Json(json!({"items": items})).into_response(),
        Err(e) => (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            Json(
                json!({"type":"about:blank","title":"Error","status":500, "detail": e.to_string()}),
            ),
        )
            .into_response(),
    }
}
