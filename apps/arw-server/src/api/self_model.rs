use axum::http::HeaderMap;
use axum::response::{IntoResponse, Response};
use axum::{extract::State, Json};
use serde::Deserialize;
use serde_json::{json, Value};
use utoipa::ToSchema;

use crate::{responses, AppState};
use arw_topics as topics;

fn unauthorized() -> Response {
    (
        axum::http::StatusCode::UNAUTHORIZED,
        Json(json!({"type":"about:blank","title":"Unauthorized","status":401})),
    )
        .into_response()
}

fn error_response(status: axum::http::StatusCode, detail: &str) -> Response {
    (
        status,
        Json(json!({
            "type": "about:blank",
            "title": detail,
            "status": status.as_u16()
        })),
    )
        .into_response()
}

#[derive(Deserialize, ToSchema)]
pub struct SelfModelProposeRequest {
    pub agent: String,
    #[serde(default)]
    pub patch: Value,
    #[serde(default)]
    pub rationale: Option<String>,
}

#[utoipa::path(
    post,
    path = "/admin/self_model/propose",
    tag = "Admin/SelfModel",
    request_body = SelfModelProposeRequest,
    responses(
        (status = 200, description = "Proposal created", body = serde_json::Value),
        (status = 400, description = "Invalid request", body = serde_json::Value),
        (status = 401, description = "Unauthorized", body = serde_json::Value)
    )
)]
pub async fn self_model_propose(
    headers: HeaderMap,
    State(state): State<AppState>,
    Json(req): Json<SelfModelProposeRequest>,
) -> impl IntoResponse {
    if !crate::admin_ok(&headers).await {
        return unauthorized();
    }
    match crate::self_model::propose_update(&req.agent, req.patch, req.rationale.clone()).await {
        Ok(env) => {
            let mut payload = json!({
                "agent": env.get("agent").and_then(|v| v.as_str()).unwrap_or_default(),
                "proposal_id": env.get("id").and_then(|v| v.as_str()).unwrap_or_default(),
                "touches_policies": false,
                "widens_scope": false,
                "rationale": req.rationale,
            });
            responses::attach_corr(&mut payload);
            state
                .bus()
                .publish(topics::TOPIC_SELFMODEL_PROPOSED, &payload);
            Json(env).into_response()
        }
        Err(crate::self_model::SelfModelError::InvalidAgent) => {
            error_response(axum::http::StatusCode::BAD_REQUEST, "invalid_agent")
        }
        Err(crate::self_model::SelfModelError::Serde(_)) => {
            error_response(axum::http::StatusCode::BAD_REQUEST, "invalid_patch")
        }
        Err(e) => {
            tracing::warn!("self_model_propose failed: {}", e);
            error_response(axum::http::StatusCode::INTERNAL_SERVER_ERROR, "error")
        }
    }
}

#[derive(Deserialize, ToSchema)]
pub struct SelfModelApplyRequest {
    pub proposal_id: String,
}

#[utoipa::path(
    post,
    path = "/admin/self_model/apply",
    tag = "Admin/SelfModel",
    request_body = SelfModelApplyRequest,
    responses(
        (status = 200, description = "Proposal applied", body = serde_json::Value),
        (status = 400, description = "Invalid request", body = serde_json::Value),
        (status = 401, description = "Unauthorized", body = serde_json::Value),
        (status = 404, description = "Proposal not found", body = serde_json::Value)
    )
)]
pub async fn self_model_apply(
    headers: HeaderMap,
    State(state): State<AppState>,
    Json(req): Json<SelfModelApplyRequest>,
) -> impl IntoResponse {
    if !crate::admin_ok(&headers).await {
        return unauthorized();
    }
    match crate::self_model::apply_proposal(&req.proposal_id).await {
        Ok(resp) => {
            let mut payload = json!({
                "proposal_id": req.proposal_id,
                "agent": resp.get("agent").and_then(|v| v.as_str()).unwrap_or_default(),
            });
            responses::attach_corr(&mut payload);
            state
                .bus()
                .publish(topics::TOPIC_SELFMODEL_UPDATED, &payload);
            Json(resp).into_response()
        }
        Err(crate::self_model::SelfModelError::MissingProposal) => {
            error_response(axum::http::StatusCode::NOT_FOUND, "proposal_not_found")
        }
        Err(crate::self_model::SelfModelError::InvalidProposal) => {
            error_response(axum::http::StatusCode::BAD_REQUEST, "invalid_proposal")
        }
        Err(crate::self_model::SelfModelError::InvalidAgent) => {
            error_response(axum::http::StatusCode::BAD_REQUEST, "invalid_agent")
        }
        Err(e) => {
            tracing::warn!("self_model_apply failed: {}", e);
            error_response(axum::http::StatusCode::INTERNAL_SERVER_ERROR, "error")
        }
    }
}
