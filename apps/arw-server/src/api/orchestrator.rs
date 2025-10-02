use axum::http::HeaderMap;
use axum::response::IntoResponse;
use axum::{
    extract::{Path, Query, State},
    Json,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use utoipa::ToSchema;

use crate::AppState;
use arw_topics as topics;

use arw_runtime::RuntimeRestartBudget;
use chrono::SecondsFormat as ChronoSecondsFormat;

/// List available mini-agents (placeholder).
#[utoipa::path(
    get,
    path = "/orchestrator/mini_agents",
    tag = "Orchestrator",
    responses(
        (status = 200, body = serde_json::Value),
        (status = 501, description = "Kernel disabled", body = arw_protocol::ProblemDetails)
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
        (status = 401, body = arw_protocol::ProblemDetails),
        (status = 501, description = "Kernel disabled", body = arw_protocol::ProblemDetails)
    )
)]
pub async fn orchestrator_start_training(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<OrchestratorStartReq>,
) -> axum::response::Response {
    if let Err(resp) = crate::responses::require_admin(&headers) {
        return *resp;
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
    state.bus().publish(
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
            state2.bus().publish(
                topics::TOPIC_ORCHESTRATOR_JOB_PROGRESS,
                &json!({"id": id_clone, "progress": p}),
            );
            if i < steps {
                tokio::time::sleep(std::time::Duration::from_millis(500)).await;
            }
        }
        state2.bus().publish(
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
        state2.bus().publish(
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
        (status = 501, description = "Kernel disabled", body = arw_protocol::ProblemDetails)
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

fn default_restart_true() -> bool {
    true
}

#[derive(Deserialize, ToSchema)]
pub struct RuntimeRestoreRequest {
    #[serde(default = "default_restart_true")]
    pub restart: bool,
    #[serde(default)]
    pub preset: Option<String>,
}

#[derive(Serialize, ToSchema)]
pub struct RuntimeRestoreResponse {
    pub ok: bool,
    pub runtime_id: String,
    pub pending: bool,
    pub restart_budget: RuntimeRestartBudgetView,
}

#[derive(Serialize, ToSchema)]
pub struct RuntimeRestoreDeniedResponse {
    pub ok: bool,
    pub runtime_id: String,
    pub pending: bool,
    pub reason: String,
    pub restart_budget: RuntimeRestartBudgetView,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct RuntimeRestartBudgetView {
    pub window_seconds: u64,
    pub max_restarts: u32,
    pub used: u32,
    pub remaining: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reset_at: Option<String>,
}

impl From<RuntimeRestartBudget> for RuntimeRestartBudgetView {
    fn from(budget: RuntimeRestartBudget) -> Self {
        let reset_at = budget
            .reset_at
            .map(|ts| ts.to_rfc3339_opts(ChronoSecondsFormat::Secs, true));
        Self {
            window_seconds: budget.window_seconds,
            max_restarts: budget.max_restarts,
            used: budget.used,
            remaining: budget.remaining,
            reset_at,
        }
    }
}

/// Request a managed runtime restore.
#[utoipa::path(
    post,
    path = "/orchestrator/runtimes/{id}/restore",
    tag = "Orchestrator",
    params(("id" = String, Path, description = "Runtime identifier")),
    request_body = RuntimeRestoreRequest,
    responses(
        (status = 202, description = "Restore requested", body = RuntimeRestoreResponse),
        (status = 429, description = "Restart budget exhausted", body = RuntimeRestoreDeniedResponse),
        (status = 401, description = "Unauthorized", body = arw_protocol::ProblemDetails)
    )
)]
pub async fn orchestrator_runtime_restore(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(runtime_id): Path<String>,
    Json(req): Json<RuntimeRestoreRequest>,
) -> impl IntoResponse {
    if let Err(resp) = crate::responses::require_admin(&headers) {
        return *resp;
    }

    match state
        .runtime()
        .request_restore(&runtime_id, req.restart, req.preset.clone())
        .await
    {
        Ok(budget) => (
            axum::http::StatusCode::ACCEPTED,
            Json(RuntimeRestoreResponse {
                ok: true,
                runtime_id,
                pending: true,
                restart_budget: budget.into(),
            }),
        )
            .into_response(),
        Err(denied) => (
            axum::http::StatusCode::TOO_MANY_REQUESTS,
            Json(RuntimeRestoreDeniedResponse {
                ok: false,
                runtime_id,
                pending: false,
                reason: "Restart budget exhausted".to_string(),
                restart_budget: denied.budget.into(),
            }),
        )
            .into_response(),
    }
}
