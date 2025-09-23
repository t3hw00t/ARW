use axum::http::HeaderMap;
use axum::response::IntoResponse;
use axum::{
    extract::{Query, State},
    Json,
};
use serde_json::{json, Value};

use crate::{runtime_matrix, self_model, state_observer, training, world, AppState};
use serde::Deserialize;

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
    // Simple episode rollup: group last 1000 events by corr_id
    let rows = state
        .kernel()
        .recent_events_async(1000, None)
        .await
        .unwrap_or_default();
    use std::collections::BTreeMap;
    let mut by_corr: BTreeMap<String, Vec<Value>> = BTreeMap::new();
    for r in rows {
        let cid = r.corr_id.unwrap_or_else(|| "".to_string());
        if cid.is_empty() {
            continue;
        }
        by_corr
            .entry(cid)
            .or_default()
            .push(json!({"id": r.id, "time": r.time, "kind": r.kind, "payload": r.payload}));
    }
    let mut items: Vec<Value> = Vec::new();
    for (cid, evs) in by_corr.into_iter() {
        let start = evs
            .first()
            .and_then(|e| e.get("time").cloned())
            .unwrap_or(json!(null));
        let end = evs
            .last()
            .and_then(|e| e.get("time").cloned())
            .unwrap_or(json!(null));
        items.push(json!({"id": cid, "events": evs, "start": start, "end": end}));
    }
    Json(json!({"items": items})).into_response()
}

/// Bus and per-route counters snapshot.
#[utoipa::path(
    get,
    path = "/state/route_stats",
    tag = "State",
    responses((status = 200, description = "Route stats", body = serde_json::Value))
)]
pub async fn state_route_stats(State(state): State<AppState>) -> impl IntoResponse {
    let bus = state.bus().stats();
    let metrics = state.metrics().snapshot();
    Json(json!({
        "bus": {
            "published": bus.published,
            "delivered": bus.delivered,
            "receivers": bus.receivers,
            "lagged": bus.lagged,
            "no_receivers": bus.no_receivers
        },
        "events": metrics.events,
        "routes": metrics.routes,
        "tasks": metrics.tasks
    }))
}

/// Background tasks status snapshot.
#[utoipa::path(
    get,
    path = "/state/tasks",
    tag = "State",
    responses((status = 200, description = "Background tasks", body = serde_json::Value))
)]
pub async fn state_tasks(State(state): State<AppState>) -> impl IntoResponse {
    let tasks = state.metrics().tasks_snapshot();
    Json(json!({ "tasks": tasks }))
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
    let (version, items) = state_observer::observations_snapshot();
    Json(json!({"version": version, "items": items})).into_response()
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
    let (version, items) = state_observer::beliefs_snapshot();
    Json(json!({"version": version, "items": items})).into_response()
}

/// Recent intents stream (rolling window).
#[utoipa::path(
    get,
    path = "/state/intents",
    tag = "State",
    operation_id = "state_intents_doc",
    description = "Recent intents stream (rolling window).",
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
    Json(json!({"items": state_observer::intents_snapshot()})).into_response()
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
    Json(json!({"nodes": nodes})).into_response()
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
    let map = world::snapshot_project_map(q.proj.as_deref());
    Json(serde_json::to_value(map).unwrap_or_else(|_| json!({}))).into_response()
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
    let items = world::select_top_claims_diverse(q.proj.as_deref(), &query, k, lambda);
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
    Json(json!({"items": items})).into_response()
}

/// Experiment events snapshot (public read-model).
#[utoipa::path(
    get,
    path = "/state/experiments",
    tag = "State",
    responses((status = 200, description = "Experiment events", body = serde_json::Value))
)]
pub async fn state_experiments(State(state): State<AppState>) -> impl IntoResponse {
    let items = state.experiments().state_events().await;
    Json(json!({"items": items})).into_response()
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
    let items = state
        .kernel()
        .list_actions_async(limit.clamp(1, 2000))
        .await
        .unwrap_or_default();
    let items: Vec<Value> = items
        .into_iter()
        .map(crate::api::actions::sanitize_action_record)
        .collect();
    Json(json!({"items": items})).into_response()
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
    let settings = crate::api::egress_settings::current_settings();
    Json(json!({
        "count": count,
        "items": items,
        "settings": settings,
    }))
    .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;
    use arw_policy::PolicyEngine;
    use axum::{body::to_bytes, http::StatusCode};
    use serde_json::Value;
    use std::collections::HashMap;
    use std::sync::Arc;
    use tempfile::tempdir;
    use tokio::sync::Mutex;

    async fn build_state(path: &std::path::Path) -> AppState {
        std::env::set_var("ARW_STATE_DIR", path.display().to_string());
        let bus = arw_events::Bus::new_with_replay(16, 16);
        let kernel = arw_kernel::Kernel::open(path).expect("init kernel");
        let policy = PolicyEngine::load_from_env();
        let policy_arc = Arc::new(Mutex::new(policy));
        let host: Arc<dyn arw_wasi::ToolHost> = Arc::new(arw_wasi::NoopHost);
        AppState::builder(bus, kernel, policy_arc, host, true)
            .with_sse_capacity(16)
            .build()
            .await
    }

    #[tokio::test]
    async fn state_actions_sanitizes_guard_metadata() {
        let temp = tempdir().expect("tempdir");
        let state = build_state(temp.path()).await;

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

        let params: HashMap<String, String> = HashMap::new();
        let response = state_actions(HeaderMap::new(), State(state.clone()), Query(params)).await;
        let (parts, body) = response.into_response().into_parts();
        assert_eq!(parts.status, StatusCode::OK);
        let bytes = to_bytes(body, usize::MAX).await.expect("body bytes");
        let value: Value = serde_json::from_slice(&bytes).expect("json");
        let items = value["items"].as_array().expect("items array");
        assert_eq!(items.len(), 1);
        let item = &items[0];
        assert_eq!(item["id"].as_str(), Some(action_id.as_str()));
        assert!(item["output"].is_null());
        assert!(item.get("guard").is_none());
        assert!(item.get("posture").is_none());
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
    Json(json!({
        "items": items,
        "status": status_filter,
        "limit": limit
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
    Json(training::telemetry_snapshot(&state)).into_response()
}

/// Model catalog read-model.
#[utoipa::path(
    get,
    path = "/state/models",
    tag = "State",
    operation_id = "state_models_doc",
    description = "Model catalog read-model.",
    responses((status = 200, description = "Model catalog", body = serde_json::Value))
)]
pub async fn state_models(State(state): State<AppState>) -> impl IntoResponse {
    let items = state.models().list().await;
    Json(json!({"items": items}))
}

/// Runtime matrix snapshot.
#[utoipa::path(
    get,
    path = "/state/runtime_matrix",
    tag = "State",
    responses(
        (status = 200, description = "Runtime matrix", body = serde_json::Value),
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
    let items = runtime_matrix::snapshot();
    Json(json!({"items": items})).into_response()
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
