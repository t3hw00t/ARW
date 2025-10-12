use axum::http::HeaderMap;
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::IntoResponse;
use axum::{
    extract::{Path, Query, State},
    Json,
};
use chrono::{DateTime, SecondsFormat, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{json, Number, Value};
use std::convert::Infallible;
use std::time::Duration;
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;
use tracing::{error, warn};
use utoipa::ToSchema;

use crate::{memory_service, read_models, AppState};
use arw_topics as topics;

fn now_timestamp_pair() -> (String, i64) {
    let now = Utc::now();
    (
        now.to_rfc3339_opts(SecondsFormat::Millis, true),
        now.timestamp_millis(),
    )
}

#[cfg(test)]
fn compute_memory_hash(
    lane: &str,
    kind: &Option<String>,
    key: &Option<String>,
    value: &Value,
) -> String {
    memory_service::MemoryUpsertInput {
        lane: lane.to_string(),
        kind: kind.clone(),
        key: key.clone(),
        value: value.clone(),
        ..Default::default()
    }
    .into_insert_owned()
    .compute_hash()
}

const MEMORY_SNAPSHOT_EVENT: &str = "memory.snapshot";
const MEMORY_PATCH_EVENT: &str = "memory.patch";

fn ensure_memory_recent_snapshot(snapshot: &mut Value, refresh_generated: bool) {
    let Some(obj) = snapshot.as_object_mut() else {
        return;
    };
    if let Some(items) = obj.get_mut("items").and_then(|value| value.as_array_mut()) {
        memory_service::attach_memory_ptrs(items);
        let summary = crate::read_models::summarize_memory_recent_items(items);
        obj.insert("summary".into(), summary);
    } else {
        obj.insert(
            "summary".into(),
            json!({
                "lanes": {},
                "modular": {
                    "recent": [],
                    "pending_human_review": 0,
                    "blocked": 0,
                }
            }),
        );
    }

    let mut generated_iso = obj
        .get("generated")
        .and_then(Value::as_str)
        .map(str::to_owned);
    let mut generated_ms = obj.get("generated_ms").and_then(Value::as_i64);

    if refresh_generated || generated_iso.is_none() {
        let (iso, ms) = now_timestamp_pair();
        generated_iso = Some(iso);
        generated_ms = Some(ms);
    } else if generated_ms.is_none() {
        generated_ms = generated_iso
            .as_ref()
            .and_then(|iso| DateTime::parse_from_rfc3339(iso).ok())
            .map(|dt| dt.timestamp_millis())
            .or_else(|| {
                let (iso, ms) = now_timestamp_pair();
                generated_iso = Some(iso);
                Some(ms)
            });
    }

    if let Some(iso) = generated_iso {
        obj.insert("generated".into(), Value::String(iso));
    }
    if let Some(ms) = generated_ms {
        obj.insert("generated_ms".into(), Value::Number(Number::from(ms)));
    }
}

/// Stream memory read-model patches and snapshots via SSE.
#[utoipa::path(
    get,
    path = "/state/memory",
    tag = "Memory",
    params(("lane" = Option<String>, Query)),
    responses(
        (status = 200, description = "Memory stream", content_type = "text/event-stream"),
        (status = 501, description = "Kernel disabled", body = arw_protocol::ProblemDetails),
        (status = 500, description = "Kernel error", body = serde_json::Value)
    )
)]
pub async fn state_memory_stream(
    State(state): State<AppState>,
    Query(q): Query<std::collections::HashMap<String, String>>,
) -> axum::response::Response {
    if !state.kernel_enabled() {
        return crate::responses::kernel_disabled();
    }

    let lane = q
        .get("lane")
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    let read_model_id = lane
        .as_ref()
        .map(|lane| format!("memory_lane_{}", lane))
        .unwrap_or_else(|| "memory_recent".to_string());
    let lane_name = lane.clone();

    let mut current_snapshot =
        if let Some(value) = crate::read_models::cached_read_model(&read_model_id) {
            value
        } else {
            match state
                .kernel()
                .list_recent_memory_async(lane_name.clone(), 200)
                .await
            {
                Ok(items) => {
                    let bundle = read_models::build_memory_recent_bundle(items);
                    if let Some(lane_key) = lane_name {
                        bundle
                            .lane_snapshots
                            .get(&lane_key)
                            .cloned()
                            .unwrap_or_else(|| {
                                json!({
                                    "lane": lane_key,
                                    "items": [],
                                    "generated": bundle.generated,
                                    "generated_ms": bundle.generated_ms,
                                })
                            })
                    } else {
                        bundle.snapshot
                    }
                }
                Err(err) => {
                    return (
                        axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                        Json(json!({
                            "type": "about:blank",
                            "title": "Error",
                            "status": 500,
                            "detail": err.to_string()
                        })),
                    )
                        .into_response();
                }
            }
        };

    if lane.is_none() {
        ensure_memory_recent_snapshot(&mut current_snapshot, false);
    }

    let (tx, rx) = mpsc::channel::<Result<Event, Infallible>>(64);
    let state_clone = state.clone();
    let sender = tx.clone();
    tokio::spawn(async move {
        if let Ok(event) = Event::default()
            .event(MEMORY_SNAPSHOT_EVENT)
            .json_data(json!({"snapshot": current_snapshot.clone()}))
        {
            if sender.send(Ok(event)).await.is_err() {
                return;
            }
        } else {
            error!("failed to serialize initial memory snapshot event");
            return;
        }

        let mut bus_rx = state_clone.bus().subscribe();
        while let Ok(env) = bus_rx.recv().await {
            if env.kind != topics::TOPIC_READMODEL_PATCH {
                continue;
            }
            let id = env.payload.get("id").and_then(|v| v.as_str()).unwrap_or("");
            if id != read_model_id {
                continue;
            }
            let Some(patch_val) = env.payload.get("patch") else {
                continue;
            };
            let patch_ops: Vec<json_patch::PatchOperation> =
                match serde_json::from_value(patch_val.clone()) {
                    Ok(ops) => ops,
                    Err(err) => {
                        warn!("deserialize memory patch failed: {}", err);
                        continue;
                    }
                };
            let mut next_snapshot = current_snapshot.clone();
            if let Err(err) = json_patch::patch(&mut next_snapshot, &patch_ops) {
                warn!("apply memory patch failed: {}", err);
                continue;
            }
            current_snapshot = next_snapshot;
            if lane.is_none() {
                ensure_memory_recent_snapshot(&mut current_snapshot, true);
            }
            let payload = json!({
                "patch": patch_val.clone(),
                "snapshot": current_snapshot.clone(),
            });
            match Event::default()
                .event(MEMORY_PATCH_EVENT)
                .json_data(&payload)
            {
                Ok(event) => {
                    if sender.send(Ok(event)).await.is_err() {
                        break;
                    }
                }
                Err(err) => {
                    warn!("serialize memory patch event failed: {}", err);
                }
            }
        }
    });

    drop(tx);

    Sse::new(ReceiverStream::new(rx))
        .keep_alive(
            KeepAlive::new()
                .interval(Duration::from_secs(15))
                .text(":keep-alive"),
        )
        .into_response()
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct MemoryRecentResponse {
    pub items: Vec<Value>,
    pub summary: Value,
    pub generated: String,
    #[serde(rename = "generated_ms")]
    pub generated_ms: i64,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct MemoryModularReviewResponse {
    pub pending_human_review: u64,
    pub blocked: u64,
    pub recent: Vec<Value>,
    pub generated: String,
    #[serde(rename = "generated_ms")]
    pub generated_ms: i64,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct MemoryLaneResponse {
    pub lane: String,
    pub items: Vec<Value>,
    pub generated: String,
    #[serde(rename = "generated_ms")]
    pub generated_ms: i64,
}

/// Most recent memories (per lane).
#[cfg_attr(
    not(test),
    utoipa::path(
        get,
        path = "/state/memory/recent",
        tag = "Memory",
        params(("lane" = Option<String>, Query), ("limit" = Option<i64>, Query)),
        responses(
            (status = 200, body = MemoryRecentResponse),
            (status = 501, description = "Kernel disabled", body = arw_protocol::ProblemDetails)
        )
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
            let bundle = read_models::build_memory_recent_bundle(items);
            let items = bundle
                .snapshot
                .get("items")
                .and_then(|v| v.as_array())
                .cloned()
                .unwrap_or_default();
            let summary = bundle
                .snapshot
                .get("summary")
                .cloned()
                .unwrap_or_else(|| json!({}));
            let generated = bundle.generated.clone();
            let generated_ms = bundle.generated_ms as i64;
            (
                axum::http::StatusCode::OK,
                Json(MemoryRecentResponse {
                    items,
                    summary,
                    generated,
                    generated_ms,
                }),
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

/// Modular memory review summary.
#[cfg_attr(
    not(test),
    utoipa::path(
        get,
        path = "/state/memory/modular",
        tag = "Memory",
        params(("limit" = Option<i64>, Query)),
        responses(
            (status = 200, body = MemoryModularReviewResponse),
            (status = 501, description = "Kernel disabled", body = arw_protocol::ProblemDetails)
        )
    )
)]
pub async fn state_memory_modular(
    State(state): State<AppState>,
    Query(q): Query<std::collections::HashMap<String, String>>,
) -> impl IntoResponse {
    if !state.kernel_enabled() {
        return crate::responses::kernel_disabled();
    }
    let limit = q
        .get("limit")
        .and_then(|s| s.parse::<i64>().ok())
        .unwrap_or(200);
    match state.kernel().list_recent_memory_async(None, limit).await {
        Ok(items) => {
            let bundle = read_models::build_memory_recent_bundle(items);
            let modular = bundle.modular;
            let pending = modular
                .get("pending_human_review")
                .and_then(|v| v.as_u64())
                .unwrap_or(0);
            let blocked = modular.get("blocked").and_then(|v| v.as_u64()).unwrap_or(0);
            let recent = modular
                .get("recent")
                .and_then(|v| v.as_array())
                .cloned()
                .unwrap_or_default();
            (
                axum::http::StatusCode::OK,
                Json(MemoryModularReviewResponse {
                    pending_human_review: pending,
                    blocked,
                    recent,
                    generated: bundle.generated,
                    generated_ms: bundle.generated_ms as i64,
                }),
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

/// Lane-specific memory snapshot (REST snapshot, not SSE).
#[cfg_attr(
    not(test),
    utoipa::path(
        get,
        path = "/state/memory/lane/{lane}",
        tag = "Memory",
        params(("lane" = String, Path), ("limit" = Option<i64>, Query)),
        responses(
            (status = 200, body = MemoryLaneResponse),
            (status = 404, description = "Unknown lane"),
            (status = 501, description = "Kernel disabled", body = arw_protocol::ProblemDetails)
        )
    )
)]
pub async fn state_memory_lane(
    State(state): State<AppState>,
    Path(lane): Path<String>,
    Query(q): Query<std::collections::HashMap<String, String>>,
) -> impl IntoResponse {
    if !state.kernel_enabled() {
        return crate::responses::kernel_disabled();
    }
    let lane = lane.trim().to_string();
    if lane.is_empty() {
        return (
            axum::http::StatusCode::BAD_REQUEST,
            Json(json!({
                "type": "about:blank",
                "title": "Invalid lane",
                "status": 400,
                "detail": "Lane must not be empty",
            })),
        )
            .into_response();
    }
    let limit = q
        .get("limit")
        .and_then(|v| v.parse::<i64>().ok())
        .unwrap_or(200);
    match state
        .kernel()
        .list_recent_memory_async(Some(lane.clone()), limit)
        .await
    {
        Ok(items) => {
            let bundle = read_models::build_memory_recent_bundle(items);
            let snapshot = bundle
                .lane_snapshots
                .get(&lane)
                .cloned()
                .unwrap_or_else(|| {
                    json!({
                        "lane": lane,
                        "items": [],
                        "generated": bundle.generated,
                        "generated_ms": bundle.generated_ms,
                    })
                });
            let items = snapshot
                .get("items")
                .and_then(|v| v.as_array())
                .cloned()
                .unwrap_or_default();
            (
                axum::http::StatusCode::OK,
                Json(MemoryLaneResponse {
                    lane: lane.clone(),
                    items,
                    generated: bundle.generated,
                    generated_ms: bundle.generated_ms as i64,
                }),
            )
                .into_response()
        }
        Err(err) => (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({
                "type": "about:blank",
                "title": "Error",
                "status": 500,
                "detail": err.to_string()
            })),
        )
            .into_response(),
    }
}

#[derive(Deserialize, ToSchema, Default)]
pub struct MemoryEmbeddingReq {
    pub vector: Vec<f32>,
    #[serde(default)]
    pub hint: Option<String>,
}

#[derive(Deserialize, ToSchema, Default)]
pub struct MemoryApplyReq {
    pub lane: String,
    #[serde(default)]
    pub kind: Option<String>,
    #[serde(default)]
    pub key: Option<String>,
    pub value: Value,
    #[serde(default)]
    pub text: Option<String>,
    #[serde(default)]
    pub agent_id: Option<String>,
    #[serde(default)]
    pub project_id: Option<String>,
    #[serde(default)]
    pub durability: Option<String>,
    #[serde(default)]
    pub trust: Option<f64>,
    #[serde(default)]
    pub privacy: Option<String>,
    #[serde(default)]
    pub ttl_s: Option<i64>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub keywords: Vec<String>,
    #[serde(default)]
    pub embedding: Option<MemoryEmbeddingReq>,
    #[serde(default)]
    pub score: Option<f64>,
    #[serde(default)]
    pub prob: Option<f64>,
    #[serde(default)]
    pub entities: Value,
    #[serde(default)]
    pub source: Value,
    #[serde(default)]
    pub links: Value,
    #[serde(default)]
    pub extra: Value,
    #[serde(default)]
    pub dedupe: bool,
    #[serde(default)]
    pub topics: Vec<memory_service::MemoryTopicHint>,
}

/// Insert a memory item (admin helper).
#[cfg_attr(
    not(test),
    utoipa::path(
        post,
        path = "/admin/memory/apply",
        tag = "Admin/Memory",
        request_body = MemoryApplyReq,
        responses(
            (status = 201, description = "Created", body = serde_json::Value),
            (status = 401, description = "Unauthorized", body = arw_protocol::ProblemDetails),
            (status = 501, description = "Kernel disabled", body = arw_protocol::ProblemDetails)
        )
    )
)]
pub async fn admin_memory_apply(
    headers: HeaderMap,
    State(state): State<AppState>,
    Json(req): Json<MemoryApplyReq>,
) -> impl IntoResponse {
    if let Err(resp) = crate::responses::require_admin(&headers).await {
        return *resp;
    }
    if !state.kernel_enabled() {
        return crate::responses::kernel_disabled();
    }
    let mut body = memory_service::MemoryUpsertInput {
        id: None,
        lane: req.lane,
        kind: req.kind,
        key: req.key,
        value: req.value,
        text: req.text,
        agent_id: req.agent_id,
        project_id: req.project_id,
        durability: req.durability,
        trust: req.trust,
        privacy: req.privacy,
        ttl_s: req.ttl_s,
        tags: req.tags,
        keywords: req.keywords,
        embedding: req
            .embedding
            .map(|emb| memory_service::MemoryEmbeddingInput {
                vector: emb.vector,
                hint: emb.hint,
            }),
        score: req.score,
        prob: req.prob,
        entities: req.entities,
        source: req.source,
        links: req.links,
        extra: req.extra,
        dedupe: req.dedupe,
        topics: req.topics,
    };
    if body.privacy.is_none() {
        body.privacy = Some("private".to_string());
    }

    match memory_service::upsert_memory(&state, body, "admin.memory.apply").await {
        Ok(result) => {
            let body = json!({
                "id": result.id,
                "record": result.record,
                "applied": result.applied
            });
            (axum::http::StatusCode::CREATED, Json(body)).into_response()
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
#[cfg_attr(
    not(test),
    utoipa::path(
        get,
        path = "/admin/memory",
        tag = "Admin/Memory",
        params(("lane" = Option<String>, Query), ("limit" = Option<i64>, Query)),
        responses(
            (status = 200, description = "Memory snapshot", body = serde_json::Value),
            (status = 401, description = "Unauthorized", body = arw_protocol::ProblemDetails),
            (status = 501, description = "Kernel disabled", body = arw_protocol::ProblemDetails)
        )
    )
)]
pub async fn admin_memory_list(
    headers: HeaderMap,
    State(state): State<AppState>,
    Query(q): Query<std::collections::HashMap<String, String>>,
) -> impl IntoResponse {
    if let Err(resp) = crate::responses::require_admin(&headers).await {
        return *resp;
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
        Ok(mut items) => {
            memory_service::attach_memory_ptrs(&mut items);
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::env;
    use crate::{memory_service, read_models};
    use arw_policy::PolicyEngine;
    use arw_wasi::ToolHost;
    use axum::{
        body::to_bytes,
        http::{HeaderMap, HeaderValue, StatusCode},
        routing::get,
        Router,
    };
    use http_body_util::BodyExt;
    use serde_json::{json, Value};
    use std::collections::VecDeque;
    use std::sync::Arc;
    use tempfile::tempdir;
    use tokio::time::{timeout, Duration};
    use tower::ServiceExt;

    async fn build_state(dir: &std::path::Path, env_guard: &mut env::EnvGuard) -> AppState {
        env_guard.set("ARW_DEBUG", "1");
        crate::util::reset_state_dir_for_tests();
        env_guard.set("ARW_STATE_DIR", dir.display().to_string());
        let bus = arw_events::Bus::new_with_replay(64, 64);
        let kernel = arw_kernel::Kernel::open(dir).expect("init kernel for tests");
        let policy = PolicyEngine::load_from_env();
        let policy_handle = crate::policy::PolicyHandle::new(policy, bus.clone());
        let host: Arc<dyn ToolHost> = Arc::new(arw_wasi::NoopHost);
        AppState::builder(bus, kernel, policy_handle, host, true)
            .with_sse_capacity(64)
            .build()
            .await
    }

    #[derive(Debug, Default, Clone)]
    struct SseRecord {
        event: Option<String>,
        data: Option<String>,
    }

    fn parse_sse(buffer: &mut String) -> Vec<SseRecord> {
        let mut out = Vec::new();
        while let Some(idx) = buffer.find("\n\n") {
            let chunk = buffer[..idx].to_string();
            *buffer = buffer[idx + 2..].to_string();
            if chunk.trim().is_empty() {
                continue;
            }
            let mut record = SseRecord::default();
            for line in chunk.lines() {
                if let Some(rest) = line.strip_prefix("event: ") {
                    record.event = Some(rest.trim().to_string());
                } else if let Some(rest) = line.strip_prefix("data: ") {
                    record.data = Some(rest.trim().to_string());
                }
            }
            out.push(record);
        }
        out
    }

    #[tokio::test]
    async fn memory_recent_includes_generated_and_ptrs() {
        let temp = tempdir().expect("tmp");
        let mut ctx = crate::test_support::begin_state_env(temp.path());
        let state = build_state(temp.path(), &mut ctx.env).await;

        let insert_owned = memory_service::MemoryUpsertInput {
            lane: "semantic".to_string(),
            kind: Some("note".to_string()),
            key: Some("focus".to_string()),
            value: json!({"text": "focus test"}),
            ..Default::default()
        }
        .into_insert_owned();
        let (inserted_id, inserted_record) = state
            .kernel()
            .insert_memory_with_record_async(insert_owned)
            .await
            .expect("insert memory");
        assert_eq!(
            inserted_record.get("id").and_then(Value::as_str),
            Some(inserted_id.as_str()),
        );
        assert_eq!(
            inserted_record.get("lane").and_then(Value::as_str),
            Some("semantic"),
        );
        assert_eq!(
            inserted_record
                .get("value")
                .and_then(|v| v.get("text"))
                .and_then(Value::as_str),
            Some("focus test"),
        );

        let app = Router::new()
            .route("/state/memory/recent", get(state_memory_recent))
            .with_state(state.clone());

        let response = app
            .oneshot(
                axum::http::Request::builder()
                    .uri("/state/memory/recent")
                    .body(axum::body::Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");
        assert_eq!(response.status(), axum::http::StatusCode::OK);

        let body_bytes = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body bytes");
        let parsed: MemoryRecentResponse =
            serde_json::from_slice(&body_bytes).expect("memory recent json");
        assert!(
            !parsed.items.is_empty(),
            "expected at least one memory item"
        );
        assert!(
            chrono::DateTime::parse_from_rfc3339(&parsed.generated).is_ok(),
            "generated timestamp should be RFC3339"
        );
        assert!(
            parsed.items.iter().all(|item| item.get("ptr").is_some()),
            "memory items should include ptr metadata"
        );
        assert!(
            parsed
                .summary
                .get("modular")
                .and_then(|v| v.get("recent"))
                .map(|v| v.is_array())
                .unwrap_or(true),
            "summary.modular.recent should be an array when present"
        );
        let parsed_dt = DateTime::parse_from_rfc3339(&parsed.generated).expect("parse generated");
        assert_eq!(
            parsed_dt.timestamp_millis(),
            parsed.generated_ms,
            "generated_ms should align with generated timestamp",
        );
        assert!(parsed.generated_ms > 0, "generated_ms should be positive");
    }

    #[tokio::test]
    async fn memory_modular_summary_endpoint_returns_counts() {
        let temp = tempdir().expect("tmp");
        let mut ctx = crate::test_support::begin_state_env(temp.path());
        let state = build_state(temp.path(), &mut ctx.env).await;

        let insert_owned = memory_service::MemoryUpsertInput {
            lane: "short_term".to_string(),
            kind: Some("conversation.turn".to_string()),
            value: json!({
                "agent_id": "assistant.chat",
                "turn_id": "turn-1",
                "payload_kind": "chat",
                "lifecycle": {
                    "stage": "pending_human_review",
                    "validation_gate": "required"
                },
                "payload_summary": {
                    "text_preview": "hello"
                }
            }),
            ..Default::default()
        }
        .into_insert_owned();
        let (inserted_id, inserted_record) = state
            .kernel()
            .insert_memory_with_record_async(insert_owned)
            .await
            .expect("insert memory");
        assert_eq!(
            inserted_record.get("id").and_then(Value::as_str),
            Some(inserted_id.as_str()),
        );
        assert_eq!(
            inserted_record.get("lane").and_then(Value::as_str),
            Some("short_term"),
        );
        assert!(inserted_record
            .get("value")
            .and_then(|v| v.get("payload_summary"))
            .is_some());

        let app = Router::new()
            .route("/state/memory/modular", get(state_memory_modular))
            .with_state(state.clone());

        let response = app
            .oneshot(
                axum::http::Request::builder()
                    .uri("/state/memory/modular")
                    .body(axum::body::Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");
        assert_eq!(response.status(), axum::http::StatusCode::OK);

        let body_bytes = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body bytes");
        let parsed: MemoryModularReviewResponse =
            serde_json::from_slice(&body_bytes).expect("memory modular json");
        assert!(parsed.pending_human_review >= 1);
        assert!(parsed.generated_ms > 0);
        assert!(!parsed.recent.is_empty());
    }

    #[tokio::test]
    async fn memory_lane_endpoint_returns_filtered_items() {
        let temp = tempdir().expect("tmp");
        let mut ctx = crate::test_support::begin_state_env(temp.path());
        let state = build_state(temp.path(), &mut ctx.env).await;

        let insert_short = memory_service::MemoryUpsertInput {
            lane: "short_term".to_string(),
            kind: Some("conversation.turn".to_string()),
            value: json!({ "payload_kind": "chat", "intent": "draft_response" }),
            ..Default::default()
        }
        .into_insert_owned();
        let (short_id, short_record) = state
            .kernel()
            .insert_memory_with_record_async(insert_short)
            .await
            .expect("insert short-term memory");
        assert_eq!(
            short_record.get("id").and_then(Value::as_str),
            Some(short_id.as_str()),
        );
        assert_eq!(
            short_record.get("lane").and_then(Value::as_str),
            Some("short_term"),
        );

        let insert_semantic = memory_service::MemoryUpsertInput {
            lane: "semantic".to_string(),
            kind: Some("note".to_string()),
            value: json!({"text": "doc"}),
            ..Default::default()
        }
        .into_insert_owned();
        let (semantic_id, semantic_record) = state
            .kernel()
            .insert_memory_with_record_async(insert_semantic)
            .await
            .expect("insert semantic memory");
        assert_eq!(
            semantic_record.get("id").and_then(Value::as_str),
            Some(semantic_id.as_str()),
        );
        assert_eq!(
            semantic_record.get("lane").and_then(Value::as_str),
            Some("semantic"),
        );

        let app = Router::new()
            .route("/state/memory/lane/{lane}", get(state_memory_lane))
            .with_state(state.clone());

        let response = app
            .oneshot(
                axum::http::Request::builder()
                    .uri("/state/memory/lane/short_term")
                    .body(axum::body::Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");
        assert_eq!(response.status(), axum::http::StatusCode::OK);

        let body_bytes = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body bytes");
        let parsed: MemoryLaneResponse = serde_json::from_slice(&body_bytes).expect("lane json");
        assert_eq!(parsed.lane, "short_term");
        assert!(parsed.items.iter().all(|item| {
            item.get("lane").and_then(|v| v.as_str()).unwrap_or("") == "short_term"
        }));
        assert!(!parsed.items.is_empty());
    }

    #[tokio::test]
    async fn memory_stream_provides_snapshot_and_patch() {
        let temp = tempdir().expect("tmp");
        let mut ctx = crate::test_support::begin_state_env(temp.path());
        let state = build_state(temp.path(), &mut ctx.env).await;

        let initial_value = json!({"text": "hello"});
        let insert_owned = memory_service::MemoryUpsertInput {
            lane: "semantic".to_string(),
            kind: Some("note".to_string()),
            key: Some("hello".to_string()),
            value: initial_value,
            tags: vec!["demo".to_string()],
            score: Some(0.8),
            ..Default::default()
        }
        .into_insert_owned();
        let (inserted_id, inserted_record) = state
            .kernel()
            .insert_memory_with_record_async(insert_owned)
            .await
            .expect("insert memory");
        assert_eq!(
            inserted_record.get("id").and_then(Value::as_str),
            Some(inserted_id.as_str()),
        );
        assert_eq!(
            inserted_record
                .get("tags")
                .and_then(Value::as_array)
                .map(|tags| tags.len())
                .unwrap_or_default(),
            1,
        );

        let snapshot_now = state
            .kernel()
            .list_recent_memory_async(None, 200)
            .await
            .expect("list memory");
        let mut snapshot_items = snapshot_now.clone();
        memory_service::attach_memory_ptrs(&mut snapshot_items);
        let (generated, generated_ms) = now_timestamp_pair();
        read_models::publish_read_model_patch(
            &state.bus(),
            "memory_recent",
            &json!({
                "items": snapshot_items,
                "generated": generated,
                "generated_ms": generated_ms,
            }),
        );

        let app = Router::new()
            .route("/state/memory", get(state_memory_stream))
            .with_state(state.clone());

        let response = app
            .clone()
            .oneshot(
                axum::http::Request::builder()
                    .uri("/state/memory")
                    .body(axum::body::Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");
        assert_eq!(response.status(), axum::http::StatusCode::OK);

        let mut body = response.into_body();
        let mut buffer = String::new();
        let mut events = VecDeque::new();

        while events
            .iter()
            .all(|ev: &SseRecord| ev.event.as_deref() != Some(MEMORY_SNAPSHOT_EVENT))
        {
            let frame = timeout(Duration::from_secs(1), body.frame())
                .await
                .expect("snapshot frame timeout")
                .expect("snapshot frame present")
                .expect("data frame");
            let bytes = frame.into_data().expect("frame data");
            buffer.push_str(&String::from_utf8_lossy(&bytes));
            events.extend(parse_sse(&mut buffer));
        }

        let snapshot_event = events
            .iter()
            .find(|ev| ev.event.as_deref() == Some(MEMORY_SNAPSHOT_EVENT))
            .expect("snapshot event");
        let snapshot_json = snapshot_event
            .data
            .as_ref()
            .and_then(|d| serde_json::from_str::<serde_json::Value>(d).ok())
            .expect("snapshot json");
        assert_eq!(
            snapshot_json
                .get("snapshot")
                .and_then(|s| s.get("items"))
                .and_then(|items| items.as_array())
                .map(|arr| arr.len())
                .unwrap_or_default(),
            1,
        );
        let snapshot_generated = snapshot_json["snapshot"]["generated"]
            .as_str()
            .expect("snapshot generated");
        let snapshot_generated_ms = snapshot_json["snapshot"]["generated_ms"]
            .as_i64()
            .expect("snapshot generated ms");
        let snapshot_dt = DateTime::parse_from_rfc3339(snapshot_generated).expect("snapshot dt");
        assert_eq!(snapshot_dt.timestamp_millis(), snapshot_generated_ms);

        let insert_owned = memory_service::MemoryUpsertInput {
            lane: "semantic".to_string(),
            kind: Some("note".to_string()),
            key: Some("second".to_string()),
            value: json!({"text": "second"}),
            tags: vec!["demo".to_string()],
            score: Some(0.5),
            ..Default::default()
        }
        .into_insert_owned();
        let (second_id, second_record) = state
            .kernel()
            .insert_memory_with_record_async(insert_owned)
            .await
            .expect("insert second memory");
        assert_eq!(
            second_record.get("id").and_then(Value::as_str),
            Some(second_id.as_str()),
        );
        assert!(second_record
            .get("value")
            .and_then(|v| v.get("text"))
            .and_then(Value::as_str)
            .is_some());

        let mut updated_snapshot = state
            .kernel()
            .list_recent_memory_async(None, 200)
            .await
            .expect("list updated memory");
        memory_service::attach_memory_ptrs(&mut updated_snapshot);
        let (generated, generated_ms) = now_timestamp_pair();
        read_models::publish_read_model_patch(
            &state.bus(),
            "memory_recent",
            &json!({
                "items": updated_snapshot,
                "generated": generated,
                "generated_ms": generated_ms,
            }),
        );

        let patch_event = timeout(Duration::from_millis(500), async {
            loop {
                let frame = body
                    .frame()
                    .await
                    .expect("patch frame")
                    .expect("patch data");
                let bytes = frame.into_data().expect("patch bytes");
                buffer.push_str(&String::from_utf8_lossy(&bytes));
                events.extend(parse_sse(&mut buffer));
                if let Some(found) = events
                    .iter()
                    .find(|ev| ev.event.as_deref() == Some(MEMORY_PATCH_EVENT))
                {
                    break Some(found.clone());
                }
            }
        })
        .await
        .expect("patch event present")
        .expect("patch event");

        let patch_json = patch_event
            .data
            .as_ref()
            .and_then(|d| serde_json::from_str::<serde_json::Value>(d).ok())
            .expect("patch json");
        let items_len = patch_json
            .get("snapshot")
            .and_then(|v| v.get("items"))
            .and_then(|v| v.as_array())
            .map(|arr| arr.len())
            .unwrap_or_default();
        assert_eq!(
            items_len, 2,
            "snapshot after patch should include two items"
        );
        let patch_generated = patch_json["snapshot"]["generated"]
            .as_str()
            .expect("patch generated");
        let patch_generated_ms = patch_json["snapshot"]["generated_ms"]
            .as_i64()
            .expect("patch generated ms");
        let patch_dt = DateTime::parse_from_rfc3339(patch_generated).expect("patch dt");
        assert_eq!(patch_dt.timestamp_millis(), patch_generated_ms);
    }

    #[tokio::test]
    async fn memory_apply_emits_record_and_applied_events() {
        let temp = tempdir().expect("temp dir");
        let mut ctx = crate::test_support::begin_state_env(temp.path());
        let state = build_state(temp.path(), &mut ctx.env).await;
        let bus = state.bus();
        let mut rx = bus.subscribe_filtered(
            vec![
                topics::TOPIC_MEMORY_RECORD_PUT.to_string(),
                topics::TOPIC_MEMORY_APPLIED.to_string(),
            ],
            Some(16),
        );

        let target_id = format!("ui_selftest_{}", Utc::now().timestamp_millis());
        let request = MemoryApplyReq {
            lane: "ephemeral".into(),
            kind: Some("note".into()),
            key: Some("summary".into()),
            value: json!({
                "test_id": target_id,
                "content": "captured from debug self-test"
            }),
            tags: vec!["alpha".into(), "Alpha".into(), "notes".into()],
            score: Some(0.42),
            prob: Some(0.84),
            ..Default::default()
        };

        let mut headers = HeaderMap::new();
        headers.insert("X-ARW-Admin", HeaderValue::from_static("ok"));

        let response = admin_memory_apply(headers, State(state.clone()), Json(request))
            .await
            .into_response();
        let (parts, body) = response.into_parts();
        assert_eq!(parts.status, StatusCode::CREATED);
        let body_bytes = to_bytes(body, usize::MAX).await.expect("body bytes");
        let response_json: Value = serde_json::from_slice(&body_bytes).expect("json response");
        assert_eq!(
            response_json["record"]["value"]["test_id"].as_str(),
            Some(target_id.as_str())
        );
        assert!(response_json["applied"]["value_preview"].as_str().is_some());
        let lane = response_json["record"]["lane"].as_str().expect("lane str");
        let kind_opt = response_json["record"]["kind"]
            .as_str()
            .map(|s| s.to_string());
        let key_opt = response_json["record"]["key"]
            .as_str()
            .map(|s| s.to_string());
        let expected_hash =
            compute_memory_hash(lane, &kind_opt, &key_opt, &response_json["record"]["value"]);
        assert_eq!(
            response_json["record"]["hash"].as_str(),
            Some(expected_hash.as_str())
        );

        let envelope = timeout(Duration::from_secs(1), rx.recv())
            .await
            .expect("event timeout")
            .expect("event present");
        assert_eq!(envelope.kind, topics::TOPIC_MEMORY_RECORD_PUT);
        let payload = envelope.payload;
        assert_eq!(payload["lane"].as_str(), Some("ephemeral"));
        assert_eq!(payload["kind"].as_str(), Some("note"));
        assert_eq!(payload["key"].as_str(), Some("summary"));
        assert_eq!(payload["score"].as_f64(), Some(0.42));
        assert_eq!(payload["prob"].as_f64(), Some(0.84));
        assert!(payload["hash"].as_str().is_some());
        assert!(payload["ptr"].is_object());
        let tags = payload["tags"].as_array().expect("tags array");
        assert_eq!(tags.len(), 2); // deduped (alpha, notes)
        assert_eq!(
            payload["value"]["test_id"].as_str(),
            Some(target_id.as_str())
        );

        let envelope = timeout(Duration::from_secs(1), rx.recv())
            .await
            .expect("applied event timeout")
            .expect("applied event present");
        assert_eq!(envelope.kind, topics::TOPIC_MEMORY_APPLIED);
        let payload = envelope.payload;
        assert_eq!(payload["source"].as_str(), Some("admin.memory.apply"));
        assert_eq!(
            payload["value"]["test_id"].as_str(),
            Some(target_id.as_str())
        );
        assert!(payload["value_preview"].as_str().is_some());
        assert!(payload["value_bytes"].as_u64().is_some());
        assert!(payload["applied_at"].as_str().is_some());
        let tags = payload["tags"].as_array().expect("tags array");
        assert_eq!(tags.len(), 2);
        let payload_lane = payload["lane"].as_str().expect("lane");
        let payload_kind = payload["kind"].as_str().map(|s| s.to_string());
        let payload_key = payload["key"].as_str().map(|s| s.to_string());
        let expected_payload_hash =
            compute_memory_hash(payload_lane, &payload_kind, &payload_key, &payload["value"]);
        assert_eq!(
            payload["hash"].as_str(),
            Some(expected_payload_hash.as_str())
        );
    }
}
