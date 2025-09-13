use crate::AppState;
use arw_macros::{arw_admin, arw_gate};
use axum::{extract::Query, extract::State, response::IntoResponse, Json};
use serde::Deserialize;
use serde_json::{json, Value};

#[derive(Deserialize)]
pub struct ListQs {
    pub proj: String,
}

#[arw_admin(
    method = "GET",
    path = "/admin/goldens/list",
    summary = "List goldens for a project"
)]
#[arw_gate("goldens:list")]
pub async fn goldens_list(Query(q): Query<ListQs>) -> impl IntoResponse {
    let set = super::goldens::load(&q.proj).await;
    super::ok(serde_json::to_value(set).unwrap_or(json!({}))).into_response()
}

#[derive(Deserialize)]
pub struct AddReq {
    pub proj: String,
    #[serde(default)]
    pub id: Option<String>,
    pub kind: String,
    #[serde(default)]
    pub input: Value,
    #[serde(default)]
    pub expect: Value,
}

#[arw_admin(
    method = "POST",
    path = "/admin/goldens/add",
    summary = "Add a golden item"
)]
#[arw_gate("goldens:add")]
pub async fn goldens_add(Json(req): Json<AddReq>) -> impl IntoResponse {
    let mut set = super::goldens::load(&req.proj).await;
    let id = req
        .id
        .unwrap_or_else(|| format!("{}-{}", req.kind, uuid::Uuid::new_v4()));
    let it = super::goldens::GoldenItem {
        id: id.clone(),
        kind: req.kind,
        input: req.input,
        expect: req.expect,
    };
    set.items.push(it);
    match super::goldens::save(&req.proj, &set).await {
        Ok(()) => super::ok(json!({"added": 1, "id": id})).into_response(),
        Err(e) => super::ApiError::internal(&e).into_response(),
    }
}

#[derive(Deserialize)]
pub struct RunReq {
    pub proj: String,
    #[serde(default)]
    pub limit: Option<usize>,
    #[serde(default)]
    pub temperature: Option<f64>,
    #[serde(default)]
    pub vote_k: Option<usize>,
}

#[arw_admin(
    method = "POST",
    path = "/admin/goldens/run",
    summary = "Evaluate goldens for a project"
)]
#[arw_gate("goldens:run")]
pub async fn goldens_run(
    State(state): State<AppState>,
    Json(req): Json<RunReq>,
) -> impl IntoResponse {
    let set = super::goldens::load(&req.proj).await;
    let opts = super::goldens::EvalOptions {
        limit: req.limit,
        temperature: req.temperature,
        vote_k: req.vote_k,
        retrieval_k: None,
        mmr_lambda: None,
        compression_aggr: None,
        context_budget_tokens: None,
        context_item_budget_tokens: None,
        context_format: None,
        include_provenance: None,
        context_item_template: None,
        context_header: None,
        context_footer: None,
        joiner: None,
    };
    let summary = super::goldens::evaluate_chat_items(&set, &opts, Some(req.proj.as_str())).await;
    // Emit event for observability
    let mut payload = json!({
        "proj": req.proj,
        "total": summary.total,
        "passed": summary.passed,
        "failed": summary.failed,
        "avg_latency_ms": summary.avg_latency_ms,
    });
    super::corr::ensure_corr(&mut payload);
    state.bus.publish("Goldens.Evaluated", &payload);
    super::ok(serde_json::to_value(summary).unwrap()).into_response()
}
