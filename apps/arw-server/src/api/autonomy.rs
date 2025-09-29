use std::str::FromStr;

use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::IntoResponse;
use axum::Json;
use serde::{Deserialize, Serialize};
use tracing::warn;
use utoipa::{IntoParams, ToSchema};

use crate::autonomy::{AutonomyLaneSnapshot, AutonomyMode, FlushScope};
use crate::{responses, AppState};

#[derive(Serialize, ToSchema)]
pub struct AutonomyLanesResponse {
    pub lanes: Vec<AutonomyLaneSnapshot>,
}

#[derive(Serialize, ToSchema)]
pub struct AutonomyLaneResponse {
    pub lane: AutonomyLaneSnapshot,
}

#[derive(Default, Deserialize, ToSchema)]
pub struct AutonomyActionRequest {
    #[serde(default)]
    pub operator: Option<String>,
    #[serde(default)]
    pub reason: Option<String>,
    #[serde(default)]
    pub mode: Option<String>,
}

#[derive(Default, Deserialize, ToSchema, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct AutonomyJobsQuery {
    #[serde(default)]
    pub state: Option<String>,
}

#[utoipa::path(
    get,
    path = "/state/autonomy/lanes",
    tag = "State/Autonomy",
    responses((status = 200, description = "Autonomy lanes", body = AutonomyLanesResponse))
)]
pub async fn state_autonomy_lanes(
    headers: HeaderMap,
    State(state): State<AppState>,
) -> impl IntoResponse {
    if let Err(resp) = responses::require_admin(&headers) {
        return resp;
    }
    let counts = live_action_counts(&state).await;
    let mut lanes = state.autonomy().lanes().await;
    if let Some((running, queued)) = counts {
        let mut refreshed = Vec::with_capacity(lanes.len());
        for lane in lanes.into_iter() {
            refreshed.push(
                state
                    .autonomy()
                    .record_job_counts(&lane.lane_id, Some(running), Some(queued))
                    .await,
            );
        }
        lanes = refreshed;
    }
    Json(AutonomyLanesResponse { lanes }).into_response()
}

#[utoipa::path(
    get,
    path = "/state/autonomy/lanes/{lane}",
    tag = "State/Autonomy",
    responses(
        (status = 200, description = "Autonomy lane", body = AutonomyLaneResponse),
        (status = 404, description = "Lane not found", body = serde_json::Value)
    )
)]
pub async fn state_autonomy_lane(
    headers: HeaderMap,
    Path(lane_id): Path<String>,
    State(state): State<AppState>,
) -> impl IntoResponse {
    if let Err(resp) = responses::require_admin(&headers) {
        return resp;
    }
    match state.autonomy().lane(&lane_id).await {
        Some(existing) => {
            let lane = if let Some((running, queued)) = live_action_counts(&state).await {
                state
                    .autonomy()
                    .record_job_counts(&lane_id, Some(running), Some(queued))
                    .await
            } else {
                existing
            };
            Json(AutonomyLaneResponse { lane }).into_response()
        }
        None => responses::problem_response(
            StatusCode::NOT_FOUND,
            "Lane not found",
            Some(&format!("lane '{}' not present", lane_id)),
        ),
    }
}
#[utoipa::path(
    post,
    path = "/admin/autonomy/{lane}/pause",
    tag = "Admin/Autonomy",
    request_body = AutonomyActionRequest,
    responses((status = 200, description = "Lane paused", body = AutonomyLaneResponse))
)]
pub async fn autonomy_pause(
    headers: HeaderMap,
    Path(lane_id): Path<String>,
    State(state): State<AppState>,
    payload: Option<Json<AutonomyActionRequest>>,
) -> impl IntoResponse {
    if let Err(resp) = responses::require_admin(&headers) {
        return resp;
    }
    let body = payload.map(|wrapper| wrapper.0).unwrap_or_default();
    let snapshot = state
        .autonomy()
        .pause_lane(&lane_id, body.operator, body.reason)
        .await;
    let lane = if let Some((running, queued)) = live_action_counts(&state).await {
        state
            .autonomy()
            .record_job_counts(&lane_id, Some(running), Some(queued))
            .await
    } else {
        snapshot
    };
    Json(AutonomyLaneResponse { lane }).into_response()
}
#[utoipa::path(
    post,
    path = "/admin/autonomy/{lane}/resume",
    tag = "Admin/Autonomy",
    request_body = AutonomyActionRequest,
    responses((status = 200, description = "Lane resumed", body = AutonomyLaneResponse))
)]
pub async fn autonomy_resume(
    headers: HeaderMap,
    Path(lane_id): Path<String>,
    State(state): State<AppState>,
    payload: Option<Json<AutonomyActionRequest>>,
) -> impl IntoResponse {
    if let Err(resp) = responses::require_admin(&headers) {
        return resp;
    }
    let body = payload.map(|wrapper| wrapper.0).unwrap_or_default();
    let target_mode = match body.mode.as_deref() {
        Some(raw) => match AutonomyMode::from_str(raw) {
            Ok(mode) => mode,
            Err(_) => {
                return responses::problem_response(
                    StatusCode::BAD_REQUEST,
                    "Invalid mode",
                    Some("mode must be one of guided, autonomous, paused"),
                );
            }
        },
        None => AutonomyMode::Guided,
    };

    if matches!(target_mode, AutonomyMode::Paused) {
        return responses::problem_response(
            StatusCode::BAD_REQUEST,
            "Invalid mode",
            Some("pause via POST /admin/autonomy/{lane}/pause"),
        );
    }

    let mut operator = body.operator;
    let mut reason = body.reason;
    let registry = state.autonomy();
    let snapshot = if matches!(target_mode, AutonomyMode::Guided) {
        registry
            .resume_lane(&lane_id, operator.take(), reason.take())
            .await
    } else {
        registry
            .set_lane_mode(&lane_id, target_mode, operator.take(), reason.take())
            .await
    };
    let lane = if let Some((running, queued)) = live_action_counts(&state).await {
        state
            .autonomy()
            .record_job_counts(&lane_id, Some(running), Some(queued))
            .await
    } else {
        snapshot
    };
    Json(AutonomyLaneResponse { lane }).into_response()
}
#[utoipa::path(
    delete,
    path = "/admin/autonomy/{lane}/jobs",
    tag = "Admin/Autonomy",
    params(AutonomyJobsQuery),
    responses((status = 200, description = "Jobs flushed", body = AutonomyLaneResponse))
)]
pub async fn autonomy_jobs_clear(
    headers: HeaderMap,
    Path(lane_id): Path<String>,
    Query(query): Query<AutonomyJobsQuery>,
    State(state): State<AppState>,
) -> impl IntoResponse {
    if let Err(resp) = responses::require_admin(&headers) {
        return resp;
    }
    let scope = match query.state.as_deref().map(|s| s.trim()) {
        None | Some("") | Some("all") => FlushScope::All,
        Some("queued") | Some("queued_only") => FlushScope::QueuedOnly,
        Some("in_flight") | Some("inflight") => FlushScope::InFlightOnly,
        Some(other) => {
            return responses::problem_response(
                StatusCode::BAD_REQUEST,
                "Invalid state parameter",
                Some(&format!(
                    "unsupported value '{}'; expected all, queued, in_flight",
                    other
                )),
            );
        }
    };
    let counts_after_clear = if state.kernel_enabled() {
        for state_name in states_for_scope(scope) {
            if let Err(err) = state
                .kernel()
                .delete_actions_by_state_async(state_name)
                .await
            {
                warn!(?err, lane = %lane_id, state = %state_name, "failed to clear autonomy jobs");
                return responses::problem_response(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "Failed to flush jobs",
                    Some("Unable to clear actions from queue"),
                );
            }
        }
        live_action_counts(&state).await
    } else {
        None
    };
    let snapshot = state.autonomy().flush_jobs(&lane_id, scope).await;
    let lane = if let Some((running, queued)) = counts_after_clear {
        state
            .autonomy()
            .record_job_counts(&lane_id, Some(running), Some(queued))
            .await
    } else {
        snapshot
    };
    Json(AutonomyLaneResponse { lane }).into_response()
}

fn states_for_scope(scope: FlushScope) -> &'static [&'static str] {
    match scope {
        FlushScope::All => &["queued", "running"],
        FlushScope::QueuedOnly => &["queued"],
        FlushScope::InFlightOnly => &["running"],
    }
}

async fn live_action_counts(state: &AppState) -> Option<(u64, u64)> {
    if !state.kernel_enabled() {
        return None;
    }
    let running = match state.kernel().count_actions_by_state_async("running").await {
        Ok(value) => value,
        Err(err) => {
            warn!(?err, "failed to count running actions");
            return None;
        }
    };
    let queued = match state.kernel().count_actions_by_state_async("queued").await {
        Ok(value) => value,
        Err(err) => {
            warn!(?err, "failed to count queued actions");
            return None;
        }
    };
    let running = if running >= 0 { running as u64 } else { 0 };
    let queued = if queued >= 0 { queued as u64 } else { 0 };
    Some((running, queued))
}
