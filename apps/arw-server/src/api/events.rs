use axum::http::HeaderMap;
use axum::response::IntoResponse;
use axum::{
    extract::{Query, State},
    response::sse::{Event as SseEvent, KeepAlive, Sse},
};
use chrono::SecondsFormat;
use tokio_stream::StreamExt as _;
use uuid::Uuid;
// no local json macro use here

use crate::AppState;
use arw_topics as topics;
use sha2::Digest as _;

/// Server‑Sent Events stream of envelopes.
#[utoipa::path(
    get,
    path = "/events",
    tag = "Events",
    operation_id = "events_sse_doc",
    description = "Server‑Sent Events stream of envelopes; supports replay and prefix filters.",
    params(
        ("after" = Option<i64>, Query, description = "Resume after id or Last-Event-ID header"),
        ("replay" = Option<usize>, Query, description = "Replay the last N events (when after not set)"),
        ("prefix" = Option<String>, Query, description = "CSV of event kind prefixes to include")
    ),
    responses(
        (status = 200, description = "SSE stream of events", content_type = "text/event-stream"),
        (status = 501, description = "Kernel disabled", body = serde_json::Value)
    )
)]
pub async fn events_sse(
    State(state): State<AppState>,
    Query(q): Query<std::collections::HashMap<String, String>>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if !crate::admin_ok(&headers) {
        return (
            axum::http::StatusCode::UNAUTHORIZED,
            axum::Json(serde_json::json!({
                "type":"about:blank","title":"Unauthorized","status":401
            })),
        )
            .into_response();
    }
    if !state.kernel_enabled()
        && (q.contains_key("after")
            || q.contains_key("replay")
            || headers.get("last-event-id").is_some())
    {
        return (
            axum::http::StatusCode::NOT_IMPLEMENTED,
            axum::Json(serde_json::json!({
                "type":"about:blank",
                "title":"Kernel Disabled",
                "status":501,
                "detail":"Event replay is unavailable when ARW_KERNEL_ENABLE=0"
            })),
        )
            .into_response();
    }
    let (tx, rx) = tokio::sync::mpsc::channel::<(arw_events::Envelope, Option<String>)>(128);
    let last_event_id_hdr: Option<String> = headers
        .get("last-event-id")
        .and_then(|h| h.to_str().ok())
        .map(|s| s.to_string());
    let resume_from = q.get("after").cloned().or(last_event_id_hdr.clone());
    let replay_param = q
        .get("replay")
        .and_then(|s| s.parse::<usize>().ok())
        .unwrap_or_default();
    let prefixes: Vec<String> = q
        .get("prefix")
        .map(|s| {
            s.split(',')
                .map(|p| p.trim())
                .filter(|p| !p.is_empty())
                .map(|p| p.to_string())
                .collect()
        })
        .unwrap_or_default();

    let request_id = headers
        .get("x-request-id")
        .and_then(|h| h.to_str().ok())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .unwrap_or_else(|| Uuid::new_v4().to_string());
    let replay_mode = if resume_from.is_some() {
        "after"
    } else if replay_param > 0 {
        "recent"
    } else {
        "live"
    };
    let handshake_payload = serde_json::json!({
        "request_id": request_id,
        "resume_from": resume_from,
        "replay": {
            "mode": replay_mode,
            "count": replay_param
        },
        "prefixes": if prefixes.is_empty() {
            serde_json::Value::Null
        } else {
            serde_json::Value::from(prefixes.clone())
        },
        "kernel_replay": state.kernel_enabled(),
    });
    let handshake_env = arw_events::Envelope {
        time: chrono::Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true),
        kind: topics::TOPIC_SERVICE_CONNECTED.into(),
        payload: handshake_payload,
        policy: None,
        ce: None,
    };
    let _ = tx.send((handshake_env, Some("0".to_string()))).await;

    // Optional resume: prioritize after=ID or Last-Event-ID over replay
    let mut did_replay = false;
    if let Some(after_s) = resume_from.clone() {
        if let Ok(aid) = after_s.parse::<i64>() {
            if let Ok(rows) = state.kernel().recent_events_async(1000, Some(aid)).await {
                let tx2 = tx.clone();
                tokio::spawn(async move {
                    for r in rows {
                        let env = arw_events::Envelope {
                            time: r.time,
                            kind: r.kind,
                            payload: r.payload,
                            policy: None,
                            ce: None,
                        };
                        let _ = tx2.send((env, Some(r.id.to_string()))).await;
                    }
                });
                did_replay = true;
            }
        }
    }
    // Optional replay=N parameter (only if no after/Last-Event-ID)
    if !did_replay && replay_param > 0 {
        if let Ok(rows) = state
            .kernel()
            .recent_events_async(replay_param as i64, None)
            .await
        {
            let tx2 = tx.clone();
            tokio::spawn(async move {
                for r in rows {
                    let env = arw_events::Envelope {
                        time: r.time,
                        kind: r.kind,
                        payload: r.payload,
                        policy: None,
                        ce: None,
                    };
                    let _ = tx2.send((env, Some(r.id.to_string()))).await;
                }
            });
        }
    }
    let mut bus_rx = state.bus().subscribe();
    let sse_ids = state.sse_ids();
    tokio::spawn(async move {
        while let Ok(env) = bus_rx.recv().await {
            if prefixes.is_empty() || prefixes.iter().any(|p| env.kind.starts_with(p)) {
                let mut hasher = sha2::Sha256::new();
                hasher.update(env.time.as_bytes());
                hasher.update(env.kind.as_bytes());
                if let Ok(pbytes) = serde_json::to_vec(&env.payload) {
                    hasher.update(&pbytes);
                }
                let digest = hasher.finalize();
                let key = u64::from_le_bytes([
                    digest[0], digest[1], digest[2], digest[3], digest[4], digest[5], digest[6],
                    digest[7],
                ]);
                let id_opt = {
                    let cache = sse_ids.lock().await;
                    cache.get(key).map(|v| v.to_string())
                };
                let _ = tx.send((env, id_opt)).await;
            }
        }
    });
    let mode = std::env::var("ARW_EVENTS_SSE_MODE")
        .ok()
        .unwrap_or_else(|| "envelope".into());
    let decorate = std::env::var("ARW_EVENTS_SSE_DECORATE")
        .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
        .unwrap_or(false);
    let stream_request_id = request_id.clone();
    let stream = tokio_stream::wrappers::ReceiverStream::new(rx).map(move |(env, sid)| {
        let mut ev = SseEvent::default().event(env.kind.clone());
        if let Some(idv) = sid.clone() {
            ev = ev.id(idv);
        }
        let data = if mode == "ce-structured" {
            // Basic CloudEvents 1.0 structured JSON
            let ce = serde_json::json!({
                "specversion": "1.0",
                "id": sid.clone().unwrap_or_else(|| {
                    let mut hasher = sha2::Sha256::new();
                    hasher.update(env.time.as_bytes());
                    hasher.update(env.kind.as_bytes());
                    if let Ok(pbytes) = serde_json::to_vec(&env.payload) { hasher.update(&pbytes); }
                    let digest = hasher.finalize();
                    hex::encode(digest)
                }),
                "type": env.kind,
                "source": "urn:arw:server",
                "time": env.time,
                "datacontenttype": "application/json",
                "data": env.payload
            });
            serde_json::to_string(&ce).unwrap_or("{}".to_string())
        } else {
            serde_json::to_string(&env).unwrap_or("{}".to_string())
        };
        if decorate {
            let mut final_data =
                serde_json::from_str::<serde_json::Value>(&data).unwrap_or(serde_json::json!({}));
            if let serde_json::Value::Object(ref mut obj) = final_data {
                obj.entry("request_id")
                    .or_insert_with(|| serde_json::Value::String(stream_request_id.clone()));
            }
            ev = ev.data(serde_json::to_string(&final_data).unwrap_or("{}".to_string()));
        } else {
            ev = ev.data(data);
        }
        Result::<SseEvent, std::convert::Infallible>::Ok(ev)
    });
    let mut response = Sse::new(stream)
        .keep_alive(
            KeepAlive::new()
                .interval(std::time::Duration::from_secs(10))
                .text("keep-alive"),
        )
        .into_response();
    response
        .headers_mut()
        .insert("x-request-id", request_id.parse().unwrap());
    response
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{read_models, AppState};
    use arw_topics::TOPIC_READMODEL_PATCH;
    use axum::http::{HeaderMap, HeaderName, HeaderValue, StatusCode};
    use http_body_util::BodyExt;
    use serde_json::json;
    use sha2::Digest;
    use std::{collections::HashMap, sync::Arc, time::Duration};
    use tempfile::tempdir;
    use tokio::{sync::Mutex, time::timeout};

    async fn build_state(path: &std::path::Path) -> AppState {
        std::env::set_var("ARW_STATE_DIR", path.display().to_string());
        let bus = arw_events::Bus::new_with_replay(64, 64);
        let kernel = arw_kernel::Kernel::open(path).expect("init kernel for tests");
        let policy = arw_policy::PolicyEngine::load_from_env();
        let policy_arc = Arc::new(Mutex::new(policy));
        let host: Arc<dyn arw_wasi::ToolHost> = Arc::new(arw_wasi::NoopHost);
        AppState::builder(bus, kernel, policy_arc, host, true)
            .with_sse_capacity(64)
            .build()
            .await
    }

    fn parse_sse_events(buffer: &mut String) -> Vec<SseRecord> {
        let mut out = Vec::new();
        while let Some(idx) = buffer.find("\n\n") {
            let event_chunk = buffer[..idx].to_string();
            *buffer = buffer[idx + 2..].to_string();
            if !event_chunk.trim().is_empty() {
                out.push(SseRecord::from_chunk(&event_chunk));
            }
        }
        out
    }

    #[derive(Debug, Default, Clone)]
    struct SseRecord {
        event: Option<String>,
        id: Option<String>,
        data: Option<String>,
    }

    impl SseRecord {
        fn from_chunk(chunk: &str) -> Self {
            let mut record = Self::default();
            for line in chunk.lines() {
                if let Some(rest) = line.strip_prefix("event: ") {
                    record.event = Some(rest.trim().to_string());
                } else if let Some(rest) = line.strip_prefix("id: ") {
                    record.id = Some(rest.trim().to_string());
                } else if let Some(rest) = line.strip_prefix("data: ") {
                    record.data = Some(rest.trim().to_string());
                }
            }
            record
        }
    }

    #[tokio::test]
    async fn events_sse_replays_read_model_patches() {
        let temp = tempdir().expect("tempdir");
        let state = build_state(temp.path()).await;

        let bus = state.bus();
        let mut rx = bus.subscribe();
        read_models::publish_read_model_patch(
            &bus,
            "tests.sse_fixture",
            &json!({"items": [{"id": "fixture"}]}),
        );

        let env = timeout(Duration::from_secs(1), rx.recv())
            .await
            .expect("patch event on bus")
            .expect("bus closed unexpectedly");
        state.metrics().record_event(&env.kind);
        let row_id = state
            .kernel()
            .append_event_async(&env)
            .await
            .expect("append event to kernel");
        let mut hasher = sha2::Sha256::new();
        hasher.update(env.time.as_bytes());
        hasher.update(env.kind.as_bytes());
        if let Ok(payload_bytes) = serde_json::to_vec(&env.payload) {
            hasher.update(&payload_bytes);
        }
        let digest = hasher.finalize();
        let key = u64::from_le_bytes([
            digest[0], digest[1], digest[2], digest[3], digest[4], digest[5], digest[6], digest[7],
        ]);
        {
            let sse_ids = state.sse_ids();
            let mut cache = sse_ids.lock().await;
            cache.insert(key, row_id);
        }
        let row_id_str = row_id.to_string();

        // Initial SSE request with replay
        let mut params = HashMap::new();
        params.insert("prefix".to_string(), TOPIC_READMODEL_PATCH.to_string());
        params.insert("replay".to_string(), "5".to_string());
        let response = events_sse(State(state.clone()), Query(params), HeaderMap::new())
            .await
            .into_response();
        assert_eq!(response.status(), StatusCode::OK);

        let mut body = response.into_body();
        let mut buffer = String::new();
        let mut patch_event: Option<SseRecord> = None;
        while patch_event.is_none() {
            let frame = body
                .frame()
                .await
                .expect("frame available")
                .expect("frame data");
            let bytes = frame.into_data().expect("data frame");
            buffer.push_str(&String::from_utf8_lossy(&bytes));
            for ev in parse_sse_events(&mut buffer) {
                if ev.event.as_deref() == Some(TOPIC_READMODEL_PATCH) {
                    patch_event = Some(ev);
                    break;
                }
            }
        }
        let patch_event = patch_event.expect("patch event parsed");
        assert_eq!(patch_event.event.as_deref(), Some(TOPIC_READMODEL_PATCH));
        assert_eq!(patch_event.id.as_deref(), Some(row_id_str.as_str()));
        let data_json = patch_event
            .data
            .as_ref()
            .and_then(|d| serde_json::from_str::<serde_json::Value>(d).ok())
            .expect("patch data json");
        assert_eq!(data_json["kind"].as_str(), Some(TOPIC_READMODEL_PATCH));
        let payload = data_json
            .get("payload")
            .and_then(|v| v.as_object())
            .expect("patch payload");
        assert_eq!(
            payload.get("id").and_then(|v| v.as_str()),
            Some("tests.sse_fixture")
        );
        let patch_ops = payload
            .get("patch")
            .and_then(|v| v.as_array())
            .expect("patch ops");
        assert!(!patch_ops.is_empty(), "expected diff payload");

        // Resume using Last-Event-ID; expect handshake but no replayed patch
        let mut headers = HeaderMap::new();
        headers.insert(
            HeaderName::from_static("last-event-id"),
            HeaderValue::from_str(&row_id_str).expect("header value"),
        );
        let mut params = HashMap::new();
        params.insert("prefix".to_string(), TOPIC_READMODEL_PATCH.to_string());
        let resume_response = events_sse(State(state.clone()), Query(params), headers)
            .await
            .into_response();
        assert_eq!(resume_response.status(), StatusCode::OK);

        let mut resume_body = resume_response.into_body();
        // Handshake event should arrive immediately
        let frame = resume_body
            .frame()
            .await
            .expect("resume frame")
            .expect("resume data");
        let handshake_bytes = frame.into_data().expect("handshake bytes");
        let mut buffer = String::from_utf8_lossy(&handshake_bytes).to_string();
        let handshake_events = parse_sse_events(&mut buffer);
        assert!(handshake_events
            .iter()
            .any(|ev| ev.event.as_deref() == Some("service.connected")));

        // No replayed patch should arrive within a short timeout
        let no_event = timeout(Duration::from_millis(100), resume_body.frame()).await;
        assert!(no_event.is_err(), "unexpected replay events after resume");
    }
}
