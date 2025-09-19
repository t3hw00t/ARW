use crate::{admin_ok, goldens, responses, AppState};
use arw_topics as topics;
use axum::http::HeaderMap;
use axum::response::IntoResponse;
use axum::{extract::State, Json};
use serde::Deserialize;
use serde_json::json;
use utoipa::ToSchema;

#[derive(Deserialize, ToSchema)]
pub struct GoldensListQuery {
    #[serde(default)]
    pub proj: Option<String>,
}

#[utoipa::path(
    get,
    path = "/admin/goldens/list",
    tag = "Admin/Experiments",
    params(("proj" = Option<String>, Query, description = "Project name")),
    responses(
        (status = 200, description = "Project goldens", body = serde_json::Value),
        (status = 401, description = "Unauthorized"),
    )
)]
pub async fn goldens_list(
    headers: HeaderMap,
    State(_state): State<AppState>,
    axum::extract::Query(q): axum::extract::Query<GoldensListQuery>,
) -> impl IntoResponse {
    if !admin_ok(&headers) {
        return unauthorized();
    }
    let proj = q.proj.unwrap_or_else(|| "default".into());
    let set = goldens::load(&proj).await;
    Json(json!({"project": proj, "items": set.items})).into_response()
}

#[derive(Deserialize, ToSchema)]
pub struct GoldensAddRequest {
    pub proj: String,
    #[serde(default)]
    pub id: Option<String>,
    pub kind: String,
    #[serde(default)]
    pub input: serde_json::Value,
    #[serde(default)]
    pub expect: serde_json::Value,
}

#[utoipa::path(
    post,
    path = "/admin/goldens/add",
    tag = "Admin/Experiments",
    request_body = GoldensAddRequest,
    responses(
        (status = 200, description = "Golden added", body = serde_json::Value),
        (status = 400, description = "Persist failed"),
        (status = 401, description = "Unauthorized"),
    )
)]
pub async fn goldens_add(
    headers: HeaderMap,
    Json(req): Json<GoldensAddRequest>,
) -> impl IntoResponse {
    if !admin_ok(&headers) {
        return unauthorized();
    }
    let mut set = goldens::load(&req.proj).await;
    let id = req
        .id
        .unwrap_or_else(|| format!("{}-{}", req.kind, set.items.len().saturating_add(1)));
    let item = goldens::GoldenItem {
        id: id.clone(),
        kind: req.kind,
        input: req.input,
        expect: req.expect,
    };
    set.items.push(item);
    match goldens::save(&req.proj, &set).await {
        Ok(()) => Json(json!({"ok": true, "id": id})).into_response(),
        Err(e) => (
            axum::http::StatusCode::BAD_REQUEST,
            Json(json!({"type":"about:blank","title":"Persist failed","status":400,"detail":e})),
        )
            .into_response(),
    }
}

#[derive(Deserialize, ToSchema)]
pub struct GoldensRunRequest {
    pub proj: String,
    #[serde(default)]
    pub limit: Option<usize>,
    #[serde(default)]
    pub temperature: Option<f64>,
    #[serde(default)]
    pub vote_k: Option<usize>,
    #[serde(default)]
    pub retrieval_k: Option<usize>,
    #[serde(default)]
    pub mmr_lambda: Option<f64>,
    #[serde(default)]
    pub compression_aggr: Option<f64>,
}

#[utoipa::path(
    post,
    path = "/admin/goldens/run",
    tag = "Admin/Experiments",
    request_body = GoldensRunRequest,
    responses(
        (status = 200, description = "Evaluation summary", body = serde_json::Value),
        (status = 401, description = "Unauthorized"),
    )
)]
pub async fn goldens_run(
    headers: HeaderMap,
    State(state): State<AppState>,
    Json(req): Json<GoldensRunRequest>,
) -> impl IntoResponse {
    if !admin_ok(&headers) {
        return unauthorized();
    }
    let opts = goldens::EvalOptions {
        limit: req.limit,
        temperature: req.temperature,
        vote_k: req.vote_k,
        retrieval_k: req.retrieval_k,
        mmr_lambda: req.mmr_lambda,
        compression_aggr: req.compression_aggr,
        ..goldens::EvalOptions::default()
    };
    let set = goldens::load(&req.proj).await;
    let summary = goldens::evaluate_chat_items(&set, &opts, Some(req.proj.as_str())).await;
    let mut payload = json!({
        "proj": req.proj,
        "total": summary.total,
        "passed": summary.passed,
        "failed": summary.failed,
        "avg_latency_ms": summary.avg_latency_ms,
        "avg_ctx_tokens": summary.avg_ctx_tokens,
        "avg_ctx_items": summary.avg_ctx_items,
    });
    responses::attach_corr(&mut payload);
    state
        .bus()
        .publish(topics::TOPIC_GOLDENS_EVALUATED, &payload);
    Json(summary).into_response()
}

fn unauthorized() -> axum::response::Response {
    (
        axum::http::StatusCode::UNAUTHORIZED,
        Json(json!({"type":"about:blank","title":"Unauthorized","status":401})),
    )
        .into_response()
}
