use axum::http::HeaderMap;
use axum::response::IntoResponse;
use axum::{
    extract::{Query, State},
    Json,
};
use serde_json::{json, Value};
use std::collections::BTreeMap;

use crate::{
    metrics,
    runtime_matrix::{self, RuntimeMatrixEntry},
    self_model, state_observer, training, world, AppState,
};
use chrono::{SecondsFormat, Utc};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

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

pub(crate) async fn build_episode_rollups(state: &AppState, limit: usize) -> (Vec<Value>, u64) {
    let rows = state
        .kernel()
        .recent_events_async(limit as i64, None)
        .await
        .unwrap_or_default();
    let mut by_corr: BTreeMap<String, Vec<Value>> = BTreeMap::new();
    let mut max_id: u64 = 0;
    for r in rows {
        let corr_id = r.corr_id.unwrap_or_default();
        if corr_id.is_empty() {
            continue;
        }
        if r.id > 0 {
            max_id = max_id.max(r.id as u64);
        }
        by_corr.entry(corr_id).or_default().push(json!({
            "id": r.id,
            "time": r.time,
            "kind": r.kind,
            "payload": r.payload,
        }));
    }
    let mut items: Vec<Value> = Vec::new();
    for (cid, evs) in by_corr.into_iter() {
        let start = evs
            .first()
            .and_then(|e| e.get("time").cloned())
            .unwrap_or(Value::Null);
        let end = evs
            .last()
            .and_then(|e| e.get("time").cloned())
            .unwrap_or(Value::Null);
        items.push(json!({
            "id": cid,
            "events": evs,
            "start": start,
            "end": end,
        }));
    }
    (items, max_id)
}

/// Episode rollups grouped by correlation id.
#[utoipa::path(
    get,
    path = "/state/episodes",
    tag = "State",
    responses(
        (status = 200, description = "Episode rollups", body = serde_json::Value),
        (status = 501, description = "Kernel disabled", body = serde_json::Value)
    )
)]
pub async fn state_episodes(
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
    let (items, version) = build_episode_rollups(&state, 1000).await;
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
    responses(
        (status = 200, description = "Recent observations", body = serde_json::Value),
        (status = 401, description = "Unauthorized", body = serde_json::Value)
    )
)]
pub async fn state_observations(headers: HeaderMap) -> impl IntoResponse {
    if !crate::admin_ok(&headers) {
        return (
            axum::http::StatusCode::UNAUTHORIZED,
            Json(json!({"type":"about:blank","title":"Unauthorized","status":401})),
        )
            .into_response();
    }
    let (version, items) = state_observer::observations_snapshot().await;
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
    params(
        ("limit" = Option<i64>, Query, description = "Max items (1-2000)")
    ),
    responses(
        (status = 200, description = "Actions list", body = serde_json::Value),
        (status = 501, description = "Kernel disabled", body = serde_json::Value)
    )
)]
pub async fn state_actions(
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
    let version = crate::state_observer::actions_version_value();
    if let Some(resp) =
        crate::api::http_utils::state_version_not_modified(&headers, "actions", version)
    {
        return resp;
    }
    let items = state
        .kernel()
        .list_actions_async(limit.clamp(1, 2000))
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
    use chrono::{DateTime, SecondsFormat, Utc};
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

        let params: HashMap<String, String> = HashMap::new();
        let response = state_actions(HeaderMap::new(), State(state.clone()), Query(params)).await;
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

        let params: HashMap<String, String> = HashMap::new();
        let first = state_actions(
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
            .unwrap_or_else(|| HeaderValue::from_static("\"state-actions-v0\""));

        let mut headers = HeaderMap::new();
        headers.insert(header::IF_NONE_MATCH, etag);
        let response = state_actions(headers, State(state), Query(params))
            .await
            .into_response();
        assert_eq!(response.status(), StatusCode::NOT_MODIFIED);
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

        let first = state_observations(HeaderMap::new()).await.into_response();
        let etag = first
            .headers()
            .get(header::ETAG)
            .cloned()
            .expect("observations etag");

        let mut headers = HeaderMap::new();
        headers.insert(header::IF_NONE_MATCH, etag);
        let response = state_observations(headers).await.into_response();
        assert_eq!(response.status(), StatusCode::NOT_MODIFIED);
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
            payload: json!({"corr_id": corr, "step": "start"}),
            policy: None,
            ce: None,
        };
        let env2 = arw_events::Envelope {
            time: t2.to_rfc3339_opts(SecondsFormat::Millis, true),
            kind: "tasks.completed".to_string(),
            payload: json!({"corr_id": corr, "step": "end"}),
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

        let response = state_episodes(HeaderMap::new(), State(state.clone()))
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
        assert_eq!(events.len(), 2);
        let seq_set: std::collections::HashSet<_> = events
            .iter()
            .map(|ev| ev["payload"]["step"].as_str().unwrap_or(""))
            .collect();
        assert!(seq_set.contains("start"));
        assert!(seq_set.contains("end"));
        let start = item["start"].as_str().expect("start time");
        let end = item["end"].as_str().expect("end time");
        assert!(start == env1.time || start == env2.time);
        assert!(end == env1.time || end == env2.time);
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

        let first = state_episodes(HeaderMap::new(), State(state.clone()))
            .await
            .into_response();
        let etag = first
            .headers()
            .get(header::ETAG)
            .cloned()
            .expect("episodes etag");

        let mut headers = HeaderMap::new();
        headers.insert(header::IF_NONE_MATCH, etag);
        let response = state_episodes(headers, State(state)).await.into_response();
        assert_eq!(response.status(), StatusCode::NOT_MODIFIED);
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
