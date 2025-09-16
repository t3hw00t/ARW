use crate::AppState;
use anyhow::Result;
use chrono::SecondsFormat;
use metrics::{counter, histogram};
use serde_json::{json, Map, Value};
use std::cmp::Ordering;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::time::Duration;
use std::time::Instant;

use arw_topics as topics;

pub const STREAM_EVENT_STARTED: &str = topics::TOPIC_WORKING_SET_STARTED;
pub const STREAM_EVENT_SEED: &str = topics::TOPIC_WORKING_SET_SEED;
pub const STREAM_EVENT_EXPANDED: &str = topics::TOPIC_WORKING_SET_EXPANDED;
pub const STREAM_EVENT_QUERY_EXPANDED: &str = topics::TOPIC_WORKING_SET_EXPAND_QUERY;
pub const STREAM_EVENT_SELECTED: &str = topics::TOPIC_WORKING_SET_SELECTED;
pub const STREAM_EVENT_COMPLETED: &str = topics::TOPIC_WORKING_SET_COMPLETED;

#[derive(Clone, Debug)]
pub struct WorkingSetSpec {
    pub query: Option<String>,
    pub embed: Option<Vec<f32>>,
    pub lanes: Vec<String>,
    pub limit: usize,
    pub expand_per_seed: usize,
    pub diversity_lambda: f32,
    pub min_score: f32,
    pub project: Option<String>,
    pub lane_bonus: f32,
    pub scorer: Option<String>,
    pub expand_query: bool,
    pub expand_query_top_k: usize,
}

impl WorkingSetSpec {
    pub fn normalize(&mut self) {
        self.lanes = self
            .lanes
            .iter()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
        self.lanes.sort();
        self.lanes.dedup();
        if self.lanes.is_empty() {
            self.lanes = default_lanes();
        }
        if self.limit == 0 {
            self.limit = default_limit();
        }
        self.limit = self.limit.clamp(1, 256);
        self.expand_per_seed = self.expand_per_seed.min(16);
        if !self.diversity_lambda.is_finite() {
            self.diversity_lambda = default_diversity_lambda();
        }
        self.diversity_lambda = self.diversity_lambda.clamp(0.0, 1.0);
        if !self.min_score.is_finite() {
            self.min_score = default_min_score();
        }
        self.min_score = self.min_score.clamp(0.0, 1.0);
        if !self.lane_bonus.is_finite() {
            self.lane_bonus = default_lane_bonus();
        }
        self.lane_bonus = self.lane_bonus.clamp(0.0, 1.0);
        let scorer_name = self
            .scorer
            .as_ref()
            .map(|s| s.trim().to_ascii_lowercase())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(default_scorer);
        self.scorer = Some(scorer_name);
        if self.expand_query_top_k == 0 {
            self.expand_query_top_k = default_expand_query_top_k();
        }
        self.expand_query_top_k = self.expand_query_top_k.clamp(1, 32);
    }

    pub fn scorer_label(&self) -> String {
        self.scorer.clone().unwrap_or_else(default_scorer)
    }

    pub fn snapshot(&self) -> Value {
        json!({
            "query_provided": self.query.is_some(),
            "lanes": self.lanes,
            "limit": self.limit,
            "expand_per_seed": self.expand_per_seed,
            "diversity_lambda": self.diversity_lambda,
            "min_score": self.min_score,
            "project": self.project,
            "lane_bonus": self.lane_bonus,
            "scorer": self.scorer,
            "expand_query": self.expand_query,
            "expand_query_top_k": self.expand_query_top_k,
        })
    }
}

#[derive(Debug)]
pub struct WorkingSet {
    pub items: Vec<Value>,
    pub seeds: Vec<Value>,
    pub expanded: Vec<Value>,
    pub diagnostics: Value,
    pub summary: WorkingSetSummary,
}

#[derive(Debug, Clone, Default)]
pub struct WorkingSetSummary {
    pub target_limit: usize,
    pub lanes_requested: usize,
    pub selected: usize,
    pub avg_cscore: f32,
    pub max_cscore: f32,
    pub min_cscore: f32,
    pub threshold_hits: usize,
    pub total_candidates: usize,
    pub lane_counts: BTreeMap<String, usize>,
    pub min_score: f32,
    pub scorer: String,
}

impl WorkingSetSummary {
    pub fn to_json(&self) -> Value {
        let mut lanes = serde_json::Map::new();
        for (lane, count) in self.lane_counts.iter() {
            lanes.insert(lane.clone(), json!(count));
        }
        json!({
            "target_limit": self.target_limit,
            "lanes_requested": self.lanes_requested,
            "selected": self.selected,
            "avg_cscore": self.avg_cscore,
            "max_cscore": self.max_cscore,
            "min_cscore": self.min_cscore,
            "threshold_hits": self.threshold_hits,
            "total_candidates": self.total_candidates,
            "lane_counts": Value::Object(lanes),
            "min_score": self.min_score,
            "scorer": self.scorer,
        })
    }
}

#[derive(Clone, Debug)]
pub struct WorkingSetStreamEvent {
    pub iteration: usize,
    pub kind: String,
    pub payload: Value,
}

pub trait WorkingSetObserver {
    fn emit(&mut self, kind: &'static str, payload: Value);
}

impl WorkingSetObserver for () {
    fn emit(&mut self, _kind: &'static str, _payload: Value) {}
}

pub struct ChannelObserver {
    iteration: usize,
    tx: tokio::sync::mpsc::Sender<WorkingSetStreamEvent>,
}

impl ChannelObserver {
    pub fn new(iteration: usize, tx: tokio::sync::mpsc::Sender<WorkingSetStreamEvent>) -> Self {
        Self { iteration, tx }
    }
}

impl WorkingSetObserver for ChannelObserver {
    fn emit(&mut self, kind: &'static str, payload: Value) {
        let evt = WorkingSetStreamEvent {
            iteration: self.iteration,
            kind: kind.to_string(),
            payload,
        };
        let _ = self.tx.blocking_send(evt);
    }
}

#[derive(Clone)]
pub struct BusObserver {
    bus: arw_events::Bus,
    iteration: usize,
    corr_id: Option<String>,
    project: Option<String>,
    query: Option<String>,
}

impl BusObserver {
    pub fn new(
        bus: arw_events::Bus,
        iteration: usize,
        corr_id: Option<String>,
        project: Option<String>,
        query: Option<String>,
    ) -> Self {
        Self {
            bus,
            iteration,
            corr_id,
            project,
            query,
        }
    }

    fn enrich_value(&self, payload: Value) -> Value {
        let mut map: Map<String, Value> = match payload {
            Value::Object(map) => map,
            other => {
                let mut map = Map::new();
                map.insert("value".into(), other);
                map
            }
        };
        map.insert("iteration".into(), json!(self.iteration));
        if let Some(corr) = &self.corr_id {
            map.insert("corr_id".into(), Value::String(corr.clone()));
        }
        if let Some(project) = &self.project {
            map.insert("project".into(), Value::String(project.clone()));
        }
        if let Some(query) = &self.query {
            map.insert("query".into(), Value::String(query.clone()));
        }
        Value::Object(map)
    }

    fn publish_enriched(&self, kind: &'static str, value: &Value) {
        self.bus.publish(kind, value);
    }
}

impl WorkingSetObserver for BusObserver {
    fn emit(&mut self, kind: &'static str, payload: Value) {
        let enriched = self.enrich_value(payload);
        self.publish_enriched(kind, &enriched);
    }
}

pub struct CompositeObserver<A> {
    first: A,
    second: BusObserver,
}

impl<A> CompositeObserver<A> {
    pub fn new(first: A, second: BusObserver) -> Self {
        Self { first, second }
    }
}

impl<A> WorkingSetObserver for CompositeObserver<A>
where
    A: WorkingSetObserver,
{
    fn emit(&mut self, kind: &'static str, payload: Value) {
        let enriched = self.second.enrich_value(payload);
        self.first.emit(kind, enriched.clone());
        self.second.publish_enriched(kind, &enriched);
    }
}

pub fn assemble(state: &AppState, spec: &WorkingSetSpec) -> Result<WorkingSet> {
    let mut observer = (); // no-op
    assemble_with_observer(state, spec, &mut observer)
}

pub fn assemble_with_observer<O: WorkingSetObserver>(
    state: &AppState,
    spec: &WorkingSetSpec,
    observer: &mut O,
) -> Result<WorkingSet> {
    let mut builder = WorkingSetBuilder::new(state, spec.clone());
    builder.build(observer)
}

struct WorkingSetBuilder<'a> {
    state: &'a AppState,
    spec: WorkingSetSpec,
}

impl<'a> WorkingSetBuilder<'a> {
    fn new(state: &'a AppState, spec: WorkingSetSpec) -> Self {
        Self { state, spec }
    }

    fn build<O: WorkingSetObserver>(&mut self, observer: &mut O) -> Result<WorkingSet> {
        let mut spec = self.spec.clone();
        spec.normalize();
        let scorer_label = spec.scorer_label();
        let scorer = resolve_scorer(Some(scorer_label.as_str()));
        observer.emit(
            STREAM_EVENT_STARTED,
            json!({
                "spec": spec.snapshot(),
                "scorer": scorer.name(),
            }),
        );
        counter!("arw_context_scorer_used_total", 1, "scorer" => scorer.name());

        let total_start = Instant::now();
        let mut lanes: Vec<Option<String>> = if spec.lanes.is_empty() {
            vec![None]
        } else {
            spec.lanes.iter().cloned().map(Some).collect()
        };
        lanes.dedup_by(|a, b| a.as_ref().map(|s| s.as_str()) == b.as_ref().map(|s| s.as_str()));

        let mut candidates: HashMap<String, Candidate> = HashMap::new();
        let mut seeds_raw: Vec<Value> = Vec::new();
        let mut expanded_raw: Vec<Value> = Vec::new();
        let mut seed_infos: Vec<SeedInfo> = Vec::new();

        let retrieve_start = Instant::now();
        for lane in lanes.iter() {
            let fetch_k = ((spec.limit * 3) + spec.expand_per_seed).max(10) as i64;
            let mut items = self.state.kernel.select_memory_hybrid(
                spec.query.as_deref(),
                spec.embed.as_deref(),
                lane.as_deref(),
                fetch_k,
            )?;
            for item in items.drain(..) {
                let lane_override = lane.clone().or_else(|| {
                    item.get("lane")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string())
                });
                if let Some((candidate, seed)) =
                    build_seed_candidate(item, lane_override, spec.project.as_deref())
                {
                    let payload = candidate.value.clone();
                    let lane_for_event = candidate.lane.clone();
                    observer.emit(
                        STREAM_EVENT_SEED,
                        json!({"item": payload.clone(), "lane": lane_for_event.clone()}),
                    );
                    counter!(
                        "arw_context_seed_candidates_total",
                        1,
                        "lane" => lane_for_event.unwrap_or_else(|| "unknown".into())
                    );
                    seeds_raw.push(payload);
                    seed_infos.push(seed);
                    insert_candidate(&mut candidates, candidate);
                }
            }
        }
        let retrieve_elapsed = retrieve_start.elapsed();
        histogram!(
            "arw_context_phase_duration_ms",
            retrieve_elapsed.as_secs_f64() * 1000.0,
            "phase" => "retrieve"
        );

        let mut expand_query_elapsed = Duration::from_millis(0);
        if spec.expand_query {
            let expand_start = Instant::now();
            let added = self.pseudo_relevance_expand(
                &spec,
                &lanes,
                &seed_infos,
                &mut candidates,
                &mut expanded_raw,
                observer,
            )?;
            expand_query_elapsed = expand_start.elapsed();
            if added > 0 {
                counter!("arw_context_query_expansion_total", added as u64);
            }
        }

        let expand_start = Instant::now();
        if spec.expand_per_seed > 0 {
            for seed in seed_infos.clone() {
                let links = self
                    .state
                    .kernel
                    .list_memory_links(&seed.id, spec.expand_per_seed as i64)
                    .unwrap_or_default();
                for link in links {
                    if let Some(dst_id) = link.get("dst_id").and_then(|v| v.as_str()) {
                        if dst_id == seed.id {
                            continue;
                        }
                        if candidates.contains_key(dst_id) {
                            continue;
                        }
                        if let Ok(Some(record)) = self.state.kernel.get_memory(dst_id) {
                            if let Some(candidate) = build_expansion_candidate(
                                record,
                                &seed,
                                &link,
                                spec.project.as_deref(),
                            ) {
                                let payload = candidate.value.clone();
                                let lane_for_event = candidate.lane.clone();
                                observer.emit(
                                    STREAM_EVENT_EXPANDED,
                                    json!({"item": payload.clone(), "lane": lane_for_event.clone()}),
                                );
                                counter!(
                                    "arw_context_link_expansion_total",
                                    1,
                                    "lane" => lane_for_event.unwrap_or_else(|| "unknown".into())
                                );
                                expanded_raw.push(payload);
                                insert_candidate(&mut candidates, candidate);
                            }
                        }
                    }
                }
            }
        }
        let expand_elapsed = expand_start.elapsed();
        histogram!(
            "arw_context_phase_duration_ms",
            expand_elapsed.as_secs_f64() * 1000.0,
            "phase" => "link_expand"
        );

        let mut all_candidates: Vec<Candidate> = candidates.into_values().collect();
        all_candidates.sort_by(|a, b| b.cscore.partial_cmp(&a.cscore).unwrap_or(Ordering::Equal));
        let has_above = all_candidates.iter().any(|c| c.cscore >= spec.min_score);
        let candidate_total = all_candidates.len();

        let select_start = Instant::now();
        let (selected, lane_counts) =
            select_candidates(all_candidates, &spec, has_above, scorer.as_ref(), observer);
        let select_elapsed = select_start.elapsed();
        histogram!(
            "arw_context_phase_duration_ms",
            select_elapsed.as_secs_f64() * 1000.0,
            "phase" => "select"
        );

        let items: Vec<Value> = selected.iter().map(|c| c.value.clone()).collect();
        let summary = WorkingSetSummary::from_selection(
            &spec,
            &selected,
            &lane_counts,
            has_above,
            candidate_total,
            scorer.name(),
        );

        let total_elapsed = total_start.elapsed();
        histogram!(
            "arw_context_phase_duration_ms",
            total_elapsed.as_secs_f64() * 1000.0,
            "phase" => "total"
        );

        let diagnostics = build_diagnostics(
            &spec,
            &seed_infos,
            &expanded_raw,
            &lane_counts,
            items.len(),
            has_above,
            &summary,
            retrieve_elapsed,
            expand_query_elapsed,
            expand_elapsed,
            select_elapsed,
            total_elapsed,
            scorer.name(),
        );

        observer.emit(
            STREAM_EVENT_COMPLETED,
            json!({
                "items": items.clone(),
                "seeds": seeds_raw.clone(),
                "expanded": expanded_raw.clone(),
                "summary": summary.to_json(),
                "diagnostics": diagnostics.clone(),
            }),
        );

        Ok(WorkingSet {
            items,
            seeds: seeds_raw,
            expanded: expanded_raw,
            diagnostics,
            summary,
        })
    }

    fn pseudo_relevance_expand<O: WorkingSetObserver>(
        &self,
        spec: &WorkingSetSpec,
        lanes: &[Option<String>],
        seed_infos: &[SeedInfo],
        candidates: &mut HashMap<String, Candidate>,
        expanded_raw: &mut Vec<Value>,
        observer: &mut O,
    ) -> Result<usize> {
        let mut seeds_with_embed: Vec<(&SeedInfo, &Vec<f32>)> = seed_infos
            .iter()
            .filter_map(|seed| seed.embed.as_ref().map(|embed| (seed, embed)))
            .collect();
        if seeds_with_embed.len() < 2 {
            return Ok(0);
        }
        seeds_with_embed.sort_by(|a, b| {
            b.0.cscore
                .partial_cmp(&a.0.cscore)
                .unwrap_or(Ordering::Equal)
        });
        let top_k = spec.expand_query_top_k.min(seeds_with_embed.len());
        let dims = seeds_with_embed[0].1.len();
        if dims == 0 {
            return Ok(0);
        }
        let mut avg = vec![0f32; dims];
        let mut weight_sum = 0f32;
        let mut seed_ids: Vec<String> = Vec::new();
        for (seed, embed) in seeds_with_embed.iter().take(top_k) {
            if embed.len() != dims {
                continue;
            }
            let weight = seed.cscore.max(0.05);
            for (i, value) in embed.iter().enumerate() {
                avg[i] += value * weight;
            }
            weight_sum += weight;
            seed_ids.push(seed.id.clone());
        }
        if weight_sum == 0.0 {
            return Ok(0);
        }
        for value in avg.iter_mut() {
            *value /= weight_sum;
        }
        let mut added = 0usize;
        let fetch_k = ((spec.limit * 2) + spec.expand_per_seed).max(12) as i64;
        for lane in lanes.iter() {
            let mut items = self.state.kernel.select_memory_hybrid(
                spec.query.as_deref(),
                Some(avg.as_slice()),
                lane.as_deref(),
                fetch_k,
            )?;
            for item in items.drain(..) {
                let lane_override = lane.clone().or_else(|| {
                    item.get("lane")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string())
                });
                if let Some(candidate) = build_query_expansion_candidate(
                    item,
                    lane_override,
                    spec.project.as_deref(),
                    &seed_ids,
                ) {
                    if candidates.contains_key(&candidate.id) {
                        continue;
                    }
                    let payload = candidate.value.clone();
                    let lane_for_event = candidate.lane.clone();
                    observer.emit(
                        STREAM_EVENT_QUERY_EXPANDED,
                        json!({
                            "item": payload.clone(),
                            "lane": lane_for_event.clone(),
                            "seeds_used": seed_ids.clone(),
                        }),
                    );
                    counter!(
                        "arw_context_query_expansion_candidates_total",
                        1,
                        "lane" => lane_for_event.unwrap_or_else(|| "unknown".into())
                    );
                    expanded_raw.push(payload);
                    insert_candidate(candidates, candidate);
                    added += 1;
                }
            }
        }
        Ok(added)
    }
}

#[derive(Clone)]
struct Candidate {
    id: String,
    lane: Option<String>,
    key: Option<String>,
    tags: Vec<String>,
    embed: Option<Vec<f32>>,
    cscore: f32,
    value: Value,
}

#[derive(Clone)]
struct SeedInfo {
    id: String,
    cscore: f32,
    lane: Option<String>,
    embed: Option<Vec<f32>>,
}

trait CandidateScorer: Send + Sync {
    fn name(&self) -> &'static str;
    fn score(&self, candidate: &Candidate, ctx: &SelectionContext) -> f32;
}

struct SelectionContext<'a> {
    spec: &'a WorkingSetSpec,
    selected: &'a [Candidate],
    lane_counts: &'a BTreeMap<String, usize>,
    require_threshold: bool,
}

struct MmrdScorer;

impl CandidateScorer for MmrdScorer {
    fn name(&self) -> &'static str {
        "mmrd"
    }

    fn score(&self, candidate: &Candidate, ctx: &SelectionContext) -> f32 {
        if ctx.require_threshold && candidate.cscore < ctx.spec.min_score {
            return f32::MIN;
        }
        let lane_bonus = candidate
            .lane
            .as_ref()
            .map(|lane| lane_bonus(ctx.lane_counts, lane, ctx.spec.lane_bonus))
            .unwrap_or(0.0);
        mmr_score(
            candidate,
            ctx.selected,
            ctx.spec.diversity_lambda,
            lane_bonus,
        )
    }
}

struct ConfidenceScorer;

impl CandidateScorer for ConfidenceScorer {
    fn name(&self) -> &'static str {
        "confidence"
    }

    fn score(&self, candidate: &Candidate, ctx: &SelectionContext) -> f32 {
        if ctx.require_threshold && candidate.cscore < ctx.spec.min_score {
            return f32::MIN;
        }
        let lane_bonus = candidate
            .lane
            .as_ref()
            .map(|lane| lane_bonus(ctx.lane_counts, lane, ctx.spec.lane_bonus))
            .unwrap_or(0.0);
        candidate.cscore + lane_bonus
    }
}

fn resolve_scorer(name: Option<&str>) -> Box<dyn CandidateScorer + Send + Sync> {
    match name.unwrap_or("mmrd") {
        "confidence" | "greedy" => Box::new(ConfidenceScorer),
        _ => Box::new(MmrdScorer),
    }
}

fn select_candidates<O: WorkingSetObserver>(
    mut candidates: Vec<Candidate>,
    spec: &WorkingSetSpec,
    has_above: bool,
    scorer: &dyn CandidateScorer,
    observer: &mut O,
) -> (Vec<Candidate>, BTreeMap<String, usize>) {
    let mut selected: Vec<Candidate> = Vec::new();
    let mut lane_counts: BTreeMap<String, usize> = BTreeMap::new();
    while !candidates.is_empty() && selected.len() < spec.limit {
        let mut best_idx: Option<usize> = None;
        let mut best_score = f32::MIN;
        for (idx, cand) in candidates.iter().enumerate() {
            let ctx = SelectionContext {
                spec,
                selected: &selected,
                lane_counts: &lane_counts,
                require_threshold: has_above,
            };
            let score = scorer.score(cand, &ctx);
            if score.is_finite() && (best_idx.is_none() || score > best_score) {
                best_idx = Some(idx);
                best_score = score;
            }
        }
        let idx = match best_idx {
            Some(i) => i,
            None => break,
        };
        let cand = candidates.swap_remove(idx);
        let lane_key = cand.lane.clone().unwrap_or_else(|| "unknown".to_string());
        *lane_counts.entry(lane_key.clone()).or_insert(0) += 1;
        counter!("arw_context_selected_total", 1, "lane" => lane_key);
        observer.emit(
            STREAM_EVENT_SELECTED,
            json!({
                "rank": selected.len(),
                "item": cand.value.clone(),
                "score": cand.cscore,
                "scorer": scorer.name(),
            }),
        );
        selected.push(cand);
    }
    (selected, lane_counts)
}

fn lane_bonus(counts: &BTreeMap<String, usize>, lane: &str, bonus: f32) -> f32 {
    if counts.get(lane).copied().unwrap_or(0) == 0 {
        bonus
    } else {
        0.0
    }
}

fn mmr_score(candidate: &Candidate, selected: &[Candidate], lambda: f32, lane_bonus: f32) -> f32 {
    let lambda = lambda.clamp(0.0, 1.0);
    if selected.is_empty() {
        return candidate.cscore + lane_bonus;
    }
    let mut max_sim = 0f32;
    for existing in selected.iter() {
        let sim = candidate_similarity(candidate, existing);
        if sim > max_sim {
            max_sim = sim;
        }
    }
    let base = candidate.cscore + lane_bonus;
    lambda * base - (1.0 - lambda) * max_sim
}

fn candidate_similarity(a: &Candidate, b: &Candidate) -> f32 {
    if let (Some(ref ea), Some(ref eb)) = (&a.embed, &b.embed) {
        let sim = cosine(ea, eb);
        if sim.is_finite() {
            return sim;
        }
    }
    if let (Some(ref ka), Some(ref kb)) = (&a.key, &b.key) {
        if !ka.is_empty() && !kb.is_empty() && ka == kb {
            return 1.0;
        }
    }
    let tag_sim = jaccard(&a.tags, &b.tags);
    if tag_sim.is_finite() {
        tag_sim
    } else {
        0.0
    }
}

fn build_seed_candidate(
    mut value: Value,
    lane: Option<String>,
    project: Option<&str>,
) -> Option<(Candidate, SeedInfo)> {
    let id = value.get("id").and_then(|v| v.as_str())?.to_string();
    let mut lane = lane.or_else(|| {
        value
            .get("lane")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
    });
    if lane.is_none() {
        lane = value
            .get("value")
            .and_then(|v| v.get("lane"))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
    }
    if let Some(ref lane_val) = lane {
        if let Some(obj) = value.as_object_mut() {
            obj.insert("lane".into(), json!(lane_val));
        }
    }
    let base_score = value.get("cscore").and_then(|v| v.as_f64()).unwrap_or(0.0) as f32;
    let sim = value.get("sim").and_then(|v| v.as_f64()).unwrap_or(0.0) as f32;
    let fts = value
        .get("_fts_hit")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let recency = recency_score(value.get("updated").and_then(|v| v.as_str()));
    let util = value.get("score").and_then(|v| v.as_f64()).unwrap_or(0.0) as f32;
    let affinity = project.map(|p| project_affinity(&value, p)).unwrap_or(1.0);
    let cscore = (base_score * affinity).clamp(0.0, 1.0);
    if let Some(obj) = value.as_object_mut() {
        obj.insert("source".into(), json!("seed"));
        obj.insert(
            "explain".into(),
            json!({
                "kind": "seed",
                "components": {
                    "sim": sim,
                    "fts": if fts { 1.0 } else { 0.0 },
                    "recency": recency,
                    "utility": util,
                    "project_affinity": affinity,
                },
                "base_cscore": base_score,
                "cscore": cscore,
            }),
        );
        obj.insert("cscore".into(), json!(cscore));
    }
    let embed = parse_embed(&value);
    let candidate = Candidate::from_value(id.clone(), lane.clone(), value, cscore);
    let seed = SeedInfo {
        id,
        cscore,
        lane,
        embed,
    };
    Some((candidate, seed))
}

fn build_expansion_candidate(
    mut record: Value,
    seed: &SeedInfo,
    link: &Value,
    project: Option<&str>,
) -> Option<Candidate> {
    let id = record.get("id").and_then(|v| v.as_str())?.to_string();
    if id == seed.id {
        return None;
    }
    let lane = record
        .get("lane")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .or_else(|| seed.lane.clone());
    if let Some(ref lane_val) = lane {
        if let Some(obj) = record.as_object_mut() {
            obj.insert("lane".into(), json!(lane_val));
        }
    }
    let weight = link.get("weight").and_then(|v| v.as_f64()).unwrap_or(1.0) as f32;
    let recency = recency_score(record.get("updated").and_then(|v| v.as_str()));
    let affinity = project.map(|p| project_affinity(&record, p)).unwrap_or(1.0);
    let raw_score = 0.5 * seed.cscore + 0.3 * weight + 0.2 * recency;
    let cscore = (raw_score * affinity).clamp(0.0, 1.0);
    if let Some(obj) = record.as_object_mut() {
        obj.insert("source".into(), json!("expanded"));
        let mut link_meta = serde_json::Map::new();
        link_meta.insert("from".into(), json!(seed.id));
        if let Some(rel) = link.get("rel") {
            link_meta.insert("rel".into(), rel.clone());
        }
        if let Some(weight_val) = link.get("weight") {
            link_meta.insert("weight".into(), weight_val.clone());
        }
        obj.insert("link".into(), Value::Object(link_meta));
        obj.insert(
            "explain".into(),
            json!({
                "kind": "expanded",
                "components": {
                    "seed_score": seed.cscore,
                    "link_weight": weight,
                    "recency": recency,
                    "project_affinity": affinity,
                },
                "cscore": cscore,
            }),
        );
        obj.insert("cscore".into(), json!(cscore));
    }
    Some(Candidate::from_value(id, lane, record, cscore))
}

fn build_query_expansion_candidate(
    mut value: Value,
    lane: Option<String>,
    project: Option<&str>,
    seeds_used: &[String],
) -> Option<Candidate> {
    let id = value.get("id").and_then(|v| v.as_str())?.to_string();
    let lane = lane.or_else(|| {
        value
            .get("lane")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
    });
    if let Some(ref lane_val) = lane {
        if let Some(obj) = value.as_object_mut() {
            obj.insert("lane".into(), json!(lane_val));
        }
    }
    let base_score = value.get("cscore").and_then(|v| v.as_f64()).unwrap_or(0.0) as f32;
    let recency = recency_score(value.get("updated").and_then(|v| v.as_str()));
    let support = (seeds_used.len() as f32) / (seeds_used.len().max(1) as f32);
    let affinity = project.map(|p| project_affinity(&value, p)).unwrap_or(1.0);
    let raw_score = 0.5 * base_score + 0.3 * recency + 0.2 * support;
    let cscore = (raw_score * affinity).clamp(0.0, 1.0);
    if let Some(obj) = value.as_object_mut() {
        obj.insert("source".into(), json!("expanded_query"));
        obj.insert(
            "explain".into(),
            json!({
                "kind": "expanded_query",
                "components": {
                    "base": base_score,
                    "recency": recency,
                    "support": support,
                    "project_affinity": affinity,
                },
                "seeds_used": seeds_used,
                "cscore": cscore,
            }),
        );
        obj.insert("cscore".into(), json!(cscore));
    }
    Some(Candidate::from_value(id, lane, value, cscore))
}

fn insert_candidate(map: &mut HashMap<String, Candidate>, candidate: Candidate) {
    match map.entry(candidate.id.clone()) {
        std::collections::hash_map::Entry::Vacant(v) => {
            v.insert(candidate);
        }
        std::collections::hash_map::Entry::Occupied(mut occ) => {
            if candidate.cscore > occ.get().cscore {
                occ.insert(candidate);
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn build_diagnostics(
    spec: &WorkingSetSpec,
    seeds: &[SeedInfo],
    expanded: &[Value],
    lane_counts: &BTreeMap<String, usize>,
    selected: usize,
    has_above: bool,
    summary: &WorkingSetSummary,
    retrieve_elapsed: Duration,
    expand_query_elapsed: Duration,
    expand_elapsed: Duration,
    select_elapsed: Duration,
    total_elapsed: Duration,
    scorer_name: &str,
) -> Value {
    let counts = json!({
        "seeds": seeds.len(),
        "expanded": expanded.len(),
        "selected": selected,
        "candidates": summary.total_candidates,
    });
    let mut lanes = serde_json::Map::new();
    for (lane, count) in lane_counts.iter() {
        lanes.insert(lane.clone(), json!(count));
    }
    json!({
        "params": spec.snapshot(),
        "counts": counts,
        "lanes": Value::Object(lanes),
        "had_candidates_above_threshold": has_above,
        "summary": summary.to_json(),
        "timings_ms": {
            "retrieve": retrieve_elapsed.as_secs_f64() * 1000.0,
            "query_expand": expand_query_elapsed.as_secs_f64() * 1000.0,
            "link_expand": expand_elapsed.as_secs_f64() * 1000.0,
            "select": select_elapsed.as_secs_f64() * 1000.0,
            "total": total_elapsed.as_secs_f64() * 1000.0,
        },
        "scorer": scorer_name,
        "generated_at": chrono::Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true),
    })
}

impl Candidate {
    fn from_value(id: String, lane: Option<String>, value: Value, cscore: f32) -> Self {
        let key = value
            .get("key")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .filter(|s| !s.is_empty());
        let tags = value
            .get("tags")
            .and_then(|v| v.as_str())
            .map(|s| {
                s.split(',')
                    .map(|t| t.trim().to_ascii_lowercase())
                    .filter(|t| !t.is_empty())
                    .map(|t| t.to_string())
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        let embed = parse_embed(&value);
        Candidate {
            id,
            lane,
            key,
            tags,
            embed,
            cscore,
            value,
        }
    }
}

fn parse_embed(value: &Value) -> Option<Vec<f32>> {
    let raw = value.get("embed")?;
    if let Some(s) = raw.as_str() {
        if let Ok(v) = serde_json::from_str::<Value>(s) {
            return parse_embed(&v);
        }
        return None;
    }
    match raw {
        Value::Array(arr) => {
            let mut out = Vec::with_capacity(arr.len());
            for v in arr {
                if let Some(f) = v.as_f64() {
                    out.push(f as f32);
                }
            }
            if out.is_empty() {
                None
            } else {
                Some(out)
            }
        }
        _ => None,
    }
}

fn cosine(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }
    let mut dot = 0f32;
    let mut norm_a = 0f32;
    let mut norm_b = 0f32;
    for (ai, bi) in a.iter().zip(b.iter()) {
        dot += ai * bi;
        norm_a += ai * ai;
        norm_b += bi * bi;
    }
    if norm_a <= f32::EPSILON || norm_b <= f32::EPSILON {
        return 0.0;
    }
    dot / (norm_a.sqrt() * norm_b.sqrt())
}

fn jaccard(a: &[String], b: &[String]) -> f32 {
    if a.is_empty() || b.is_empty() {
        return 0.0;
    }
    let mut set_a: HashSet<&str> = HashSet::new();
    for t in a {
        set_a.insert(t.as_str());
    }
    let mut set_b: HashSet<&str> = HashSet::new();
    for t in b {
        set_b.insert(t.as_str());
    }
    let inter = set_a.intersection(&set_b).count() as f32;
    let union = set_a.union(&set_b).count() as f32;
    if union <= f32::EPSILON {
        0.0
    } else {
        inter / union
    }
}

fn recency_score(updated: Option<&str>) -> f32 {
    if let Some(s) = updated {
        if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(s) {
            let age = chrono::Utc::now() - dt.with_timezone(&chrono::Utc);
            let secs = age.num_seconds().max(0) as f32;
            if secs < 3600.0 {
                return 1.0;
            }
            if secs < 86_400.0 {
                return 0.8;
            }
            if secs < 604_800.0 {
                return 0.6;
            }
            if secs < 2_592_000.0 {
                return 0.4;
            }
        }
    }
    0.2
}

fn project_affinity(value: &Value, project: &str) -> f32 {
    let target = project.to_ascii_lowercase();
    if value
        .get("project")
        .and_then(|v| v.as_str())
        .map(|s| s.eq_ignore_ascii_case(project))
        .unwrap_or(false)
    {
        return 1.0;
    }
    if value
        .get("value")
        .and_then(|v| v.get("proj"))
        .and_then(|v| v.as_str())
        .map(|s| s.eq_ignore_ascii_case(project))
        .unwrap_or(false)
    {
        return 1.0;
    }
    if value
        .get("value")
        .and_then(|v| v.get("project"))
        .and_then(|v| v.as_str())
        .map(|s| s.eq_ignore_ascii_case(project))
        .unwrap_or(false)
    {
        return 1.0;
    }
    if value
        .get("tags")
        .and_then(|v| v.as_str())
        .map(|s| {
            s.split(',')
                .map(|t| t.trim().to_ascii_lowercase())
                .any(|t| t == target)
        })
        .unwrap_or(false)
    {
        return 0.9;
    }
    if value
        .get("value")
        .and_then(|v| v.get("tags"))
        .map(|t| match t {
            Value::Array(arr) => arr
                .iter()
                .filter_map(|v| v.as_str())
                .map(|s| s.to_ascii_lowercase())
                .any(|s| s == target),
            Value::String(s) => s
                .split(',')
                .map(|t| t.trim().to_ascii_lowercase())
                .any(|t| t == target),
            _ => false,
        })
        .unwrap_or(false)
    {
        return 0.9;
    }
    0.75
}

impl WorkingSetSummary {
    fn from_selection(
        spec: &WorkingSetSpec,
        selected: &[Candidate],
        lane_counts: &BTreeMap<String, usize>,
        has_above: bool,
        total_candidates: usize,
        scorer: &str,
    ) -> Self {
        let mut avg = 0f32;
        let mut max = 0f32;
        let mut min = if selected.is_empty() { 0f32 } else { f32::MAX };
        let mut hits = 0usize;
        for cand in selected.iter() {
            avg += cand.cscore;
            if cand.cscore > max {
                max = cand.cscore;
            }
            if cand.cscore < min {
                min = cand.cscore;
            }
            if cand.cscore >= spec.min_score {
                hits += 1;
            }
        }
        if selected.is_empty() {
            min = 0.0;
        } else {
            avg /= selected.len() as f32;
        }
        WorkingSetSummary {
            target_limit: spec.limit,
            lanes_requested: spec.lanes.len(),
            selected: selected.len(),
            avg_cscore: if selected.is_empty() { 0.0 } else { avg },
            max_cscore: max,
            min_cscore: min,
            threshold_hits: if has_above { hits } else { 0 },
            total_candidates,
            lane_counts: lane_counts.clone(),
            min_score: spec.min_score,
            scorer: scorer.to_string(),
        }
    }
}

fn env_flag(key: &str) -> Option<bool> {
    std::env::var(key).ok().and_then(|v| {
        let v = v.trim().to_ascii_lowercase();
        match v.as_str() {
            "1" | "true" | "yes" | "on" => Some(true),
            "0" | "false" | "no" | "off" => Some(false),
            _ => None,
        }
    })
}

pub fn default_lanes() -> Vec<String> {
    std::env::var("ARW_CONTEXT_LANES_DEFAULT")
        .ok()
        .map(|s| {
            s.split(',')
                .map(|v| v.trim().to_string())
                .filter(|v| !v.is_empty())
                .collect::<Vec<_>>()
        })
        .filter(|lanes: &Vec<String>| !lanes.is_empty())
        .unwrap_or_else(|| {
            vec![
                "semantic".to_string(),
                "procedural".to_string(),
                "episodic".to_string(),
            ]
        })
}

pub fn default_limit() -> usize {
    std::env::var("ARW_CONTEXT_K")
        .ok()
        .and_then(|s| s.parse::<usize>().ok())
        .filter(|v| *v > 0)
        .unwrap_or(18)
}

pub fn default_expand_per_seed() -> usize {
    std::env::var("ARW_CONTEXT_EXPAND_PER_SEED")
        .ok()
        .and_then(|s| s.parse::<usize>().ok())
        .unwrap_or(3)
        .min(16)
}

pub fn default_diversity_lambda() -> f32 {
    std::env::var("ARW_CONTEXT_DIVERSITY_LAMBDA")
        .ok()
        .and_then(|s| s.parse::<f32>().ok())
        .unwrap_or(0.72)
}

pub fn default_min_score() -> f32 {
    std::env::var("ARW_CONTEXT_MIN_SCORE")
        .ok()
        .and_then(|s| s.parse::<f32>().ok())
        .unwrap_or(0.1)
}

pub fn default_lane_bonus() -> f32 {
    std::env::var("ARW_CONTEXT_LANE_BONUS")
        .ok()
        .and_then(|s| s.parse::<f32>().ok())
        .unwrap_or(0.05)
        .clamp(0.0, 1.0)
}

pub fn default_expand_query() -> bool {
    env_flag("ARW_CONTEXT_EXPAND_QUERY").unwrap_or(false)
}

pub fn default_expand_query_top_k() -> usize {
    std::env::var("ARW_CONTEXT_EXPAND_QUERY_TOP_K")
        .ok()
        .and_then(|s| s.parse::<usize>().ok())
        .filter(|v| *v > 0)
        .unwrap_or(4)
        .min(32)
}

pub fn default_scorer() -> String {
    std::env::var("ARW_CONTEXT_SCORER")
        .ok()
        .map(|s| s.trim().to_ascii_lowercase())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "mmrd".to_string())
}

pub fn default_max_iterations() -> usize {
    std::env::var("ARW_CONTEXT_COVERAGE_MAX_ITERS")
        .ok()
        .and_then(|s| s.parse::<usize>().ok())
        .filter(|v| *v > 0)
        .unwrap_or(2)
        .min(6)
}

pub fn default_streaming_enabled() -> bool {
    env_flag("ARW_CONTEXT_STREAM_DEFAULT").unwrap_or(false)
}
