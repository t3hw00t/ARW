use axum::http::HeaderMap;
use axum::response::IntoResponse;
use axum::{
    extract::{Query, State},
    Json,
};
use serde::Deserialize;
use serde_json::{json, Value};
use utoipa::ToSchema;

use crate::{admin_ok, util, AppState};
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

/// Most recent memories (per lane).
#[utoipa::path(
    get,
    path = "/state/memory/recent",
    tag = "Memory",
    params(("lane" = Option<String>, Query), ("limit" = Option<i64>, Query)),
    responses(
        (status = 200, body = serde_json::Value),
        (status = 501, description = "Kernel disabled", body = serde_json::Value)
    )
)]
pub async fn state_memory_recent(
    State(state): State<AppState>,
    Query(q): Query<std::collections::HashMap<String, String>>,
) -> impl IntoResponse {
    if !state.kernel_enabled() {
        return crate::responses::kernel_disabled();
    }
    let lane = q.get("lane").map(|s| s.as_str());
    let limit = q
        .get("limit")
        .and_then(|s| s.parse::<i64>().ok())
        .unwrap_or(100);
    let lane_owned = lane.map(|s| s.to_string());
    match state
        .kernel()
        .list_recent_memory_async(lane_owned, limit)
        .await
    {
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
pub struct MemoryApplyReq {
    pub lane: String,
    #[serde(default)]
    pub kind: Option<String>,
    #[serde(default)]
    pub key: Option<String>,
    pub value: Value,
    #[serde(default)]
    pub tags: Option<Vec<String>>,
    #[serde(default)]
    pub embed: Option<Vec<f32>>,
    #[serde(default)]
    pub score: Option<f64>,
    #[serde(default)]
    pub prob: Option<f64>,
}

/// Insert a memory item (admin helper).
#[utoipa::path(
    post,
    path = "/admin/memory/apply",
    tag = "Admin/Memory",
    request_body = MemoryApplyReq,
    responses(
        (status = 201, description = "Created", body = serde_json::Value),
        (status = 401, description = "Unauthorized"),
        (status = 501, description = "Kernel disabled", body = serde_json::Value)
    )
)]
pub async fn admin_memory_apply(
    headers: HeaderMap,
    State(state): State<AppState>,
    Json(req): Json<MemoryApplyReq>,
) -> impl IntoResponse {
    if !admin_ok(&headers) {
        return (
            axum::http::StatusCode::UNAUTHORIZED,
            Json(json!({"type":"about:blank","title":"Unauthorized","status":401})),
        )
            .into_response();
    }
    if !state.kernel_enabled() {
        return crate::responses::kernel_disabled();
    }
    let MemoryApplyReq {
        lane,
        kind,
        key,
        value,
        tags,
        embed,
        score,
        prob,
    } = req;
    match state
        .kernel()
        .insert_memory_async(
            None,
            lane.clone(),
            kind.clone(),
            key.clone(),
            value.clone(),
            embed,
            tags.clone(),
            score,
            prob,
        )
        .await
    {
        Ok(id) => {
            state.bus().publish(
                topics::TOPIC_MEMORY_RECORD_PUT,
                &json!({
                    "id": id,
                    "lane": lane,
                    "kind": kind,
                    "key": key,
                    "tags": tags,
                }),
            );
            (axum::http::StatusCode::CREATED, Json(json!({"id": id}))).into_response()
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

/// List recent memory items (admin helper).
#[utoipa::path(
    get,
    path = "/admin/memory",
    tag = "Admin/Memory",
    params(("lane" = Option<String>, Query), ("limit" = Option<i64>, Query)),
    responses(
        (status = 200, description = "Memory snapshot", body = serde_json::Value),
        (status = 401, description = "Unauthorized"),
        (status = 501, description = "Kernel disabled", body = serde_json::Value)
    )
)]
pub async fn admin_memory_list(
    headers: HeaderMap,
    State(state): State<AppState>,
    Query(q): Query<std::collections::HashMap<String, String>>,
) -> impl IntoResponse {
    if !admin_ok(&headers) {
        return (
            axum::http::StatusCode::UNAUTHORIZED,
            Json(json!({"type":"about:blank","title":"Unauthorized","status":401})),
        )
            .into_response();
    }
    if !state.kernel_enabled() {
        return crate::responses::kernel_disabled();
    }
    let lane = q.get("lane").cloned();
    let limit = q
        .get("limit")
        .and_then(|s| s.parse::<i64>().ok())
        .unwrap_or(100);
    match state.kernel().list_recent_memory_async(lane, limit).await {
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
