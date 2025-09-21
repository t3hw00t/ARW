use axum::response::IntoResponse;
use axum::{extract::State, Json};
use serde::Deserialize;
use serde_json::json;
use utoipa::ToSchema;

use crate::AppState;

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
    let ttl = req.ttl_secs.unwrap_or(3600);
    let until = chrono::Utc::now() + chrono::Duration::seconds(ttl as i64);
    let ttl_until = until.to_rfc3339();
    let id = uuid::Uuid::new_v4().to_string();
    if let Err(e) = state
        .kernel()
        .insert_lease_async(
            id.clone(),
            "local".to_string(),
            req.capability.clone(),
            req.scope.clone(),
            ttl_until.clone(),
            req.budget,
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
    (
        axum::http::StatusCode::CREATED,
        Json(json!({"id": id, "ttl_until": ttl_until})),
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
    match state.kernel().list_leases_async(200).await {
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
