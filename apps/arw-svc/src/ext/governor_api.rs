use crate::resources::governor_service::GovernorService;
use crate::AppState;
use arw_macros::{arw_admin, arw_gate};
use axum::{extract::State, response::IntoResponse, Json};
use serde::Deserialize;

#[arw_admin(
    method = "GET",
    path = "/admin/governor/profile",
    summary = "Get governor profile"
)]
pub(crate) async fn governor_get(State(state): State<AppState>) -> impl IntoResponse {
    if let Some(svc) = state.resources.get::<GovernorService>() {
        let p = svc.profile_get().await;
        return super::ok(serde_json::json!({"profile": p})).into_response();
    }
    super::governor_get().await.into_response()
}
#[derive(Deserialize, utoipa::ToSchema)]
pub(crate) struct SetProfile {
    name: String,
}
#[arw_gate("governor:set")]
#[arw_admin(
    method = "POST",
    path = "/admin/governor/profile",
    summary = "Set governor profile"
)]
pub(crate) async fn governor_set(
    State(state): State<AppState>,
    Json(req): Json<SetProfile>,
) -> impl IntoResponse {
    if let Some(svc) = state.resources.get::<GovernorService>() {
        svc.profile_set(&state, req.name).await;
        return super::ok(serde_json::json!({})).into_response();
    }
    let req2 = super::SetProfile { name: req.name };
    super::governor_set(State(state), Json(req2))
        .await
        .into_response()
}

#[arw_admin(
    method = "GET",
    path = "/admin/governor/hints",
    summary = "Get governor hints"
)]
pub(crate) async fn governor_hints_get(State(state): State<AppState>) -> impl IntoResponse {
    if let Some(svc) = state.resources.get::<GovernorService>() {
        let v = svc.hints_get().await;
        return super::ok(v).into_response();
    }
    super::governor_hints_get().await.into_response()
}
#[derive(Deserialize, utoipa::ToSchema)]
pub(crate) struct Hints {
    #[serde(default)]
    max_concurrency: Option<usize>,
    #[serde(default)]
    event_buffer: Option<usize>,
    #[serde(default)]
    http_timeout_secs: Option<u64>,
    #[serde(default)]
    mode: Option<String>,
    #[serde(default)]
    slo_ms: Option<u64>,
    // Extended knobs
    #[serde(default)]
    retrieval_k: Option<usize>,
    #[serde(default)]
    retrieval_div: Option<f64>,
    #[serde(default)]
    mmr_lambda: Option<f64>,
    #[serde(default)]
    compression_aggr: Option<f64>,
    #[serde(default)]
    vote_k: Option<u8>,
    #[serde(default)]
    context_budget_tokens: Option<usize>,
    #[serde(default)]
    context_item_budget_tokens: Option<usize>,
    #[serde(default)]
    context_format: Option<String>,
    #[serde(default)]
    include_provenance: Option<bool>,
    #[serde(default)]
    context_item_template: Option<String>,
    #[serde(default)]
    context_header: Option<String>,
    #[serde(default)]
    context_footer: Option<String>,
    #[serde(default)]
    joiner: Option<String>,
}
#[arw_gate("governor:hints:set")]
#[arw_admin(
    method = "POST",
    path = "/admin/governor/hints",
    summary = "Set governor hints"
)]
pub(crate) async fn governor_hints_set(
    State(state): State<AppState>,
    Json(req): Json<Hints>,
) -> impl IntoResponse {
    if let Some(svc) = state.resources.get::<GovernorService>() {
        svc.hints_set_values(
            &state,
            req.max_concurrency,
            req.event_buffer,
            req.http_timeout_secs,
            req.mode,
            req.slo_ms,
            req.retrieval_k,
            req.retrieval_div,
            req.mmr_lambda,
            req.compression_aggr,
            req.vote_k,
            req.context_budget_tokens,
            req.context_item_budget_tokens,
            req.context_format.clone(),
            req.include_provenance,
            req.context_item_template.clone(),
            req.context_header.clone(),
            req.context_footer.clone(),
            req.joiner.clone(),
        )
        .await;
        return super::ok(serde_json::json!({})).into_response();
    }
    let req2 = super::Hints {
        max_concurrency: req.max_concurrency,
        event_buffer: req.event_buffer,
        http_timeout_secs: req.http_timeout_secs,
        mode: req.mode,
        slo_ms: req.slo_ms,
        retrieval_k: req.retrieval_k,
        retrieval_div: req.retrieval_div,
        mmr_lambda: req.mmr_lambda,
        compression_aggr: req.compression_aggr,
        vote_k: req.vote_k,
        context_budget_tokens: req.context_budget_tokens,
        context_item_budget_tokens: req.context_item_budget_tokens,
        context_format: req.context_format,
        include_provenance: req.include_provenance,
        context_item_template: req.context_item_template,
        context_header: req.context_header,
        context_footer: req.context_footer,
        joiner: req.joiner,
    };
    super::governor_hints_set(State(state), Json(req2))
        .await
        .into_response()
}
