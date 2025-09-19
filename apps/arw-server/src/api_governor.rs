use axum::http::HeaderMap;
use axum::response::IntoResponse;
use axum::{extract::State, Json};
use serde::{Deserialize, Serialize};
use serde_json::json;
use utoipa::ToSchema;

use crate::{admin_ok, governor, AppState};

fn unauthorized() -> axum::response::Response {
    (
        axum::http::StatusCode::UNAUTHORIZED,
        Json(json!({"type":"about:blank","title":"Unauthorized","status":401})),
    )
        .into_response()
}

#[derive(Serialize, ToSchema)]
pub struct GovernorProfileResponse {
    pub profile: String,
}

#[derive(Deserialize, ToSchema)]
pub struct GovernorProfileRequest {
    pub name: String,
}

#[utoipa::path(
    get,
    path = "/admin/governor/profile",
    tag = "Admin/Introspect",
    responses(
        (status = 200, description = "Governor profile", body = GovernorProfileResponse),
        (status = 401, description = "Unauthorized")
    )
)]
pub async fn governor_profile_get(
    headers: HeaderMap,
    State(state): State<AppState>,
) -> impl IntoResponse {
    if !admin_ok(&headers) {
        return unauthorized();
    }
    let profile = state.governor().profile().await;
    Json(GovernorProfileResponse { profile }).into_response()
}

#[utoipa::path(
    post,
    path = "/admin/governor/profile",
    tag = "Admin/Introspect",
    request_body = GovernorProfileRequest,
    responses(
        (status = 200, description = "Profile updated", body = serde_json::Value),
        (status = 401, description = "Unauthorized")
    )
)]
pub async fn governor_profile_set(
    headers: HeaderMap,
    State(state): State<AppState>,
    Json(req): Json<GovernorProfileRequest>,
) -> impl IntoResponse {
    if !admin_ok(&headers) {
        return unauthorized();
    }
    state.governor().set_profile(&state.bus(), req.name).await;
    Json(json!({"ok": true})).into_response()
}

#[derive(Deserialize, ToSchema, Default)]
pub struct GovernorHintsRequest {
    #[serde(default)]
    pub max_concurrency: Option<usize>,
    #[serde(default)]
    pub event_buffer: Option<usize>,
    #[serde(default)]
    pub http_timeout_secs: Option<u64>,
    #[serde(default)]
    pub mode: Option<String>,
    #[serde(default)]
    pub slo_ms: Option<u64>,
    #[serde(default)]
    pub retrieval_k: Option<usize>,
    #[serde(default)]
    pub retrieval_div: Option<f64>,
    #[serde(default)]
    pub mmr_lambda: Option<f64>,
    #[serde(default)]
    pub compression_aggr: Option<f64>,
    #[serde(default)]
    pub vote_k: Option<u8>,
    #[serde(default)]
    pub context_budget_tokens: Option<usize>,
    #[serde(default)]
    pub context_item_budget_tokens: Option<usize>,
    #[serde(default)]
    pub context_format: Option<String>,
    #[serde(default)]
    pub include_provenance: Option<bool>,
    #[serde(default)]
    pub context_item_template: Option<String>,
    #[serde(default)]
    pub context_header: Option<String>,
    #[serde(default)]
    pub context_footer: Option<String>,
    #[serde(default)]
    pub joiner: Option<String>,
}

#[utoipa::path(
    get,
    path = "/admin/governor/hints",
    tag = "Admin/Introspect",
    responses(
        (status = 200, description = "Governor hints", body = governor::Hints),
        (status = 401, description = "Unauthorized")
    )
)]
pub async fn governor_hints_get(
    headers: HeaderMap,
    State(state): State<AppState>,
) -> impl IntoResponse {
    if !admin_ok(&headers) {
        return unauthorized();
    }
    Json(state.governor().hints().await).into_response()
}

#[utoipa::path(
    post,
    path = "/admin/governor/hints",
    tag = "Admin/Introspect",
    request_body = GovernorHintsRequest,
    responses(
        (status = 200, description = "Hints updated", body = serde_json::Value),
        (status = 401, description = "Unauthorized")
    )
)]
pub async fn governor_hints_set(
    headers: HeaderMap,
    State(state): State<AppState>,
    Json(req): Json<GovernorHintsRequest>,
) -> impl IntoResponse {
    if !admin_ok(&headers) {
        return unauthorized();
    }
    let bus = state.bus();
    state
        .governor()
        .apply_hints(
            &bus,
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
            req.context_format,
            req.include_provenance,
            req.context_item_template,
            req.context_header,
            req.context_footer,
            req.joiner,
        )
        .await;
    Json(json!({"ok": true})).into_response()
}
