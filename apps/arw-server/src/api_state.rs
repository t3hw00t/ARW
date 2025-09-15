use axum::{extract::{State, Query}, Json};
use axum::response::IntoResponse;
use serde_json::{json, Value};

use crate::AppState;

pub async fn state_episodes(State(state): State<AppState>) -> impl IntoResponse {
    // Simple episode rollup: group last 1000 events by corr_id
    let rows = state.kernel.recent_events(1000, None).unwrap_or_default();
    use std::collections::BTreeMap;
    let mut by_corr: BTreeMap<String, Vec<Value>> = BTreeMap::new();
    for r in rows {
        let cid = r.corr_id.unwrap_or_else(|| "".to_string());
        if cid.is_empty() { continue; }
        by_corr.entry(cid).or_default().push(json!({"id": r.id, "time": r.time, "kind": r.kind, "payload": r.payload}));
    }
    let mut items: Vec<Value> = Vec::new();
    for (cid, evs) in by_corr.into_iter() {
        let start = evs.first().and_then(|e| e.get("time").cloned()).unwrap_or(json!(null));
        let end = evs.last().and_then(|e| e.get("time").cloned()).unwrap_or(json!(null));
        items.push(json!({"id": cid, "events": evs, "start": start, "end": end}));
    }
    Json(json!({"items": items}))
}

pub async fn state_route_stats(State(state): State<AppState>) -> impl IntoResponse {
    let s = state.bus.stats();
    Json(json!({
        "published": s.published,
        "delivered": s.delivered,
        "lagged": s.lagged,
        "no_receivers": s.no_receivers,
        "receivers": s.receivers
    }))
}

pub async fn state_contributions(State(state): State<AppState>) -> impl IntoResponse {
    let items = state.kernel.list_contributions(200).unwrap_or_default();
    Json(json!({"items": items}))
}

pub async fn state_actions(State(state): State<AppState>, Query(q): Query<std::collections::HashMap<String, String>>) -> impl IntoResponse {
    let limit = q.get("limit").and_then(|s| s.parse::<i64>().ok()).unwrap_or(200);
    let items = state.kernel.list_actions(limit.max(1).min(2000)).unwrap_or_default();
    Json(json!({"items": items}))
}

pub async fn state_egress(State(state): State<AppState>, Query(q): Query<std::collections::HashMap<String, String>>) -> impl IntoResponse {
    let limit = q.get("limit").and_then(|s| s.parse::<i64>().ok()).unwrap_or(200);
    let items = state.kernel.list_egress(limit.max(1).min(2000)).unwrap_or_default();
    Json(json!({"items": items}))
}

pub async fn state_models() -> impl IntoResponse {
    use tokio::fs as afs;
    let path = crate::state_dir().join("models.json");
    let items: Vec<Value> = match afs::read(&path).await {
        Ok(bytes) => serde_json::from_slice(&bytes).ok().and_then(|v: Value| v.as_array().cloned()).unwrap_or_else(crate::util::default_models),
        Err(_) => crate::util::default_models(),
    };
    Json(json!({"items": items}))
}

pub async fn state_self_list() -> impl IntoResponse {
    use tokio::fs as afs;
    let dir = crate::state_dir().join("self");
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

pub async fn state_self_get(axum::extract::Path(agent): axum::extract::Path<String>) -> impl IntoResponse {
    use tokio::fs as afs;
    let path = crate::state_dir().join("self").join(format!("{}.json", agent));
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
