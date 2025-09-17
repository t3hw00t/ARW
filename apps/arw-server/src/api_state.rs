use axum::response::IntoResponse;
use axum::{
    extract::{Query, State},
    Json,
};
use serde_json::{json, Value};

use crate::AppState;

/// Episode rollups grouped by correlation id.
#[utoipa::path(
    get,
    path = "/state/episodes",
    tag = "State",
    responses((status = 200, description = "Episode rollups", body = serde_json::Value))
)]
pub async fn state_episodes(State(state): State<AppState>) -> impl IntoResponse {
    // Simple episode rollup: group last 1000 events by corr_id
    let rows = state.kernel.recent_events(1000, None).unwrap_or_default();
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
    Json(json!({"items": items}))
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
    let metrics = state.metrics.snapshot();
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

/// Kernel contributions snapshot.
#[utoipa::path(
    get,
    path = "/state/contributions",
    tag = "State",
    responses((status = 200, description = "Contributions list", body = serde_json::Value))
)]
pub async fn state_contributions(State(state): State<AppState>) -> impl IntoResponse {
    let items = state.kernel.list_contributions(200).unwrap_or_default();
    Json(json!({"items": items}))
}

/// Recent actions list.
#[utoipa::path(
    get,
    path = "/state/actions",
    tag = "State",
    params(
        ("limit" = Option<i64>, Query, description = "Max items (1-2000)")
    ),
    responses((status = 200, description = "Actions list", body = serde_json::Value))
)]
pub async fn state_actions(
    State(state): State<AppState>,
    Query(q): Query<std::collections::HashMap<String, String>>,
) -> impl IntoResponse {
    let limit = q
        .get("limit")
        .and_then(|s| s.parse::<i64>().ok())
        .unwrap_or(200);
    let items = state
        .kernel
        .list_actions(limit.clamp(1, 2000))
        .unwrap_or_default();
    Json(json!({"items": items}))
}

/// Recent egress ledger list.
#[utoipa::path(
    get,
    path = "/state/egress",
    tag = "State",
    params(("limit" = Option<i64>, Query, description = "Max items (1-2000)")),
    responses((status = 200, description = "Egress ledger", body = serde_json::Value))
)]
pub async fn state_egress(
    State(state): State<AppState>,
    Query(q): Query<std::collections::HashMap<String, String>>,
) -> impl IntoResponse {
    let limit = q
        .get("limit")
        .and_then(|s| s.parse::<i64>().ok())
        .unwrap_or(200);
    let items = state
        .kernel
        .list_egress(limit.clamp(1, 2000))
        .unwrap_or_default();
    Json(json!({"items": items}))
}

/// Model catalog read‑model.
#[utoipa::path(
    get,
    path = "/state/models",
    tag = "State",
    responses((status = 200, description = "Model catalog", body = serde_json::Value))
)]
pub async fn state_models() -> impl IntoResponse {
    use tokio::fs as afs;
    let path = crate::util::state_dir().join("models.json");
    let items: Vec<Value> = match afs::read(&path).await {
        Ok(bytes) => serde_json::from_slice(&bytes)
            .ok()
            .and_then(|v: Value| v.as_array().cloned())
            .unwrap_or_else(crate::util::default_models),
        Err(_) => crate::util::default_models(),
    };
    Json(json!({"items": items}))
}

/// Self model index.
#[utoipa::path(
    get,
    path = "/state/self",
    tag = "State",
    responses((status = 200, description = "Agents list", body = serde_json::Value))
)]
pub async fn state_self_list() -> impl IntoResponse {
    use tokio::fs as afs;
    let dir = crate::util::state_dir().join("self");
    let mut agents: Vec<String> = Vec::new();
    if let Ok(mut rd) = afs::read_dir(&dir).await {
        while let Ok(Some(ent)) = rd.next_entry().await {
            if let Some(name) = ent.file_name().to_str() {
                if name.ends_with(".json") {
                    agents.push(name.trim_end_matches(".json").to_string());
                }
            }
        }
    }
    agents.sort();
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
    use tokio::fs as afs;
    let path = crate::util::state_dir()
        .join("self")
        .join(format!("{}.json", agent));
    match afs::read(&path).await {
        Ok(bytes) => match serde_json::from_slice::<Value>(&bytes) {
            Ok(v) => (axum::http::StatusCode::OK, Json(v)),
            Err(_) => (
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"type":"about:blank","title":"Error","status":500})),
            ),
        },
        Err(_) => (
            axum::http::StatusCode::NOT_FOUND,
            Json(json!({"type":"about:blank","title":"Not Found","status":404})),
        ),
    }
}
