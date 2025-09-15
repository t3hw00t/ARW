use crate::{coverage, working_set, AppState};
use metrics::{counter, histogram};
use serde_json::{json, Map, Value};
use std::collections::HashSet;
use std::future::Future;
use std::time::Instant;
use tokio::sync::mpsc;
use tokio::task::JoinError;

pub(crate) enum ContextIterationEvent {
    Summary {
        iteration: usize,
        payload: Value,
        diagnostics: Option<Value>,
    },
    Error {
        iteration: usize,
        payload: Value,
    },
}

pub(crate) struct ContextLoopResult {
    pub(crate) final_spec: working_set::WorkingSetSpec,
    pub(crate) last_verdict: coverage::CoverageVerdict,
    pub(crate) final_working_set: Option<working_set::WorkingSet>,
    pub(crate) error: Option<IterationError>,
}

pub(crate) struct SyncIterationCollector {
    debug: bool,
    entries: Vec<Value>,
}

impl SyncIterationCollector {
    pub(crate) fn new(debug: bool) -> Self {
        Self {
            debug,
            entries: Vec::new(),
        }
    }

    pub(crate) fn observe(&mut self, event: &ContextIterationEvent) {
        match event {
            ContextIterationEvent::Summary {
                payload,
                diagnostics,
                ..
            } => {
                let mut entry = payload.as_object().cloned().unwrap_or_default();
                if self.debug {
                    if let Some(diag) = diagnostics {
                        entry.insert("diagnostics".into(), diag.clone());
                    }
                }
                self.entries.push(Value::Object(entry));
            }
            ContextIterationEvent::Error { .. } => {}
        }
    }

    pub(crate) fn into_inner(self) -> Vec<Value> {
        self.entries
    }
}

pub(crate) struct StreamIterationEmitter {
    sender: mpsc::Sender<working_set::WorkingSetStreamEvent>,
}

impl StreamIterationEmitter {
    pub(crate) fn new(sender: mpsc::Sender<working_set::WorkingSetStreamEvent>) -> Self {
        Self { sender }
    }

    pub(crate) fn handle(&self, event: ContextIterationEvent) -> impl Future<Output = ()> + Send {
        let sender = self.sender.clone();
        async move {
            match event {
                ContextIterationEvent::Summary {
                    iteration, payload, ..
                } => {
                    let _ = sender
                        .send(working_set::WorkingSetStreamEvent {
                            iteration,
                            kind: "working_set.iteration.summary".into(),
                            payload,
                        })
                        .await;
                }
                ContextIterationEvent::Error {
                    iteration, payload, ..
                } => {
                    let _ = sender
                        .send(working_set::WorkingSetStreamEvent {
                            iteration,
                            kind: "working_set.error".into(),
                            payload,
                        })
                        .await;
                }
            }
        }
    }
}

pub(crate) async fn drive_context_loop<F, Fut>(
    state: AppState,
    base_spec: working_set::WorkingSetSpec,
    corr_id: Option<String>,
    max_iterations: usize,
    stream_sender: Option<mpsc::Sender<working_set::WorkingSetStreamEvent>>,
    capture_diagnostics: bool,
    mut on_event: F,
) -> ContextLoopResult
where
    F: FnMut(ContextIterationEvent) -> Fut,
    Fut: Future<Output = ()> + Send,
{
    let mut current_spec = base_spec.clone();
    let mut final_spec = current_spec.clone();
    let mut last_verdict = coverage::CoverageVerdict::satisfied();
    let mut final_working_set: Option<working_set::WorkingSet> = None;
    let mut error: Option<IterationError> = None;

    for iteration in 0..max_iterations {
        let outcome = run_context_iteration(
            iteration,
            max_iterations,
            state.clone(),
            current_spec.clone(),
            corr_id.clone(),
            stream_sender.clone(),
        )
        .await;

        match outcome {
            IterationOutcome::Success(success) => {
                let success = *success;
                let diagnostics = if capture_diagnostics {
                    Some(success.working_set.diagnostics.clone())
                } else {
                    None
                };
                let summary_event = ContextIterationEvent::Summary {
                    iteration,
                    payload: success.summary_payload.clone(),
                    diagnostics,
                };
                on_event(summary_event).await;
                last_verdict = success.verdict.clone();
                final_spec = success.spec_used.clone();
                let continue_loop = success.verdict.needs_more
                    && iteration + 1 < max_iterations
                    && success.next_spec.is_some();
                if continue_loop {
                    if let Some(next_spec) = success.next_spec {
                        current_spec = next_spec;
                    }
                } else {
                    final_working_set = Some(success.working_set);
                    break;
                }
            }
            IterationOutcome::Error(err) => {
                let err = *err;
                let error_event = ContextIterationEvent::Error {
                    iteration,
                    payload: err.payload.clone(),
                };
                on_event(error_event).await;
                final_spec = err.spec.clone();
                error = Some(err);
                break;
            }
        }
    }

    ContextLoopResult {
        final_spec,
        last_verdict,
        final_working_set,
        error,
    }
}

struct IterationSuccess {
    working_set: working_set::WorkingSet,
    verdict: coverage::CoverageVerdict,
    summary_payload: Value,
    next_spec: Option<working_set::WorkingSetSpec>,
    spec_used: working_set::WorkingSetSpec,
}

pub(crate) struct IterationError {
    pub(crate) payload: Value,
    pub(crate) detail: String,
    pub(crate) spec: working_set::WorkingSetSpec,
}

enum IterationOutcome {
    Success(Box<IterationSuccess>),
    Error(Box<IterationError>),
}

async fn run_context_iteration(
    iteration: usize,
    max_iterations: usize,
    state: AppState,
    spec: working_set::WorkingSetSpec,
    corr_id: Option<String>,
    stream_sender: Option<mpsc::Sender<working_set::WorkingSetStreamEvent>>,
) -> IterationOutcome {
    let bus = state.bus.clone();
    let iteration_start = Instant::now();
    let corr_for_payload = corr_id.clone();
    let spec_for_payload = spec.clone();

    let join = tokio::task::spawn_blocking({
        let state_for_block = state.clone();
        let bus_for_block = bus.clone();
        let corr_for_block = corr_id.clone();
        let sender_for_block = stream_sender.clone();
        move || {
            let spec_for_block = spec;
            let bus_observer = working_set::BusObserver::new(
                bus_for_block,
                iteration,
                corr_for_block,
                spec_for_block.project.clone(),
                spec_for_block.query.clone(),
            );
            let outcome = match sender_for_block {
                Some(sender) => {
                    let chan_observer = working_set::ChannelObserver::new(iteration, sender);
                    let mut observer =
                        working_set::CompositeObserver::new(chan_observer, bus_observer);
                    working_set::assemble_with_observer(
                        &state_for_block,
                        &spec_for_block,
                        &mut observer,
                    )
                }
                None => {
                    let mut observer = bus_observer;
                    working_set::assemble_with_observer(
                        &state_for_block,
                        &spec_for_block,
                        &mut observer,
                    )
                }
            };
            (outcome, spec_for_block)
        }
    })
    .await;

    let elapsed = iteration_start.elapsed();
    let duration_ms = elapsed.as_secs_f64() * 1000.0;

    match join {
        Ok((Ok(ws), spec_used)) => {
            let verdict = coverage::assess(&ws);
            let mut next_spec_candidate: Option<working_set::WorkingSetSpec> = None;
            if verdict.needs_more && iteration + 1 < max_iterations {
                next_spec_candidate = Some(adjust_spec_for_iteration(
                    iteration, &spec_used, &ws, &verdict,
                ));
            }
            let summary_payload = build_iteration_summary_payload(
                iteration,
                &spec_used,
                &ws.summary,
                &verdict,
                corr_id.as_ref(),
                next_spec_candidate.as_ref(),
                duration_ms,
            );
            let needs_more_label = if verdict.needs_more { "true" } else { "false" };
            histogram!(
                "arw_context_iteration_duration_ms",
                duration_ms,
                "outcome" => "success",
                "needs_more" => needs_more_label,
            );
            counter!(
                "arw_context_iteration_total",
                1,
                "outcome" => "success",
                "needs_more" => needs_more_label,
            );
            bus.publish("working_set.iteration.summary", &summary_payload);
            IterationOutcome::Success(Box::new(IterationSuccess {
                working_set: ws,
                verdict,
                summary_payload,
                next_spec: next_spec_candidate,
                spec_used,
            }))
        }
        Ok((Err(err), spec_used)) => {
            let detail = err.to_string();
            let error_payload = build_working_set_error_payload(
                iteration,
                &spec_used,
                detail.clone(),
                corr_id.as_ref(),
                duration_ms,
            );
            histogram!(
                "arw_context_iteration_duration_ms",
                duration_ms,
                "outcome" => "error",
            );
            counter!("arw_context_iteration_total", 1, "outcome" => "error");
            bus.publish("working_set.error", &error_payload);
            IterationOutcome::Error(Box::new(IterationError {
                payload: error_payload,
                detail,
                spec: spec_used,
            }))
        }
        Err(join_err) => {
            let detail = format_join_error(join_err);
            let error_payload = build_working_set_error_payload(
                iteration,
                &spec_for_payload,
                detail.clone(),
                corr_for_payload.as_ref(),
                duration_ms,
            );
            histogram!(
                "arw_context_iteration_duration_ms",
                duration_ms,
                "outcome" => "join_error",
            );
            counter!("arw_context_iteration_total", 1, "outcome" => "join_error");
            bus.publish("working_set.error", &error_payload);
            IterationOutcome::Error(Box::new(IterationError {
                payload: error_payload,
                detail,
                spec: spec_for_payload,
            }))
        }
    }
}

fn format_join_error(join_err: JoinError) -> String {
    if join_err.is_cancelled() {
        "context assembly worker was cancelled".to_string()
    } else if join_err.is_panic() {
        let panic = join_err.into_panic();
        let panic_ref = panic.as_ref();
        if let Some(msg) = panic_ref.downcast_ref::<&str>() {
            format!("context assembly worker panicked: {}", msg)
        } else if let Some(msg) = panic_ref.downcast_ref::<String>() {
            format!("context assembly worker panicked: {}", msg)
        } else {
            "context assembly worker panicked".to_string()
        }
    } else {
        join_err.to_string()
    }
}

fn build_iteration_summary_payload(
    iteration: usize,
    spec: &working_set::WorkingSetSpec,
    summary: &working_set::WorkingSetSummary,
    verdict: &coverage::CoverageVerdict,
    corr_id: Option<&String>,
    next_spec: Option<&working_set::WorkingSetSpec>,
    duration_ms: f64,
) -> Value {
    let mut payload = Map::new();
    payload.insert("index".into(), json!(iteration));
    payload.insert("iteration".into(), json!(iteration));
    payload.insert("spec".into(), spec.snapshot());
    payload.insert("summary".into(), summary.to_json());
    let coverage_obj = json!({
        "needs_more": verdict.needs_more,
        "reasons": verdict.reasons,
    });
    payload.insert("coverage".into(), coverage_obj);
    payload.insert("coverage_gap".into(), json!(verdict.needs_more));
    if !verdict.reasons.is_empty() {
        payload.insert("reasons".into(), json!(verdict.reasons.clone()));
    }
    if let Some(cid) = corr_id {
        payload.insert("corr_id".into(), json!(cid));
    }
    if let Some(project) = spec.project.as_ref() {
        payload.insert("project".into(), json!(project));
    }
    if let Some(query) = spec.query.as_ref() {
        payload.insert("query".into(), json!(query));
    }
    if let Some(next_spec) = next_spec {
        payload.insert("next_spec".into(), next_spec.snapshot());
    }
    payload.insert("duration_ms".into(), json!(duration_ms));
    Value::Object(payload)
}

fn build_working_set_error_payload(
    iteration: usize,
    spec: &working_set::WorkingSetSpec,
    error: String,
    corr_id: Option<&String>,
    duration_ms: f64,
) -> Value {
    let mut payload = Map::new();
    payload.insert("index".into(), json!(iteration));
    payload.insert("iteration".into(), json!(iteration));
    payload.insert("error".into(), json!(error));
    payload.insert("spec".into(), spec.snapshot());
    payload.insert("duration_ms".into(), json!(duration_ms));
    if let Some(cid) = corr_id {
        payload.insert("corr_id".into(), json!(cid));
    }
    if let Some(project) = spec.project.as_ref() {
        payload.insert("project".into(), json!(project));
    }
    if let Some(query) = spec.query.as_ref() {
        payload.insert("query".into(), json!(query));
    }
    Value::Object(payload)
}

fn adjust_spec_for_iteration(
    iteration: usize,
    prev: &working_set::WorkingSetSpec,
    ws: &working_set::WorkingSet,
    verdict: &coverage::CoverageVerdict,
) -> working_set::WorkingSetSpec {
    let mut next = prev.clone();
    let reasons: HashSet<&str> = verdict.reasons.iter().map(|s| s.as_str()).collect();

    if reasons.contains("below_target_limit") || reasons.contains("no_items_selected") {
        let bump = ((next.limit as f32 * 0.5).ceil() as usize).max(4);
        next.limit = (next.limit + bump).min(256);
        next.expand_per_seed = (next.expand_per_seed + 2).min(16);
    } else {
        next.limit = (next.limit + 4).min(256);
        next.expand_per_seed = (next.expand_per_seed + 1).min(16);
    }

    if reasons.contains("no_items_above_threshold") {
        next.min_score = (next.min_score * 0.75).clamp(0.01, 1.0);
        next.expand_query = true;
        next.expand_query_top_k = (next.expand_query_top_k + 4).min(32);
    } else if reasons.contains("weak_average_score") {
        next.min_score = (next.min_score * 0.85).clamp(0.01, 1.0);
        next.expand_query = true;
        next.expand_query_top_k = (next.expand_query_top_k + 2).min(32);
    } else {
        next.min_score = (next.min_score * 0.9).clamp(0.01, 1.0);
    }

    if reasons.contains("low_lane_diversity") {
        let mut seen: HashSet<String> = next.lanes.iter().map(|lane| lane.to_string()).collect();
        for item in ws.seeds.iter().chain(ws.expanded.iter()) {
            if let Some(lane) = item
                .get("lane")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
            {
                if seen.insert(lane.clone()) {
                    next.lanes.push(lane);
                }
            }
        }
        next.lanes.sort();
        next.lanes.dedup();
        if next.lanes.len() > 4 {
            next.lanes.truncate(4);
        }
        next.diversity_lambda = (next.diversity_lambda * 1.05).clamp(0.5, 1.0);
    } else if iteration > 0 {
        next.diversity_lambda = (next.diversity_lambda * 0.96).clamp(0.4, 1.0);
    }

    if iteration >= 1 && verdict.needs_more {
        next.expand_query = true;
        next.expand_per_seed = (next.expand_per_seed + 1).min(16);
    }
    if iteration >= 2 && verdict.needs_more {
        next.limit = (next.limit + 8).min(256);
        next.expand_query_top_k = (next.expand_query_top_k + 4).min(32);
    }

    next.normalize();
    next
}
