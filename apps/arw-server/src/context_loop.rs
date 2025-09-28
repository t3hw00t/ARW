use crate::{coverage, working_set, AppState};
use metrics::{counter, gauge, histogram};
use serde_json::{json, Map, Number, Value};
use std::collections::HashSet;
use std::future::Future;
use std::time::Instant;
use tokio::sync::mpsc;
use tokio::task::JoinError;

use arw_topics as topics;
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
                            kind: topics::TOPIC_WORKING_SET_ITERATION_SUMMARY.into(),
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
                            kind: topics::TOPIC_WORKING_SET_ERROR.into(),
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
    let bus = state.bus();
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
            bus.publish(
                topics::TOPIC_WORKING_SET_ITERATION_SUMMARY,
                &summary_payload,
            );
            let coverage_payload = build_context_coverage_payload(
                iteration,
                &spec_used,
                &ws.summary,
                &verdict,
                corr_id.as_ref(),
                duration_ms,
            );
            bus.publish(topics::TOPIC_CONTEXT_COVERAGE, &coverage_payload);
            let recall_event = build_context_recall_risk_payload(
                iteration,
                &spec_used,
                &ws.summary,
                &verdict,
                corr_id.as_ref(),
                duration_ms,
            );
            histogram!(
                "arw_context_recall_risk_score",
                recall_event.score,
                "level" => recall_event.level,
                "needs_more" => needs_more_label,
            );
            counter!(
                "arw_context_recall_risk_total",
                1,
                "level" => recall_event.level,
                "needs_more" => needs_more_label,
            );
            if recall_event.at_risk {
                counter!(
                    "arw_context_recall_risk_flagged_total",
                    1,
                    "level" => recall_event.level,
                );
            }
            bus.publish(topics::TOPIC_CONTEXT_RECALL_RISK, &recall_event.payload);
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
            bus.publish(topics::TOPIC_WORKING_SET_ERROR, &error_payload);
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
            bus.publish(topics::TOPIC_WORKING_SET_ERROR, &error_payload);
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

#[cfg(test)]
#[allow(clippy::items_after_test_module)]
mod tests {
    use super::*;
    use crate::working_set::{WorkingSet, WorkingSetSpec, WorkingSetSummary};
    use crate::AppState;
    use arw_policy::PolicyEngine;
    use arw_wasi::ToolHost;
    use serde_json::json;
    use std::collections::{BTreeMap, HashSet};
    use std::sync::Arc;
    use tempfile::tempdir;
    use tokio::sync::Mutex as AsyncMutex;

    async fn build_state(
        path: &std::path::Path,
        env_guard: &mut crate::test_support::env::EnvGuard,
    ) -> AppState {
        env_guard.set("ARW_DEBUG", "1");
        crate::util::reset_state_dir_for_tests();
        env_guard.set("ARW_STATE_DIR", path.display().to_string());
        let bus = arw_events::Bus::new_with_replay(16, 16);
        let kernel = arw_kernel::Kernel::open(path).expect("init kernel");
        let policy = PolicyEngine::load_from_env();
        let policy_arc = Arc::new(AsyncMutex::new(policy));
        let host: Arc<dyn ToolHost> = Arc::new(arw_wasi::NoopHost);
        AppState::builder(bus, kernel, policy_arc, host, true)
            .with_sse_capacity(16)
            .build()
            .await
    }

    fn base_spec() -> WorkingSetSpec {
        WorkingSetSpec {
            query: None,
            embed: None,
            lanes: vec!["docs".to_string()],
            limit: 8,
            expand_per_seed: 2,
            diversity_lambda: 0.5,
            min_score: 0.6,
            project: None,
            lane_bonus: 0.3,
            scorer: Some("mmrd".into()),
            expand_query: false,
            expand_query_top_k: 6,
            slot_budgets: BTreeMap::new(),
        }
    }

    fn working_set_with_summary(summary: WorkingSetSummary) -> WorkingSet {
        WorkingSet {
            items: Vec::new(),
            seeds: vec![json!({"lane": "analysis"})],
            expanded: vec![json!({"lane": "code"})],
            diagnostics: json!({}),
            summary,
        }
    }

    #[test]
    fn recall_risk_payload_combines_gaps() {
        let mut lane_counts = BTreeMap::new();
        lane_counts.insert("analysis".to_string(), 1usize);
        let mut slot_counts = BTreeMap::new();
        slot_counts.insert("instructions".to_string(), 0usize);
        let mut slot_budgets = BTreeMap::new();
        slot_budgets.insert("instructions".to_string(), 2usize);
        let summary = WorkingSetSummary {
            target_limit: 8,
            lanes_requested: 2,
            selected: 3,
            avg_cscore: 0.25,
            max_cscore: 0.3,
            min_cscore: 0.6,
            threshold_hits: 0,
            total_candidates: 9,
            lane_counts,
            slot_counts,
            slot_budgets: slot_budgets.clone(),
            min_score: 0.6,
            scorer: "mmrd".into(),
        };

        let ws = working_set_with_summary(summary.clone());
        let verdict = coverage::assess(&ws);
        let mut spec = base_spec();
        spec.lanes = vec!["analysis".into(), "docs".into()];
        spec.slot_budgets = slot_budgets.clone();
        spec.project = Some("alpha".into());
        spec.query = Some("how to seed".into());
        spec.normalize();

        let event = build_context_recall_risk_payload(0, &spec, &summary, &verdict, None, 42.0);
        assert!(event.score >= 0.74 && event.score <= 0.76);
        assert_eq!(event.level, "high");
        assert!(event.at_risk);

        let payload = event.payload.as_object().expect("payload object");
        assert_eq!(payload["level"], json!("high"));
        assert_eq!(
            payload["components"]["coverage_shortfall"]
                .as_f64()
                .unwrap(),
            0.625
        );
        assert_eq!(payload["components"]["slot_gap"].as_f64().unwrap(), 1.0);
        assert_eq!(payload["selected_ratio"].as_f64().unwrap(), 0.375);
        assert_eq!(
            payload["spec"]["slot_budgets"].as_object().unwrap().len(),
            1
        );
        assert_eq!(payload["components"]["slots"]["instructions"], json!(1.0));
        assert_eq!(payload["spec"]["slot_budgets"]["instructions"], json!(2));
        assert_eq!(payload["query"], json!("how to seed"));
        assert_eq!(payload["spec"]["query_provided"], json!(true));
        assert_eq!(payload["spec"]["project"], json!("alpha"));
        assert_eq!(payload["duration_ms"], json!(42.0));
    }

    #[test]
    fn coverage_payload_captures_slot_budgets_and_metadata() {
        let mut lane_counts = BTreeMap::new();
        lane_counts.insert("docs".to_string(), 1usize);
        let mut slot_counts = BTreeMap::new();
        slot_counts.insert("instructions".to_string(), 0usize);
        let mut slot_budgets = BTreeMap::new();
        slot_budgets.insert("instructions".to_string(), 2usize);
        let summary = WorkingSetSummary {
            target_limit: 4,
            lanes_requested: 2,
            selected: 1,
            avg_cscore: 0.35,
            max_cscore: 0.45,
            min_cscore: 0.6,
            threshold_hits: 0,
            total_candidates: 5,
            lane_counts,
            slot_counts: slot_counts.clone(),
            slot_budgets: slot_budgets.clone(),
            min_score: 0.6,
            scorer: "mmrd".into(),
        };
        let ws = working_set_with_summary(summary.clone());
        let verdict = coverage::assess(&ws);

        let mut spec = base_spec();
        spec.lanes = vec!["analysis".into(), "docs".into()];
        spec.slot_budgets = slot_budgets.clone();
        spec.project = Some("project-1".into());
        spec.query = Some("seed question".into());
        spec.normalize();

        let corr = "episode-1".to_string();
        let payload =
            build_context_coverage_payload(0, &spec, &summary, &verdict, Some(&corr), 150.0);
        assert_eq!(payload["iteration"], json!(0));
        assert_eq!(payload["needs_more"], json!(true));
        assert!(payload["reasons"]
            .as_array()
            .unwrap()
            .iter()
            .any(|r| r == "slot_underfilled:instructions"));
        assert_eq!(payload["corr_id"], json!("episode-1"));
        assert_eq!(payload["duration_ms"], json!(150.0));
        assert_eq!(payload["project"], json!("project-1"));
        assert_eq!(payload["query"], json!("seed question"));
        assert_eq!(payload["spec"]["query_provided"], json!(true));
        assert_eq!(
            payload["summary"]["slots"]["counts"]["instructions"],
            json!(0)
        );
        assert_eq!(
            payload["summary"]["slots"]["budgets"]["instructions"],
            json!(2)
        );
        assert_eq!(payload["spec"]["slot_budgets"]["instructions"], json!(2));
    }

    #[test]
    fn adjust_spec_reacts_to_coverage_reasons() {
        let mut lane_counts = BTreeMap::new();
        lane_counts.insert("docs".to_string(), 3usize);
        let summary = WorkingSetSummary {
            target_limit: 8,
            lanes_requested: 3,
            selected: 3,
            avg_cscore: 0.32,
            max_cscore: 0.35,
            min_cscore: 0.1,
            threshold_hits: 0,
            total_candidates: 11,
            lane_counts,
            slot_counts: BTreeMap::new(),
            slot_budgets: BTreeMap::new(),
            min_score: 0.6,
            scorer: "mmrd".into(),
        };
        let ws = working_set_with_summary(summary);
        let verdict = coverage::assess(&ws);
        let reasons: HashSet<_> = verdict.reasons.iter().map(|s| s.as_str()).collect();
        assert!(reasons.contains("below_target_limit"));
        assert!(reasons.contains("low_lane_diversity"));
        assert!(reasons.contains("weak_average_score"));
        assert!(reasons.contains("no_items_above_threshold"));

        let next = adjust_spec_for_iteration(0, &base_spec(), &ws, &verdict);
        assert_eq!(next.limit, 12);
        assert_eq!(next.expand_per_seed, 4);
        assert!(next.expand_query);
        assert_eq!(next.expand_query_top_k, 10);
        assert!((next.min_score - 0.45).abs() < f32::EPSILON);

        let mut lanes = next.lanes.clone();
        lanes.sort();
        assert_eq!(lanes, vec!["analysis", "code", "docs"]);
    }

    #[tokio::test]
    async fn stream_emitter_matches_summary_payloads() {
        let temp = tempdir().expect("tempdir");
        let mut ctx = crate::test_support::begin_state_env(temp.path());
        let state = build_state(temp.path(), &mut ctx.env).await;

        let mut spec = WorkingSetSpec {
            query: Some("demo".into()),
            embed: None,
            lanes: crate::working_set::default_lanes(),
            limit: crate::working_set::default_limit(),
            expand_per_seed: crate::working_set::default_expand_per_seed(),
            diversity_lambda: crate::working_set::default_diversity_lambda(),
            min_score: crate::working_set::default_min_score(),
            project: None,
            lane_bonus: crate::working_set::default_lane_bonus(),
            scorer: Some(crate::working_set::default_scorer()),
            expand_query: crate::working_set::default_expand_query(),
            expand_query_top_k: crate::working_set::default_expand_query_top_k(),
            slot_budgets: BTreeMap::new(),
        };
        spec.normalize();

        let collected = Arc::new(AsyncMutex::new(Vec::new()));
        let collected_clone = collected.clone();
        let (tx, mut rx) = tokio::sync::mpsc::channel(32);
        let emitter = Arc::new(StreamIterationEmitter::new(tx.clone()));
        let emitter_for_closure = emitter.clone();

        let result = drive_context_loop(
            state.clone(),
            spec,
            Some("stream-corr".to_string()),
            2,
            Some(tx.clone()),
            true,
            move |event| {
                let collected = collected_clone.clone();
                let emitter = emitter_for_closure.clone();
                async move {
                    if let ContextIterationEvent::Summary { payload, .. } = &event {
                        collected.lock().await.push(payload.clone());
                    }
                    emitter.handle(event).await;
                }
            },
        )
        .await;

        if let Some(err) = result.error {
            panic!("context loop should succeed: {}", err.detail);
        }
        let summary_payloads = collected.lock().await.clone();
        assert!(
            !summary_payloads.is_empty(),
            "expected at least one summary event"
        );

        drop(emitter);
        drop(tx);

        let mut streamed_payloads = Vec::new();
        while let Some(ev) = rx.recv().await {
            if ev.kind == topics::TOPIC_WORKING_SET_ITERATION_SUMMARY {
                streamed_payloads.push(ev.payload);
            }
        }

        assert_eq!(
            summary_payloads.len(),
            streamed_payloads.len(),
            "streamed summary count should match collector",
        );
        for (summary, streamed) in summary_payloads.iter().zip(streamed_payloads.iter()) {
            assert_eq!(summary, streamed);
        }
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

fn build_context_coverage_payload(
    iteration: usize,
    spec: &working_set::WorkingSetSpec,
    summary: &working_set::WorkingSetSummary,
    verdict: &coverage::CoverageVerdict,
    corr_id: Option<&String>,
    duration_ms: f64,
) -> Value {
    for reason in verdict.reasons.iter() {
        if let Some(slot) = reason.strip_prefix("slot_underfilled:") {
            counter!(
                "arw_context_slot_underfilled_total",
                1,
                "slot" => slot.to_string(),
            );
        }
    }
    for (slot, budget) in summary.slot_budgets.iter() {
        if *budget == 0 {
            continue;
        }
        let have = summary
            .slot_counts
            .get(slot)
            .copied()
            .unwrap_or(0)
            .min(*budget);
        let denom = (*budget).min(summary.selected.max(1)).max(1) as f64;
        let ratio = (have as f64 / denom).clamp(0.0, 1.0);
        gauge!(
            "arw_context_slot_fill_ratio",
            ratio,
            "slot" => slot.clone(),
        );
    }

    let mut payload = Map::new();
    payload.insert("iteration".into(), json!(iteration));
    payload.insert("needs_more".into(), json!(verdict.needs_more));
    payload.insert("reasons".into(), json!(verdict.reasons));
    payload.insert("summary".into(), summary.to_json());
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

struct RecallRiskEvent {
    payload: Value,
    score: f64,
    level: &'static str,
    at_risk: bool,
}

fn json_number(value: f64) -> Value {
    Number::from_f64(value)
        .map(Value::Number)
        .unwrap_or(Value::Null)
}

fn build_context_recall_risk_payload(
    iteration: usize,
    spec: &working_set::WorkingSetSpec,
    summary: &working_set::WorkingSetSummary,
    verdict: &coverage::CoverageVerdict,
    corr_id: Option<&String>,
    duration_ms: f64,
) -> RecallRiskEvent {
    const W_COVERAGE: f64 = 0.4;
    const W_LANE: f64 = 0.2;
    const W_SLOT: f64 = 0.2;
    const W_QUALITY: f64 = 0.2;

    let target_limit = summary.target_limit.max(1) as f64;
    let selected = summary.selected.min(summary.target_limit) as f64;
    let coverage_ratio = (selected / target_limit).clamp(0.0, 1.0);
    let coverage_shortfall = (1.0 - coverage_ratio).clamp(0.0, 1.0);

    let desired_lanes = summary.lanes_requested.max(1);
    let lane_count = summary
        .lane_counts
        .iter()
        .filter(|(_, count)| **count > 0)
        .count()
        .max(if summary.selected > 0 { 1 } else { 0 });
    let lane_gap = if desired_lanes == 0 {
        0.0
    } else {
        (desired_lanes.saturating_sub(lane_count) as f64 / desired_lanes as f64).clamp(0.0, 1.0)
    };

    let mut slot_breakdown = Map::new();
    let mut slot_gap: f64 = 0.0;
    for (slot, budget) in summary.slot_budgets.iter() {
        if *budget == 0 {
            continue;
        }
        let have = summary
            .slot_counts
            .get(slot)
            .copied()
            .unwrap_or(0)
            .min(*budget);
        let gap = ((*budget as f64 - have as f64) / *budget as f64).clamp(0.0, 1.0);
        slot_gap = slot_gap.max(gap);
        gauge!(
            "arw_context_slot_gap_latest",
            gap,
            "slot" => slot.clone(),
        );
        histogram!(
            "arw_context_slot_gap",
            gap,
            "slot" => slot.clone(),
        );
        slot_breakdown.insert(slot.clone(), json_number(gap));
    }
    if slot_breakdown.is_empty() {
        slot_gap = 0.0;
    }

    let min_score = summary.min_score as f64;
    let avg_cscore = summary.avg_cscore as f64;
    let max_cscore = summary.max_cscore as f64;
    let threshold_gap: f64 = if summary.threshold_hits == 0 && max_cscore < min_score {
        1.0
    } else {
        0.0
    };
    let avg_gap = if min_score > 0.0 {
        ((min_score - avg_cscore).max(0.0) / min_score).clamp(0.0, 1.0)
    } else {
        0.0
    };
    let quality_gap = threshold_gap.max(avg_gap);

    let mut weighted_total = 0.0;
    let mut weight_sum = 0.0;

    weighted_total += coverage_shortfall * W_COVERAGE;
    weight_sum += W_COVERAGE;

    if desired_lanes > 1 {
        weighted_total += lane_gap * W_LANE;
        weight_sum += W_LANE;
    }

    if !slot_breakdown.is_empty() {
        weighted_total += slot_gap * W_SLOT;
        weight_sum += W_SLOT;
    }

    weighted_total += quality_gap * W_QUALITY;
    weight_sum += W_QUALITY;

    if weight_sum == 0.0 {
        weight_sum = 1.0;
    }

    let score = (weighted_total / weight_sum).clamp(0.0, 1.0);
    let level = if score >= 0.7 {
        "high"
    } else if score >= 0.4 {
        "medium"
    } else {
        "low"
    };
    let at_risk = verdict.needs_more || score >= 0.4;

    let mut components = Map::new();
    components.insert("coverage_shortfall".into(), json_number(coverage_shortfall));
    components.insert("lane_gap".into(), json_number(lane_gap));
    if !slot_breakdown.is_empty() {
        components.insert("slot_gap".into(), json_number(slot_gap));
        components.insert("slots".into(), Value::Object(slot_breakdown));
    }
    components.insert("quality_gap".into(), json_number(quality_gap));

    let mut payload = Map::new();
    payload.insert("iteration".into(), json!(iteration));
    payload.insert("score".into(), json_number(score));
    payload.insert("level".into(), json!(level));
    payload.insert("at_risk".into(), json!(at_risk));
    payload.insert("components".into(), Value::Object(components));
    payload.insert("selected_ratio".into(), json_number(coverage_ratio));
    payload.insert("desired_lanes".into(), json!(desired_lanes));
    payload.insert("lane_count".into(), json!(lane_count));
    payload.insert("needs_more".into(), json!(verdict.needs_more));
    payload.insert("reasons".into(), json!(verdict.reasons));
    payload.insert("summary".into(), summary.to_json());
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

    RecallRiskEvent {
        payload: Value::Object(payload),
        score,
        level,
        at_risk,
    }
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
