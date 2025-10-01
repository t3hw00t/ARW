use std::str::FromStr;

use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::IntoResponse;
use axum::Json;
use chrono::{SecondsFormat, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tracing::warn;
use utoipa::{IntoParams, ToSchema};

use crate::autonomy::{AutonomyBudgets, AutonomyLaneSnapshot, AutonomyMode, FlushScope};
use crate::{responses, AppState};
use arw_topics as topics;

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

#[derive(Deserialize, ToSchema)]
pub struct AutonomyBudgetsRequest {
    pub wall_clock_secs: Option<u64>,
    pub tokens: Option<u64>,
    pub spend_cents: Option<u64>,
    #[serde(default)]
    pub clear: bool,
    #[serde(default)]
    pub dry_run: bool,
}

#[derive(Serialize, ToSchema)]
pub struct AutonomyBudgetsResponse {
    pub ok: bool,
    pub lane: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub snapshot: Option<AutonomyLaneSnapshot>,
    pub dry_run: bool,
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
        return *resp;
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
        return *resp;
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
        return *resp;
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

/// Update or clear autonomy lane budgets.
#[utoipa::path(
    post,
    path = "/admin/autonomy/{lane}/budgets",
    tag = "Admin/Autonomy",
    request_body = AutonomyBudgetsRequest,
    responses(
        (status = 200, description = "Budgets updated", body = AutonomyBudgetsResponse),
        (status = 400, description = "Invalid request", body = serde_json::Value),
        (status = 401, description = "Unauthorized", body = serde_json::Value)
    )
)]
pub async fn autonomy_budgets_update(
    headers: HeaderMap,
    Path(lane_id): Path<String>,
    State(state): State<AppState>,
    Json(req): Json<AutonomyBudgetsRequest>,
) -> impl IntoResponse {
    if let Err(resp) = responses::require_admin(&headers) {
        return *resp;
    }

    if !req.clear
        && req.wall_clock_secs.is_none()
        && req.tokens.is_none()
        && req.spend_cents.is_none()
    {
        return responses::problem_response(
            StatusCode::BAD_REQUEST,
            "Bad Request",
            Some("provide budget fields or set clear=true"),
        );
    }

    let budgets_opt = if req.clear {
        None
    } else {
        Some(AutonomyBudgets {
            wall_clock_remaining_secs: req.wall_clock_secs,
            tokens_remaining: req.tokens,
            spend_remaining_cents: req.spend_cents,
        })
    };

    if req.dry_run {
        let preview = predicted_snapshot(&state, &lane_id, budgets_opt.clone()).await;
        return Json(AutonomyBudgetsResponse {
            ok: true,
            lane: lane_id,
            snapshot: Some(preview),
            dry_run: true,
        })
        .into_response();
    }

    let snapshot = state
        .autonomy()
        .update_budgets(&lane_id, budgets_opt.clone())
        .await;
    persist_lane_budget(&state, &lane_id, &budgets_opt).await;
    state.bus().publish(
        topics::TOPIC_AUTONOMY_BUDGET_UPDATED,
        &json!({
            "lane": lane_id,
            "budgets": budgets_value(&budgets_opt),
            "cleared": budgets_opt.is_none(),
        }),
    );

    Json(AutonomyBudgetsResponse {
        ok: true,
        lane: snapshot.lane_id.clone(),
        snapshot: Some(snapshot),
        dry_run: false,
    })
    .into_response()
}

fn budgets_value(budgets: &Option<AutonomyBudgets>) -> Value {
    match budgets {
        Some(b) => json!({
            "wall_clock_secs": b.wall_clock_remaining_secs,
            "tokens": b.tokens_remaining,
            "spend_cents": b.spend_remaining_cents,
        }),
        None => Value::Null,
    }
}

async fn predicted_snapshot(
    state: &AppState,
    lane: &str,
    budgets: Option<AutonomyBudgets>,
) -> AutonomyLaneSnapshot {
    let mut snapshot = state
        .autonomy()
        .lane(lane)
        .await
        .unwrap_or_else(|| AutonomyLaneSnapshot::new(lane));
    snapshot.budgets = budgets.clone();
    let millis = Utc::now().timestamp_millis();
    snapshot.last_budget_update_ms = Some(if millis < 0 { 0 } else { millis as u64 });
    snapshot
}

async fn persist_lane_budget(state: &AppState, lane: &str, budgets: &Option<AutonomyBudgets>) {
    let config = state.config_state();
    let mut guard = config.lock().await;
    if !guard.is_object() {
        *guard = json!({});
    }
    let cfg_obj = guard.as_object_mut().expect("config object");
    let autonomy_entry = cfg_obj
        .entry("autonomy".to_string())
        .or_insert_with(|| Value::Object(serde_json::Map::new()));
    if !autonomy_entry.is_object() {
        *autonomy_entry = Value::Object(serde_json::Map::new());
    }
    let autonomy_obj = autonomy_entry.as_object_mut().expect("autonomy object");
    let lanes_entry = autonomy_obj
        .entry("lanes".to_string())
        .or_insert_with(|| Value::Object(serde_json::Map::new()));
    if !lanes_entry.is_object() {
        *lanes_entry = Value::Object(serde_json::Map::new());
    }
    let lanes_obj = lanes_entry.as_object_mut().expect("lanes object");
    if let Some(b) = budgets {
        let lane_entry = lanes_obj
            .entry(lane.to_string())
            .or_insert_with(|| Value::Object(serde_json::Map::new()));
        let lane_obj = lane_entry.as_object_mut().expect("lane object");
        lane_obj.insert(
            "budgets".into(),
            json!({
                "wall_clock_secs": b.wall_clock_remaining_secs,
                "tokens": b.tokens_remaining,
                "spend_cents": b.spend_remaining_cents,
            }),
        );
        lane_obj.insert(
            "updated_at".into(),
            Value::String(Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true)),
        );
    } else if let Some(existing) = lanes_obj.get_mut(lane) {
        let remove_lane = if let Some(obj) = existing.as_object_mut() {
            obj.remove("budgets");
            obj.remove("updated_at");
            obj.is_empty()
        } else {
            true
        };
        if remove_lane {
            lanes_obj.remove(lane);
        }
    }
    if lanes_obj.is_empty() {
        autonomy_obj.remove("lanes");
    }
    if autonomy_obj.is_empty() {
        cfg_obj.remove("autonomy");
    }
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
        return *resp;
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
        return *resp;
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
