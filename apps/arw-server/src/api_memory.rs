use axum::response::IntoResponse;
use axum::{
    extract::{Query, State},
    Json,
};
use serde::Deserialize;
use serde_json::{json, Value};
use utoipa::ToSchema;

use crate::{util, working_set, AppState};
use arw_topics as topics;

fn attach_memory_ptrs(items: Vec<Value>) -> Vec<Value> {
    items
        .into_iter()
        .map(|mut item| {
            util::attach_memory_ptr(&mut item);
            item
        })
        .collect()
}

#[derive(Deserialize, ToSchema)]
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
/// Insert a memory item into a lane.
#[utoipa::path(post, path = "/memory/put", tag = "Memory", request_body = MemPutReq, responses((status = 201, body = serde_json::Value)))]
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
        req.embed.as_deref(),
        req.tags.as_deref(),
        req.score,
        req.prob,
    ) {
        Ok(id) => {
            state.bus.publish(
                topics::TOPIC_MEMORY_RECORD_PUT,
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
        Ok(items) => {
            let items = attach_memory_ptrs(items);
            Json(json!({"items": items, "mode": mode}))
        }
        Err(e) => Json(json!({"items": [], "error": e.to_string()})),
    }
}

#[derive(Deserialize, ToSchema)]
pub(crate) struct MemEmbedReq {
    pub embed: Vec<f32>,
    #[serde(default)]
    pub lane: Option<String>,
    #[serde(default)]
    pub limit: Option<i64>,
}
/// Nearest neighbors by embedding.
#[utoipa::path(post, path = "/memory/search_embed", tag = "Memory", request_body = MemEmbedReq, responses((status = 200, body = serde_json::Value)))]
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
        Ok(items) => {
            let items = attach_memory_ptrs(items);
            (
                axum::http::StatusCode::OK,
                Json(json!({"items": items, "mode": "embed"})),
            )
                .into_response()
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

#[derive(Deserialize, ToSchema)]
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
/// Hybrid retrieval with filters.
#[utoipa::path(post, path = "/state/memory/select_hybrid", tag = "Memory", request_body = MemHybridReq, responses((status = 200, body = serde_json::Value)))]
pub async fn memory_select_hybrid(
    State(state): State<AppState>,
    Json(req): Json<MemHybridReq>,
) -> impl IntoResponse {
    let lane_opt = req.lane.as_deref();
    let limit = req.limit.unwrap_or(20);
    let res =
        state
            .kernel
            .select_memory_hybrid(req.q.as_deref(), req.embed.as_deref(), lane_opt, limit);
    match res {
        Ok(items) => {
            let items = attach_memory_ptrs(items);
            (
                axum::http::StatusCode::OK,
                Json(json!({"items": items, "mode": "hybrid"})),
            )
                .into_response()
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

#[derive(Deserialize, ToSchema)]
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
    #[serde(default)]
    pub lanes: Option<Vec<String>>,
    #[serde(default)]
    pub diversity_lambda: Option<f32>,
    #[serde(default)]
    pub min_score: Option<f32>,
    #[serde(default)]
    pub include_sources: Option<bool>,
    #[serde(default)]
    pub debug: Option<bool>,
    #[serde(default)]
    pub lane_bonus: Option<f32>,
    #[serde(default)]
    pub project: Option<String>,
    #[serde(default)]
    pub expand_query: Option<bool>,
    #[serde(default)]
    pub expand_query_top_k: Option<usize>,
    #[serde(default)]
    pub scorer: Option<String>,
}
/// Coherence-ranked selection (optionally show sources and diagnostics).
#[utoipa::path(post, path = "/memory/select_coherent", tag = "Memory", request_body = MemCoherentReq, responses((status = 200, body = serde_json::Value)))]
pub async fn memory_select_coherent(
    State(state): State<AppState>,
    Json(req): Json<MemCoherentReq>,
) -> impl IntoResponse {
    let spec = spec_from_req(&req);
    let response = match working_set::assemble(&state, &spec) {
        Ok(ws) => {
            let working_set::WorkingSet {
                items,
                seeds,
                expanded,
                diagnostics,
                ..
            } = ws;
            let items = attach_memory_ptrs(items);
            let seeds = attach_memory_ptrs(seeds);
            let expanded = attach_memory_ptrs(expanded);
            let mut body = json!({"items": items, "mode": "coherent"});
            if req.include_sources.unwrap_or(false) {
                body["seeds"] = json!(seeds);
                body["expanded"] = json!(expanded);
            }
            if req.debug.unwrap_or(false) {
                body["diagnostics"] = diagnostics;
            }
            (axum::http::StatusCode::OK, Json(body))
        }
        Err(e) => (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({
                "type": "about:blank",
                "title": "Error",
                "status": 500,
                "detail": e.to_string()
            })),
        ),
    };
    response.into_response()
}

/// Most recent memories (per lane).
#[utoipa::path(get, path = "/state/memory/recent", tag = "Memory", params(("lane" = Option<String>, Query), ("limit" = Option<i64>, Query)), responses((status = 200, body = serde_json::Value)))]
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
        Ok(items) => {
            let items = attach_memory_ptrs(items);
            (axum::http::StatusCode::OK, Json(json!({"items": items}))).into_response()
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

#[derive(Deserialize, ToSchema)]
pub(crate) struct MemLinkReq {
    pub src_id: String,
    pub dst_id: String,
    #[serde(default)]
    pub rel: Option<String>,
    #[serde(default)]
    pub weight: Option<f64>,
}
/// Create a link between memory ids.
#[utoipa::path(post, path = "/memory/link", tag = "Memory", request_body = MemLinkReq, responses((status = 201, body = serde_json::Value)))]
pub async fn memory_link_put(
    State(state): State<AppState>,
    Json(req): Json<MemLinkReq>,
) -> impl IntoResponse {
    match state
        .kernel
        .insert_memory_link(&req.src_id, &req.dst_id, req.rel.as_deref(), req.weight)
    {
        Ok(()) => {
            state.bus.publish(
                topics::TOPIC_MEMORY_LINK_PUT,
                &json!({
                    "src_id": req.src_id,
                    "dst_id": req.dst_id,
                    "rel": req.rel,
                    "weight": req.weight
                }),
            );
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

/// List relationships for a memory id.
#[utoipa::path(get, path = "/state/memory/links", tag = "Memory", params(("id" = String, Query), ("limit" = Option<i64>, Query)), responses((status = 200, body = serde_json::Value)))]
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

/// Explainability payload for coherence results.
#[utoipa::path(post, path = "/state/memory/explain_coherent", tag = "Memory", request_body = MemCoherentReq, responses((status = 200, body = serde_json::Value)))]
pub async fn memory_explain_coherent(
    State(state): State<AppState>,
    Json(req): Json<MemCoherentReq>,
) -> impl IntoResponse {
    let spec = spec_from_req(&req);
    let response = match working_set::assemble(&state, &spec) {
        Ok(ws) => {
            let working_set::WorkingSet {
                items,
                seeds,
                expanded,
                diagnostics,
                ..
            } = ws;
            let items = attach_memory_ptrs(items);
            let seeds = attach_memory_ptrs(seeds);
            let expanded = attach_memory_ptrs(expanded);
            (
                axum::http::StatusCode::OK,
                Json(json!({
                    "items": items,
                    "mode": "coherent_explain",
                    "seeds": seeds,
                    "expanded": expanded,
                    "diagnostics": diagnostics
                })),
            )
        }
        Err(e) => (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({
                "type": "about:blank",
                "title": "Error",
                "status": 500,
                "detail": e.to_string()
            })),
        ),
    };
    response.into_response()
}

fn spec_from_req(req: &MemCoherentReq) -> working_set::WorkingSetSpec {
    let mut lanes = if let Some(list) = req.lanes.clone() {
        if list.is_empty() {
            working_set::default_lanes()
        } else {
            list
        }
    } else if let Some(lane) = req.lane.clone() {
        vec![lane]
    } else {
        working_set::default_lanes()
    };
    if lanes.is_empty() {
        lanes = working_set::default_lanes();
    }
    let limit = req
        .limit
        .unwrap_or(working_set::default_limit() as i64)
        .max(1);
    let expand = req
        .expand_per_seed
        .unwrap_or(working_set::default_expand_per_seed() as i64)
        .max(0);
    let mut spec = working_set::WorkingSetSpec {
        query: req.q.clone(),
        embed: req.embed.clone(),
        lanes,
        limit: limit as usize,
        expand_per_seed: expand as usize,
        diversity_lambda: req
            .diversity_lambda
            .unwrap_or_else(working_set::default_diversity_lambda),
        min_score: req.min_score.unwrap_or_else(working_set::default_min_score),
        project: req.project.clone(),
        lane_bonus: req
            .lane_bonus
            .unwrap_or_else(working_set::default_lane_bonus),
        scorer: req.scorer.clone(),
        expand_query: req
            .expand_query
            .unwrap_or_else(working_set::default_expand_query),
        expand_query_top_k: req
            .expand_query_top_k
            .unwrap_or_else(working_set::default_expand_query_top_k),
    };
    spec.normalize();
    spec
}
