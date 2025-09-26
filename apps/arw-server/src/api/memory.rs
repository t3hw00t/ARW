use axum::http::HeaderMap;
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::IntoResponse;
use axum::{
    extract::{Query, State},
    Json,
};
use chrono::{SecondsFormat, Utc};
use hex::encode as hex_encode;
use serde::Deserialize;
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};
use std::convert::Infallible;
use std::time::Duration;
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;
use tracing::{error, warn};
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

const VALUE_PREVIEW_MAX_CHARS: usize = 240;

fn now_timestamp() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true)
}

fn compute_memory_hash(
    lane: &str,
    kind: &Option<String>,
    key: &Option<String>,
    value: &Value,
) -> String {
    let mut hasher = Sha256::new();
    hasher.update(lane.as_bytes());
    if let Some(k) = kind {
        hasher.update(k.as_bytes());
    }
    if let Some(k) = key {
        hasher.update(k.as_bytes());
    }
    if let Ok(bytes) = serde_json::to_vec(value) {
        hasher.update(bytes);
    }
    hex_encode(hasher.finalize())
}

fn truncate_chars(input: &str, limit: usize) -> (String, bool) {
    let mut out = String::new();
    let mut truncated = false;
    for (idx, ch) in input.chars().enumerate() {
        if idx >= limit {
            truncated = true;
            break;
        }
        out.push(ch);
    }
    if truncated {
        out.push('…');
    }
    (out, truncated)
}

fn preview_from_value(value: &Value) -> Option<(String, bool)> {
    match value {
        Value::String(s) => Some(truncate_chars(s, VALUE_PREVIEW_MAX_CHARS)),
        _ => serde_json::to_string(value)
            .ok()
            .map(|s| truncate_chars(&s, VALUE_PREVIEW_MAX_CHARS)),
    }
}

fn normalize_tags(tags: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for tag in tags {
        let trimmed = tag.trim();
        if trimmed.is_empty() {
            continue;
        }
        if !out
            .iter()
            .any(|existing| existing.eq_ignore_ascii_case(trimmed))
        {
            out.push(trimmed.to_string());
        }
    }
    out
}

fn parse_tags_field(value: Option<&Value>) -> Vec<String> {
    match value {
        Some(Value::String(s)) => s
            .split(',')
            .map(|part| part.trim())
            .filter(|part| !part.is_empty())
            .map(|part| part.to_string())
            .collect(),
        Some(Value::Array(values)) => values
            .iter()
            .filter_map(|v| v.as_str())
            .map(|s| s.to_string())
            .collect(),
        _ => Vec::new(),
    }
}

fn build_memory_record_event(
    id: &str,
    lane: &str,
    kind: Option<&String>,
    key: Option<&String>,
    value: &Value,
    tags: &[String],
    score: Option<f64>,
    prob: Option<f64>,
    hash: &str,
    updated: &str,
) -> Value {
    let mut map = Map::new();
    map.insert("id".into(), json!(id));
    map.insert("lane".into(), json!(lane));
    if let Some(k) = kind {
        map.insert("kind".into(), json!(k));
    }
    if let Some(k) = key {
        map.insert("key".into(), json!(k));
    }
    map.insert("value".into(), value.clone());
    map.insert("tags".into(), json!(tags));
    if let Some(s) = score {
        map.insert("score".into(), json!(s));
    }
    if let Some(p) = prob {
        map.insert("prob".into(), json!(p));
    }
    if !hash.is_empty() {
        map.insert("hash".into(), json!(hash));
    }
    map.insert("updated".into(), json!(updated));
    let mut value = Value::Object(map);
    util::attach_memory_ptr(&mut value);
    value
}

fn build_memory_applied_event(record: &Value, source: &str) -> Value {
    let mut obj = record.as_object().cloned().unwrap_or_else(Map::new);
    obj.insert("source".into(), json!(source));
    let value_clone = obj.get("value").cloned();
    if let Some(value) = value_clone {
        if let Some((preview, truncated)) = preview_from_value(&value) {
            obj.insert("value_preview".into(), json!(preview));
            obj.insert("value_preview_truncated".into(), json!(truncated));
        }
        if let Ok(bytes) = serde_json::to_vec(&value) {
            obj.insert("value_bytes".into(), json!(bytes.len()));
        }
        obj.insert("value".into(), value);
    }
    if !obj.contains_key("applied_at") {
        if let Some(updated) = obj.get("updated").cloned() {
            obj.insert("applied_at".into(), updated);
        } else {
            obj.insert("applied_at".into(), json!(now_timestamp()));
        }
    }
    Value::Object(obj)
}

const MEMORY_SNAPSHOT_EVENT: &str = "memory.snapshot";
const MEMORY_PATCH_EVENT: &str = "memory.patch";

/// Stream memory read-model patches and snapshots via SSE.
#[utoipa::path(
    get,
    path = "/state/memory",
    tag = "Memory",
    responses(
        (status = 200, description = "Memory stream", content_type = "text/event-stream"),
        (status = 501, description = "Kernel disabled", body = serde_json::Value),
        (status = 500, description = "Kernel error", body = serde_json::Value)
    )
)]
pub async fn state_memory_stream(State(state): State<AppState>) -> axum::response::Response {
    if !state.kernel_enabled() {
        return crate::responses::kernel_disabled();
    }

    let mut current_snapshot =
        if let Some(value) = crate::read_models::cached_read_model("memory_recent") {
            value
        } else {
            match state.kernel().list_recent_memory_async(None, 200).await {
                Ok(items) => json!({
                    "items": attach_memory_ptrs(items),
                    "generated": now_timestamp(),
                }),
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

    if let Some(items) = current_snapshot
        .get_mut("items")
        .and_then(|value| value.as_array_mut())
    {
        for item in items.iter_mut() {
            util::attach_memory_ptr(item);
        }
    }

    let (tx, rx) = mpsc::channel::<Result<Event, Infallible>>(64);
    let state_clone = state.clone();
    let sender = tx.clone();
    tokio::spawn(async move {
        if let Ok(event) = Event::default()
            .event(MEMORY_SNAPSHOT_EVENT)
            .json_data(&json!({"snapshot": current_snapshot.clone()}))
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
            if id != "memory_recent" {
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

/// Most recent memories (per lane).
#[cfg_attr(
    not(test),
    utoipa::path(
        get,
        path = "/state/memory/recent",
        tag = "Memory",
        params(("lane" = Option<String>, Query), ("limit" = Option<i64>, Query)),
        responses(
            (status = 200, body = serde_json::Value),
            (status = 501, description = "Kernel disabled", body = serde_json::Value)
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
#[cfg_attr(
    not(test),
    utoipa::path(
        post,
        path = "/admin/memory/apply",
        tag = "Admin/Memory",
        request_body = MemoryApplyReq,
        responses(
            (status = 201, description = "Created", body = serde_json::Value),
            (status = 401, description = "Unauthorized"),
            (status = 501, description = "Kernel disabled", body = serde_json::Value)
        )
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
            let default_updated = now_timestamp();
            let mut stored_value = value.clone();
            let mut stored_tags = tags.clone().unwrap_or_default();
            let mut stored_hash: Option<String> = None;
            let mut updated: Option<String> = None;

            match state.kernel().get_memory_async(id.clone()).await {
                Ok(Some(record)) => {
                    if let Some(obj) = record.as_object() {
                        if let Some(v) = obj.get("value") {
                            stored_value = v.clone();
                        }
                        if stored_tags.is_empty() {
                            stored_tags = parse_tags_field(obj.get("tags"));
                        }
                        if let Some(h) = obj.get("hash").and_then(|v| v.as_str()) {
                            stored_hash = Some(h.to_string());
                        }
                        if let Some(u) = obj.get("updated").and_then(|v| v.as_str()) {
                            updated = Some(u.to_string());
                        }
                    }
                }
                Ok(None) => {
                    warn!("memory: inserted id {id} missing on reload");
                }
                Err(err) => {
                    warn!(?err, "memory: failed to reload record {id}");
                }
            }

            let normalized_tags = normalize_tags(&stored_tags);
            let stored_hash = stored_hash
                .unwrap_or_else(|| compute_memory_hash(&lane, &kind, &key, &stored_value));
            let updated = updated.unwrap_or(default_updated);

            let record_event = build_memory_record_event(
                &id,
                &lane,
                kind.as_ref(),
                key.as_ref(),
                &stored_value,
                &normalized_tags,
                score,
                prob,
                &stored_hash,
                &updated,
            );

            state
                .bus()
                .publish(topics::TOPIC_MEMORY_RECORD_PUT, &record_event);

            let applied_event = build_memory_applied_event(&record_event, "admin.memory.apply");
            state
                .bus()
                .publish(topics::TOPIC_MEMORY_APPLIED, &applied_event);

            let body = json!({
                "id": id,
                "record": record_event,
                "applied": applied_event
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
            (status = 401, description = "Unauthorized"),
            (status = 501, description = "Kernel disabled", body = serde_json::Value)
        )
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::read_models;
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
    use tokio::sync::Mutex;
    use tokio::time::{timeout, Duration};
    use tower::ServiceExt;

    async fn build_state(dir: &std::path::Path) -> AppState {
        std::env::set_var("ARW_DEBUG", "1");
        crate::util::reset_state_dir_for_tests();
        std::env::set_var("ARW_STATE_DIR", dir.display().to_string());
        let bus = arw_events::Bus::new_with_replay(64, 64);
        let kernel = arw_kernel::Kernel::open(dir).expect("init kernel for tests");
        let policy = PolicyEngine::load_from_env();
        let policy_arc = Arc::new(Mutex::new(policy));
        let host: Arc<dyn ToolHost> = Arc::new(arw_wasi::NoopHost);
        AppState::builder(bus, kernel, policy_arc, host, true)
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
    async fn memory_stream_provides_snapshot_and_patch() {
        let temp = tempdir().expect("tmp");
        let _state_guard = crate::util::scoped_state_dir_for_tests(temp.path());
        let state = build_state(temp.path()).await;

        let initial_value = json!({"text": "hello"});
        let _ = state
            .kernel()
            .insert_memory_async(
                None,
                "semantic".to_string(),
                Some("note".to_string()),
                Some("hello".to_string()),
                initial_value,
                None,
                Some(vec!["demo".to_string()]),
                Some(0.8),
                None,
            )
            .await
            .expect("insert memory");

        let snapshot_now = state
            .kernel()
            .list_recent_memory_async(None, 200)
            .await
            .expect("list memory");
        read_models::publish_read_model_patch(
            &state.bus(),
            "memory_recent",
            &json!({"items": attach_memory_ptrs(snapshot_now.clone()), "generated": now_timestamp()}),
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
            let frame = body.frame().await.expect("frame").expect("data frame");
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

        let _ = state
            .kernel()
            .insert_memory_async(
                None,
                "semantic".to_string(),
                Some("note".to_string()),
                Some("second".to_string()),
                json!({"text": "second"}),
                None,
                Some(vec!["demo".to_string()]),
                Some(0.5),
                None,
            )
            .await
            .expect("insert second memory");

        let updated_snapshot = state
            .kernel()
            .list_recent_memory_async(None, 200)
            .await
            .expect("list updated memory");
        read_models::publish_read_model_patch(
            &state.bus(),
            "memory_recent",
            &json!({
                "items": attach_memory_ptrs(updated_snapshot),
                "generated": now_timestamp()
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
    }

    #[tokio::test]
    async fn memory_apply_emits_record_and_applied_events() {
        let temp = tempdir().expect("temp dir");
        let _state_guard = crate::util::scoped_state_dir_for_tests(temp.path());
        let state = build_state(temp.path()).await;
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
            tags: Some(vec!["alpha".into(), "Alpha".into(), "notes".into()]),
            embed: None,
            score: Some(0.42),
            prob: Some(0.84),
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
        assert_eq!(
            response_json["applied"]["value_preview"].as_str().is_some(),
            true
        );
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
