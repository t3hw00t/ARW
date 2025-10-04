use axum::http::{HeaderMap, StatusCode};
use axum::response::IntoResponse;
use axum::{
    extract::{Path, Query, State},
    Json,
};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};

use crate::{
    metrics,
    runtime_matrix::{self, RuntimeMatrixEntry},
    self_model, state_observer, training, world, AppState,
};
use chrono::{DateTime, SecondsFormat, Utc};
use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};

#[derive(Debug, Default, Deserialize, ToSchema, IntoParams)]
#[serde(default)]
pub struct StateObservationsQuery {
    /// Limit the number of items returned (most recent first); defaults to all retained observations.
    pub limit: Option<usize>,
    /// Restrict results to event kinds matching this prefix (e.g. `actions.`).
    pub kind_prefix: Option<String>,
    /// Only include observations emitted after this RFC3339 timestamp.
    pub since: Option<String>,
}

#[derive(Debug, Default, Deserialize, IntoParams)]
#[serde(default)]
pub struct StateActionsQuery {
    /// Max number of rows to return (1-2000).
    pub limit: Option<i64>,
    /// Filter by exact action state (e.g., queued, running, completed).
    pub state: Option<String>,
    /// Restrict results to action kinds beginning with this prefix (e.g., `chat.`).
    pub kind_prefix: Option<String>,
    /// Only include actions updated at or after this RFC3339 timestamp.
    #[serde(rename = "updated_since")]
    pub updated_since: Option<String>,
}

fn numeric_version_from_field(items: &[Value], field: &str) -> u64 {
    items
        .iter()
        .filter_map(|item| item.get(field))
        .filter_map(|value| value.as_i64())
        .map(|id| id.max(0) as u64)
        .max()
        .unwrap_or(0)
}

#[derive(Clone, Serialize, ToSchema)]
pub struct ModelsCatalogResponse {
    #[schema(value_type = Vec<serde_json::Value>)]
    pub items: Vec<Value>,
}

#[derive(Clone, Serialize, ToSchema)]
pub struct RuntimeMatrixResponse {
    pub items: BTreeMap<String, RuntimeMatrixEntry>,
    pub ttl_seconds: u64,
}

#[derive(Debug, Default, Deserialize, IntoParams)]
#[serde(default)]
pub struct EpisodesQuery {
    /// Maximum number of episodes to return (1-1000, default 1000).
    pub limit: Option<usize>,
    /// Filter to episodes that include the specified project id.
    pub project: Option<String>,
    /// Return only episodes that contain error events.
    pub errors_only: Option<bool>,
    /// Keep episodes whose kinds start with this prefix (e.g. `tasks.`).
    pub kind_prefix: Option<String>,
    /// Only include episodes whose last event timestamp is at or after this RFC3339 time.
    pub since: Option<String>,
}

#[derive(Debug, Default, Deserialize, IntoParams)]
#[serde(default)]
pub struct EpisodeSnapshotQuery {
    /// Maximum number of events to include in the snapshot (1-2000, default 1000).
    pub limit: Option<usize>,
}

#[derive(Clone)]
pub(crate) struct EpisodeRollup {
    id: String,
    start: Option<String>,
    end: Option<String>,
    last: Option<String>,
    duration_ms: Option<u64>,
    first_kind: Option<String>,
    last_kind: Option<String>,
    count: u64,
    errors: u64,
    projects: Vec<String>,
    actors: Vec<String>,
    kinds: Vec<String>,
    events: Vec<Value>,
    last_dt: Option<DateTime<Utc>>,
}

impl EpisodeRollup {
    fn matches_project(&self, project: &str) -> bool {
        self.projects.iter().any(|p| p == project)
    }

    fn matches_kind_prefix(&self, prefix: &str) -> bool {
        self.kinds.iter().any(|k| k.starts_with(prefix))
    }

    fn matches_since(&self, since: DateTime<Utc>) -> bool {
        match self.last_dt {
            Some(last) => last >= since,
            None => false,
        }
    }

    pub(crate) fn into_value(self) -> Value {
        let mut episode = serde_json::Map::new();
        episode.insert("id".to_string(), Value::String(self.id));
        if let Some(start) = self.start {
            episode.insert("start".to_string(), Value::String(start));
        }
        if let Some(end) = self.end {
            episode.insert("end".to_string(), Value::String(end));
        }
        if let Some(last) = self.last {
            episode.insert("last".to_string(), Value::String(last));
        }
        if let Some(dur) = self.duration_ms {
            episode.insert("duration_ms".to_string(), json!(dur));
        }
        episode.insert("count".to_string(), json!(self.count));
        episode.insert("errors".to_string(), json!(self.errors));
        if let Some(first) = self.first_kind {
            episode.insert("first_kind".to_string(), json!(first));
        }
        if let Some(last_kind) = self.last_kind {
            episode.insert("last_kind".to_string(), json!(last_kind));
        }
        if !self.projects.is_empty() {
            episode.insert(
                "projects".to_string(),
                Value::Array(self.projects.into_iter().map(Value::String).collect()),
            );
        }
        if !self.actors.is_empty() {
            episode.insert(
                "actors".to_string(),
                Value::Array(self.actors.into_iter().map(Value::String).collect()),
            );
        }
        if !self.kinds.is_empty() {
            episode.insert(
                "kinds".to_string(),
                Value::Array(self.kinds.into_iter().map(Value::String).collect()),
            );
        }
        episode.insert("events".to_string(), Value::Array(self.events.clone()));
        episode.insert("items".to_string(), Value::Array(self.events));
        Value::Object(episode)
    }
}

fn parse_utc(ts: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(ts)
        .map(|dt| dt.with_timezone(&Utc))
        .ok()
}

fn event_is_error(kind: &str, payload: &Value) -> bool {
    let lowered = kind.to_ascii_lowercase();
    if lowered.contains(".error")
        || lowered.contains(".failed")
        || lowered.contains(".denied")
        || lowered.contains(".panic")
    {
        return true;
    }
    let status = payload
        .get("status")
        .and_then(|v| v.as_str())
        .map(|s| s.trim().to_ascii_lowercase());
    if matches!(
        status.as_deref(),
        Some("error" | "failed" | "denied" | "panic")
    ) {
        return true;
    }
    if payload
        .get("level")
        .and_then(|v| v.as_str())
        .map(|s| s.eq_ignore_ascii_case("error"))
        .unwrap_or(false)
    {
        return true;
    }
    if payload.get("ok").and_then(|v| v.as_bool()) == Some(false) {
        return true;
    }
    payload.get("error").is_some() || payload.get("err").is_some()
}

fn episode_from_events(corr_id: String, evs: Vec<arw_kernel::EventRow>) -> Option<EpisodeRollup> {
    if evs.is_empty() {
        return None;
    }
    let mut events_json: Vec<Value> = Vec::with_capacity(evs.len());
    let mut start_iso: Option<String> = None;
    let mut end_iso: Option<String> = None;
    let mut start_dt: Option<DateTime<Utc>> = None;
    let mut end_dt: Option<DateTime<Utc>> = None;
    let mut errors_count: u64 = 0;
    let mut projects: BTreeSet<String> = BTreeSet::new();
    let mut actors: BTreeSet<String> = BTreeSet::new();
    let mut kinds: BTreeSet<String> = BTreeSet::new();
    let mut first_kind: Option<String> = None;
    let mut last_kind: Option<String> = None;
    for event in evs {
        if start_iso.is_none() {
            start_iso = Some(event.time.clone());
            start_dt = parse_utc(&event.time);
            first_kind = Some(event.kind.clone());
        }
        end_iso = Some(event.time.clone());
        end_dt = parse_utc(&event.time);
        last_kind = Some(event.kind.clone());
        kinds.insert(event.kind.clone());
        if let Some(proj) = event.proj.as_ref() {
            if !proj.is_empty() {
                projects.insert(proj.clone());
            }
        } else if let Some(proj) = event
            .payload
            .get("proj")
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
        {
            projects.insert(proj.to_string());
        }
        if let Some(actor) = event.actor.as_ref() {
            if !actor.is_empty() {
                actors.insert(actor.clone());
            }
        } else if let Some(actor) = event
            .payload
            .get("actor")
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
        {
            actors.insert(actor.to_string());
        }
        let is_error = event_is_error(&event.kind, &event.payload);
        if is_error {
            errors_count = errors_count.saturating_add(1);
        }
        let mut obj = serde_json::Map::new();
        obj.insert("id".to_string(), json!(event.id));
        obj.insert("time".to_string(), json!(event.time));
        obj.insert("kind".to_string(), json!(event.kind));
        if let Some(actor) = event.actor {
            obj.insert("actor".to_string(), json!(actor));
        }
        if let Some(proj) = event.proj {
            obj.insert("proj".to_string(), json!(proj));
        }
        obj.insert("payload".to_string(), event.payload);
        if is_error {
            obj.insert("error".to_string(), Value::Bool(true));
        }
        events_json.push(Value::Object(obj));
    }

    let event_count = events_json.len();
    let last_dt = end_dt;
    let duration_ms = match (start_dt.as_ref(), last_dt.as_ref()) {
        (Some(start), Some(end)) if end.timestamp_millis() >= start.timestamp_millis() => {
            Some((end.timestamp_millis() - start.timestamp_millis()) as u64)
        }
        _ => None,
    };

    Some(EpisodeRollup {
        id: corr_id,
        start: start_iso,
        end: end_iso.clone(),
        last: end_iso,
        duration_ms,
        first_kind,
        last_kind,
        count: event_count as u64,
        errors: errors_count,
        projects: projects.into_iter().collect(),
        actors: actors.into_iter().collect(),
        kinds: kinds.into_iter().collect(),
        events: events_json,
        last_dt,
    })
}

pub(crate) async fn build_episode_rollups(
    state: &AppState,
    limit: usize,
) -> (Vec<EpisodeRollup>, u64) {
    let rows = state
        .kernel()
        .recent_events_async(limit as i64, None)
        .await
        .unwrap_or_default();
    let mut by_corr: BTreeMap<String, Vec<arw_kernel::EventRow>> = BTreeMap::new();
    let mut max_id: u64 = 0;
    for r in rows {
        let corr_id = r.corr_id.clone().unwrap_or_default();
        if corr_id.is_empty() {
            continue;
        }
        if r.id > 0 {
            max_id = max_id.max(r.id as u64);
        }
        by_corr.entry(corr_id).or_default().push(r);
    }
    let mut items: Vec<EpisodeRollup> = Vec::new();
    for (cid, evs) in by_corr.into_iter() {
        if let Some(episode) = episode_from_events(cid, evs) {
            items.push(episode);
        }
    }
    (items, max_id)
}

/// Episode rollups grouped by correlation id.
#[utoipa::path(
    get,
    path = "/state/episodes",
    tag = "State",
    params(EpisodesQuery),
    responses(
        (status = 200, description = "Episode rollups", body = serde_json::Value),
        (status = 400, description = "Invalid query parameter", body = serde_json::Value),
        (status = 501, description = "Kernel disabled", body = serde_json::Value)
    )
)]
pub async fn state_episodes(
    headers: HeaderMap,
    Query(query): Query<EpisodesQuery>,
    State(state): State<AppState>,
) -> impl IntoResponse {
    if !crate::admin_ok(&headers) {
        return (
            axum::http::StatusCode::UNAUTHORIZED,
            Json(json!({"type":"about:blank","title":"Unauthorized","status":401})),
        )
            .into_response();
    }
    if !state.kernel_enabled() {
        return crate::responses::kernel_disabled();
    }
    let since_dt = if let Some(ref since_raw) = query.since {
        match parse_utc(since_raw) {
            Some(dt) => Some(dt),
            None => {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(json!({
                        "type": "about:blank",
                        "title": "Invalid query parameter",
                        "detail": "since must be a valid RFC3339 timestamp",
                        "status": 400,
                    })),
                )
                    .into_response();
            }
        }
    } else {
        None
    };

    let (mut episodes, version) = build_episode_rollups(&state, 1000).await;

    if let Some(project) = query.project.as_ref() {
        episodes.retain(|ep| ep.matches_project(project));
    }
    if query.errors_only.unwrap_or(false) {
        episodes.retain(|ep| ep.errors > 0);
    }
    if let Some(prefix) = query.kind_prefix.as_ref() {
        episodes.retain(|ep| ep.matches_kind_prefix(prefix));
    }
    if let Some(since) = since_dt {
        episodes.retain(|ep| ep.matches_since(since));
    }

    episodes.sort_by(|a, b| {
        b.last_dt
            .cmp(&a.last_dt)
            .then_with(|| b.count.cmp(&a.count))
            .then_with(|| a.id.cmp(&b.id))
    });

    let limit = query.limit.unwrap_or(1000).clamp(1, 1000);
    if episodes.len() > limit {
        episodes.truncate(limit);
    }

    let items: Vec<Value> = episodes.into_iter().map(|ep| ep.into_value()).collect();
    if let Some(resp) =
        crate::api::http_utils::state_version_not_modified(&headers, "episodes", version)
    {
        return resp;
    }
    let mut response = Json(json!({"version": version, "items": items})).into_response();
    crate::api::http_utils::apply_state_version_headers(
        response.headers_mut(),
        "episodes",
        version,
    );
    response
}

/// Episode snapshot for a specific correlation id.
#[utoipa::path(
    get,
    path = "/state/episode/{id}/snapshot",
    tag = "State",
    params(
        ("id" = String, Path, description = "Correlation id of the episode"),
        EpisodeSnapshotQuery
    ),
    responses(
        (status = 200, description = "Episode snapshot", body = serde_json::Value),
        (status = 400, description = "Invalid query parameter", body = serde_json::Value),
        (status = 401, description = "Unauthorized"),
        (status = 404, description = "Episode not found"),
        (status = 501, description = "Kernel disabled", body = serde_json::Value)
    )
)]
pub async fn state_episode_snapshot(
    headers: HeaderMap,
    Path(id): Path<String>,
    Query(query): Query<EpisodeSnapshotQuery>,
    State(state): State<AppState>,
) -> impl IntoResponse {
    if !crate::admin_ok(&headers) {
        return (
            StatusCode::UNAUTHORIZED,
            Json(json!({"type":"about:blank","title":"Unauthorized","status":401})),
        )
            .into_response();
    }
    if !state.kernel_enabled() {
        return crate::responses::kernel_disabled();
    }

    let limit = query.limit.unwrap_or(1000).clamp(1, 2000) as i64;
    let events = match state
        .kernel()
        .events_by_corr_id_async(&id, Some(limit))
        .await
    {
        Ok(evs) => evs,
        Err(err) => {
            tracing::warn!(target: "arw::state", corr_id = %id, error = ?err, "failed to load episode snapshot");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({
                    "type": "about:blank",
                    "title": "Failed to load episode",
                    "status": 500,
                })),
            )
                .into_response();
        }
    };

    if events.is_empty() {
        return (
            StatusCode::NOT_FOUND,
            Json(json!({
                "type": "about:blank",
                "title": "Episode not found",
                "status": 404,
            })),
        )
            .into_response();
    }

    let version = events
        .iter()
        .map(|ev| ev.id.max(0) as u64)
        .max()
        .unwrap_or(0);
    if let Some(resp) = crate::api::http_utils::state_version_not_modified(
        &headers,
        &format!("episode-snapshot-{id}"),
        version,
    ) {
        return resp;
    }

    let episode = episode_from_events(id.clone(), events)
        .map(|ep| ep.into_value())
        .unwrap_or_else(|| json!({ "id": id.clone(), "events": [], "items": [] }));
    let mut response = Json(json!({ "version": version, "episode": episode })).into_response();
    crate::api::http_utils::apply_state_version_headers(
        response.headers_mut(),
        &format!("episode-snapshot-{id}"),
        version,
    );
    response
}

/// Bus and per-route counters snapshot.
#[utoipa::path(
    get,
    path = "/state/route_stats",
    tag = "State",
    responses((status = 200, description = "Route stats", body = serde_json::Value))
)]
pub async fn state_route_stats(
    headers: HeaderMap,
    State(state): State<AppState>,
) -> impl IntoResponse {
    let summary = state.metrics().snapshot();
    let bus = state.bus().stats();
    let cache = state.tool_cache().stats();
    let version = state.metrics().routes_version();
    if let Some(resp) =
        crate::api::http_utils::state_version_not_modified(&headers, "route-stats", version)
    {
        return resp;
    }
    let body = metrics::route_stats_snapshot(&summary, &bus, &cache);
    let mut response = Json(body).into_response();
    crate::api::http_utils::apply_state_version_headers(
        response.headers_mut(),
        "route-stats",
        version,
    );
    response
}

/// Background tasks status snapshot.
#[utoipa::path(
    get,
    path = "/state/tasks",
    tag = "State",
    responses((status = 200, description = "Background tasks", body = serde_json::Value))
)]
pub async fn state_tasks(headers: HeaderMap, State(state): State<AppState>) -> impl IntoResponse {
    let (version, tasks) = state.metrics().tasks_snapshot_with_version();
    if let Some(resp) =
        crate::api::http_utils::state_version_not_modified(&headers, "tasks", version)
    {
        return resp;
    }
    let mut response = Json(json!({ "version": version, "tasks": tasks })).into_response();
    crate::api::http_utils::apply_state_version_headers(response.headers_mut(), "tasks", version);
    response
}

/// Recent observations from the event bus.
#[utoipa::path(
    get,
    path = "/state/observations",
    tag = "State",
    operation_id = "state_observations_doc",
    description = "Recent observations from the event bus.",
    params(StateObservationsQuery),
    responses(
        (status = 200, description = "Recent observations", body = serde_json::Value),
        (status = 401, description = "Unauthorized", body = serde_json::Value)
    )
)]
pub async fn state_observations(
    headers: HeaderMap,
    Query(params): Query<StateObservationsQuery>,
) -> impl IntoResponse {
    if !crate::admin_ok(&headers) {
        return (
            axum::http::StatusCode::UNAUTHORIZED,
            Json(json!({"type":"about:blank","title":"Unauthorized","status":401})),
        )
            .into_response();
    }
    let StateObservationsQuery {
        limit,
        kind_prefix,
        since,
    } = params;
    let kind_prefix_ref = kind_prefix.as_deref();
    let since_filter: Option<DateTime<Utc>> = match since {
        Some(raw) => match DateTime::parse_from_rfc3339(raw.trim()) {
            Ok(dt) => Some(dt.with_timezone(&Utc)),
            Err(_) => {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(json!({
                        "type": "about:blank",
                        "title": "Invalid `since` value",
                        "detail": "`since` must be an RFC3339 timestamp (e.g., 2025-10-02T17:15:00Z)",
                        "status": 400
                    })),
                )
                    .into_response();
            }
        },
        None => None,
    };
    let (version, items) =
        state_observer::observations_snapshot(limit, kind_prefix_ref, since_filter).await;
    if let Some(resp) =
        crate::api::http_utils::state_version_not_modified(&headers, "observations", version)
    {
        return resp;
    }
    let mut response = Json(json!({"version": version, "items": items})).into_response();
    crate::api::http_utils::apply_state_version_headers(
        response.headers_mut(),
        "observations",
        version,
    );
    response
}

/// Current beliefs snapshot derived from events.
#[utoipa::path(
    get,
    path = "/state/beliefs",
    tag = "State",
    operation_id = "state_beliefs_doc",
    description = "Current beliefs snapshot derived from events.",
    responses(
        (status = 200, description = "Beliefs snapshot", body = serde_json::Value),
        (status = 401, description = "Unauthorized", body = serde_json::Value)
    )
)]
pub async fn state_beliefs(headers: HeaderMap) -> impl IntoResponse {
    if !crate::admin_ok(&headers) {
        return (
            axum::http::StatusCode::UNAUTHORIZED,
            Json(json!({"type":"about:blank","title":"Unauthorized","status":401})),
        )
            .into_response();
    }
    let (version, items) = state_observer::beliefs_snapshot().await;
    if let Some(resp) =
        crate::api::http_utils::state_version_not_modified(&headers, "beliefs", version)
    {
        return resp;
    }
    let mut response = Json(json!({"version": version, "items": items})).into_response();
    crate::api::http_utils::apply_state_version_headers(response.headers_mut(), "beliefs", version);
    response
}

/// Recent intents stream (rolling window) with a monotonic version counter.
#[utoipa::path(
    get,
    path = "/state/intents",
    tag = "State",
    operation_id = "state_intents_doc",
    description = "Recent intents stream (rolling window) with a monotonic version counter.",
    responses(
        (status = 200, description = "Recent intents", body = serde_json::Value),
        (status = 401, description = "Unauthorized", body = serde_json::Value)
    )
)]
pub async fn state_intents(headers: HeaderMap) -> impl IntoResponse {
    if !crate::admin_ok(&headers) {
        return (
            axum::http::StatusCode::UNAUTHORIZED,
            Json(json!({"type":"about:blank","title":"Unauthorized","status":401})),
        )
            .into_response();
    }
    let (version, items) = state_observer::intents_snapshot().await;
    if let Some(resp) =
        crate::api::http_utils::state_version_not_modified(&headers, "intents", version)
    {
        return resp;
    }
    let mut response = Json(json!({"version": version, "items": items})).into_response();
    crate::api::http_utils::apply_state_version_headers(response.headers_mut(), "intents", version);
    response
}

/// Crash log snapshot from state_dir/crash and crash/archive.
#[utoipa::path(
    get,
    path = "/state/crashlog",
    tag = "State",
    responses(
        (status = 200, description = "Crash log", body = serde_json::Value),
        (status = 401, description = "Unauthorized", body = serde_json::Value)
    )
)]
pub async fn state_crashlog(headers: HeaderMap) -> impl IntoResponse {
    if !crate::admin_ok(&headers) {
        return (
            axum::http::StatusCode::UNAUTHORIZED,
            Json(json!({"type":"about:blank","title":"Unauthorized","status":401})),
        )
            .into_response();
    }
    let value = crate::read_models::crashlog_snapshot().await;
    Json(value).into_response()
}

/// Screenshots OCR index snapshot.
#[utoipa::path(
    get,
    path = "/state/screenshots",
    tag = "State",
    operation_id = "state_screenshots_doc",
    description = "Indexed OCR sidecars for captured screenshots, grouped by source path and language.",
    responses(
        (status = 200, description = "Screenshots index", body = serde_json::Value),
        (status = 401, description = "Unauthorized", body = serde_json::Value)
    )
)]
pub async fn state_screenshots(headers: HeaderMap) -> impl IntoResponse {
    if !crate::admin_ok(&headers) {
        return (
            axum::http::StatusCode::UNAUTHORIZED,
            Json(json!({"type":"about:blank","title":"Unauthorized","status":401})),
        )
            .into_response();
    }
    let value = crate::read_models::screenshots_snapshot().await;
    Json(value).into_response()
}

/// Aggregated service health (read-model built from service.health events).
#[utoipa::path(
    get,
    path = "/state/service_health",
    tag = "State",
    responses(
        (status = 200, description = "Service health", body = serde_json::Value),
        (status = 401, description = "Unauthorized", body = serde_json::Value)
    )
)]
pub async fn state_service_health(headers: HeaderMap) -> impl IntoResponse {
    if !crate::admin_ok(&headers) {
        return (
            axum::http::StatusCode::UNAUTHORIZED,
            Json(json!({"type":"about:blank","title":"Unauthorized","status":401})),
        )
            .into_response();
    }
    let value = crate::read_models::cached_read_model("service_health")
        .unwrap_or_else(|| json!({"history": [], "last": null}));
    Json(value).into_response()
}

/// Consolidated service status: safe-mode, last crash, and last health signal.
#[utoipa::path(
    get,
    path = "/state/service_status",
    tag = "State",
    responses(
        (status = 200, description = "Service status", body = serde_json::Value),
        (status = 401, description = "Unauthorized", body = serde_json::Value)
    )
)]
pub async fn state_service_status(headers: HeaderMap) -> impl IntoResponse {
    if !crate::admin_ok(&headers) {
        return (
            axum::http::StatusCode::UNAUTHORIZED,
            Json(json!({"type":"about:blank","title":"Unauthorized","status":401})),
        )
            .into_response();
    }
    let until_ms = crate::crashguard::safe_mode_until_ms();
    let safe_mode = if until_ms > 0 {
        json!({"active": true, "until_ms": until_ms})
    } else {
        json!({"active": false})
    };
    let crashlog = crate::read_models::cached_read_model("crashlog")
        .unwrap_or_else(|| json!({"count": 0, "items": []}));
    let last_crash = crashlog
        .get("items")
        .and_then(|v| v.as_array())
        .and_then(|arr| arr.first())
        .cloned()
        .unwrap_or(Value::Null);
    let service_health = crate::read_models::cached_read_model("service_health")
        .unwrap_or_else(|| json!({"history": [], "last": null}));
    let last_health = service_health.get("last").cloned().unwrap_or(Value::Null);
    Json(json!({
        "safe_mode": safe_mode,
        "last_crash": last_crash,
        "last_health": last_health,
    }))
    .into_response()
}

/// Guardrails circuit-breaker metrics snapshot.
#[utoipa::path(
    get,
    path = "/state/guardrails_metrics",
    tag = "State",
    responses(
        (status = 200, description = "Guardrails metrics", body = serde_json::Value),
        (status = 401, description = "Unauthorized", body = serde_json::Value)
    )
)]
pub async fn state_guardrails_metrics(headers: HeaderMap) -> impl IntoResponse {
    if !crate::admin_ok(&headers) {
        return (
            axum::http::StatusCode::UNAUTHORIZED,
            Json(json!({"type":"about:blank","title":"Unauthorized","status":401})),
        )
            .into_response();
    }
    Json(crate::tools::guardrails_metrics_value()).into_response()
}

/// Active policy capsules snapshot.
#[utoipa::path(
    get,
    path = "/state/policy/capsules",
    tag = "Policy",
    responses((status = 200, description = "Active capsules", body = serde_json::Value))
)]
pub async fn state_policy_capsules(State(state): State<AppState>) -> impl IntoResponse {
    Json(state.capsules().snapshot().await)
}

/// Cluster nodes snapshot.
#[utoipa::path(
    get,
    path = "/state/cluster",
    tag = "State",
    operation_id = "state_cluster_doc",
    description = "Cluster nodes snapshot (admin-only).",
    responses(
        (status = 200, description = "Cluster nodes", body = serde_json::Value),
        (status = 401, description = "Unauthorized", body = serde_json::Value)
    )
)]
pub async fn state_cluster(headers: HeaderMap, State(state): State<AppState>) -> impl IntoResponse {
    if !crate::admin_ok(&headers) {
        return (
            axum::http::StatusCode::UNAUTHORIZED,
            Json(json!({"type":"about:blank","title":"Unauthorized","status":401})),
        )
            .into_response();
    }
    let nodes = state.cluster().snapshot().await;
    let now = Utc::now();
    let generated = now.to_rfc3339_opts(SecondsFormat::Millis, true);
    let generated_ms = now.timestamp_millis();
    let generated_ms = if generated_ms < 0 {
        0
    } else {
        generated_ms as u64
    };
    Json(json!({
        "nodes": nodes,
        "generated": generated,
        "generated_ms": generated_ms,
    }))
    .into_response()
}

#[derive(Deserialize, utoipa::ToSchema)]
pub struct WorldQuery {
    #[serde(default)]
    pub proj: Option<String>,
}

/// Project world model snapshot (belief graph view).
#[utoipa::path(
    get,
    path = "/state/world",
    tag = "State",
    operation_id = "state_world_doc",
    description = "Project world model snapshot (belief graph view).",
    params(("proj" = Option<String>, Query, description = "Project id")),
    responses(
        (status = 200, description = "World model", body = serde_json::Value),
        (status = 401, description = "Unauthorized", body = serde_json::Value)
    )
)]
pub async fn state_world(headers: HeaderMap, Query(q): Query<WorldQuery>) -> impl IntoResponse {
    if !crate::admin_ok(&headers) {
        return (
            axum::http::StatusCode::UNAUTHORIZED,
            Json(json!({"type":"about:blank","title":"Unauthorized","status":401})),
        )
            .into_response();
    }
    let map = world::snapshot_project_map(q.proj.as_deref()).await;
    let version = map.version;
    if let Some(resp) =
        crate::api::http_utils::state_version_not_modified(&headers, "world", version)
    {
        return resp;
    }
    let body = serde_json::to_value(map).unwrap_or_else(|_| json!({}));
    let mut response = Json(body).into_response();
    crate::api::http_utils::apply_state_version_headers(response.headers_mut(), "world", version);
    response
}

#[derive(Deserialize, utoipa::ToSchema)]
pub struct WorldSelectQuery {
    #[serde(default)]
    pub proj: Option<String>,
    #[serde(default)]
    pub q: Option<String>,
    #[serde(default)]
    pub k: Option<usize>,
    #[serde(default)]
    pub lambda: Option<f64>,
}

/// Select top-k claims for a query.
#[utoipa::path(
    get,
    path = "/state/world/select",
    tag = "State",
    operation_id = "state_world_select_doc",
    description = "Select top-k claims for a query.",
    params(
        ("proj" = Option<String>, Query, description = "Project id"),
        ("q" = Option<String>, Query, description = "Query string"),
        ("k" = Option<usize>, Query, description = "Top K"),
        ("lambda" = Option<f64>, Query, description = "Diversity weight (0-1)")
    ),
    responses(
        (status = 200, description = "Selected claims", body = serde_json::Value),
        (status = 401, description = "Unauthorized", body = serde_json::Value)
    )
)]
pub async fn state_world_select(
    headers: HeaderMap,
    Query(q): Query<WorldSelectQuery>,
) -> impl IntoResponse {
    if !crate::admin_ok(&headers) {
        return (
            axum::http::StatusCode::UNAUTHORIZED,
            Json(json!({"type":"about:blank","title":"Unauthorized","status":401})),
        )
            .into_response();
    }
    let query = q.q.unwrap_or_default();
    let k = q.k.unwrap_or(8);
    let lambda = q.lambda.unwrap_or(0.5);
    let items = world::select_top_claims_diverse(q.proj.as_deref(), &query, k, lambda).await;
    Json(json!({"items": items})).into_response()
}

/// Kernel contributions snapshot.
#[utoipa::path(
    get,
    path = "/state/contributions",
    tag = "State",
    responses(
        (status = 200, description = "Contributions list", body = serde_json::Value),
        (status = 501, description = "Kernel disabled", body = serde_json::Value)
    )
)]
pub async fn state_contributions(
    headers: HeaderMap,
    State(state): State<AppState>,
) -> impl IntoResponse {
    if !crate::admin_ok(&headers) {
        return (
            axum::http::StatusCode::UNAUTHORIZED,
            Json(json!({"type":"about:blank","title":"Unauthorized","status":401})),
        )
            .into_response();
    }
    if !state.kernel_enabled() {
        return crate::responses::kernel_disabled();
    }
    let items = state
        .kernel()
        .list_contributions_async(200)
        .await
        .unwrap_or_default();
    let version = numeric_version_from_field(&items, "id");
    if let Some(resp) =
        crate::api::http_utils::state_version_not_modified(&headers, "contributions", version)
    {
        return resp;
    }
    let mut response = Json(json!({"version": version, "items": items})).into_response();
    crate::api::http_utils::apply_state_version_headers(
        response.headers_mut(),
        "contributions",
        version,
    );
    response
}

/// Experiment events snapshot (public read-model).
#[utoipa::path(
    get,
    path = "/state/experiments",
    tag = "State",
    responses((status = 200, description = "Experiment events", body = serde_json::Value))
)]
pub async fn state_experiments(
    headers: HeaderMap,
    State(state): State<AppState>,
) -> impl IntoResponse {
    let (version, items) = state.experiments().state_events_snapshot().await;
    if let Some(resp) =
        crate::api::http_utils::state_version_not_modified(&headers, "experiments", version)
    {
        return resp;
    }
    let mut response = Json(json!({"version": version, "items": items})).into_response();
    crate::api::http_utils::apply_state_version_headers(
        response.headers_mut(),
        "experiments",
        version,
    );
    response
}

/// Recent actions list.
#[utoipa::path(
    get,
    path = "/state/actions",
    tag = "State",
    operation_id = "state_actions_doc",
    description = "Recent actions list (most recent first).",
    params(StateActionsQuery),
    responses(
        (status = 200, description = "Actions list", body = serde_json::Value),
        (status = 501, description = "Kernel disabled", body = serde_json::Value)
    )
)]
pub async fn state_actions(
    headers: HeaderMap,
    State(state): State<AppState>,
    Query(params): Query<StateActionsQuery>,
) -> impl IntoResponse {
    if !crate::admin_ok(&headers) {
        return (
            axum::http::StatusCode::UNAUTHORIZED,
            Json(json!({"type":"about:blank","title":"Unauthorized","status":401})),
        )
            .into_response();
    }
    if !state.kernel_enabled() {
        return crate::responses::kernel_disabled();
    }
    let version = crate::state_observer::actions_version_value();
    if let Some(resp) =
        crate::api::http_utils::state_version_not_modified(&headers, "actions", version)
    {
        return resp;
    }
    let mut options = arw_kernel::ActionListOptions::new(params.limit.unwrap_or(200));
    options.limit = options.clamped_limit();
    options.state = params.state;
    options.kind_prefix = params.kind_prefix;
    options.updated_since = params.updated_since;
    let items = state
        .kernel()
        .list_actions_async(options)
        .await
        .unwrap_or_default();
    let items: Vec<Value> = items
        .into_iter()
        .map(crate::api::actions::sanitize_action_record)
        .collect();
    let mut response = Json(json!({"version": version, "items": items})).into_response();
    crate::api::http_utils::apply_state_version_headers(response.headers_mut(), "actions", version);
    response
}

/// Recent egress ledger list.
#[utoipa::path(
    get,
    path = "/state/egress",
    tag = "State",
    params(("limit" = Option<i64>, Query, description = "Max items (1-2000)")),
    responses(
        (status = 200, description = "Egress ledger", body = serde_json::Value),
        (status = 501, description = "Kernel disabled", body = serde_json::Value)
    )
)]
pub async fn state_egress(
    headers: HeaderMap,
    State(state): State<AppState>,
    Query(q): Query<std::collections::HashMap<String, String>>,
) -> impl IntoResponse {
    if !crate::admin_ok(&headers) {
        return (
            axum::http::StatusCode::UNAUTHORIZED,
            Json(json!({"type":"about:blank","title":"Unauthorized","status":401})),
        )
            .into_response();
    }
    let limit = q
        .get("limit")
        .and_then(|s| s.parse::<i64>().ok())
        .unwrap_or(200);
    if !state.kernel_enabled() {
        return crate::responses::kernel_disabled();
    }
    let items = state
        .kernel()
        .list_egress_async(limit.clamp(1, 2000))
        .await
        .unwrap_or_default();
    let count = items.len();
    let version = numeric_version_from_field(&items, "id");
    if let Some(resp) =
        crate::api::http_utils::state_version_not_modified(&headers, "egress", version)
    {
        return resp;
    }
    let settings = crate::api::egress_settings::current_settings(&state).await;
    let mut response = Json(json!({
        "version": version,
        "count": count,
        "items": items,
        "settings": settings,
    }))
    .into_response();
    crate::api::http_utils::apply_state_version_headers(response.headers_mut(), "egress", version);
    response
}

#[cfg(test)]
mod tests {
    use super::*;
    use arw_policy::PolicyEngine;
    use arw_topics;
    use axum::{
        body::to_bytes,
        extract::Query,
        http::{header, HeaderMap, HeaderValue, StatusCode},
    };
    use chrono::{DateTime, Duration, SecondsFormat, Utc};
    use serde_json::{json, Value};
    use std::collections::HashMap;
    use std::sync::Arc;
    use tempfile::tempdir;

    async fn build_state(
        path: &std::path::Path,
        env_guard: &mut crate::test_support::env::EnvGuard,
    ) -> AppState {
        env_guard.set("ARW_DEBUG", "1");
        crate::util::reset_state_dir_for_tests();
        env_guard.set("ARW_STATE_DIR", path.display().to_string());
        let bus = arw_events::Bus::new_with_replay(16, 16);
        let kernel = arw_kernel::Kernel::open(path).expect("init kernel");
        let policy = PolicyEngine::load_from_env();
        let policy_handle = crate::policy::PolicyHandle::new(policy, bus.clone());
        let host: Arc<dyn arw_wasi::ToolHost> = Arc::new(arw_wasi::NoopHost);
        AppState::builder(bus, kernel, policy_handle, host, true)
            .with_sse_capacity(16)
            .build()
            .await
    }

    #[tokio::test]
    async fn state_actions_sanitizes_guard_metadata() {
        let temp = tempdir().expect("tempdir");
        let mut ctx = crate::test_support::begin_state_env(temp.path());
        crate::state_observer::reset_for_tests().await;
        let state = build_state(temp.path(), &mut ctx.env).await;

        let action_id = uuid::Uuid::new_v4().to_string();
        state
            .kernel()
            .insert_action_async(
                &action_id,
                "net.http.get",
                &json!({"url": "https://example.com"}),
                None,
                None,
                "completed",
            )
            .await
            .expect("insert action");

        let stored_output = json!({
            "value": {"status": "ok"},
            "posture": "secure",
            "guard": {
                "allowed": true,
                "policy_allow": false,
                "required_capabilities": ["net:http", "io:egress"],
                "lease": {
                    "id": "lease-1",
                    "subject": Some("local"),
                    "capability": "net:http",
                    "scope": Some("repo"),
                    "ttl_until": "2099-01-01T00:00:00Z"
                }
            }
        });

        state
            .kernel()
            .update_action_result_async(action_id.clone(), Some(stored_output), None)
            .await
            .expect("store output");

        let env = arw_events::Envelope {
            time: Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true),
            kind: "actions.completed".to_string(),
            payload: json!({"id": action_id, "status": "completed"}),
            policy: None,
            ce: None,
        };
        crate::state_observer::ingest_for_tests(&env).await;

        let response = state_actions(
            HeaderMap::new(),
            State(state.clone()),
            Query(StateActionsQuery::default()),
        )
        .await;
        let (parts, body) = response.into_response().into_parts();
        assert_eq!(parts.status, StatusCode::OK);
        assert_eq!(
            parts
                .headers
                .get(header::ETAG)
                .and_then(|v| v.to_str().ok()),
            Some("\"state-actions-v1\"")
        );
        let bytes = to_bytes(body, usize::MAX).await.expect("body bytes");
        let value: Value = serde_json::from_slice(&bytes).expect("json");
        assert_eq!(value["version"].as_u64(), Some(1));
        let items = value["items"].as_array().expect("items array");
        assert_eq!(items.len(), 1);
        let item = &items[0];
        assert_eq!(item["id"].as_str(), Some(action_id.as_str()));
        assert!(item["output"].is_null());
        assert!(item.get("guard").is_none());
        assert!(item.get("posture").is_none());
    }

    #[tokio::test]
    async fn state_intents_includes_version() {
        let temp = tempdir().expect("tempdir");
        let mut ctx = crate::test_support::begin_state_env(temp.path());
        crate::state_observer::reset_for_tests().await;

        let _state = build_state(temp.path(), &mut ctx.env).await;

        let env = arw_events::Envelope {
            time: Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true),
            kind: "intents.proposed".to_string(),
            payload: json!({"corr_id": "demo", "goal": "test"}),
            policy: None,
            ce: None,
        };

        crate::state_observer::ingest_for_tests(&env).await;

        let response = state_intents(HeaderMap::new()).await.into_response();
        let (parts, body) = response.into_parts();
        assert_eq!(parts.status, StatusCode::OK);
        assert_eq!(
            parts
                .headers
                .get(header::ETAG)
                .and_then(|v| v.to_str().ok()),
            Some("\"state-intents-v1\"")
        );
        let bytes = to_bytes(body, usize::MAX).await.expect("body bytes");
        let value: Value = serde_json::from_slice(&bytes).expect("json");
        assert_eq!(value["version"].as_u64(), Some(1));
        let items = value["items"].as_array().expect("items array");
        assert_eq!(items.len(), 1);
        assert_eq!(items[0]["kind"].as_str(), Some("intents.proposed"));

        // Ingest another event and ensure version increments.
        let env2 = arw_events::Envelope {
            time: Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true),
            kind: "intents.accepted".to_string(),
            payload: json!({"corr_id": "demo", "goal": "test"}),
            policy: None,
            ce: None,
        };
        crate::state_observer::ingest_for_tests(&env2).await;

        let response = state_intents(HeaderMap::new()).await.into_response();
        let (parts, body) = response.into_parts();
        assert_eq!(parts.status, StatusCode::OK);
        assert_eq!(
            parts
                .headers
                .get(header::ETAG)
                .and_then(|v| v.to_str().ok()),
            Some("\"state-intents-v2\"")
        );
        let bytes = to_bytes(body, usize::MAX).await.expect("body bytes 2");
        let value: Value = serde_json::from_slice(&bytes).expect("json 2");
        assert_eq!(value["version"].as_u64(), Some(2));
        let items = value["items"].as_array().expect("items array 2");
        assert_eq!(items.len(), 2);

        crate::state_observer::reset_for_tests().await;
    }

    #[tokio::test]
    async fn state_intents_honors_if_none_match() {
        let temp = tempdir().expect("tempdir");
        let mut ctx = crate::test_support::begin_state_env(temp.path());
        crate::state_observer::reset_for_tests().await;

        let _state = build_state(temp.path(), &mut ctx.env).await;
        let env = arw_events::Envelope {
            time: Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true),
            kind: "intents.proposed".to_string(),
            payload: json!({"corr_id": "demo", "goal": "test"}),
            policy: None,
            ce: None,
        };
        crate::state_observer::ingest_for_tests(&env).await;

        let first = state_intents(HeaderMap::new()).await.into_response();
        let etag = first.headers().get(header::ETAG).cloned().expect("etag");

        let mut headers = HeaderMap::new();
        headers.insert(header::IF_NONE_MATCH, etag.clone());
        let response = state_intents(headers).await.into_response();
        assert_eq!(response.status(), StatusCode::NOT_MODIFIED);
        crate::state_observer::reset_for_tests().await;
    }

    #[tokio::test]
    async fn state_actions_honors_if_none_match() {
        let temp = tempdir().expect("tempdir");
        let mut ctx = crate::test_support::begin_state_env(temp.path());
        crate::state_observer::reset_for_tests().await;
        let state = build_state(temp.path(), &mut ctx.env).await;

        let action_id = uuid::Uuid::new_v4().to_string();
        state
            .kernel()
            .insert_action_async(
                &action_id,
                "net.http.get",
                &json!({"url": "https://example.com"}),
                None,
                None,
                "completed",
            )
            .await
            .expect("insert action");
        state
            .kernel()
            .update_action_result_async(action_id.clone(), None, None)
            .await
            .expect("store result");
        let env = arw_events::Envelope {
            time: Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true),
            kind: "actions.completed".to_string(),
            payload: json!({"id": "action-1", "status": "completed"}),
            policy: None,
            ce: None,
        };
        crate::state_observer::ingest_for_tests(&env).await;

        let first = state_actions(
            HeaderMap::new(),
            State(state.clone()),
            Query(StateActionsQuery::default()),
        )
        .await
        .into_response();
        let etag = first
            .headers()
            .get(header::ETAG)
            .cloned()
            .unwrap_or_else(|| HeaderValue::from_static("\"state-actions-v0\""));

        let mut headers = HeaderMap::new();
        headers.insert(header::IF_NONE_MATCH, etag);
        let response = state_actions(headers, State(state), Query(StateActionsQuery::default()))
            .await
            .into_response();
        assert_eq!(response.status(), StatusCode::NOT_MODIFIED);
        crate::state_observer::reset_for_tests().await;
    }

    #[tokio::test]
    async fn state_actions_supports_filters() {
        let temp = tempdir().expect("tempdir");
        let mut ctx = crate::test_support::begin_state_env(temp.path());
        ctx.env.set("ARW_DEBUG", "1");
        crate::state_observer::reset_for_tests().await;
        let state = build_state(temp.path(), &mut ctx.env).await;

        let kernel = state.kernel();
        kernel
            .insert_action_async(
                "a1",
                "chat.reply",
                &json!({"input": "ok"}),
                None,
                None,
                "completed",
            )
            .await
            .expect("insert action a1");
        kernel
            .insert_action_async("a2", "chat.search", &json!({}), None, None, "failed")
            .await
            .expect("insert action a2");
        kernel
            .insert_action_async("a3", "tools.build", &json!({}), None, None, "running")
            .await
            .expect("insert action a3");

        let query = StateActionsQuery {
            limit: Some(10),
            state: Some("completed".to_string()),
            kind_prefix: Some("chat.".to_string()),
            updated_since: None,
        };
        let response = state_actions(HeaderMap::new(), State(state), Query(query))
            .await
            .into_response();
        let (parts, body) = response.into_parts();
        assert_eq!(parts.status, StatusCode::OK);
        let bytes = to_bytes(body, usize::MAX).await.expect("body");
        let value: Value = serde_json::from_slice(&bytes).expect("json");
        let items = value["items"].as_array().cloned().unwrap_or_default();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0]["id"].as_str(), Some("a1"));

        crate::state_observer::reset_for_tests().await;
    }

    #[tokio::test]
    async fn state_observations_honors_if_none_match() {
        let mut env_guard = crate::test_support::env::guard();
        env_guard.set("ARW_DEBUG", "1");
        crate::state_observer::reset_for_tests().await;

        let envelope = arw_events::Envelope {
            time: Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true),
            kind: "obs.debug".to_string(),
            payload: json!({"message": "hello"}),
            policy: None,
            ce: None,
        };
        crate::state_observer::ingest_for_tests(&envelope).await;

        let first = state_observations(HeaderMap::new(), Query(StateObservationsQuery::default()))
            .await
            .into_response();
        let etag = first
            .headers()
            .get(header::ETAG)
            .cloned()
            .expect("observations etag");

        let mut headers = HeaderMap::new();
        headers.insert(header::IF_NONE_MATCH, etag);
        let response = state_observations(headers, Query(StateObservationsQuery::default()))
            .await
            .into_response();
        assert_eq!(response.status(), StatusCode::NOT_MODIFIED);
        crate::state_observer::reset_for_tests().await;
    }

    #[tokio::test]
    async fn state_observations_supports_limit_and_prefix() {
        let mut env_guard = crate::test_support::env::guard();
        env_guard.set("ARW_DEBUG", "1");
        crate::state_observer::reset_for_tests().await;

        let envs = [
            arw_events::Envelope {
                time: Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true),
                kind: "obs.one".to_string(),
                payload: json!({"seq": 1}),
                policy: None,
                ce: None,
            },
            arw_events::Envelope {
                time: Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true),
                kind: "intents.proposed".to_string(),
                payload: json!({"seq": 2}),
                policy: None,
                ce: None,
            },
            arw_events::Envelope {
                time: Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true),
                kind: "obs.two".to_string(),
                payload: json!({"seq": 3}),
                policy: None,
                ce: None,
            },
        ];

        for env in &envs {
            crate::state_observer::ingest_for_tests(env).await;
        }

        let params = StateObservationsQuery {
            limit: Some(1),
            kind_prefix: None,
            since: None,
        };
        let response = state_observations(HeaderMap::new(), Query(params))
            .await
            .into_response();
        let (parts, body) = response.into_parts();
        assert_eq!(parts.status, StatusCode::OK);
        let bytes = to_bytes(body, usize::MAX).await.expect("body bytes");
        let value: Value = serde_json::from_slice(&bytes).expect("json");
        assert_eq!(value["items"].as_array().map(|a| a.len()), Some(1));
        assert_eq!(value["items"][0]["payload"]["seq"].as_i64(), Some(3));

        let params = StateObservationsQuery {
            limit: None,
            kind_prefix: Some("obs.".to_string()),
            since: None,
        };
        let response = state_observations(HeaderMap::new(), Query(params))
            .await
            .into_response();
        let (parts, body) = response.into_parts();
        assert_eq!(parts.status, StatusCode::OK);
        let bytes = to_bytes(body, usize::MAX).await.expect("body bytes");
        let value: Value = serde_json::from_slice(&bytes).expect("json");
        assert_eq!(value["items"].as_array().map(|a| a.len()), Some(2));
        assert_eq!(value["items"][0]["payload"]["seq"].as_i64(), Some(1));
        assert_eq!(value["items"][1]["payload"]["seq"].as_i64(), Some(3));

        crate::state_observer::reset_for_tests().await;
    }

    #[tokio::test]
    async fn state_observations_supports_since_filter() {
        let mut env_guard = crate::test_support::env::guard();
        env_guard.set("ARW_DEBUG", "1");
        crate::state_observer::reset_for_tests().await;

        let older = Utc::now() - Duration::seconds(60);
        let newer = Utc::now();
        let envs = [
            arw_events::Envelope {
                time: older.to_rfc3339_opts(SecondsFormat::Millis, true),
                kind: "obs.old".to_string(),
                payload: json!({"seq": 1}),
                policy: None,
                ce: None,
            },
            arw_events::Envelope {
                time: newer.to_rfc3339_opts(SecondsFormat::Millis, true),
                kind: "obs.new".to_string(),
                payload: json!({"seq": 2}),
                policy: None,
                ce: None,
            },
        ];

        for env in &envs {
            crate::state_observer::ingest_for_tests(env).await;
        }

        let threshold = older + Duration::seconds(1);
        let params = StateObservationsQuery {
            limit: None,
            kind_prefix: None,
            since: Some(threshold.to_rfc3339_opts(SecondsFormat::Millis, true)),
        };
        let response = state_observations(HeaderMap::new(), Query(params))
            .await
            .into_response();
        let (parts, body) = response.into_parts();
        assert_eq!(parts.status, StatusCode::OK);
        let bytes = to_bytes(body, usize::MAX).await.expect("body bytes");
        let value: Value = serde_json::from_slice(&bytes).expect("json");
        let items = value["items"].as_array().cloned().unwrap_or_default();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0]["kind"].as_str(), Some("obs.new"));

        crate::state_observer::reset_for_tests().await;
    }

    #[tokio::test]
    async fn state_observations_rejects_invalid_since() {
        let mut env_guard = crate::test_support::env::guard();
        env_guard.set("ARW_DEBUG", "1");
        crate::state_observer::reset_for_tests().await;

        let params = StateObservationsQuery {
            limit: None,
            kind_prefix: None,
            since: Some("not-a-timestamp".to_string()),
        };
        let response = state_observations(HeaderMap::new(), Query(params))
            .await
            .into_response();
        let (parts, body) = response.into_parts();
        assert_eq!(parts.status, StatusCode::BAD_REQUEST);
        let bytes = to_bytes(body, usize::MAX).await.expect("body bytes");
        let value: Value = serde_json::from_slice(&bytes).expect("json");
        assert_eq!(value["title"].as_str(), Some("Invalid `since` value"));

        crate::state_observer::reset_for_tests().await;
    }

    #[tokio::test]
    async fn state_beliefs_honors_if_none_match() {
        let mut env_guard = crate::test_support::env::guard();
        env_guard.set("ARW_DEBUG", "1");
        crate::state_observer::reset_for_tests().await;

        let envelope = arw_events::Envelope {
            time: Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true),
            kind: "beliefs.updated".to_string(),
            payload: json!({"claim": "alpha"}),
            policy: None,
            ce: None,
        };
        crate::state_observer::ingest_for_tests(&envelope).await;

        let first = state_beliefs(HeaderMap::new()).await.into_response();
        let etag = first
            .headers()
            .get(header::ETAG)
            .cloned()
            .expect("beliefs etag");

        let mut headers = HeaderMap::new();
        headers.insert(header::IF_NONE_MATCH, etag);
        let response = state_beliefs(headers).await.into_response();
        assert_eq!(response.status(), StatusCode::NOT_MODIFIED);
        crate::state_observer::reset_for_tests().await;
    }

    #[tokio::test]
    async fn state_world_honors_if_none_match() {
        let mut env_guard = crate::test_support::env::guard();
        env_guard.set("ARW_DEBUG", "1");
        crate::world::reset_for_tests().await;

        let env = arw_events::Envelope {
            time: Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true),
            kind: arw_topics::TOPIC_PROJECTS_CREATED.to_string(),
            payload: json!({"name": "demo"}),
            policy: None,
            ce: None,
        };
        crate::world::ingest_for_tests(&env).await;

        let first = state_world(HeaderMap::new(), Query(WorldQuery { proj: None }))
            .await
            .into_response();
        let etag = first
            .headers()
            .get(header::ETAG)
            .cloned()
            .expect("world etag");

        let mut headers = HeaderMap::new();
        headers.insert(header::IF_NONE_MATCH, etag);
        let response = state_world(headers, Query(WorldQuery { proj: None }))
            .await
            .into_response();
        assert_eq!(response.status(), StatusCode::NOT_MODIFIED);

        crate::world::reset_for_tests().await;
    }

    #[tokio::test]
    async fn state_contributions_honors_if_none_match() {
        let temp = tempdir().expect("tempdir");
        let mut ctx = crate::test_support::begin_state_env(temp.path());
        let state = build_state(temp.path(), &mut ctx.env).await;

        state
            .kernel()
            .append_contribution_async("local", "test", 1.0, "unit", None, None, None)
            .await
            .expect("append contribution");

        let first = state_contributions(HeaderMap::new(), State(state.clone()))
            .await
            .into_response();
        let etag = first
            .headers()
            .get(header::ETAG)
            .cloned()
            .expect("contributions etag");

        let mut headers = HeaderMap::new();
        headers.insert(header::IF_NONE_MATCH, etag);
        let response = state_contributions(headers, State(state))
            .await
            .into_response();
        assert_eq!(response.status(), StatusCode::NOT_MODIFIED);
    }

    #[tokio::test]
    async fn state_egress_honors_if_none_match() {
        let temp = tempdir().expect("tempdir");
        let mut ctx = crate::test_support::begin_state_env(temp.path());
        let state = build_state(temp.path(), &mut ctx.env).await;

        state
            .kernel()
            .append_egress_async(
                "allow".to_string(),
                None,
                None,
                None,
                None,
                Some(128),
                Some(256),
                Some("corr".to_string()),
                None,
                Some("secure".to_string()),
                None,
            )
            .await
            .expect("append egress");

        let params: HashMap<String, String> = HashMap::new();
        let first = state_egress(
            HeaderMap::new(),
            State(state.clone()),
            Query(params.clone()),
        )
        .await
        .into_response();
        let etag = first
            .headers()
            .get(header::ETAG)
            .cloned()
            .expect("egress etag");

        let mut headers = HeaderMap::new();
        headers.insert(header::IF_NONE_MATCH, etag);
        let response = state_egress(headers, State(state), Query(params))
            .await
            .into_response();
        assert_eq!(response.status(), StatusCode::NOT_MODIFIED);
    }

    #[tokio::test]
    async fn state_tasks_honors_if_none_match() {
        let temp = tempdir().expect("tempdir");
        let mut ctx = crate::test_support::begin_state_env(temp.path());
        let state = build_state(temp.path(), &mut ctx.env).await;

        let metrics = state.metrics();
        metrics.task_started("demo");
        metrics.task_completed("demo");

        let first = state_tasks(HeaderMap::new(), State(state.clone()))
            .await
            .into_response();
        let etag = first
            .headers()
            .get(header::ETAG)
            .cloned()
            .expect("tasks etag");

        let mut headers = HeaderMap::new();
        headers.insert(header::IF_NONE_MATCH, etag);
        let response = state_tasks(headers, State(state)).await.into_response();
        assert_eq!(response.status(), StatusCode::NOT_MODIFIED);
    }

    #[tokio::test]
    async fn state_experiments_honors_if_none_match() {
        let temp = tempdir().expect("tempdir");
        let mut ctx = crate::test_support::begin_state_env(temp.path());
        let state = build_state(temp.path(), &mut ctx.env).await;

        let experiments = state.experiments();
        experiments.reset_for_tests().await;
        experiments
            .publish_start("demo".into(), vec!["A".into()], None, None)
            .await;

        let first = state_experiments(HeaderMap::new(), State(state.clone()))
            .await
            .into_response();
        let etag = first
            .headers()
            .get(header::ETAG)
            .cloned()
            .expect("experiments etag");

        let mut headers = HeaderMap::new();
        headers.insert(header::IF_NONE_MATCH, etag);
        let response = state_experiments(headers, State(state))
            .await
            .into_response();
        assert_eq!(response.status(), StatusCode::NOT_MODIFIED);
    }

    #[tokio::test]
    async fn state_route_stats_honors_if_none_match() {
        let temp = tempdir().expect("tempdir");
        let mut ctx = crate::test_support::begin_state_env(temp.path());
        let state = build_state(temp.path(), &mut ctx.env).await;

        // Simulate route metrics update
        state.metrics().record_route("GET /demo", 200, 42);

        let first = state_route_stats(HeaderMap::new(), State(state.clone()))
            .await
            .into_response();
        let etag = first
            .headers()
            .get(header::ETAG)
            .cloned()
            .expect("route stats etag");

        let mut headers = HeaderMap::new();
        headers.insert(header::IF_NONE_MATCH, etag);
        let response = state_route_stats(headers, State(state))
            .await
            .into_response();
        assert_eq!(response.status(), StatusCode::NOT_MODIFIED);
    }

    #[tokio::test]
    async fn state_episodes_returns_rollups() {
        let temp = tempdir().expect("tempdir");
        let mut env_guard = crate::test_support::env::guard();
        let state = build_state(temp.path(), &mut env_guard).await;

        let corr = "run-123";
        let t1: DateTime<Utc> = Utc::now();
        let t2 = t1 + chrono::Duration::milliseconds(25);
        let env1 = arw_events::Envelope {
            time: t1.to_rfc3339_opts(SecondsFormat::Millis, true),
            kind: "tasks.started".to_string(),
            payload: json!({"corr_id": corr, "step": "start", "proj": "demo"}),
            policy: None,
            ce: None,
        };
        let env2 = arw_events::Envelope {
            time: t2.to_rfc3339_opts(SecondsFormat::Millis, true),
            kind: "tasks.completed".to_string(),
            payload: json!({"corr_id": corr, "step": "end", "proj": "demo"}),
            policy: None,
            ce: None,
        };
        let t3 = t2 + chrono::Duration::milliseconds(10);
        let env3 = arw_events::Envelope {
            time: t3.to_rfc3339_opts(SecondsFormat::Millis, true),
            kind: "tasks.failed".to_string(),
            payload: json!({"corr_id": corr, "step": "error", "status": "failed", "proj": "demo"}),
            policy: None,
            ce: None,
        };

        state
            .kernel()
            .append_event_async(&env1)
            .await
            .expect("append start event");
        state
            .kernel()
            .append_event_async(&env2)
            .await
            .expect("append end event");
        state
            .kernel()
            .append_event_async(&env3)
            .await
            .expect("append error event");

        let response = state_episodes(
            HeaderMap::new(),
            Query(EpisodesQuery::default()),
            State(state.clone()),
        )
        .await
        .into_response();
        let (parts, body) = response.into_parts();
        assert_eq!(parts.status, StatusCode::OK);
        let bytes = to_bytes(body, usize::MAX).await.expect("body bytes");
        let value: Value = serde_json::from_slice(&bytes).expect("json");
        assert!(value["version"].as_u64().unwrap_or_default() > 0);
        let items = value["items"].as_array().expect("items array");
        assert_eq!(items.len(), 1);
        let item = &items[0];
        assert_eq!(item["id"].as_str(), Some(corr));
        let events = item["events"].as_array().expect("events array");
        assert_eq!(events.len(), 3);
        let seq_set: std::collections::HashSet<_> = events
            .iter()
            .map(|ev| ev["payload"]["step"].as_str().unwrap_or(""))
            .collect();
        assert!(seq_set.contains("start"));
        assert!(seq_set.contains("end"));
        assert!(seq_set.contains("error"));
        let start = item["start"].as_str().expect("start time");
        let end = item["end"].as_str().expect("end time");
        assert!(start == env1.time || start == env2.time);
        assert!(end == env2.time || end == env3.time);
        assert_eq!(item["count"].as_u64(), Some(3));
        assert_eq!(item["errors"].as_u64(), Some(1));
        assert_eq!(item["last_kind"].as_str(), Some("tasks.failed"));
        assert_eq!(item["duration_ms"].as_u64(), Some(35));
        assert!(item["projects"]
            .as_array()
            .map(|arr| arr.contains(&json!("demo")))
            .unwrap_or(false));
        let items_arr = item["items"].as_array().expect("items array");
        assert_eq!(items_arr.len(), 3);
        assert_eq!(items_arr[2]["error"].as_bool(), Some(true));
    }

    #[tokio::test]
    async fn state_episodes_honors_if_none_match() {
        let temp = tempdir().expect("tempdir");
        let mut env_guard = crate::test_support::env::guard();
        let state = build_state(temp.path(), &mut env_guard).await;

        let corr = "run-etag";
        let env = arw_events::Envelope {
            time: Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true),
            kind: "tasks.started".to_string(),
            payload: json!({"corr_id": corr}),
            policy: None,
            ce: None,
        };
        state
            .kernel()
            .append_event_async(&env)
            .await
            .expect("append event");

        let first = state_episodes(
            HeaderMap::new(),
            Query(EpisodesQuery::default()),
            State(state.clone()),
        )
        .await
        .into_response();
        let etag = first
            .headers()
            .get(header::ETAG)
            .cloned()
            .expect("episodes etag");

        let mut headers = HeaderMap::new();
        headers.insert(header::IF_NONE_MATCH, etag);
        let response = state_episodes(headers, Query(EpisodesQuery::default()), State(state))
            .await
            .into_response();
        assert_eq!(response.status(), StatusCode::NOT_MODIFIED);
    }

    #[tokio::test]
    async fn state_episodes_supports_filters() {
        let temp = tempdir().expect("tempdir");
        let mut env_guard = crate::test_support::env::guard();
        let state = build_state(temp.path(), &mut env_guard).await;

        let t0 = Utc::now();
        let corr_demo = "run-demo";
        let corr_other = "run-other";

        let events = vec![
            arw_events::Envelope {
                time: t0.to_rfc3339_opts(SecondsFormat::Millis, true),
                kind: "tasks.started".to_string(),
                payload: json!({"corr_id": corr_demo, "step": "start", "proj": "demo"}),
                policy: None,
                ce: None,
            },
            arw_events::Envelope {
                time: (t0 + chrono::Duration::milliseconds(5))
                    .to_rfc3339_opts(SecondsFormat::Millis, true),
                kind: "tasks.failed".to_string(),
                payload: json!({"corr_id": corr_demo, "step": "error", "proj": "demo"}),
                policy: None,
                ce: None,
            },
            arw_events::Envelope {
                time: (t0 + chrono::Duration::milliseconds(10))
                    .to_rfc3339_opts(SecondsFormat::Millis, true),
                kind: "tasks.started".to_string(),
                payload: json!({"corr_id": corr_other, "step": "start", "proj": "other"}),
                policy: None,
                ce: None,
            },
            arw_events::Envelope {
                time: (t0 + chrono::Duration::milliseconds(15))
                    .to_rfc3339_opts(SecondsFormat::Millis, true),
                kind: "tasks.completed".to_string(),
                payload: json!({"corr_id": corr_other, "step": "end", "proj": "other"}),
                policy: None,
                ce: None,
            },
        ];

        for env in events {
            state
                .kernel()
                .append_event_async(&env)
                .await
                .expect("append event");
        }

        let since =
            (t0 + chrono::Duration::milliseconds(1)).to_rfc3339_opts(SecondsFormat::Millis, true);
        let query = EpisodesQuery {
            limit: Some(5),
            project: Some("demo".to_string()),
            errors_only: Some(true),
            kind_prefix: Some("tasks.".to_string()),
            since: Some(since),
        };

        let response = state_episodes(HeaderMap::new(), Query(query), State(state))
            .await
            .into_response();
        assert_eq!(response.status(), StatusCode::OK);
        let (_, body) = response.into_parts();
        let bytes = to_bytes(body, usize::MAX).await.expect("body");
        let value: Value = serde_json::from_slice(&bytes).expect("json");
        let items = value["items"].as_array().expect("items array");
        assert_eq!(items.len(), 1);
        assert_eq!(items[0]["id"].as_str(), Some(corr_demo));
        assert_eq!(items[0]["errors"].as_u64(), Some(1));
    }

    #[tokio::test]
    async fn state_episode_snapshot_returns_episode() {
        let temp = tempdir().expect("tempdir");
        let mut env_guard = crate::test_support::env::guard();
        let state = build_state(temp.path(), &mut env_guard).await;

        let corr = "snapshot-1";
        let envs = [
            arw_events::Envelope {
                time: Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true),
                kind: "tasks.started".to_string(),
                payload: json!({"corr_id": corr, "step": "start"}),
                policy: None,
                ce: None,
            },
            arw_events::Envelope {
                time: (Utc::now() + chrono::Duration::milliseconds(10))
                    .to_rfc3339_opts(SecondsFormat::Millis, true),
                kind: "tasks.completed".to_string(),
                payload: json!({"corr_id": corr, "step": "end"}),
                policy: None,
                ce: None,
            },
        ];

        for env in envs.iter() {
            state
                .kernel()
                .append_event_async(env)
                .await
                .expect("append event");
        }

        let response = state_episode_snapshot(
            HeaderMap::new(),
            Path(corr.to_string()),
            Query(EpisodeSnapshotQuery::default()),
            State(state.clone()),
        )
        .await
        .into_response();
        assert_eq!(response.status(), StatusCode::OK);
        let (parts, body) = response.into_parts();
        assert!(parts.headers.get(header::ETAG).is_some());
        let bytes = to_bytes(body, usize::MAX).await.expect("body");
        let value: Value = serde_json::from_slice(&bytes).expect("json");
        assert_eq!(value["version"].as_u64().unwrap_or_default(), 2);
        assert_eq!(value["episode"]["id"].as_str(), Some(corr));
        assert_eq!(
            value["episode"]["events"].as_array().map(|a| a.len()),
            Some(2)
        );

        // Not modified path
        let etag = parts.headers.get(header::ETAG).cloned().unwrap();
        let mut headers = HeaderMap::new();
        headers.insert(header::IF_NONE_MATCH, etag);
        let not_modified = state_episode_snapshot(
            headers,
            Path(corr.to_string()),
            Query(EpisodeSnapshotQuery::default()),
            State(state.clone()),
        )
        .await
        .into_response();
        assert_eq!(not_modified.status(), StatusCode::NOT_MODIFIED);
    }

    #[tokio::test]
    async fn state_episode_snapshot_missing_returns_404() {
        let temp = tempdir().expect("tempdir");
        let mut env_guard = crate::test_support::env::guard();
        let state = build_state(temp.path(), &mut env_guard).await;

        let response = state_episode_snapshot(
            HeaderMap::new(),
            Path("unknown".to_string()),
            Query(EpisodeSnapshotQuery::default()),
            State(state),
        )
        .await
        .into_response();
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }
}

/// Research watcher queue snapshot.
#[utoipa::path(
    get,
    path = "/state/research_watcher",
    tag = "State",
    params(
        ("status" = Option<String>, Query, description = "Filter by status"),
        ("limit" = Option<i64>, Query, description = "Max items (1-500)")
    ),
    responses(
        (status = 200, description = "Research watcher items", body = serde_json::Value),
        (status = 401, description = "Unauthorized"),
        (status = 501, description = "Kernel disabled", body = serde_json::Value)
    )
)]
pub async fn state_research_watcher(
    headers: HeaderMap,
    State(state): State<AppState>,
    Query(q): Query<std::collections::HashMap<String, String>>,
) -> impl IntoResponse {
    if !crate::admin_ok(&headers) {
        return (
            axum::http::StatusCode::UNAUTHORIZED,
            Json(json!({"type":"about:blank","title":"Unauthorized","status":401})),
        )
            .into_response();
    }
    if !state.kernel_enabled() {
        return crate::responses::kernel_disabled();
    }
    let status_filter = q.get("status").cloned();
    let limit = q
        .get("limit")
        .and_then(|s| s.parse::<i64>().ok())
        .unwrap_or(100);
    let items = state
        .kernel()
        .list_research_watcher_items_async(status_filter.clone(), limit)
        .await
        .unwrap_or_default();
    Json(json!({
        "items": items,
        "status": status_filter,
        "limit": limit
    }))
    .into_response()
}

/// Staging queue snapshot.
#[utoipa::path(
    get,
    path = "/state/staging/actions",
    tag = "State",
    params(
        ("status" = Option<String>, Query, description = "Filter by status"),
        ("limit" = Option<i64>, Query, description = "Max items (1-500)")
    ),
    responses(
        (status = 200, description = "Staging actions", body = serde_json::Value),
        (status = 401, description = "Unauthorized"),
        (status = 501, description = "Kernel disabled", body = serde_json::Value)
    )
)]
pub async fn state_staging_actions(
    headers: HeaderMap,
    State(state): State<AppState>,
    Query(q): Query<std::collections::HashMap<String, String>>,
) -> impl IntoResponse {
    if !crate::admin_ok(&headers) {
        return (
            axum::http::StatusCode::UNAUTHORIZED,
            Json(json!({"type":"about:blank","title":"Unauthorized","status":401})),
        )
            .into_response();
    }
    if !state.kernel_enabled() {
        return crate::responses::kernel_disabled();
    }
    let status_filter = q.get("status").cloned();
    let limit = q
        .get("limit")
        .and_then(|s| s.parse::<i64>().ok())
        .unwrap_or(100);
    let items = state
        .kernel()
        .list_staging_actions_async(status_filter.clone(), limit)
        .await
        .unwrap_or_default();
    let now = Utc::now();
    let generated = now.to_rfc3339_opts(SecondsFormat::Millis, true);
    let generated_ms = now.timestamp_millis();
    let generated_ms = if generated_ms < 0 {
        0
    } else {
        generated_ms as u64
    };
    Json(json!({
        "items": items,
        "status": status_filter,
        "limit": limit,
        "generated": generated,
        "generated_ms": generated_ms
    }))
    .into_response()
}

/// Training telemetry snapshot.
#[utoipa::path(
    get,
    path = "/state/training/telemetry",
    tag = "State",
    responses(
        (status = 200, description = "Training metrics", body = serde_json::Value),
        (status = 401, description = "Unauthorized")
    )
)]
pub async fn state_training_telemetry(
    headers: HeaderMap,
    State(state): State<AppState>,
) -> impl IntoResponse {
    if !crate::admin_ok(&headers) {
        return (
            axum::http::StatusCode::UNAUTHORIZED,
            Json(json!({"type":"about:blank","title":"Unauthorized","status":401})),
        )
            .into_response();
    }
    Json(training::telemetry_snapshot(&state).await).into_response()
}

/// Persistent logic unit action history.
#[utoipa::path(
    get,
    path = "/state/training/actions",
    tag = "State",
    params(
        ("limit" = Option<usize>, Query, description = "Items to return (1-500)", example = 50),
        ("offset" = Option<usize>, Query, description = "Items to skip from the newest entry", example = 0)
    ),
    responses(
        (status = 200, description = "Logic unit action history", body = serde_json::Value),
        (status = 401, description = "Unauthorized")
    )
)]
pub async fn state_training_actions(
    headers: HeaderMap,
    State(state): State<AppState>,
    Query(q): Query<std::collections::HashMap<String, String>>,
) -> impl IntoResponse {
    if !crate::admin_ok(&headers) {
        return (
            axum::http::StatusCode::UNAUTHORIZED,
            Json(json!({"type":"about:blank","title":"Unauthorized","status":401})),
        )
            .into_response();
    }
    let limit = q
        .get("limit")
        .and_then(|s| s.parse::<usize>().ok())
        .map(|n| n.clamp(1, 500))
        .unwrap_or(50);
    let offset = q
        .get("offset")
        .and_then(|s| s.parse::<usize>().ok())
        .unwrap_or(0);
    let (items, total) = state.logic_history().snapshot(offset, limit).await;
    Json(json!({
        "items": items,
        "total": total,
        "limit": limit,
        "offset": offset,
    }))
    .into_response()
}

/// Model catalog read-model.
#[utoipa::path(
    get,
    path = "/state/models",
    tag = "State",
    operation_id = "state_models_doc",
    description = "Model catalog read-model.",
    responses((status = 200, description = "Model catalog", body = ModelsCatalogResponse))
)]
pub async fn state_models(State(state): State<AppState>) -> impl IntoResponse {
    let items = state.models().list().await;
    Json(ModelsCatalogResponse { items })
}

/// Runtime matrix snapshot.
#[utoipa::path(
    get,
    path = "/state/runtime_matrix",
    tag = "State",
    responses(
        (status = 200, description = "Runtime matrix", body = RuntimeMatrixResponse),
        (status = 401, description = "Unauthorized", body = serde_json::Value)
    )
)]
pub async fn state_runtime_matrix(
    headers: HeaderMap,
    State(_state): State<AppState>,
) -> impl IntoResponse {
    if !crate::admin_ok(&headers) {
        return (
            axum::http::StatusCode::UNAUTHORIZED,
            Json(json!({"type":"about:blank","title":"Unauthorized","status":401})),
        )
            .into_response();
    }
    let items = runtime_matrix::snapshot().await;
    let items: BTreeMap<String, RuntimeMatrixEntry> = items.into_iter().collect();
    Json(RuntimeMatrixResponse {
        items,
        ttl_seconds: runtime_matrix::ttl_seconds(),
    })
    .into_response()
}

/// Runtime supervisor snapshot.
#[utoipa::path(
    get,
    path = "/state/runtime_supervisor",
    tag = "State",
    responses(
        (status = 200, description = "Runtime supervisor snapshot", body = serde_json::Value),
        (status = 401, description = "Unauthorized", body = serde_json::Value)
    )
)]
pub async fn state_runtime_supervisor(
    headers: HeaderMap,
    State(state): State<AppState>,
) -> impl IntoResponse {
    if !crate::admin_ok(&headers) {
        return (
            axum::http::StatusCode::UNAUTHORIZED,
            Json(json!({"type":"about:blank","title":"Unauthorized","status":401})),
        )
            .into_response();
    }
    let snapshot: arw_runtime::RegistrySnapshot = state.runtime().snapshot().await;
    Json(serde_json::to_value(snapshot).unwrap_or_else(|_| json!({"runtimes": []}))).into_response()
}

/// Self model index.
#[utoipa::path(
    get,
    path = "/state/self",
    tag = "State",
    responses((status = 200, description = "Agents list", body = serde_json::Value))
)]
pub async fn state_self_list() -> impl IntoResponse {
    let agents = self_model::list_agents().await;
    Json(json!({"agents": agents}))
}

/// Self model by id.
#[utoipa::path(
    get,
    path = "/state/self/{agent}",
    tag = "State",
    params(("agent" = String, Path, description = "Agent id")),
    responses(
        (status = 200, description = "Agent self model", body = serde_json::Value),
        (status = 404, description = "Not found")
    )
)]
pub async fn state_self_get(
    axum::extract::Path(agent): axum::extract::Path<String>,
) -> impl IntoResponse {
    match self_model::load(&agent).await {
        Ok(Some(v)) => (axum::http::StatusCode::OK, Json(v)),
        Ok(None) | Err(self_model::SelfModelError::InvalidAgent) => (
            axum::http::StatusCode::NOT_FOUND,
            Json(json!({"type":"about:blank","title":"Not Found","status":404})),
        ),
        Err(self_model::SelfModelError::Serde(_)) | Err(self_model::SelfModelError::Io(_)) => (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"type":"about:blank","title":"Error","status":500})),
        ),
        Err(_) => (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"type":"about:blank","title":"Error","status":500})),
        ),
    }
}
