use axum::response::sse::{Event, Sse};
use axum::response::IntoResponse;
use axum::{extract::State, Json};
use serde::Deserialize;
use serde_json::{json, Value};
use std::convert::Infallible;
use std::future::ready;
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;
use tokio_stream::StreamExt;
use utoipa::ToSchema;

use crate::{
    context_loop::{
        drive_context_loop, ContextLoopResult, StreamIterationEmitter, SyncIterationCollector,
    },
    util, working_set, AppState,
};
use arw_topics as topics;

#[derive(Deserialize, ToSchema)]
pub(crate) struct AssembleReq {
    #[serde(default)]
    pub proj: Option<String>,
    #[serde(default)]
    pub q: Option<String>,
    #[serde(default)]
    pub embed: Option<Vec<f32>>,
    #[serde(default)]
    pub lanes: Option<Vec<String>>,
    #[serde(default)]
    pub limit: Option<usize>,
    #[serde(default)]
    pub expand_per_seed: Option<usize>,
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
    pub expand_query: Option<bool>,
    #[serde(default)]
    pub expand_query_top_k: Option<usize>,
    #[serde(default)]
    pub scorer: Option<String>,
    #[serde(default)]
    pub stream: Option<bool>,
    #[serde(default)]
    pub max_iterations: Option<usize>,
    #[serde(default)]
    pub corr_id: Option<String>,
}

/// Assemble context working set; optionally stream iterations via SSE.
#[utoipa::path(
    post,
    path = "/context/assemble",
    tag = "Context",
    request_body = AssembleReq,
    responses(
        (status = 200, description = "Assembled context", body = serde_json::Value),
        (status = 501, description = "Kernel disabled", body = serde_json::Value)
    )
)]
pub async fn context_assemble(
    State(state): State<AppState>,
    Json(req): Json<AssembleReq>,
) -> axum::response::Response {
    if !state.kernel_enabled() {
        return crate::responses::kernel_disabled();
    }
    let include_sources = req.include_sources.unwrap_or(false);
    let debug = req.debug.unwrap_or(false);
    let base_spec = build_spec(&req);
    let corr_id = req.corr_id.clone();
    let stream_requested = req
        .stream
        .unwrap_or_else(working_set::default_streaming_enabled);
    let max_iterations = req
        .max_iterations
        .unwrap_or_else(working_set::default_max_iterations)
        .clamp(1, 6);

    if stream_requested {
        return stream_working_set(
            state,
            base_spec.clone(),
            include_sources,
            debug,
            max_iterations,
            corr_id.clone(),
        )
        .await;
    }

    let mut collector = SyncIterationCollector::new(debug);
    let loop_result = drive_context_loop(
        state.clone(),
        base_spec.clone(),
        corr_id.clone(),
        max_iterations,
        None,
        debug,
        |event| {
            collector.observe(&event);
            ready(())
        },
    )
    .await;

    let ContextLoopResult {
        final_spec,
        last_verdict,
        final_working_set,
        error,
    } = loop_result;

    if let Some(err) = error {
        return (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({
                "type": "about:blank",
                "title": "Error",
                "status": 500,
                "detail": err.detail
            })),
        )
            .into_response();
    }

    let ws = match final_working_set {
        Some(ws) => ws,
        None => {
            return (
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({
                    "type": "about:blank",
                    "title": "Error",
                    "status": 500,
                    "detail": "context assembly did not produce a working set"
                })),
            )
                .into_response();
        }
    };

    let iterations_meta = collector.into_inner();

    let working_set::WorkingSet {
        items,
        seeds,
        expanded,
        diagnostics,
        summary,
    } = ws;
    let mut working = json!({
        "items": items,
        "counts": {
            "items": items.len(),
            "seeds": seeds.len(),
            "expanded": expanded.len()
        },
        "summary": summary.to_json(),
        "iterations": Value::Array(iterations_meta.clone()),
        "coverage": json!({
            "needs_more": last_verdict.needs_more,
            "reasons": last_verdict.reasons
        })
    });
    working["final_spec"] = final_spec.snapshot();
    if include_sources || debug {
        working["seeds"] = json!(seeds);
        working["expanded"] = json!(expanded);
    }
    if debug {
        working["diagnostics"] = diagnostics;
    }
    let beliefs = working.get("items").cloned().unwrap_or_else(|| json!([]));
    let mut body = json!({
        "query": req.q,
        "project": req.proj,
        "lanes": final_spec.lanes.clone(),
        "limit": final_spec.limit,
        "expand_per_seed": final_spec.expand_per_seed,
        "diversity_lambda": final_spec.diversity_lambda,
        "min_score": final_spec.min_score,
        "scorer": final_spec.scorer_label(),
        "expand_query": final_spec.expand_query,
        "expand_query_top_k": final_spec.expand_query_top_k,
        "max_iterations": max_iterations,
        "working_set": working,
        "beliefs": beliefs
    });
    if let Some(obj) = body.as_object_mut() {
        obj.insert("requested_spec".into(), base_spec.snapshot());
    }
    if let Some(embed) = req.embed.clone() {
        if let Some(obj) = body.as_object_mut() {
            obj.insert("embed".into(), json!(embed));
        }
    }
    if let Some(obj) = body.as_object_mut() {
        if let Some(cid) = corr_id {
            obj.insert("corr_id".into(), json!(cid));
        }
    }
    (axum::http::StatusCode::OK, Json(body)).into_response()
}

async fn stream_working_set(
    state: AppState,
    base_spec: working_set::WorkingSetSpec,
    include_sources: bool,
    debug: bool,
    max_iterations: usize,
    corr_id: Option<String>,
) -> axum::response::Response {
    if !state.kernel_enabled() {
        return crate::responses::kernel_disabled();
    }
    let (tx, rx) = mpsc::channel::<working_set::WorkingSetStreamEvent>(128);
    let state_clone = state.clone();
    let spec_clone = base_spec.clone();
    let corr_for_task = corr_id.clone();
    tokio::spawn(async move {
        let stream_sender = tx.clone();
        let emitter = StreamIterationEmitter::new(stream_sender.clone());
        let _ = drive_context_loop(
            state_clone,
            spec_clone,
            corr_for_task,
            max_iterations,
            Some(stream_sender),
            false,
            move |event| emitter.handle(event),
        )
        .await;
    });

    let stream = ReceiverStream::new(rx).filter_map(move |evt| {
        let include_sources = include_sources;
        let debug = debug;
        if !include_sources
            && matches!(
                evt.kind.as_str(),
                working_set::STREAM_EVENT_SEED
                    | working_set::STREAM_EVENT_EXPANDED
                    | working_set::STREAM_EVENT_QUERY_EXPANDED
            )
        {
            return None;
        }
        let mut payload = evt.payload;
        let corr_meta = payload.as_object().and_then(|m| m.get("corr_id")).cloned();
        let project_meta = payload.as_object().and_then(|m| m.get("project")).cloned();
        let query_meta = payload.as_object().and_then(|m| m.get("query")).cloned();
        if let Some(obj) = payload.as_object_mut() {
            if !debug {
                obj.remove("diagnostics");
            }
            if !include_sources {
                obj.remove("seeds");
                obj.remove("expanded");
            }
            // ensure metadata is reflected at the envelope level even if removed here
        }
        let mut data_map = serde_json::Map::new();
        data_map.insert("iteration".into(), json!(evt.iteration));
        if let Some(cid) = corr_meta {
            data_map.insert("corr_id".into(), cid);
        }
        if let Some(project) = project_meta {
            data_map.insert("project".into(), project);
        }
        if let Some(query) = query_meta {
            data_map.insert("query".into(), query);
        }
        data_map.insert("payload".into(), payload);
        let data =
            serde_json::to_string(&Value::Object(data_map)).unwrap_or_else(|_| "{}".to_string());
        let event = Event::default().event(evt.kind).data(data);
        Some(Ok::<_, Infallible>(event))
    });
    Sse::new(stream).into_response()
}

fn build_spec(req: &AssembleReq) -> working_set::WorkingSetSpec {
    let mut lanes = if let Some(list) = req.lanes.clone() {
        if list.is_empty() {
            working_set::default_lanes()
        } else {
            list
        }
    } else {
        working_set::default_lanes()
    };
    if lanes.is_empty() {
        lanes = working_set::default_lanes();
    }
    let mut spec = working_set::WorkingSetSpec {
        query: req.q.clone(),
        embed: req.embed.clone(),
        lanes,
        limit: req.limit.unwrap_or_else(working_set::default_limit),
        expand_per_seed: req
            .expand_per_seed
            .unwrap_or_else(working_set::default_expand_per_seed),
        diversity_lambda: req
            .diversity_lambda
            .unwrap_or_else(working_set::default_diversity_lambda),
        min_score: req.min_score.unwrap_or_else(working_set::default_min_score),
        project: req.proj.clone(),
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

#[derive(Deserialize, ToSchema)]
pub(crate) struct RehydrateReq {
    pub ptr: Value,
}
/// Rehydrate a pointer (file head or memory record), gated by policy/leases.
#[utoipa::path(
    post,
    path = "/context/rehydrate",
    tag = "Context",
    request_body = RehydrateReq,
    responses(
        (status = 200, body = serde_json::Value),
        (status = 403),
        (status = 400),
        (status = 404),
        (status = 501, description = "Kernel disabled", body = serde_json::Value)
    )
)]
pub async fn context_rehydrate(
    State(state): State<AppState>,
    Json(req): Json<RehydrateReq>,
) -> axum::response::Response {
    if !state.kernel_enabled() {
        return crate::responses::kernel_disabled();
    }
    let kind = req.ptr.get("kind").and_then(|v| v.as_str()).unwrap_or("");
    match kind {
        "file" => {
            let allow_action = state
                .policy()
                .lock()
                .await
                .evaluate_action("context.rehydrate")
                .allow;
            if !allow_action {
                let has_file_lease = state
                    .kernel()
                    .find_valid_lease_async("local", "context:rehydrate:file")
                    .await
                    .ok()
                    .flatten()
                    .is_some();
                let has_fs_lease = if has_file_lease {
                    true
                } else {
                    state
                        .kernel()
                        .find_valid_lease_async("local", "fs")
                        .await
                        .ok()
                        .flatten()
                        .is_some()
                };
                if !has_file_lease && !has_fs_lease {
                    state.bus().publish(
                        topics::TOPIC_POLICY_DECISION,
                        &json!({
                            "action": "context.rehydrate",
                            "allow": false,
                            "require_capability": "context:rehydrate:file|fs",
                            "explain": {"reason":"lease_required"}
                        }),
                    );
                    return (
                        axum::http::StatusCode::FORBIDDEN,
                        Json(
                            json!({"type":"about:blank","title":"Forbidden","status":403, "detail":"Lease required: context:rehydrate:file or fs"}),
                        ),
                    )
                        .into_response();
                }
            }
            let path = match req.ptr.get("path").and_then(|v| v.as_str()) {
                Some(s) => std::path::PathBuf::from(s),
                None => {
                    return (
                        axum::http::StatusCode::BAD_REQUEST,
                        Json(
                            json!({"type":"about:blank","title":"Bad Request","status":400, "detail":"missing path"}),
                        ),
                    )
                        .into_response();
                }
            };
            let cap_kb: u64 = std::env::var("ARW_REHYDRATE_FILE_HEAD_KB")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(64);
            match tokio::fs::metadata(&path).await {
                Ok(m) if m.is_file() => {
                    let take = std::cmp::min(m.len(), cap_kb * 1024);
                    let f = match tokio::fs::File::open(&path).await {
                        Ok(f) => f,
                        Err(_) => {
                            return (
                                axum::http::StatusCode::NOT_FOUND,
                                Json(
                                    json!({"type":"about:blank","title":"Not Found","status":404}),
                                ),
                            )
                                .into_response();
                        }
                    };
                    let mut buf = vec![0u8; take as usize];
                    use tokio::io::AsyncReadExt as _;
                    use tokio::io::BufReader as TokioBufReader;
                    let mut br = TokioBufReader::new(f);
                    let n = br.read(&mut buf).await.unwrap_or(0);
                    let content = String::from_utf8_lossy(&buf[..n]).to_string();
                    (
                        axum::http::StatusCode::OK,
                        Json(
                            json!({"ptr": req.ptr, "file": {"path": path.to_string_lossy(), "size": m.len(), "head_bytes": n as u64, "truncated": (m.len() as usize) > n }, "content": content}),
                        ),
                    )
                        .into_response()
                }
                _ => (
                    axum::http::StatusCode::BAD_REQUEST,
                    Json(
                        json!({"type":"about:blank","title":"Bad Request","status":400, "detail":"not a file"}),
                    ),
                )
                    .into_response(),
            }
        }
        "memory" => {
            let id = match req.ptr.get("id").and_then(|v| v.as_str()) {
                Some(s) if !s.is_empty() => s.to_string(),
                _ => {
                    return (
                        axum::http::StatusCode::BAD_REQUEST,
                        Json(
                            json!({"type":"about:blank","title":"Bad Request","status":400, "detail":"missing id"}),
                        ),
                    )
                        .into_response();
                }
            };
            let decision = state
                .policy()
                .lock()
                .await
                .evaluate_action("context.rehydrate.memory");
            if !decision.allow {
                let mut required_caps: Vec<String> = Vec::new();
                if let Some(cap) = decision.require_capability.clone() {
                    required_caps.push(cap);
                }
                for cap in ["context:rehydrate:memory", "context:rehydrate:file"] {
                    if !required_caps.iter().any(|c| c == cap) {
                        required_caps.push(cap.to_string());
                    }
                }
                let mut has_lease = false;
                for cap in &required_caps {
                    if state
                        .kernel()
                        .find_valid_lease_async("local", cap)
                        .await
                        .ok()
                        .flatten()
                        .is_some()
                    {
                        has_lease = true;
                        break;
                    }
                }
                if !has_lease {
                    let require_str = required_caps.join("|");
                    let require_human = required_caps.join(" or ");
                    state.bus().publish(
                        topics::TOPIC_POLICY_DECISION,
                        &json!({
                            "action": "context.rehydrate.memory",
                            "allow": false,
                            "require_capability": require_str,
                            "explain": {"reason":"lease_required"}
                        }),
                    );
                    return (
                        axum::http::StatusCode::FORBIDDEN,
                        Json(json!({
                            "type": "about:blank",
                            "title": "Forbidden",
                            "status": 403,
                            "detail": format!("Lease required: {}", require_human)
                        })),
                    )
                        .into_response();
                }
            }
            match state.kernel().get_memory_async(id.clone()).await {
                Ok(Some(mut record)) => {
                    util::attach_memory_ptr(&mut record);
                    (
                        axum::http::StatusCode::OK,
                        Json(json!({"ptr": req.ptr, "memory": record})),
                    )
                        .into_response()
                }
                Ok(None) => (
                    axum::http::StatusCode::NOT_FOUND,
                    Json(json!({"type":"about:blank","title":"Not Found","status":404})),
                )
                    .into_response(),
                Err(e) => (
                    axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                    Json(json!({
                        "type": "about:blank",
                        "title": "Error",
                        "status": 500,
                        "detail": e.to_string()
                    })),
                )
                    .into_response(),
            }
        }
        _ => (
            axum::http::StatusCode::BAD_REQUEST,
            Json(
                json!({"type":"about:blank","title":"Bad Request","status":400, "detail":"unsupported ptr kind"}),
            ),
        )
            .into_response(),
    }
}
