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
    };
    super::governor_hints_set(State(state), Json(req2))
        .await
        .into_response()
}
