use super::builder::{build_world_candidate, resolve_scorer, select_candidates, Candidate};
use super::models::{BusObserver, ChannelObserver, CompositeObserver, WorkingSetObserver};
use super::*;
use crate::test_support::env as test_env;
use chrono::{SecondsFormat, Utc};
use serde_json::json;
use std::collections::BTreeMap;
use std::sync::{Arc as StdArc, Mutex};

#[tokio::test]
async fn channel_observer_reuses_payload_arc() {
    let (tx, mut rx) = tokio::sync::mpsc::channel(1);
    let mut observer = ChannelObserver::new(7, tx);
    let payload = Arc::new(json!({"value": 1}));

    observer.emit("test.event", Arc::clone(&payload));

    let event = rx.recv().await.expect("channel event");
    assert!(StdArc::ptr_eq(&payload, &event.payload));
    assert_eq!(event.iteration, 7);
}

#[derive(Clone, Default)]
struct RecordingObserver {
    events: StdArc<Mutex<Vec<SharedValue>>>,
}

impl RecordingObserver {
    fn new() -> Self {
        Self::default()
    }

    fn events(&self) -> StdArc<Mutex<Vec<SharedValue>>> {
        StdArc::clone(&self.events)
    }
}

impl WorkingSetObserver for RecordingObserver {
    fn emit(&mut self, _kind: &'static str, payload: SharedValue) {
        self.events.lock().unwrap().push(payload);
    }
}

#[tokio::test]
async fn composite_observer_shares_payload_with_inner_observer() {
    let bus = arw_events::Bus::new(16);
    let mut bus_rx = bus.subscribe();

    let recorder = RecordingObserver::new();
    let events = recorder.events();

    let bus_observer = BusObserver::new(bus.clone(), 0, None, None, None);
    let mut composite = CompositeObserver::new(recorder, bus_observer);

    let payload = Arc::new(json!({"foo": "bar"}));
    composite.emit("custom.event", Arc::clone(&payload));

    {
        let stored = events.lock().unwrap();
        assert_eq!(stored.len(), 1);
        assert!(StdArc::ptr_eq(&payload, &stored[0]));
    }

    let envelope = bus_rx.recv().await.expect("bus event");
    assert_eq!(envelope.kind, "custom.event");
    assert_eq!(envelope.payload.get("iteration"), Some(&json!(0)));
    assert_eq!(envelope.payload.get("foo"), Some(&json!("bar")));
}

fn context_env_guard() -> test_env::EnvGuard {
    let mut guard = test_env::guard();
    guard.apply(CONTEXT_ENV_KEYS.iter().map(|&key| (key, None)));
    guard
}

fn base_spec() -> WorkingSetSpec {
    WorkingSetSpec {
        query: None,
        embed: None,
        lanes: Vec::new(),
        limit: 0,
        expand_per_seed: 0,
        diversity_lambda: f32::NAN,
        min_score: f32::NAN,
        project: None,
        persona_id: None,
        lane_bonus: f32::NAN,
        scorer: None,
        expand_query: default_expand_query(),
        expand_query_top_k: 0,
        slot_budgets: BTreeMap::new(),
    }
}

fn make_candidate(id: &str, slot: &str, score: f32) -> Candidate {
    let value = json!({
        "id": id,
        "lane": "semantic",
        "kind": slot,
        "value": {"text": format!("{slot} memo")},
        "cscore": score,
    });
    Candidate::from_value(id.to_string(), Some("semantic".into()), value, score)
}

#[test]
fn normalize_applies_defaults_when_missing() {
    let _env_guard = context_env_guard();
    let mut spec = base_spec();
    spec.expand_per_seed = 99;
    spec.normalize();

    assert_eq!(spec.lanes, default_lanes());
    assert_eq!(spec.limit, default_limit());
    assert_eq!(spec.expand_per_seed, 16);
    assert_eq!(spec.diversity_lambda, default_diversity_lambda());
    assert_eq!(spec.min_score, default_min_score());
    assert_eq!(spec.lane_bonus, default_lane_bonus());
    assert_eq!(spec.scorer_label(), default_scorer());
    assert_eq!(spec.expand_query_top_k, default_expand_query_top_k());
    assert!(spec.slot_budgets.is_empty());
    assert!(spec.persona_id.is_none());
}

#[test]
fn normalize_trims_and_clamps_inputs() {
    let _env_guard = context_env_guard();
    let mut spec = WorkingSetSpec {
        lanes: vec![
            " procedural ".into(),
            "semantic".into(),
            "".into(),
            "episodic".into(),
            "semantic".into(),
        ],
        limit: 300,
        expand_per_seed: 8,
        diversity_lambda: 0.4,
        min_score: 0.2,
        project: Some("demo".into()),
        lane_bonus: 0.7,
        scorer: Some("  CONFIDENCE  ".into()),
        expand_query: false,
        expand_query_top_k: 100,
        slot_budgets: BTreeMap::from([
            (" Evidence ".to_string(), 999usize),
            ("".to_string(), 5usize),
        ]),
        persona_id: Some(" persona-alpha ".into()),
        ..base_spec()
    };
    spec.normalize();

    assert_eq!(spec.lanes, vec!["episodic", "procedural", "semantic"]);
    assert_eq!(spec.limit, 256);
    assert_eq!(spec.expand_per_seed, 8);
    assert_eq!(spec.scorer_label(), "confidence");
    assert_eq!(spec.expand_query_top_k, 32);
    assert_eq!(spec.slot_budgets.get("evidence"), Some(&256));
    assert!(!spec.slot_budgets.contains_key(""));
    assert_eq!(spec.persona_id.as_deref(), Some("persona-alpha"));
}

#[test]
fn slot_budgets_seeded_from_json_env() {
    let mut guard = context_env_guard();
    guard.set(
        "ARW_CONTEXT_SLOT_BUDGETS",
        "{\"instructions\":2,\"plan\":3,\"evidence\":8}",
    );
    let mut spec = base_spec();
    spec.normalize();
    assert_eq!(spec.slot_budgets.get("instructions"), Some(&2));
    assert_eq!(spec.slot_budgets.get("plan"), Some(&3));
    assert_eq!(spec.slot_budgets.get("evidence"), Some(&8));
}

#[test]
fn slot_budgets_parse_pair_list() {
    let mut guard = context_env_guard();
    guard.set(
        "ARW_CONTEXT_SLOT_BUDGETS",
        "instructions=4,evidence=10, policy = 2",
    );
    let mut spec = base_spec();
    spec.normalize();
    assert_eq!(spec.slot_budgets.get("instructions"), Some(&4));
    assert_eq!(spec.slot_budgets.get("evidence"), Some(&10));
    assert_eq!(spec.slot_budgets.get("policy"), Some(&2));
}

#[test]
fn snapshot_reflects_normalized_state() {
    let _env_guard = context_env_guard();
    let mut spec = base_spec();
    spec.lanes = vec!["semantic".into()];
    spec.limit = 12;
    spec.expand_per_seed = 2;
    spec.min_score = 0.3;
    spec.scorer = Some("mmrd".into());
    spec.expand_query = true;
    spec.expand_query_top_k = 6;
    spec.persona_id = Some(" persona-beta ".into());
    spec.normalize();

    let snap = spec.snapshot();
    assert_eq!(snap["limit"], json!(12));
    assert_eq!(snap["lanes"], json!(vec!["semantic".to_string()]));
    assert_eq!(snap["expand_query"], json!(true));
    assert_eq!(snap["expand_query_top_k"], json!(6));
    assert_eq!(snap["min_score"], json!(spec.min_score));
    assert_eq!(snap["scorer"], json!(spec.scorer));
    assert_eq!(snap["persona"], json!(spec.persona_id));
}

#[test]
fn env_overrides_expand_query_default() {
    let mut env_guard = context_env_guard();
    env_guard.set("ARW_CONTEXT_EXPAND_QUERY", "true");

    assert!(default_expand_query());

    let spec = base_spec();
    assert!(spec.expand_query);
}

#[test]
fn env_overrides_expand_query_top_k_default() {
    let mut env_guard = context_env_guard();
    env_guard.set("ARW_CONTEXT_EXPAND_QUERY_TOP_K", "7");

    assert_eq!(default_expand_query_top_k(), 7);

    let mut spec = base_spec();
    spec.expand_query_top_k = 0;
    spec.normalize();
    assert_eq!(spec.expand_query_top_k, 7);
}

#[test]
fn env_overrides_streaming_default() {
    let mut env_guard = context_env_guard();
    env_guard.set("ARW_CONTEXT_STREAM_DEFAULT", "1");

    assert!(default_streaming_enabled());
}

#[test]
fn build_world_candidate_wraps_belief() {
    let _env_guard = context_env_guard();
    let ts = Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true);
    let belief = json!({
        "id": "hint-http-timeout",
        "action": "hint",
        "confidence": 0.72,
        "severity": 4,
        "ts": ts,
        "rationale": "raise timeout",
    });
    let candidate = build_world_candidate(&belief, Some("demo"));
    let candidate = candidate.expect("world candidate");
    assert!(candidate.id.starts_with("world:"));
    assert_eq!(candidate.lane.as_deref(), Some("world"));
    assert!(candidate.cscore <= 1.0 && candidate.cscore >= 0.05);
    let stored_id = candidate
        .value
        .get("value")
        .and_then(|v| v.get("record"))
        .and_then(|v| v.get("id"))
        .and_then(|v| v.as_str());
    assert_eq!(stored_id, Some("hint-http-timeout"));
}

#[test]
fn slot_budgets_cap_selection_counts() {
    let mut spec = base_spec();
    spec.lanes = vec!["semantic".into()];
    spec.limit = 4;
    spec.expand_per_seed = 0;
    spec.diversity_lambda = 0.5;
    spec.min_score = 0.1;
    spec.lane_bonus = 0.0;
    spec.scorer = Some("confidence".into());
    spec.slot_budgets =
        BTreeMap::from([("instructions".into(), 1usize), ("evidence".into(), 2usize)]);
    spec.normalize();

    let candidates = vec![
        make_candidate("inst-1", "instructions", 0.9),
        make_candidate("inst-2", "instructions", 0.8),
        make_candidate("ev-1", "evidence", 0.85),
        make_candidate("ev-2", "evidence", 0.7),
        make_candidate("ev-3", "evidence", 0.65),
        make_candidate("plan-1", "plan", 0.6),
    ];

    let scorer = resolve_scorer(spec.scorer.as_deref());
    let mut observer = ();
    let (selected, _lanes, slot_counts) =
        select_candidates(candidates, &spec, true, scorer.as_ref(), &mut observer);

    assert_eq!(slot_counts.get("instructions"), Some(&1));
    assert_eq!(slot_counts.get("evidence"), Some(&2));
    let selected_instructions = selected
        .iter()
        .filter(|c| c.slot_key() == "instructions")
        .count();
    assert_eq!(selected_instructions, 1);
    assert!(selected.len() <= spec.limit);
}
const CONTEXT_ENV_KEYS: &[&str] = &[
    "ARW_CONTEXT_COVERAGE_MAX_ITERS",
    "ARW_CONTEXT_DIVERSITY_LAMBDA",
    "ARW_CONTEXT_EXPAND_QUERY",
    "ARW_CONTEXT_EXPAND_QUERY_TOP_K",
    "ARW_CONTEXT_EXPAND_PER_SEED",
    "ARW_CONTEXT_K",
    "ARW_CONTEXT_LANE_BONUS",
    "ARW_CONTEXT_LANES_DEFAULT",
    "ARW_CONTEXT_MIN_SCORE",
    "ARW_CONTEXT_SCORER",
    "ARW_CONTEXT_SLOT_BUDGETS",
    "ARW_CONTEXT_STREAM_DEFAULT",
];
