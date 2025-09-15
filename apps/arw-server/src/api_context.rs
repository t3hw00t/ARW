use axum::response::sse::{Event, Sse};
use axum::response::IntoResponse;
use axum::{extract::State, Json};
use serde::Deserialize;
use serde_json::{json, Value};
use std::convert::Infallible;
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;
use tokio_stream::StreamExt;

use crate::{coverage, working_set, AppState};

#[derive(Deserialize)]
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
}

pub async fn context_assemble(
    State(state): State<AppState>,
    Json(req): Json<AssembleReq>,
) -> axum::response::Response {
    let include_sources = req.include_sources.unwrap_or(false);
    let debug = req.debug.unwrap_or(false);
    let base_spec = build_spec(&req);
    let stream_requested = req
        .stream
        .unwrap_or_else(working_set::default_streaming_enabled);
    let max_iterations = req
        .max_iterations
        .unwrap_or_else(working_set::default_max_iterations)
        .max(1)
        .min(6);

    if stream_requested {
        return stream_working_set(
            state,
            base_spec.clone(),
            include_sources,
            debug,
            max_iterations,
        )
        .await;
    }

    let mut iterations_meta: Vec<Value> = Vec::new();
    let mut current_spec = base_spec.clone();
    let mut last_verdict = coverage::CoverageVerdict::satisfied();
    let mut final_ws: Option<working_set::WorkingSet> = None;
    for iteration in 0..max_iterations {
        match working_set::assemble(&state, &current_spec) {
            Ok(ws) => {
                let verdict = coverage::assess(&ws);
                let mut entry = serde_json::Map::new();
                entry.insert("index".into(), json!(iteration));
                entry.insert("spec".into(), current_spec.snapshot());
                entry.insert("summary".into(), ws.summary.to_json());
                entry.insert("coverage_gap".into(), json!(verdict.needs_more));
                if !verdict.reasons.is_empty() {
                    entry.insert("reasons".into(), json!(verdict.reasons.clone()));
                }
                if debug {
                    entry.insert("diagnostics".into(), ws.diagnostics.clone());
                }
                iterations_meta.push(Value::Object(entry));
                last_verdict = verdict.clone();
                let needs_more = verdict.needs_more;
                current_spec = if needs_more && iteration + 1 < max_iterations {
                    adjust_spec_for_iteration(&current_spec, &ws, iteration + 1)
                } else {
                    current_spec
                };
                final_ws = Some(ws);
                if !needs_more || iteration + 1 >= max_iterations {
                    break;
                }
            }
            Err(e) => {
                return (
                    axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                    Json(json!({
                        "type": "about:blank",
                        "title": "Error",
                        "status": 500,
                        "detail": e.to_string()
                    })),
                )
                    .into_response();
            }
        }
    }

    let final_spec = current_spec.clone();

    let ws = match final_ws {
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
    (axum::http::StatusCode::OK, Json(body)).into_response()
}

async fn stream_working_set(
    state: AppState,
    base_spec: working_set::WorkingSetSpec,
    include_sources: bool,
    debug: bool,
    max_iterations: usize,
) -> axum::response::Response {
    let (tx, rx) = mpsc::channel::<working_set::WorkingSetStreamEvent>(128);
    let state_clone = state.clone();
    let spec_clone = base_spec.clone();
    tokio::spawn(async move {
        let mut current_spec = spec_clone;
        let mut iteration = 0usize;
        let mut sender = tx;
        loop {
            let state_for_block = state_clone.clone();
            let spec_for_block = current_spec.clone();
            let sender_for_block = sender.clone();
            let result = tokio::task::spawn_blocking(move || {
                let mut observer = working_set::ChannelObserver::new(iteration, sender_for_block);
                let spec_snapshot = spec_for_block.clone();
                working_set::assemble_with_observer(
                    &state_for_block,
                    &spec_for_block,
                    &mut observer,
                )
                .map(|ws| (ws, spec_snapshot))
            })
            .await;

            match result {
                Ok(Ok((ws, spec_used))) => {
                    let verdict = coverage::assess(&ws);
                    let mut payload = serde_json::Map::new();
                    payload.insert("index".into(), json!(iteration));
                    payload.insert("spec".into(), spec_used.snapshot());
                    payload.insert("summary".into(), ws.summary.to_json());
                    payload.insert("coverage_gap".into(), json!(verdict.needs_more));
                    if !verdict.reasons.is_empty() {
                        payload.insert("reasons".into(), json!(verdict.reasons.clone()));
                    }
                    let _ = sender
                        .send(working_set::WorkingSetStreamEvent {
                            iteration,
                            kind: "working_set.iteration.summary".into(),
                            payload: Value::Object(payload),
                        })
                        .await;
                    if verdict.needs_more && iteration + 1 < max_iterations {
                        current_spec = adjust_spec_for_iteration(&spec_used, &ws, iteration + 1);
                        iteration += 1;
                        continue;
                    }
                    break;
                }
                Ok(Err(err)) => {
                    let _ = sender
                        .send(working_set::WorkingSetStreamEvent {
                            iteration,
                            kind: "working_set.error".into(),
                            payload: json!({"error": err.to_string()}),
                        })
                        .await;
                    break;
                }
                Err(join_err) => {
                    let _ = sender
                        .send(working_set::WorkingSetStreamEvent {
                            iteration,
                            kind: "working_set.error".into(),
                            payload: json!({"error": join_err.to_string()}),
                        })
                        .await;
                    break;
                }
            }
        }
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
        if let Some(obj) = payload.as_object_mut() {
            if !debug {
                obj.remove("diagnostics");
            }
            if !include_sources {
                obj.remove("seeds");
                obj.remove("expanded");
            }
        }
        let data_json = json!({
            "iteration": evt.iteration,
            "payload": payload
        });
        let data = serde_json::to_string(&data_json).unwrap_or_else(|_| "{}".to_string());
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

fn adjust_spec_for_iteration(
    prev: &working_set::WorkingSetSpec,
    ws: &working_set::WorkingSet,
    iteration: usize,
) -> working_set::WorkingSetSpec {
    let mut next = prev.clone();
    next.limit = (next.limit + 4).min(256);
    next.expand_per_seed = (next.expand_per_seed + 1).min(16);
    next.min_score = (next.min_score * 0.9).clamp(0.01, 1.0);
    next.lane_bonus = (next.lane_bonus + 0.02).min(0.3);
    next.expand_query = true;
    if ws.summary.lane_counts.len() < 2 {
        for lane in working_set::default_lanes() {
            if !next.lanes.iter().any(|l| l.eq_ignore_ascii_case(&lane)) {
                next.lanes.push(lane);
            }
        }
    }
    if iteration > 1 {
        next.diversity_lambda = (next.diversity_lambda * 0.95).clamp(0.4, 1.0);
    }
    next.normalize();
    next
}

#[derive(Deserialize)]
pub(crate) struct RehydrateReq {
    pub ptr: Value,
}
pub async fn context_rehydrate(
    State(state): State<AppState>,
    Json(req): Json<RehydrateReq>,
) -> impl IntoResponse {
    let kind = req.ptr.get("kind").and_then(|v| v.as_str()).unwrap_or("");
    match kind {
        "file" => {
            if !state
                .policy
                .lock()
                .await
                .evaluate_action("context.rehydrate")
                .allow
            {
                if state
                    .kernel
                    .find_valid_lease("local", "context:rehydrate:file")
                    .ok()
                    .flatten()
                    .is_none()
                    && state
                        .kernel
                        .find_valid_lease("local", "fs")
                        .ok()
                        .flatten()
                        .is_none()
                {
                    state.bus.publish(
                        "policy.decision",
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
                    );
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
                    );
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
                            );
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
                }
                _ => (
                    axum::http::StatusCode::BAD_REQUEST,
                    Json(
                        json!({"type":"about:blank","title":"Bad Request","status":400, "detail":"not a file"}),
                    ),
                ),
            }
        }
        _ => (
            axum::http::StatusCode::BAD_REQUEST,
            Json(
                json!({"type":"about:blank","title":"Bad Request","status":400, "detail":"unsupported ptr kind"}),
            ),
        ),
    }
}
