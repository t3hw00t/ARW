use axum::response::IntoResponse;
use axum::{
    extract::{Query, State},
    Json,
};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::AppState;

#[derive(Deserialize)]
pub(crate) struct MemPutReq {
    pub lane: String,
    #[serde(default)]
    pub kind: Option<String>,
    #[serde(default)]
    pub key: Option<String>,
    pub value: Value,
    #[serde(default)]
    pub embed: Option<Vec<f32>>,
    #[serde(default)]
    pub tags: Option<Vec<String>>,
    #[serde(default)]
    pub score: Option<f64>,
    #[serde(default)]
    pub prob: Option<f64>,
}
pub async fn memory_put(
    State(state): State<AppState>,
    Json(req): Json<MemPutReq>,
) -> impl IntoResponse {
    match state.kernel.insert_memory(
        None,
        &req.lane,
        req.kind.as_deref(),
        req.key.as_deref(),
        &req.value,
        req.embed.as_deref().map(|v| v.as_ref()),
        req.tags.as_deref(),
        req.score,
        req.prob,
    ) {
        Ok(id) => {
            state.bus.publish(
                "memory.record.put",
                &json!({"id": id, "lane": req.lane, "kind": req.kind, "key": req.key}),
            );
            (axum::http::StatusCode::CREATED, Json(json!({"id": id })))
        }
        Err(e) => (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            Json(
                json!({"type":"about:blank","title":"Error","status":500, "detail": e.to_string()}),
            ),
        ),
    }
}

pub async fn state_memory_select(
    State(state): State<AppState>,
    Query(q): Query<std::collections::HashMap<String, String>>,
) -> impl IntoResponse {
    let query = q.get("q").cloned().unwrap_or_default();
    let lane = q.get("lane").map(|s| s.as_str());
    let limit = q
        .get("limit")
        .and_then(|s| s.parse::<i64>().ok())
        .unwrap_or(50);
    let mode = q.get("mode").map(|s| s.as_str()).unwrap_or("like");
    let res = if mode == "fts" {
        state.kernel.fts_search_memory(&query, lane, limit)
    } else {
        state.kernel.search_memory(&query, lane, limit)
    };
    match res {
        Ok(items) => Json(json!({"items": items, "mode": mode})),
        Err(e) => Json(json!({"items": [], "error": e.to_string()})),
    }
}

#[derive(Deserialize)]
pub(crate) struct MemEmbedReq {
    pub embed: Vec<f32>,
    #[serde(default)]
    pub lane: Option<String>,
    #[serde(default)]
    pub limit: Option<i64>,
}
pub async fn memory_search_embed(
    State(state): State<AppState>,
    Json(req): Json<MemEmbedReq>,
) -> impl IntoResponse {
    let lane_opt = req.lane.as_deref();
    let limit = req.limit.unwrap_or(20);
    let res = state
        .kernel
        .search_memory_by_embedding(&req.embed, lane_opt, limit);
    match res {
        Ok(items) => (
            axum::http::StatusCode::OK,
            Json(json!({"items": items, "mode": "embed"})),
        )
            .into_response(),
        Err(e) => (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            Json(
                json!({"type":"about:blank","title":"Error","status":500, "detail": e.to_string()}),
            ),
        )
            .into_response(),
    }
}

#[derive(Deserialize)]
pub(crate) struct MemHybridReq {
    #[serde(default)]
    pub q: Option<String>,
    #[serde(default)]
    pub embed: Option<Vec<f32>>,
    #[serde(default)]
    pub lane: Option<String>,
    #[serde(default)]
    pub limit: Option<i64>,
}
pub async fn memory_select_hybrid(
    State(state): State<AppState>,
    Json(req): Json<MemHybridReq>,
) -> impl IntoResponse {
    let lane_opt = req.lane.as_deref();
    let limit = req.limit.unwrap_or(20);
    let res = state.kernel.select_memory_hybrid(
        req.q.as_deref(),
        req.embed.as_deref().map(|v| v.as_ref()),
        lane_opt,
        limit,
    );
    match res {
        Ok(items) => (
            axum::http::StatusCode::OK,
            Json(json!({"items": items, "mode": "hybrid"})),
        )
            .into_response(),
        Err(e) => (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            Json(
                json!({"type":"about:blank","title":"Error","status":500, "detail": e.to_string()}),
            ),
        )
            .into_response(),
    }
}

#[derive(Deserialize)]
pub(crate) struct MemCoherentReq {
    #[serde(default)]
    pub q: Option<String>,
    #[serde(default)]
    pub embed: Option<Vec<f32>>,
    #[serde(default)]
    pub lane: Option<String>,
    #[serde(default)]
    pub limit: Option<i64>,
    #[serde(default)]
    pub expand_per_seed: Option<i64>,
}
pub async fn memory_select_coherent(
    State(state): State<AppState>,
    Json(req): Json<MemCoherentReq>,
) -> impl IntoResponse {
    let lane_opt = req.lane.as_deref();
    let limit = req.limit.unwrap_or(30);
    let expand_n = req.expand_per_seed.unwrap_or(3).max(0).min(10);
    let seeds = match state.kernel.select_memory_hybrid(
        req.q.as_deref(),
        req.embed.as_deref().map(|v| v.as_ref()),
        lane_opt,
        (limit / 2).max(1),
    ) {
        Ok(items) => items,
        Err(e) => return (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            Json(
                json!({"type":"about:blank","title":"Error","status":500, "detail": e.to_string()}),
            ),
        )
            .into_response(),
    };
    use std::collections::HashSet;
    let mut seen: HashSet<String> = HashSet::new();
    let mut scored: Vec<(f32, Value)> = Vec::new();
    // Seed scores
    for it in seeds.iter() {
        let id = it
            .get("id")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        if id.is_empty() {
            continue;
        }
        seen.insert(id.clone());
        let sc = it.get("cscore").and_then(|v| v.as_f64()).unwrap_or(0.5) as f32;
        scored.push((sc, it.clone()));
        // Expand links
        if expand_n > 0 {
            if let Ok(links) = state.kernel.list_memory_links(&id, expand_n) {
                for lk in links {
                    let dst_id = lk.get("dst_id").and_then(|v| v.as_str()).unwrap_or("");
                    if dst_id.is_empty() {
                        continue;
                    }
                    if seen.contains(dst_id) {
                        continue;
                    }
                    if let Ok(Some(mut rec)) = state.kernel.get_memory(dst_id) {
                        let weight =
                            lk.get("weight").and_then(|v| v.as_f64()).unwrap_or(1.0) as f32;
                        // recency score (same as hybrid's component)
                        let now = chrono::Utc::now();
                        let recency = rec
                            .get("updated")
                            .and_then(|v| v.as_str())
                            .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
                            .map(|t| {
                                let age = now
                                    .signed_duration_since(t.with_timezone(&chrono::Utc))
                                    .num_seconds()
                                    .max(0) as f64;
                                let hl = 3600f64 * 6f64;
                                ((-age / hl).exp()) as f32
                            })
                            .unwrap_or(0.5);
                        let cscore = 0.5 * sc + 0.3 * weight + 0.2 * recency;
                        if let Some(obj) = rec.as_object_mut() {
                            obj.insert("cscore".into(), json!(cscore));
                        }
                        seen.insert(dst_id.to_string());
                        scored.push((cscore, rec));
                    }
                }
            }
        }
    }
    // Sort and take top limit with light diversity filter (MMR-lite)
    scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
    let mut items: Vec<Value> = Vec::new();
    for (_, v) in scored.into_iter() {
        if (items.len() as i64) >= limit {
            break;
        }
        let k_new = v.get("key").and_then(|x| x.as_str()).unwrap_or("");
        let tags_new: std::collections::HashSet<&str> = v
            .get("tags")
            .and_then(|x| x.as_str())
            .unwrap_or("")
            .split(',')
            .filter(|s| !s.is_empty())
            .collect();
        let too_similar = items.iter().any(|e| {
            let k_old = e.get("key").and_then(|x| x.as_str()).unwrap_or("");
            if !k_old.is_empty() && k_old == k_new {
                return true;
            }
            let tags_old: std::collections::HashSet<&str> = e
                .get("tags")
                .and_then(|x| x.as_str())
                .unwrap_or("")
                .split(',')
                .filter(|s| !s.is_empty())
                .collect();
            let inter = tags_old.intersection(&tags_new).count();
            inter >= 3
        });
        if !too_similar {
            items.push(v);
        }
    }
    (
        axum::http::StatusCode::OK,
        Json(json!({"items": items, "mode": "coherent"})),
    )
        .into_response()
}

pub async fn state_memory_recent(
    State(state): State<AppState>,
    Query(q): Query<std::collections::HashMap<String, String>>,
) -> impl IntoResponse {
    let lane = q.get("lane").map(|s| s.as_str());
    let limit = q
        .get("limit")
        .and_then(|s| s.parse::<i64>().ok())
        .unwrap_or(100);
    match state.kernel.list_recent_memory(lane, limit) {
        Ok(items) => (axum::http::StatusCode::OK, Json(json!({"items": items}))).into_response(),
        Err(e) => (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            Json(
                json!({"type":"about:blank","title":"Error","status":500, "detail": e.to_string()}),
            ),
        )
            .into_response(),
    }
}

#[derive(Deserialize)]
pub(crate) struct MemLinkReq {
    pub src_id: String,
    pub dst_id: String,
    #[serde(default)]
    pub rel: Option<String>,
    #[serde(default)]
    pub weight: Option<f64>,
}
pub async fn memory_link_put(
    State(state): State<AppState>,
    Json(req): Json<MemLinkReq>,
) -> impl IntoResponse {
    match state
        .kernel
        .insert_memory_link(&req.src_id, &req.dst_id, req.rel.as_deref(), req.weight)
    {
        Ok(()) => {
            state.bus.publish("memory.link.put", &json!({"src_id": req.src_id, "dst_id": req.dst_id, "rel": req.rel, "weight": req.weight}));
            (axum::http::StatusCode::CREATED, Json(json!({"ok": true}))).into_response()
        }
        Err(e) => (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            Json(
                json!({"type":"about:blank","title":"Error","status":500, "detail": e.to_string()}),
            ),
        )
            .into_response(),
    }
}

pub async fn state_memory_links(
    State(state): State<AppState>,
    Query(q): Query<std::collections::HashMap<String, String>>,
) -> impl IntoResponse {
    let src_id = match q.get("id").cloned() { Some(v) => v, None => return (axum::http::StatusCode::BAD_REQUEST, Json(json!({"type":"about:blank","title":"Bad Request","status":400, "detail":"missing id"}))).into_response() };
    let limit = q
        .get("limit")
        .and_then(|s| s.parse::<i64>().ok())
        .unwrap_or(50);
    match state.kernel.list_memory_links(&src_id, limit) {
        Ok(items) => (axum::http::StatusCode::OK, Json(json!({"items": items}))).into_response(),
        Err(e) => (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            Json(
                json!({"type":"about:blank","title":"Error","status":500, "detail": e.to_string()}),
            ),
        )
            .into_response(),
    }
}

pub async fn memory_explain_coherent(
    State(state): State<AppState>,
    Json(req): Json<MemCoherentReq>,
) -> impl IntoResponse {
    let lane_opt = req.lane.as_deref();
    let limit = req.limit.unwrap_or(30);
    let expand_n = req.expand_per_seed.unwrap_or(3).max(0).min(10);
    let seeds = match state.kernel.select_memory_hybrid(
        req.q.as_deref(),
        req.embed.as_deref().map(|v| v.as_ref()),
        lane_opt,
        (limit / 2).max(1),
    ) {
        Ok(items) => items,
        Err(e) => return (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            Json(
                json!({"type":"about:blank","title":"Error","status":500, "detail": e.to_string()}),
            ),
        )
            .into_response(),
    };
    use std::collections::HashSet;
    let mut seen: HashSet<String> = HashSet::new();
    let mut scored: Vec<(f32, Value)> = Vec::new();
    let now = chrono::Utc::now();
    // Seeds with explain
    for mut it in seeds.clone() {
        let id = it
            .get("id")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        if id.is_empty() {
            continue;
        }
        seen.insert(id.clone());
        let sim = it.get("sim").and_then(|v| v.as_f64()).unwrap_or(0.0) as f32;
        let fts = it
            .get("_fts_hit")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let recency = it
            .get("updated")
            .and_then(|v| v.as_str())
            .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
            .map(|t| {
                let age = now
                    .signed_duration_since(t.with_timezone(&chrono::Utc))
                    .num_seconds()
                    .max(0) as f64;
                let hl = 3600f64 * 6f64; // 6h half-life
                ((-age / hl).exp()) as f32
            })
            .unwrap_or(0.5);
        let util = it
            .get("score")
            .and_then(|v| v.as_f64())
            .map(|s| s.max(0.0).min(1.0) as f32)
            .unwrap_or(0.0);
        let w_sim = 0.5f32;
        let w_fts = 0.2f32;
        let w_rec = 0.2f32;
        let w_util = 0.1f32;
        let fts_score = if fts { 1.0 } else { 0.0 };
        let cscore = w_sim * sim + w_fts * fts_score + w_rec * recency + w_util * util;
        if let Some(obj) = it.as_object_mut() {
            obj.insert("cscore".into(), json!(cscore));
            obj.insert(
                "explain".into(),
                json!({"sim": sim, "fts": fts, "recency": recency, "utility": util}),
            );
        }
        scored.push((cscore, it));
    }
    // Expand coherent set
    for it in seeds.iter() {
        let id = it.get("id").and_then(|v| v.as_str()).unwrap_or("");
        if id.is_empty() {
            continue;
        }
        if let Ok(links) = state.kernel.list_memory_links(id, expand_n) {
            for lk in links {
                let dst_id = lk.get("dst_id").and_then(|v| v.as_str()).unwrap_or("");
                if dst_id.is_empty() || seen.contains(dst_id) {
                    continue;
                }
                if let Ok(Some(mut rec)) = state.kernel.get_memory(dst_id) {
                    let weight = lk.get("weight").and_then(|v| v.as_f64()).unwrap_or(1.0) as f32;
                    let recency = rec
                        .get("updated")
                        .and_then(|v| v.as_str())
                        .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
                        .map(|t| {
                            let age = now
                                .signed_duration_since(t.with_timezone(&chrono::Utc))
                                .num_seconds()
                                .max(0) as f64;
                            let hl = 3600f64 * 6f64;
                            ((-age / hl).exp()) as f32
                        })
                        .unwrap_or(0.5);
                    let base = it.get("cscore").and_then(|v| v.as_f64()).unwrap_or(0.3) as f32;
                    let cscore = 0.6 * base + 0.25 * weight + 0.15 * recency;
                    if let Some(obj) = rec.as_object_mut() {
                        obj.insert("cscore".into(), json!(cscore));
                        obj.insert(
                            "explain".into(),
                            json!({"base": base, "link_weight": weight, "recency": recency}),
                        );
                    }
                    seen.insert(dst_id.to_string());
                    scored.push((cscore, rec));
                }
            }
        }
    }
    scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
    let items: Vec<Value> = scored
        .into_iter()
        .take(limit as usize)
        .map(|(_, v)| v)
        .collect();
    (
        axum::http::StatusCode::OK,
        Json(json!({"items": items, "mode": "coherent", "explain": true})),
    )
        .into_response()
}
