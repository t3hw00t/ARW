use axum::http::{HeaderMap, StatusCode};
use axum::response::sse::{Event, Sse};
use axum::response::IntoResponse;
use axum::{
    extract::{Query, State},
    Json,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::convert::Infallible;
use std::future::ready;
use std::sync::Arc;
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;
use tokio_stream::StreamExt;
use utoipa::ToSchema;

use crate::{
    context_loop::{
        drive_context_loop, ContextLoopResult, StreamIterationEmitter, SyncIterationCollector,
    },
    coverage, memory_service, util, working_set, AppState,
};
use arw_topics as topics;

#[derive(Clone, Deserialize, ToSchema)]
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
    #[serde(default)]
    pub slot_budgets: Option<BTreeMap<String, usize>>,
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
            req,
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

    let body = build_context_response(ContextResponseInputs {
        request: &req,
        base_spec: &base_spec,
        final_spec: &final_spec,
        verdict: &last_verdict,
        working_set: ws,
        iterations_meta,
        include_sources,
        debug,
        max_iterations,
        corr_id: corr_id.as_deref(),
    });

    state.bus().publish(topics::TOPIC_CONTEXT_ASSEMBLED, &body);

    (axum::http::StatusCode::OK, Json(body)).into_response()
}

async fn stream_working_set(
    state: AppState,
    req: AssembleReq,
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
    let req_for_task = req.clone();
    let base_spec_for_task = base_spec.clone();
    tokio::spawn(async move {
        let stream_sender = tx.clone();
        let emitter = StreamIterationEmitter::new(stream_sender.clone());
        let loop_result = drive_context_loop(
            state_clone.clone(),
            spec_clone,
            corr_for_task.clone(),
            max_iterations,
            Some(stream_sender),
            false,
            move |event| emitter.handle(event),
        )
        .await;

        if let Some(ws) = loop_result.final_working_set {
            let body = build_context_response(ContextResponseInputs {
                request: &req_for_task,
                base_spec: &base_spec_for_task,
                final_spec: &loop_result.final_spec,
                verdict: &loop_result.last_verdict,
                working_set: ws,
                iterations_meta: Vec::new(),
                include_sources,
                debug,
                max_iterations,
                corr_id: corr_for_task.as_deref(),
            });
            state_clone
                .bus()
                .publish(topics::TOPIC_CONTEXT_ASSEMBLED, &body);
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
        let mut payload = evt.payload.as_ref().clone();
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

struct ContextResponseInputs<'a> {
    request: &'a AssembleReq,
    base_spec: &'a working_set::WorkingSetSpec,
    final_spec: &'a working_set::WorkingSetSpec,
    verdict: &'a coverage::CoverageVerdict,
    working_set: working_set::WorkingSet,
    iterations_meta: Vec<Value>,
    include_sources: bool,
    debug: bool,
    max_iterations: usize,
    corr_id: Option<&'a str>,
}

fn build_context_response(params: ContextResponseInputs<'_>) -> Value {
    let ContextResponseInputs {
        request,
        base_spec,
        final_spec,
        verdict,
        working_set,
        iterations_meta,
        include_sources,
        debug,
        max_iterations,
        corr_id,
    } = params;

    let working_set::WorkingSet {
        items,
        seeds,
        expanded,
        diagnostics,
        summary,
    } = working_set;

    let item_count = items.len();
    let seed_count = seeds.len();
    let expanded_count = expanded.len();

    let items_json = clone_shared_values(&items);

    let preview = build_context_preview(&items_json);

    let mut working = json!({
        "items": items_json,
        "counts": {
            "items": item_count,
            "seeds": seed_count,
            "expanded": expanded_count
        },
        "summary": summary.to_json(),
        "coverage": json!({
            "needs_more": verdict.needs_more,
            "reasons": verdict.reasons
        })
    });
    working["iterations"] = Value::Array(iterations_meta);
    working["final_spec"] = final_spec.snapshot();
    if include_sources || debug {
        working["seeds"] = json!(clone_shared_values(&seeds));
        working["expanded"] = json!(clone_shared_values(&expanded));
    }
    if debug {
        working["diagnostics"] = diagnostics.as_ref().clone();
    }

    let beliefs = working.get("items").cloned().unwrap_or_else(|| json!([]));

    let mut body = json!({
        "query": request.q,
        "project": request.proj,
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
        if let Some(embed) = request.embed.clone() {
            obj.insert("embed".into(), json!(embed));
        }
        if let Some(cid) = corr_id {
            obj.insert("corr_id".into(), json!(cid));
        }
        if let Some(preview) = preview {
            obj.insert("context_preview".into(), json!(preview));
        }
    }

    body
}

fn clone_shared_values(values: &[Arc<Value>]) -> Vec<Value> {
    values.iter().map(|v| v.as_ref().clone()).collect()
}

fn build_context_preview(items: &[Value]) -> Option<String> {
    const MAX_LINES: usize = 5;
    const LINE_MAX: usize = 160;
    const TOTAL_MAX: usize = 800;

    let mut lines = Vec::new();
    for item in items.iter().take(MAX_LINES) {
        let candidate = item
            .as_object()
            .and_then(|obj| {
                obj.get("text")
                    .and_then(Value::as_str)
                    .or_else(|| obj.get("summary").and_then(Value::as_str))
                    .or_else(|| obj.get("content").and_then(Value::as_str))
                    .or_else(|| obj.get("value").and_then(Value::as_str))
            })
            .or_else(|| item.as_str());

        let text = candidate
            .map(|s| s.to_string())
            .unwrap_or_else(|| serde_json::to_string(item).unwrap_or_default());
        let Some(normalized) = normalize_whitespace(&text) else {
            continue;
        };
        if normalized.is_empty() {
            continue;
        }
        let mut line = normalized;
        if line.len() > LINE_MAX {
            line.truncate(LINE_MAX);
            line.push('…');
        }
        lines.push(format!("• {}", line));
    }

    if lines.is_empty() {
        return None;
    }

    let mut preview = lines.join("\n");
    if preview.len() > TOTAL_MAX {
        preview.truncate(TOTAL_MAX);
        preview.push('…');
    }
    Some(preview)
}

fn normalize_whitespace(input: &str) -> Option<String> {
    let mut parts = input.split_whitespace();
    let first = parts.next()?;
    let mut normalized = String::from(first);
    for part in parts {
        normalized.push(' ');
        normalized.push_str(part);
    }
    Some(normalized)
}

#[cfg(test)]
mod context_response_tests {
    use super::*;

    fn sample_spec() -> working_set::WorkingSetSpec {
        working_set::WorkingSetSpec {
            query: None,
            embed: None,
            lanes: vec!["semantic".into()],
            limit: 3,
            expand_per_seed: 0,
            diversity_lambda: 0.5,
            min_score: 0.2,
            project: None,
            lane_bonus: 0.0,
            scorer: Some("mmr".into()),
            expand_query: false,
            expand_query_top_k: 1,
            slot_budgets: BTreeMap::new(),
        }
    }

    fn sample_summary() -> working_set::WorkingSetSummary {
        working_set::WorkingSetSummary {
            target_limit: 3,
            lanes_requested: 1,
            selected: 1,
            avg_cscore: 0.4,
            max_cscore: 0.6,
            min_cscore: 0.2,
            threshold_hits: 1,
            total_candidates: 2,
            lane_counts: BTreeMap::new(),
            slot_counts: BTreeMap::new(),
            slot_budgets: BTreeMap::new(),
            min_score: 0.2,
            scorer: "mmr".into(),
        }
    }

    fn sample_request() -> AssembleReq {
        AssembleReq {
            proj: None,
            q: Some("demo".into()),
            embed: None,
            lanes: None,
            limit: None,
            expand_per_seed: None,
            diversity_lambda: None,
            min_score: None,
            lane_bonus: None,
            scorer: None,
            expand_query: None,
            expand_query_top_k: None,
            include_sources: None,
            debug: None,
            stream: None,
            max_iterations: None,
            corr_id: None,
            slot_budgets: None,
        }
    }

    fn sample_working_set() -> working_set::WorkingSet {
        working_set::WorkingSet {
            items: vec![Arc::new(json!({"id": 1, "text": "hello"}))],
            seeds: vec![Arc::new(json!({"id": "seed-1"}))],
            expanded: vec![Arc::new(json!({"id": "expanded-1"}))],
            diagnostics: Arc::new(json!({"phase": "retrieve"})),
            summary: sample_summary(),
        }
    }

    #[test]
    fn hides_sources_and_diagnostics_when_disabled() {
        let request = sample_request();
        let base_spec = sample_spec();
        let final_spec = base_spec.clone();
        let working_set = sample_working_set();
        let verdict = coverage::CoverageVerdict::satisfied();

        let response = build_context_response(ContextResponseInputs {
            request: &request,
            base_spec: &base_spec,
            final_spec: &final_spec,
            verdict: &verdict,
            working_set,
            iterations_meta: Vec::new(),
            include_sources: false,
            debug: false,
            max_iterations: 4,
            corr_id: None,
        });

        assert!(response
            .get("working_set")
            .and_then(|ws| ws.get("seeds"))
            .is_none());
        assert!(response
            .get("working_set")
            .and_then(|ws| ws.get("expanded"))
            .is_none());
        assert!(response
            .get("working_set")
            .and_then(|ws| ws.get("diagnostics"))
            .is_none());
        assert_eq!(response["working_set"]["counts"]["items"].as_u64(), Some(1));
    }

    #[test]
    fn exposes_sources_and_diagnostics_when_enabled() {
        let mut request = sample_request();
        request.corr_id = Some("corr-1".into());
        let base_spec = sample_spec();
        let final_spec = base_spec.clone();
        let working_set = sample_working_set();
        let verdict = coverage::CoverageVerdict::satisfied();

        let response = build_context_response(ContextResponseInputs {
            request: &request,
            base_spec: &base_spec,
            final_spec: &final_spec,
            verdict: &verdict,
            working_set,
            iterations_meta: Vec::new(),
            include_sources: true,
            debug: true,
            max_iterations: 4,
            corr_id: request.corr_id.as_deref(),
        });

        assert_eq!(
            response["working_set"]["seeds"].as_array().map(|a| a.len()),
            Some(1)
        );
        assert_eq!(
            response["working_set"]["expanded"]
                .as_array()
                .map(|a| a.len()),
            Some(1)
        );
        assert_eq!(
            response["working_set"]["diagnostics"],
            json!({"phase": "retrieve"})
        );
        assert_eq!(response["corr_id"], json!("corr-1"));
    }
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
        slot_budgets: BTreeMap::new(),
    };
    spec.slot_budgets = req.slot_budgets.clone().unwrap_or_default();
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
            let decision = state
                .policy()
                .evaluate_action("context.rehydrate")
                .await;
            if !decision.allow {
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
                .evaluate_action("context.rehydrate.memory")
                .await;
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
#[derive(Debug, Default, Deserialize, utoipa::IntoParams)]
#[serde(default)]
pub struct ContextCascadeQuery {
    /// Optional project filter; matches exact `project_id` or any project listed on the summary.
    pub project: Option<String>,
    /// Maximum number of summaries to return (1-200, default 80).
    pub limit: Option<usize>,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct ContextCascadeResponse {
    #[schema(value_type = Vec<serde_json::Value>)]
    pub items: Vec<Value>,
    pub generated: String,
    pub generated_ms: i64,
}

fn now_timestamp_pair() -> (String, i64) {
    let now = chrono::Utc::now();
    (
        now.to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
        now.timestamp_millis(),
    )
}

#[utoipa::path(
    get,
    path = "/state/context/cascade",
    tag = "Context",
    params(ContextCascadeQuery),
    responses(
        (status = 200, description = "Cascade summaries", body = ContextCascadeResponse),
        (status = 401, description = "Unauthorized"),
        (status = 501, description = "Kernel disabled", body = serde_json::Value)
    )
)]
pub async fn state_context_cascade(
    headers: HeaderMap,
    Query(query): Query<ContextCascadeQuery>,
    State(state): State<AppState>,
) -> impl IntoResponse {
    if !crate::admin_ok(&headers).await {
        return crate::responses::unauthorized(None);
    }
    if !state.kernel_enabled() {
        return crate::responses::kernel_disabled();
    }

    let limit = query.limit.unwrap_or(80).clamp(1, 200) as i64;
    let lane = Some("episodic_summary".to_string());
    let mut items = match state.kernel().list_recent_memory_async(lane, limit).await {
        Ok(items) => items,
        Err(err) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({
                    "type": "about:blank",
                    "title": "Error",
                    "status": 500,
                    "detail": err.to_string(),
                })),
            )
                .into_response();
        }
    };

    memory_service::attach_memory_ptrs(&mut items);

    if let Some(project) = query
        .project
        .as_ref()
        .map(|s| s.trim().to_ascii_lowercase())
    {
        if !project.is_empty() {
            items.retain(|record| cascade_matches_project(record, &project));
        }
    }

    let (generated, generated_ms) = now_timestamp_pair();
    (
        StatusCode::OK,
        Json(ContextCascadeResponse {
            items,
            generated,
            generated_ms,
        }),
    )
        .into_response()
}

fn cascade_matches_project(record: &Value, needle: &str) -> bool {
    let project_id = record
        .get("project_id")
        .and_then(Value::as_str)
        .map(|s| s.to_ascii_lowercase());
    if let Some(id) = project_id {
        if id == needle {
            return true;
        }
    }
    // Check top-level projects array
    if let Some(list) = record.get("projects").and_then(Value::as_array) {
        if list.iter().any(|v| {
            v.as_str()
                .map(|s| s.eq_ignore_ascii_case(needle))
                .unwrap_or(false)
        }) {
            return true;
        }
    }
    // Check summary payload
    record
        .get("value")
        .and_then(Value::as_object)
        .and_then(|value| value.get("projects"))
        .and_then(Value::as_array)
        .map(|list| {
            list.iter().any(|v| {
                v.as_str()
                    .map(|s| s.eq_ignore_ascii_case(needle))
                    .unwrap_or(false)
            })
        })
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cascade_matches_project_checks_project_sources() {
        let record = json!({
            "project_id": "demo",
            "value": {
                "projects": ["demo"],
            }
        });
        assert!(cascade_matches_project(&record, "demo"));
        assert!(!cascade_matches_project(&record, "other"));

        let value_only = json!({
            "value": { "projects": ["Alpha"] }
        });
        assert!(cascade_matches_project(&value_only, "alpha"));
    }
}
