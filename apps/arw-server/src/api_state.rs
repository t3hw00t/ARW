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
    let bus = state.bus.stats();
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
        "routes": metrics.routes
    }))
}

/// Legacy admin alias for route stats (`/admin/state/route_stats`).
pub async fn state_route_stats_admin(
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
    state_route_stats(State(state)).await.into_response()
}

/// Recent observations from the event bus.
#[utoipa::path(
    get,
    path = "/admin/state/observations",
    tag = "Admin/State",
    responses(
        (status = 200, description = "Recent observations", body = serde_json::Value),
        (status = 401, description = "Unauthorized", body = serde_json::Value)
    )
)]
pub async fn admin_state_observations(headers: HeaderMap) -> impl IntoResponse {
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
    path = "/admin/state/beliefs",
    tag = "Admin/State",
    responses(
        (status = 200, description = "Beliefs snapshot", body = serde_json::Value),
        (status = 401, description = "Unauthorized", body = serde_json::Value)
    )
)]
pub async fn admin_state_beliefs(headers: HeaderMap) -> impl IntoResponse {
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
    path = "/admin/state/intents",
    tag = "Admin/State",
    responses(
        (status = 200, description = "Recent intents", body = serde_json::Value),
        (status = 401, description = "Unauthorized", body = serde_json::Value)
    )
)]
pub async fn admin_state_intents(headers: HeaderMap) -> impl IntoResponse {
    if !crate::admin_ok(&headers) {
        return (
            axum::http::StatusCode::UNAUTHORIZED,
            Json(json!({"type":"about:blank","title":"Unauthorized","status":401})),
        )
            .into_response();
    }
    Json(json!({"items": state_observer::intents_snapshot()})).into_response()
}

/// Recent actions stream (rolling window).
#[utoipa::path(
    get,
    path = "/admin/state/actions",
    tag = "Admin/State",
    responses(
        (status = 200, description = "Recent actions", body = serde_json::Value),
        (status = 401, description = "Unauthorized", body = serde_json::Value)
    )
)]
pub async fn admin_state_actions(headers: HeaderMap) -> impl IntoResponse {
    if !crate::admin_ok(&headers) {
        return (
            axum::http::StatusCode::UNAUTHORIZED,
            Json(json!({"type":"about:blank","title":"Unauthorized","status":401})),
        )
            .into_response();
    }
    Json(json!({"items": state_observer::actions_snapshot()})).into_response()
}

#[utoipa::path(
    get,
    path = "/admin/state/guardrails_metrics",
    tag = "Admin/State",
    responses(
        (status = 200, description = "Guardrails metrics", body = serde_json::Value),
        (status = 401, description = "Unauthorized", body = serde_json::Value)
    )
)]
pub async fn admin_state_guardrails_metrics(headers: HeaderMap) -> impl IntoResponse {
    if !crate::admin_ok(&headers) {
        return (
            axum::http::StatusCode::UNAUTHORIZED,
            Json(json!({"type":"about:blank","title":"Unauthorized","status":401})),
        )
            .into_response();
    }
    Json(crate::tools::guardrails_metrics_value()).into_response()
}

/// Cluster nodes snapshot (admin-only).
#[utoipa::path(
    get,
    path = "/admin/state/cluster",
    tag = "Admin/State",
    responses(
        (status = 200, description = "Cluster nodes", body = serde_json::Value),
        (status = 401, description = "Unauthorized", body = serde_json::Value)
    )
)]
pub async fn admin_state_cluster(
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
    path = "/admin/state/world",
    tag = "Admin/State",
    params(("proj" = Option<String>, Query, description = "Project id")),
    responses(
        (status = 200, description = "World model", body = serde_json::Value),
        (status = 401, description = "Unauthorized", body = serde_json::Value)
    )
)]
pub async fn admin_state_world(
    headers: HeaderMap,
    Query(q): Query<WorldQuery>,
) -> impl IntoResponse {
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
    path = "/admin/state/world/select",
    tag = "Admin/State",
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
pub async fn admin_state_world_select(
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
    Json(json!({"items": items})).into_response()
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
