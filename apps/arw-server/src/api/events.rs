use axum::http::{HeaderMap, StatusCode};
use axum::response::IntoResponse;
use axum::{
    extract::{Query, State},
    response::sse::{Event as SseEvent, KeepAlive, Sse},
    Json,
};
use chrono::SecondsFormat;
use serde::{Deserialize, Serialize};
use tokio::task::spawn_blocking;
use tokio_stream::StreamExt as _;
use utoipa::ToSchema;
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
        ("Last-Event-ID" = Option<String>, Header, description = "Resume using Last-Event-ID header"),
        ("prefix" = Option<String>, Query, description = "CSV of event kind prefixes to include")
    ),
    responses(
        (status = 200, description = "SSE stream of events", content_type = "text/event-stream"),
        (status = 401, description = "Unauthorized", body = arw_protocol::ProblemDetails),
        (status = 501, description = "Kernel disabled", body = arw_protocol::ProblemDetails)
    )
)]
pub async fn events_sse(
    State(state): State<AppState>,
    Query(q): Query<std::collections::HashMap<String, String>>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if !crate::admin_ok(&headers) {
        return crate::responses::unauthorized(None);
    }
    if !state.kernel_enabled()
        && (q.contains_key("after")
            || q.contains_key("replay")
            || headers.get("last-event-id").is_some())
    {
        return crate::responses::problem_response(
            axum::http::StatusCode::NOT_IMPLEMENTED,
            "Kernel Disabled",
            Some("Event replay is unavailable when ARW_KERNEL_ENABLE=0"),
        );
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
    let decorate = crate::util::env_bool("ARW_EVENTS_SSE_DECORATE").unwrap_or(false);
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

#[derive(Debug, Deserialize, ToSchema, Default)]
#[serde(default)]
pub struct EventsJournalQuery {
    /// Maximum number of entries to return (default 200, max 1000).
    pub limit: Option<usize>,
    /// Optional CSV of event kind prefixes to include (dot.case).
    pub prefix: Option<String>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct EventsJournalResponse {
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schema(nullable, value_type = Option<Vec<String>>)]
    pub prefixes: Option<Vec<String>>,
    pub limit: usize,
    pub total_matched: usize,
    pub truncated: bool,
    pub skipped_lines: usize,
    pub source_files: Vec<String>,
    #[schema(value_type = Vec<serde_json::Value>)]
    pub entries: Vec<arw_events::Envelope>,
}

#[utoipa::path(
    get,
    path = "/admin/events/journal",
    tag = "Events",
    operation_id = "events_journal_tail",
    params(
        ("limit" = Option<usize>, Query, description = "Max entries to return (default 200, max 1000)"),
        ("prefix" = Option<String>, Query, description = "CSV of event kind prefixes to include")
    ),
    responses(
        (status = 200, description = "Tail of journal entries", body = EventsJournalResponse),
        (status = 401, description = "Unauthorized", body = arw_protocol::ProblemDetails),
        (status = 404, description = "Journal disabled", body = arw_protocol::ProblemDetails),
        (status = 500, description = "Journal read failed", body = arw_protocol::ProblemDetails)
    )
)]
pub async fn events_journal(
    State(state): State<AppState>,
    Query(query): Query<EventsJournalQuery>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if !crate::admin_ok(&headers) {
        return crate::responses::unauthorized(None);
    }
    let Some(journal_path) = state.bus().journal_path() else {
        return crate::responses::problem_response(
            StatusCode::NOT_FOUND,
            "Journal Disabled",
            Some("Set ARW_EVENTS_JOURNAL to enable event journaling."),
        );
    };
    let limit = query.limit.unwrap_or(200).min(1000);
    let prefixes: Vec<String> = query
        .prefix
        .as_ref()
        .map(|s| {
            s.split(',')
                .map(|p| p.trim())
                .filter(|p| !p.is_empty())
                .map(|p| p.to_ascii_lowercase())
                .collect()
        })
        .unwrap_or_default();
    let prefixes_clone = prefixes.clone();
    let journal_result =
        spawn_blocking(move || read_journal_tail(journal_path, limit, prefixes_clone)).await;
    match journal_result {
        Ok(Ok(result)) => {
            let truncated = result.total_matched > result.entries.len();
            let response = EventsJournalResponse {
                prefixes: if prefixes.is_empty() {
                    None
                } else {
                    Some(prefixes)
                },
                limit,
                total_matched: result.total_matched,
                truncated,
                skipped_lines: result.skipped_lines,
                source_files: result.source_files,
                entries: result.entries,
            };
            (StatusCode::OK, Json(response)).into_response()
        }
        Ok(Err(err)) => crate::responses::problem_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            "Journal Read Failed",
            Some(&err),
        ),
        Err(join_err) => crate::responses::problem_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            "Journal Read Failed",
            Some(&format!("blocking task panicked: {}", join_err)),
        ),
    }
}

struct JournalTail {
    entries: Vec<arw_events::Envelope>,
    source_files: Vec<String>,
    total_matched: usize,
    skipped_lines: usize,
}

fn read_journal_tail(
    path: std::path::PathBuf,
    limit: usize,
    prefixes: Vec<String>,
) -> Result<JournalTail, String> {
    use std::collections::VecDeque;
    use std::fs::File;
    use std::io::{BufRead, BufReader};

    let mut ordered_paths: Vec<std::path::PathBuf> = Vec::with_capacity(4);
    for idx in (1..=3).rev() {
        let candidate = path.with_extension(format!("log.{}", idx));
        if candidate.exists() {
            ordered_paths.push(candidate);
        }
    }
    if path.exists() {
        ordered_paths.push(path.clone());
    }
    if ordered_paths.is_empty() {
        return Ok(JournalTail {
            entries: Vec::new(),
            source_files: Vec::new(),
            total_matched: 0,
            skipped_lines: 0,
        });
    }
    let source_files: Vec<String> = ordered_paths
        .iter()
        .map(|p| p.display().to_string())
        .collect();
    if limit == 0 {
        return Ok(JournalTail {
            entries: Vec::new(),
            source_files,
            total_matched: 0,
            skipped_lines: 0,
        });
    }
    let mut buffer = VecDeque::with_capacity(limit);
    let mut matched = 0usize;
    let mut skipped = 0usize;
    for path in &ordered_paths {
        let file = match File::open(path) {
            Ok(f) => f,
            Err(err) => {
                skipped += 1;
                tracing::debug!("events_journal: failed to open {:?}: {}", path, err);
                continue;
            }
        };
        let reader = BufReader::new(file);
        for line in reader.lines() {
            let line = match line {
                Ok(l) => l,
                Err(_) => {
                    skipped += 1;
                    continue;
                }
            };
            if line.trim().is_empty() {
                continue;
            }
            let env: arw_events::Envelope = match serde_json::from_str(&line) {
                Ok(env) => env,
                Err(_) => {
                    skipped += 1;
                    continue;
                }
            };
            if !prefixes.is_empty() && !prefixes.iter().any(|p| env.kind.starts_with(p)) {
                continue;
            }
            matched += 1;
            if buffer.len() == limit {
                buffer.pop_front();
            }
            buffer.push_back(env);
        }
    }

    Ok(JournalTail {
        entries: buffer.into_iter().collect(),
        source_files,
        total_matched: matched,
        skipped_lines: skipped,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{read_models, test_support::env, AppState};
    use arw_topics::{TOPIC_READMODEL_PATCH, TOPIC_SERVICE_STOP, TOPIC_SERVICE_TEST};
    use axum::http::{HeaderMap, HeaderName, HeaderValue, StatusCode};
    use hex;
    use http_body_util::BodyExt;
    use json_patch::Patch;
    use serde_json::{json, Value};
    use sha2::Digest;
    use std::{collections::HashMap, sync::Arc, time::Duration};
    use tempfile::tempdir;
    use tokio::time::timeout;

    async fn build_state(path: &std::path::Path, env_guard: &mut env::EnvGuard) -> AppState {
        build_state_with_kernel(path, env_guard, true).await
    }

    async fn build_state_with_kernel(
        path: &std::path::Path,
        env_guard: &mut env::EnvGuard,
        kernel_enabled: bool,
    ) -> AppState {
        env_guard.set("ARW_DEBUG", "1");
        crate::util::reset_state_dir_for_tests();
        env_guard.set("ARW_STATE_DIR", path.display().to_string());
        let bus = arw_events::Bus::new_with_replay(64, 64);
        let kernel = arw_kernel::Kernel::open(path).expect("init kernel for tests");
        let policy = arw_policy::PolicyEngine::load_from_env();
        let policy_handle = crate::policy::PolicyHandle::new(policy, bus.clone());
        let host: Arc<dyn arw_wasi::ToolHost> = Arc::new(arw_wasi::NoopHost);
        AppState::builder(bus, kernel, policy_handle, host, kernel_enabled)
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

    fn assert_handshake(
        record: &SseRecord,
        expected_resume: Option<&str>,
        expected_mode: &str,
        expected_count: u64,
        expected_prefixes: &[&str],
    ) {
        assert_eq!(record.event.as_deref(), Some("service.connected"));
        assert_eq!(record.id.as_deref(), Some("0"));

        let env = record
            .data
            .as_ref()
            .and_then(|d| serde_json::from_str::<Value>(d).ok())
            .expect("handshake data json");
        assert_eq!(env["kind"].as_str(), Some("service.connected"));
        let payload = env
            .get("payload")
            .and_then(|v| v.as_object())
            .expect("handshake payload");

        let request_id = payload
            .get("request_id")
            .and_then(|v| v.as_str())
            .expect("request id");
        assert!(!request_id.is_empty());

        match expected_resume {
            Some(expected) => assert_eq!(
                payload.get("resume_from").and_then(|v| v.as_str()),
                Some(expected)
            ),
            None => assert!(payload
                .get("resume_from")
                .map(|v| v.is_null())
                .unwrap_or(false)),
        }

        let replay = payload
            .get("replay")
            .and_then(|v| v.as_object())
            .expect("replay payload");
        assert_eq!(
            replay.get("mode").and_then(|v| v.as_str()),
            Some(expected_mode)
        );
        assert_eq!(
            replay.get("count").and_then(|v| v.as_u64()),
            Some(expected_count)
        );

        if expected_prefixes.is_empty() {
            assert!(payload
                .get("prefixes")
                .map(|v| v.is_null())
                .unwrap_or(false));
        } else {
            let prefixes = payload
                .get("prefixes")
                .and_then(|v| v.as_array())
                .expect("prefixes array");
            assert_eq!(prefixes.len(), expected_prefixes.len());
            for (val, expected) in prefixes.iter().zip(expected_prefixes.iter()) {
                assert_eq!(val.as_str(), Some(*expected));
            }
        }

        assert_eq!(
            payload.get("kernel_replay").and_then(|v| v.as_bool()),
            Some(true)
        );
    }

    #[tokio::test]
    async fn events_sse_replays_read_model_patches() {
        let temp = tempdir().expect("tempdir");
        let mut ctx = crate::test_support::begin_state_env(temp.path());
        let state = build_state(temp.path(), &mut ctx.env).await;

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
        let mut handshake_event: Option<SseRecord> = None;
        let mut patch_event: Option<SseRecord> = None;
        while patch_event.is_none() {
            let frame = timeout(Duration::from_secs(1), body.frame())
                .await
                .expect("frame available in time")
                .expect("frame present")
                .expect("frame data");
            let bytes = frame.into_data().expect("data frame");
            buffer.push_str(&String::from_utf8_lossy(&bytes));
            for ev in parse_sse_events(&mut buffer) {
                if handshake_event.is_none() && ev.event.as_deref() == Some("service.connected") {
                    handshake_event = Some(ev.clone());
                }
                if ev.event.as_deref() == Some(TOPIC_READMODEL_PATCH) {
                    patch_event = Some(ev);
                    break;
                }
            }
        }
        let handshake_event = handshake_event.expect("handshake present");
        assert_handshake(
            &handshake_event,
            None,
            "recent",
            5,
            &[TOPIC_READMODEL_PATCH],
        );
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
        let frame = resume_body
            .frame()
            .await
            .expect("resume frame")
            .expect("resume data");
        let handshake_bytes = frame.into_data().expect("handshake bytes");
        let mut resume_buffer = String::from_utf8_lossy(&handshake_bytes).to_string();
        let handshake_event = parse_sse_events(&mut resume_buffer)
            .into_iter()
            .find(|ev| ev.event.as_deref() == Some("service.connected"))
            .expect("handshake event");
        assert_handshake(
            &handshake_event,
            Some(row_id_str.as_str()),
            "after",
            0,
            &[TOPIC_READMODEL_PATCH],
        );

        // No replayed patch should arrive within a short timeout
        let no_event = timeout(Duration::from_millis(100), resume_body.frame()).await;
        assert!(no_event.is_err(), "unexpected replay events after resume");
    }

    #[tokio::test]
    async fn events_sse_replays_projects_patch() {
        let temp = tempdir().expect("tempdir");
        let mut ctx = crate::test_support::begin_state_env(temp.path());
        let state = build_state(temp.path(), &mut ctx.env).await;

        let bus = state.bus();
        let mut rx = bus.subscribe();
        read_models::publish_read_model_patch(
            &bus,
            "projects",
            &json!({
                "items": [{
                    "name": "alpha",
                    "notes": {"content": "welcome", "bytes": 7, "modified": "2025-09-23T00:00:00Z"},
                    "tree": {"paths": {"": []}, "digest": "deadbeef"}
                }]
            }),
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

        let mut params = HashMap::new();
        params.insert("prefix".to_string(), TOPIC_READMODEL_PATCH.to_string());
        params.insert("replay".to_string(), "5".to_string());
        let response = events_sse(State(state.clone()), Query(params), HeaderMap::new())
            .await
            .into_response();
        assert_eq!(response.status(), StatusCode::OK);

        let mut body = response.into_body();
        let mut buffer = String::new();
        let mut handshake_event: Option<SseRecord> = None;
        let mut patch_event: Option<SseRecord> = None;
        while patch_event.is_none() {
            let frame = timeout(Duration::from_secs(1), body.frame())
                .await
                .expect("frame available in time")
                .expect("frame present")
                .expect("frame data");
            let bytes = frame.into_data().expect("data frame");
            buffer.push_str(&String::from_utf8_lossy(&bytes));
            for ev in parse_sse_events(&mut buffer) {
                if handshake_event.is_none() && ev.event.as_deref() == Some("service.connected") {
                    handshake_event = Some(ev.clone());
                }
                if ev.event.as_deref() == Some(TOPIC_READMODEL_PATCH) {
                    patch_event = Some(ev);
                    break;
                }
            }
        }
        let handshake_event = handshake_event.expect("handshake present");
        assert_handshake(
            &handshake_event,
            None,
            "recent",
            5,
            &[TOPIC_READMODEL_PATCH],
        );
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
        assert_eq!(payload.get("id").and_then(|v| v.as_str()), Some("projects"));
        let patch_value = payload.get("patch").cloned().expect("patch value");
        let patch: Patch = serde_json::from_value(patch_value).expect("patch decode");
        let mut doc = json!({});
        json_patch::patch(&mut doc, &patch).expect("apply patch");
        let items = doc["items"].as_array().expect("items array");
        assert_eq!(items.len(), 1);
        let item = &items[0];
        assert_eq!(item["name"].as_str(), Some("alpha"));
        assert_eq!(item["notes"]["content"].as_str(), Some("welcome"));

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
        let frame = resume_body
            .frame()
            .await
            .expect("resume frame")
            .expect("resume data");
        let handshake_bytes = frame.into_data().expect("handshake bytes");
        let mut resume_buffer = String::from_utf8_lossy(&handshake_bytes).to_string();
        let handshake_event = parse_sse_events(&mut resume_buffer)
            .into_iter()
            .find(|ev| ev.event.as_deref() == Some("service.connected"))
            .expect("handshake event");
        assert_handshake(
            &handshake_event,
            Some(row_id_str.as_str()),
            "after",
            0,
            &[TOPIC_READMODEL_PATCH],
        );

        let no_event = timeout(Duration::from_millis(100), resume_body.frame()).await;
        assert!(no_event.is_err(), "unexpected replay events after resume");
    }

    #[tokio::test]
    async fn events_sse_requires_auth_without_token() {
        let temp = tempdir().expect("tempdir");
        let mut ctx = crate::test_support::begin_state_env(temp.path());
        let state = build_state(temp.path(), &mut ctx.env).await;

        ctx.env.set("ARW_DEBUG", "0");
        ctx.env.set("ARW_ADMIN_TOKEN", "secret-token");

        let response = events_sse(State(state), Query(HashMap::new()), HeaderMap::new())
            .await
            .into_response();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

        let body_bytes = response
            .into_body()
            .collect()
            .await
            .expect("collect body")
            .to_bytes();
        let body_json: Value = serde_json::from_slice(&body_bytes).expect("problem body");
        assert_eq!(body_json["title"].as_str(), Some("Unauthorized"));
        assert_eq!(body_json["status"].as_u64(), Some(401));

        ctx.env.remove("ARW_ADMIN_TOKEN");
        ctx.env.remove("ARW_DEBUG");
    }

    #[tokio::test]
    async fn events_sse_rejects_replay_when_kernel_disabled() {
        let temp = tempdir().expect("tempdir");
        let mut ctx = crate::test_support::begin_state_env(temp.path());
        let state = build_state_with_kernel(temp.path(), &mut ctx.env, false).await;

        ctx.env.set("ARW_DEBUG", "1");

        let mut params = HashMap::new();
        params.insert("after".to_string(), "1".to_string());

        let response = events_sse(State(state), Query(params), HeaderMap::new())
            .await
            .into_response();
        assert_eq!(response.status(), StatusCode::NOT_IMPLEMENTED);

        let body_bytes = response
            .into_body()
            .collect()
            .await
            .expect("collect body")
            .to_bytes();
        let body_json: Value = serde_json::from_slice(&body_bytes).expect("json body");
        assert_eq!(body_json["title"].as_str(), Some("Kernel Disabled"));
        assert_eq!(body_json["status"].as_u64(), Some(501));

        ctx.env.remove("ARW_DEBUG");
    }

    #[tokio::test]
    async fn events_sse_accepts_hashed_admin_token() {
        let temp = tempdir().expect("tempdir");
        let mut ctx = crate::test_support::begin_state_env(temp.path());
        let state = build_state(temp.path(), &mut ctx.env).await;

        ctx.env.set("ARW_DEBUG", "0");
        ctx.env.remove("ARW_ADMIN_TOKEN");
        let presented = "secret-token";
        let digest = sha2::Sha256::digest(presented.as_bytes());
        ctx.env.set("ARW_ADMIN_TOKEN_SHA256", hex::encode(digest));

        let mut headers = HeaderMap::new();
        headers.insert(
            axum::http::header::AUTHORIZATION,
            HeaderValue::from_str(&format!("Bearer {}", presented)).expect("header value"),
        );

        let response = events_sse(State(state), Query(HashMap::new()), headers)
            .await
            .into_response();
        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn events_journal_tails_recent_entries() {
        let temp = tempdir().expect("tempdir");
        let mut ctx = crate::test_support::begin_state_env(temp.path());
        let journal_path = temp.path().join("events.jsonl");
        ctx.env.set("ARW_DEBUG", "1");
        ctx.env
            .set("ARW_EVENTS_JOURNAL", journal_path.display().to_string());

        let state = build_state(temp.path(), &mut ctx.env).await;

        let bus = state.bus();
        bus.publish(TOPIC_SERVICE_TEST, &json!({"msg": "first"}));
        bus.publish(TOPIC_SERVICE_STOP, &json!({"msg": "second"}));

        tokio::time::sleep(Duration::from_millis(100)).await;

        let response = events_journal(
            State(state.clone()),
            Query(EventsJournalQuery {
                limit: Some(1),
                prefix: None,
            }),
            HeaderMap::new(),
        )
        .await
        .into_response();
        assert_eq!(response.status(), StatusCode::OK);

        let body_bytes = response
            .into_body()
            .collect()
            .await
            .expect("collect body")
            .to_bytes();
        let body_json: Value = serde_json::from_slice(&body_bytes).expect("json body");
        let entries = body_json["entries"].as_array().expect("entries array");
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0]["kind"].as_str(), Some(TOPIC_SERVICE_STOP));
        assert!(body_json["total_matched"].as_u64().unwrap_or(0) >= 2);
        assert!(body_json["truncated"].as_bool().unwrap_or(false));

        let response_prefix = events_journal(
            State(state),
            Query(EventsJournalQuery {
                limit: Some(5),
                prefix: Some(TOPIC_SERVICE_TEST.to_string()),
            }),
            HeaderMap::new(),
        )
        .await
        .into_response();
        assert_eq!(response_prefix.status(), StatusCode::OK);
        let bytes = response_prefix
            .into_body()
            .collect()
            .await
            .expect("collect body")
            .to_bytes();
        let body_json: Value = serde_json::from_slice(&bytes).expect("json body");
        let entries = body_json["entries"].as_array().expect("entries array");
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0]["kind"].as_str(), Some(TOPIC_SERVICE_TEST));
        assert_eq!(body_json["prefixes"].as_array().map(|a| a.len()), Some(1));
        assert!(body_json["total_matched"].as_u64().unwrap_or(0) >= 1);
        assert!(!body_json["truncated"].as_bool().unwrap());

        ctx.env.remove("ARW_EVENTS_JOURNAL");
        ctx.env.remove("ARW_DEBUG");
    }
}
