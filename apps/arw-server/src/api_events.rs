use axum::http::HeaderMap;
use axum::response::IntoResponse;
use axum::{
    extract::{Query, State},
    response::sse::{Event as SseEvent, KeepAlive, Sse},
};
use tokio_stream::StreamExt as _;
// no local json macro use here

use crate::AppState;
use sha2::Digest as _;

pub async fn events_sse(
    State(state): State<AppState>,
    Query(q): Query<std::collections::HashMap<String, String>>,
    headers: HeaderMap,
) -> impl IntoResponse {
    let (tx, rx) = tokio::sync::mpsc::channel::<(arw_events::Envelope, Option<String>)>(128);
    // Optional resume: prioritize after=ID or Last-Event-ID over replay
    let mut did_replay = false;
    let last_event_id_hdr: Option<String> = headers
        .get("last-event-id")
        .and_then(|h| h.to_str().ok())
        .map(|s| s.to_string());
    if let Some(after_s) = q.get("after").cloned().or(last_event_id_hdr) {
        if let Ok(aid) = after_s.parse::<i64>() {
            if let Ok(rows) = state.kernel.recent_events(1000, Some(aid)) {
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
    if !did_replay {
        if let Some(replay_s) = q.get("replay") {
            if let Ok(n) = replay_s.parse::<usize>() {
                if n > 0 {
                    if let Ok(rows) = state.kernel.recent_events(n as i64, None) {
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
            }
        }
    }
    // Optional prefix filter (CSV or repeated values comma-joined)
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
    let mut bus_rx = state.bus.subscribe();
    let sse_ids = state.sse_id_map.clone();
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
                    let dq = sse_ids.lock().await;
                    dq.iter()
                        .rev()
                        .find(|(k, _)| *k == key)
                        .map(|(_, v)| v.to_string())
                };
                let _ = tx.send((env, id_opt)).await;
            }
        }
    });
    let mode = std::env::var("ARW_EVENTS_SSE_MODE")
        .ok()
        .unwrap_or_else(|| "envelope".into());
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
        ev = ev.data(data);
        Result::<SseEvent, std::convert::Infallible>::Ok(ev)
    });
    Sse::new(stream).keep_alive(
        KeepAlive::new()
            .interval(std::time::Duration::from_secs(10))
            .text("keep-alive"),
    )
}
