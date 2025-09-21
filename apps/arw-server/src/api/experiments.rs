use axum::http::HeaderMap;
use axum::response::IntoResponse;
use axum::{extract::State, Json};
use serde::{Deserialize, Serialize};
use serde_json::json;
use utoipa::ToSchema;

use crate::{admin_ok, experiments, AppState};

fn unauthorized() -> axum::response::Response {
    (
        axum::http::StatusCode::UNAUTHORIZED,
        Json(json!({"type":"about:blank","title":"Unauthorized","status":401})),
    )
        .into_response()
}

#[derive(Debug, Serialize, ToSchema)]
pub struct ExperimentsListResponse {
    pub items: Vec<experiments::Experiment>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct ExperimentsScoreboardResponse {
    pub items: Vec<experiments::ScoreRow>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct ExperimentsWinnersResponse {
    pub items: Vec<experiments::WinnerInfo>,
}

#[derive(Deserialize, ToSchema)]
pub struct ExperimentDefineRequest {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub variants: std::collections::HashMap<String, experiments::VariantCfg>,
}

#[utoipa::path(
    post,
    path = "/admin/experiments/define",
    tag = "Admin/Experiments",
    request_body = ExperimentDefineRequest,
    responses(
        (status = 200, description = "Experiment defined", body = serde_json::Value),
        (status = 401, description = "Unauthorized"),
    )
)]
pub async fn experiments_define(
    headers: HeaderMap,
    State(state): State<AppState>,
    Json(req): Json<ExperimentDefineRequest>,
) -> impl IntoResponse {
    if !admin_ok(&headers) {
        return unauthorized();
    }
    let exp = experiments::Experiment {
        id: req.id.clone(),
        name: req.name,
        variants: req.variants,
    };
    state.experiments().define(exp).await;
    Json(json!({"ok": true, "id": req.id})).into_response()
}

#[derive(Deserialize, ToSchema)]
pub struct ExperimentRunRequest {
    pub id: String,
    pub proj: String,
    pub variants: Vec<String>,
    #[serde(default)]
    pub budget_total_ms: Option<u64>,
}

#[utoipa::path(
    post,
    path = "/admin/experiments/run",
    tag = "Admin/Experiments",
    request_body = ExperimentRunRequest,
    responses(
        (status = 200, description = "Run outcome", body = experiments::RunOutcome),
        (status = 401, description = "Unauthorized"),
    )
)]
pub async fn experiments_run(
    headers: HeaderMap,
    State(state): State<AppState>,
    Json(req): Json<ExperimentRunRequest>,
) -> impl IntoResponse {
    if !admin_ok(&headers) {
        return unauthorized();
    }
    let outcome = state
        .experiments()
        .run_on_goldens(experiments::RunPlan {
            proj: req.proj,
            exp_id: req.id,
            variants: req.variants,
            budget_total_ms: req.budget_total_ms,
        })
        .await;
    Json(outcome).into_response()
}

#[derive(Deserialize, ToSchema)]
pub struct ExperimentActivateRequest {
    pub id: String,
    pub variant: String,
}

#[utoipa::path(
    post,
    path = "/admin/experiments/activate",
    tag = "Admin/Experiments",
    request_body = ExperimentActivateRequest,
    responses(
        (status = 200, description = "Variant activated", body = serde_json::Value),
        (status = 401, description = "Unauthorized"),
        (status = 404, description = "Unknown experiment"),
    )
)]
pub async fn experiments_activate(
    headers: HeaderMap,
    State(state): State<AppState>,
    Json(req): Json<ExperimentActivateRequest>,
) -> impl IntoResponse {
    if !admin_ok(&headers) {
        return unauthorized();
    }
    match state.experiments().activate(&req.id, &req.variant).await {
        Ok(()) => Json(json!({"ok": true})).into_response(),
        Err(e) => (
            axum::http::StatusCode::NOT_FOUND,
            Json(json!({"type":"about:blank","title":"Not Found","status":404,"detail":e})),
        )
            .into_response(),
    }
}

#[utoipa::path(
    get,
    path = "/admin/experiments/list",
    tag = "Admin/Experiments",
    responses(
        (status = 200, description = "Experiments", body = ExperimentsListResponse),
        (status = 401, description = "Unauthorized"),
    )
)]
pub async fn experiments_list(
    headers: HeaderMap,
    State(state): State<AppState>,
) -> impl IntoResponse {
    if !admin_ok(&headers) {
        return unauthorized();
    }
    let items = state.experiments().list().await;
    Json(ExperimentsListResponse { items }).into_response()
}

#[utoipa::path(
    get,
    path = "/admin/experiments/scoreboard",
    tag = "Admin/Experiments",
    responses(
        (status = 200, description = "Scoreboard", body = ExperimentsScoreboardResponse),
        (status = 401, description = "Unauthorized"),
    )
)]
pub async fn experiments_scoreboard(
    headers: HeaderMap,
    State(state): State<AppState>,
) -> impl IntoResponse {
    if !admin_ok(&headers) {
        return unauthorized();
    }
    let items = state.experiments().list_scoreboard().await;
    Json(ExperimentsScoreboardResponse { items }).into_response()
}

#[utoipa::path(
    get,
    path = "/admin/experiments/winners",
    tag = "Admin/Experiments",
    responses(
        (status = 200, description = "Winners", body = ExperimentsWinnersResponse),
        (status = 401, description = "Unauthorized"),
    )
)]
pub async fn experiments_winners(
    headers: HeaderMap,
    State(state): State<AppState>,
) -> impl IntoResponse {
    if !admin_ok(&headers) {
        return unauthorized();
    }
    let items = state.experiments().list_winners().await;
    Json(ExperimentsWinnersResponse { items }).into_response()
}

#[derive(Deserialize, ToSchema)]
pub struct ExperimentStartRequest {
    pub name: String,
    #[serde(default)]
    pub variants: Vec<String>,
    #[serde(default)]
    pub assignment: Option<serde_json::Value>,
    #[serde(default)]
    pub budgets: Option<serde_json::Value>,
}

#[utoipa::path(
    post,
    path = "/admin/experiments/start",
    tag = "Admin/Experiments",
    request_body = ExperimentStartRequest,
    responses(
        (status = 200, description = "Experiment started", body = serde_json::Value),
        (status = 401, description = "Unauthorized"),
    )
)]
pub async fn experiments_start(
    headers: HeaderMap,
    State(state): State<AppState>,
    Json(req): Json<ExperimentStartRequest>,
) -> impl IntoResponse {
    if !admin_ok(&headers) {
        return unauthorized();
    }
    let id = state
        .experiments()
        .publish_start(req.name, req.variants, req.assignment, req.budgets)
        .await;
    Json(json!({"id": id})).into_response()
}

#[derive(Deserialize, ToSchema)]
pub struct ExperimentStopRequest {
    pub id: String,
}

#[utoipa::path(
    post,
    path = "/admin/experiments/stop",
    tag = "Admin/Experiments",
    request_body = ExperimentStopRequest,
    responses(
        (status = 200, description = "Experiment stopped", body = serde_json::Value),
        (status = 401, description = "Unauthorized"),
    )
)]
pub async fn experiments_stop(
    headers: HeaderMap,
    State(state): State<AppState>,
    Json(req): Json<ExperimentStopRequest>,
) -> impl IntoResponse {
    if !admin_ok(&headers) {
        return unauthorized();
    }
    state.experiments().publish_stop(req.id).await;
    Json(json!({"ok": true})).into_response()
}

#[derive(Deserialize, ToSchema)]
pub struct ExperimentAssignRequest {
    pub id: String,
    pub variant: String,
    #[serde(default)]
    pub agent: Option<String>,
}

#[utoipa::path(
    post,
    path = "/admin/experiments/assign",
    tag = "Admin/Experiments",
    request_body = ExperimentAssignRequest,
    responses(
        (status = 200, description = "Assignment event emitted", body = serde_json::Value),
        (status = 401, description = "Unauthorized"),
    )
)]
pub async fn experiments_assign(
    headers: HeaderMap,
    State(state): State<AppState>,
    Json(req): Json<ExperimentAssignRequest>,
) -> impl IntoResponse {
    if !admin_ok(&headers) {
        return unauthorized();
    }
    state
        .experiments()
        .publish_assign(req.id, req.variant, req.agent)
        .await;
    Json(json!({"ok": true})).into_response()
}
