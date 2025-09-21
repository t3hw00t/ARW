use axum::response::IntoResponse;
use axum::{extract::State, Json};
use serde::Deserialize;
use serde_json::json;
use utoipa::ToSchema;

use crate::{read_models, AppState};
use arw_topics as topics;
use chrono::{Duration as ChronoDuration, SecondsFormat, Utc};

#[derive(Deserialize, ToSchema)]
pub(crate) struct LeaseReq {
    pub capability: String,
    #[serde(default)]
    pub scope: Option<String>,
    #[serde(default)]
    pub ttl_secs: Option<u64>,
    #[serde(default)]
    pub budget: Option<f64>,
}

/// Allocate a capability lease.
#[utoipa::path(
    post,
    path = "/leases",
    tag = "Leases",
    request_body = LeaseReq,
    responses(
        (status = 201, description = "Created", body = serde_json::Value),
        (status = 501, description = "Kernel disabled", body = serde_json::Value)
    )
)]
pub async fn leases_create(
    State(state): State<AppState>,
    Json(req): Json<LeaseReq>,
) -> impl IntoResponse {
    if !state.kernel_enabled() {
        return crate::responses::kernel_disabled();
    }
    let ttl_raw = req.ttl_secs.unwrap_or(3600);
    let ttl = ttl_raw.clamp(1, 86_400);
    let issued_at = Utc::now();
    let ttl_until_dt = issued_at + ChronoDuration::seconds(ttl as i64);
    let ttl_until = ttl_until_dt.to_rfc3339_opts(SecondsFormat::Millis, true);
    let created_at = issued_at.to_rfc3339_opts(SecondsFormat::Millis, true);
    let id = uuid::Uuid::new_v4().to_string();
    let subject = "local".to_string();
    let capability = req.capability.trim().to_string();
    if capability.is_empty() {
        return (
            axum::http::StatusCode::BAD_REQUEST,
            Json(json!({
                "type": "about:blank",
                "title": "Bad Request",
                "status": 400,
                "detail": "capability must not be empty"
            })),
        )
            .into_response();
    }
    let scope = req.scope.clone();
    let budget = req.budget;
    if let Err(e) = state
        .kernel()
        .insert_lease_async(
            id.clone(),
            subject.clone(),
            capability.clone(),
            scope.clone(),
            ttl_until.clone(),
            budget,
            None,
        )
        .await
    {
        return (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            Json(
                json!({"type":"about:blank","title":"Error","status":500, "detail": e.to_string()}),
            ),
        )
            .into_response();
    }
    let mut payload = serde_json::Map::new();
    payload.insert("id".into(), json!(id));
    payload.insert("subject".into(), json!(subject));
    payload.insert("capability".into(), json!(capability));
    payload.insert("ttl_until".into(), json!(ttl_until));
    payload.insert("created".into(), json!(created_at));
    if let Some(scope) = scope.clone() {
        payload.insert("scope".into(), json!(scope));
    }
    if let Some(budget) = budget {
        payload.insert("budget".into(), json!(budget));
    }
    state
        .bus()
        .publish(topics::TOPIC_LEASES_CREATED, &json!(payload));

    let snapshot = read_models::leases_snapshot(&state).await;
    read_models::publish_read_model_patch(&state.bus(), "policy_leases", &snapshot);

    let mut response = serde_json::Map::new();
    response.insert("id".into(), payload["id"].clone());
    response.insert("ttl_until".into(), payload["ttl_until"].clone());
    response.insert("created".into(), payload["created"].clone());
    if let Some(scope_val) = payload.get("scope").cloned() {
        response.insert("scope".into(), scope_val);
    }
    if let Some(budget_val) = payload.get("budget").cloned() {
        response.insert("budget".into(), budget_val);
    }

    (
        axum::http::StatusCode::CREATED,
        Json(serde_json::Value::Object(response)),
    )
        .into_response()
}

/// Snapshot of active leases.
#[utoipa::path(
    get,
    path = "/state/leases",
    tag = "Leases",
    responses(
        (status = 200, body = serde_json::Value),
        (status = 501, description = "Kernel disabled", body = serde_json::Value)
    )
)]
pub async fn state_leases(State(state): State<AppState>) -> impl IntoResponse {
    if !state.kernel_enabled() {
        return crate::responses::kernel_disabled();
    }
    let snapshot = read_models::leases_snapshot(&state).await;
    Json(snapshot).into_response()
}
