use crate::{util, AppState};
use anyhow::Result;
use chrono::SecondsFormat;
use metrics::{counter, histogram};
use serde_json::{json, Map, Value};
use std::cmp::Ordering;
use std::collections::{BTreeMap, BinaryHeap, HashMap, HashSet};
use std::sync::Arc;
use std::time::Duration;
use std::time::Instant;

use arw_topics as topics;

pub const STREAM_EVENT_STARTED: &str = topics::TOPIC_WORKING_SET_STARTED;
pub const STREAM_EVENT_SEED: &str = topics::TOPIC_WORKING_SET_SEED;
pub const STREAM_EVENT_EXPANDED: &str = topics::TOPIC_WORKING_SET_EXPANDED;
pub const STREAM_EVENT_QUERY_EXPANDED: &str = topics::TOPIC_WORKING_SET_EXPAND_QUERY;
pub const STREAM_EVENT_SELECTED: &str = topics::TOPIC_WORKING_SET_SELECTED;
pub const STREAM_EVENT_COMPLETED: &str = topics::TOPIC_WORKING_SET_COMPLETED;

const METRIC_WORLD_CANDIDATES: &str = "arw_context_world_candidates_total";
const DEFAULT_WORLD_LANE: &str = "world";

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
    pub slot_budgets: BTreeMap<String, usize>,
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
        self.normalize_slot_budgets();
    }

    pub fn scorer_label(&self) -> String {
        self.scorer.clone().unwrap_or_else(default_scorer)
    }

    pub fn snapshot(&self) -> Value {
        let mut snapshot = serde_json::Map::new();
        snapshot.insert("query_provided".into(), json!(self.query.is_some()));
        snapshot.insert("lanes".into(), json!(self.lanes));
        snapshot.insert("limit".into(), json!(self.limit));
        snapshot.insert("expand_per_seed".into(), json!(self.expand_per_seed));
        snapshot.insert("diversity_lambda".into(), json!(self.diversity_lambda));
        snapshot.insert("min_score".into(), json!(self.min_score));
        snapshot.insert("project".into(), json!(self.project));
        snapshot.insert("lane_bonus".into(), json!(self.lane_bonus));
        snapshot.insert("scorer".into(), json!(self.scorer));
        snapshot.insert("expand_query".into(), json!(self.expand_query));
        snapshot.insert("expand_query_top_k".into(), json!(self.expand_query_top_k));
        if !self.slot_budgets.is_empty() {
            let mut slots = serde_json::Map::new();
            for (slot, limit) in self.slot_budgets.iter() {
                slots.insert(slot.clone(), json!(limit));
            }
            snapshot.insert("slot_budgets".into(), Value::Object(slots));
        }
        Value::Object(snapshot)
    }

    fn normalize_slot_budgets(&mut self) {
        if self.slot_budgets.is_empty() {
            self.slot_budgets = default_slot_budgets();
        }
        if self.slot_budgets.is_empty() {
            return;
        }
        let mut normalized = BTreeMap::new();
        let limit_cap = self.limit.max(1);
        for (slot, value) in std::mem::take(&mut self.slot_budgets) {
            let slot = slot.trim().to_ascii_lowercase();
            if slot.is_empty() {
                continue;
            }
            let capped = value.min(limit_cap);
            if capped == 0 {
                continue;
            }
            normalized.insert(slot, capped);
        }
        self.slot_budgets = normalized;
    }

    pub fn slot_limit(&self, slot: &str) -> Option<usize> {
        if self.slot_budgets.is_empty() {
            return None;
        }
        let key = slot.trim().to_ascii_lowercase();
        self.slot_budgets
            .get(&key)
            .copied()
            .or_else(|| self.slot_budgets.get("*").copied())
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
    pub slot_counts: BTreeMap<String, usize>,
    pub slot_budgets: BTreeMap<String, usize>,
    pub min_score: f32,
    pub scorer: String,
}

impl WorkingSetSummary {
    pub fn to_json(&self) -> Value {
        let mut obj = serde_json::Map::new();
        obj.insert("target_limit".into(), json!(self.target_limit));
        obj.insert("lanes_requested".into(), json!(self.lanes_requested));
        obj.insert("selected".into(), json!(self.selected));
        obj.insert("avg_cscore".into(), json!(self.avg_cscore));
        obj.insert("max_cscore".into(), json!(self.max_cscore));
        obj.insert("min_cscore".into(), json!(self.min_cscore));
        obj.insert("threshold_hits".into(), json!(self.threshold_hits));
        obj.insert("total_candidates".into(), json!(self.total_candidates));
        let mut lanes = serde_json::Map::new();
        for (lane, count) in self.lane_counts.iter() {
            lanes.insert(lane.clone(), json!(count));
        }
        obj.insert("lane_counts".into(), Value::Object(lanes));
        if !self.slot_counts.is_empty() || !self.slot_budgets.is_empty() {
            let mut slots = serde_json::Map::new();
            if !self.slot_counts.is_empty() {
                let mut counts = serde_json::Map::new();
                for (slot, count) in self.slot_counts.iter() {
                    counts.insert(slot.clone(), json!(count));
                }
                slots.insert("counts".into(), Value::Object(counts));
            }
            if !self.slot_budgets.is_empty() {
                let mut budgets = serde_json::Map::new();
                for (slot, limit) in self.slot_budgets.iter() {
                    budgets.insert(slot.clone(), json!(limit));
                }
                slots.insert("budgets".into(), Value::Object(budgets));
            }
            obj.insert("slots".into(), Value::Object(slots));
        }
        obj.insert("min_score".into(), json!(self.min_score));
        obj.insert("scorer".into(), json!(self.scorer));
        Value::Object(obj)
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

#[allow(dead_code)]
pub fn assemble(
    state: &AppState,
    spec: &WorkingSetSpec,
    world_beliefs: Arc<[Value]>,
) -> Result<WorkingSet> {
    let mut observer = (); // no-op
    assemble_with_observer(state, spec, &mut observer, world_beliefs)
}

pub fn assemble_with_observer<O: WorkingSetObserver>(
    state: &AppState,
    spec: &WorkingSetSpec,
    observer: &mut O,
    world_beliefs: Arc<[Value]>,
) -> Result<WorkingSet> {
    let mut builder = WorkingSetBuilder::new(state, spec.clone(), world_beliefs);
    builder.build(observer)
}

struct WorkingSetBuilder<'a> {
    state: &'a AppState,
    spec: WorkingSetSpec,
    world_beliefs: Arc<[Value]>,
}

struct PendingExpansion {
    dst_id: String,
    seed: SeedInfo,
    link: Value,
}

impl<'a> WorkingSetBuilder<'a> {
    fn new(state: &'a AppState, spec: WorkingSetSpec, world_beliefs: Arc<[Value]>) -> Self {
        Self {
            state,
            spec,
            world_beliefs,
        }
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
        counter!("arw_context_scorer_used_total", "scorer" => scorer.name()).increment(1);

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
            let mut items = self.state.kernel().select_memory_hybrid(
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
                        "lane" => lane_for_event.unwrap_or_else(|| "unknown".into())
                    )
                    .increment(1);
                    seeds_raw.push(payload);
                    seed_infos.push(seed);
                    insert_candidate(&mut candidates, candidate);
                }
            }
        }
        let retrieve_elapsed = retrieve_start.elapsed();
        histogram!(
            "arw_context_phase_duration_ms",
            "phase" => "retrieve"
        )
        .record(retrieve_elapsed.as_secs_f64() * 1000.0);

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
                counter!("arw_context_query_expansion_total").increment(added as u64);
            }
        }

        let expand_start = Instant::now();
        if spec.expand_per_seed > 0 {
            let mut pending: Vec<PendingExpansion> = Vec::new();
            let mut fetch_ids: Vec<String> = Vec::new();
            let mut seen_dst: HashSet<String> = HashSet::new();
            for seed in seed_infos.iter().cloned() {
                let links = self
                    .state
                    .kernel()
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
                        let dst_owned = dst_id.to_string();
                        if !seen_dst.insert(dst_owned.clone()) {
                            continue;
                        }
                        pending.push(PendingExpansion {
                            dst_id: dst_owned.clone(),
                            seed: seed.clone(),
                            link,
                        });
                        fetch_ids.push(dst_owned);
                    }
                }
            }
            if !pending.is_empty() {
                let fetched = self
                    .state
                    .kernel()
                    .get_memory_many(&fetch_ids)
                    .unwrap_or_default();
                for entry in pending {
                    if let Some(record) = fetched.get(&entry.dst_id) {
                        if let Some(candidate) = build_expansion_candidate(
                            record.clone(),
                            &entry.seed,
                            &entry.link,
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
                                "lane" => lane_for_event.unwrap_or_else(|| "unknown".into())
                            )
                            .increment(1);
                            expanded_raw.push(payload);
                            insert_candidate(&mut candidates, candidate);
                        }
                    }
                }
            }
        }
        let expand_elapsed = expand_start.elapsed();
        histogram!(
            "arw_context_phase_duration_ms",
            "phase" => "link_expand"
        )
        .record(expand_elapsed.as_secs_f64() * 1000.0);

        self.ingest_world_beliefs(&spec, &mut candidates, &mut expanded_raw, observer);

        let candidate_total = candidates.len();
        let has_above = candidates
            .values()
            .any(|candidate| candidate.cscore >= spec.min_score);
        let all_candidates: Vec<Candidate> = candidates.into_values().collect();

        let select_start = Instant::now();
        let (selected, lane_counts, slot_counts) =
            select_candidates(all_candidates, &spec, has_above, scorer.as_ref(), observer);
        let select_elapsed = select_start.elapsed();
        histogram!(
            "arw_context_phase_duration_ms",
            "phase" => "select"
        )
        .record(select_elapsed.as_secs_f64() * 1000.0);

        let items: Vec<Value> = selected.iter().map(|c| c.value.clone()).collect();
        let summary = WorkingSetSummary::from_selection(
            &spec,
            &selected,
            &lane_counts,
            &slot_counts,
            has_above,
            candidate_total,
            scorer.name(),
        );

        let total_elapsed = total_start.elapsed();
        histogram!(
            "arw_context_phase_duration_ms",
            "phase" => "total"
        )
        .record(total_elapsed.as_secs_f64() * 1000.0);

        let diagnostics = build_diagnostics(
            &spec,
            &seed_infos,
            &expanded_raw,
            &lane_counts,
            &slot_counts,
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

    fn ingest_world_beliefs<O: WorkingSetObserver>(
        &self,
        spec: &WorkingSetSpec,
        candidates: &mut HashMap<String, Candidate>,
        expanded_raw: &mut Vec<Value>,
        observer: &mut O,
    ) {
        for belief in self.world_beliefs.iter() {
            if let Some(candidate) = build_world_candidate(belief, spec.project.as_deref()) {
                let payload = candidate.value.clone();
                let lane_for_event = candidate.lane.clone();
                observer.emit(
                    STREAM_EVENT_EXPANDED,
                    json!({
                        "item": payload.clone(),
                        "lane": lane_for_event.clone(),
                        "source": "world",
                    }),
                );
                counter!(METRIC_WORLD_CANDIDATES).increment(1);
                expanded_raw.push(payload);
                insert_candidate(candidates, candidate);
            }
        }
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
        let seed_pool = seed_infos.len();
        let mut seeds_with_embed: Vec<(&SeedInfo, &Vec<f32>)> = seed_infos
            .iter()
            .filter_map(|seed| seed.embed.as_ref().map(|embed| (seed, embed)))
            .collect();
        if seeds_with_embed.is_empty() {
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
        let mut lane_sums: HashMap<String, (Vec<f32>, f32)> = HashMap::new();
        let mut lane_seed_ids: HashMap<String, Vec<String>> = HashMap::new();
        for (seed, embed) in seeds_with_embed.iter().take(top_k) {
            if embed.len() != dims {
                continue;
            }
            let weight = seed.cscore.max(0.05);
            for (i, value) in embed.iter().enumerate() {
                avg[i] += value * weight;
            }
            weight_sum += weight;
            if let Some(lane_name) = seed.lane.as_ref() {
                let entry = lane_sums
                    .entry(lane_name.clone())
                    .or_insert_with(|| (vec![0f32; dims], 0f32));
                for (i, value) in embed.iter().enumerate() {
                    entry.0[i] += value * weight;
                }
                entry.1 += weight;
                lane_seed_ids
                    .entry(lane_name.clone())
                    .or_default()
                    .push(seed.id.clone());
            }
            seed_ids.push(seed.id.clone());
        }
        if weight_sum == 0.0 {
            return Ok(0);
        }
        for value in avg.iter_mut() {
            *value /= weight_sum;
        }
        let mut lane_vectors: HashMap<String, Vec<f32>> = HashMap::new();
        for (lane, (mut sum, weight)) in lane_sums.into_iter() {
            if weight > 0.0 {
                for value in sum.iter_mut() {
                    *value /= weight;
                }
                lane_vectors.insert(lane, sum);
            }
        }
        let global_embed = avg.as_slice();
        let global_embed_opt = (!global_embed.is_empty()).then_some(global_embed);
        let seeds_fallback = seed_ids;
        let mut added = 0usize;
        let fetch_k = ((spec.limit * 2) + spec.expand_per_seed).max(12) as i64;
        for lane in lanes.iter() {
            let embed_opt = lane
                .as_ref()
                .and_then(|lane_name| lane_vectors.get(lane_name).map(|vec| vec.as_slice()))
                .or(global_embed_opt);
            let mut items = self.state.kernel().select_memory_hybrid(
                spec.query.as_deref(),
                embed_opt,
                lane.as_deref(),
                fetch_k,
            )?;
            for item in items.drain(..) {
                let lane_override = lane.clone().or_else(|| {
                    item.get("lane")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string())
                });
                let seeds_for_lane = lane
                    .as_ref()
                    .and_then(|lane_name| lane_seed_ids.get(lane_name))
                    .unwrap_or(&seeds_fallback);
                if let Some(candidate) = build_query_expansion_candidate(
                    item,
                    lane_override,
                    spec.project.as_deref(),
                    seeds_for_lane.as_slice(),
                    seed_pool,
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
                            "seeds_used": seeds_for_lane,
                        }),
                    );
                    counter!(
                        "arw_context_query_expansion_candidates_total",
                        "lane" => lane_for_event.unwrap_or_else(|| "unknown".into())
                    )
                    .increment(1);
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
    slot_key: String,
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

#[derive(Clone)]
struct HeapEntry {
    score: f32,
    idx: usize,
    epoch: u64,
}

impl HeapEntry {
    fn new(score: f32, idx: usize, epoch: u64) -> Self {
        Self { score, idx, epoch }
    }
}

impl PartialEq for HeapEntry {
    fn eq(&self, other: &Self) -> bool {
        self.idx == other.idx
            && self.epoch == other.epoch
            && self.score.to_bits() == other.score.to_bits()
    }
}

impl Eq for HeapEntry {}

impl Ord for HeapEntry {
    fn cmp(&self, other: &Self) -> Ordering {
        match self.score.partial_cmp(&other.score) {
            Some(Ordering::Equal) => other.idx.cmp(&self.idx),
            Some(ord) => ord,
            None => Ordering::Equal,
        }
    }
}

impl PartialOrd for HeapEntry {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

fn select_candidates<O: WorkingSetObserver>(
    candidates: Vec<Candidate>,
    spec: &WorkingSetSpec,
    has_above: bool,
    scorer: &dyn CandidateScorer,
    observer: &mut O,
) -> (
    Vec<Candidate>,
    BTreeMap<String, usize>,
    BTreeMap<String, usize>,
) {
    let mut storage: Vec<Option<Candidate>> = candidates.into_iter().map(Some).collect();
    let mut selected: Vec<Candidate> = Vec::new();
    let mut lane_counts: BTreeMap<String, usize> = BTreeMap::new();
    let mut slot_counts: BTreeMap<String, usize> = BTreeMap::new();
    let use_slots = !spec.slot_budgets.is_empty();
    let mut slot_limit_cache: HashMap<String, Option<usize>> = HashMap::new();
    let mut resolve_slot_limit = |slot: &str| -> Option<usize> {
        if let Some(limit) = slot_limit_cache.get(slot) {
            *limit
        } else {
            let resolved = spec.slot_limit(slot);
            slot_limit_cache.insert(slot.to_string(), resolved);
            resolved
        }
    };
    let mut heap: BinaryHeap<HeapEntry> = BinaryHeap::new();
    let mut versions: Vec<u64> = vec![0; storage.len()];
    let mut state_epoch: u64 = 0;

    let score_candidate =
        |candidate: &Candidate, selected: &[Candidate], lane_counts: &BTreeMap<String, usize>| {
            let ctx = SelectionContext {
                spec,
                selected,
                lane_counts,
                require_threshold: has_above,
            };
            scorer.score(candidate, &ctx)
        };

    for idx in 0..storage.len() {
        if let Some(candidate) = storage[idx].as_ref() {
            let score = score_candidate(candidate, &selected, &lane_counts);
            versions[idx] = state_epoch;
            heap.push(HeapEntry::new(score, idx, state_epoch));
        }
    }
    while selected.len() < spec.limit {
        let entry = match heap.pop() {
            Some(entry) => entry,
            None => break,
        };
        if entry.idx >= storage.len() {
            continue;
        }
        if versions[entry.idx] != entry.epoch {
            continue;
        }
        let candidate_ref = match storage[entry.idx].as_ref() {
            Some(candidate) => candidate,
            None => continue,
        };
        if entry.epoch != state_epoch {
            let score = score_candidate(candidate_ref, &selected, &lane_counts);
            versions[entry.idx] = state_epoch;
            heap.push(HeapEntry::new(score, entry.idx, state_epoch));
            continue;
        }
        if !entry.score.is_finite() {
            storage[entry.idx] = None;
            continue;
        }
        let slot_key = if use_slots {
            Some(candidate_ref.slot_key().to_string())
        } else {
            None
        };
        if let (true, Some(ref key)) = (use_slots, slot_key.as_ref()) {
            if let Some(limit) = resolve_slot_limit(key.as_str()) {
                let current = slot_counts.get(key.as_str()).copied().unwrap_or(0);
                if current >= limit {
                    storage[entry.idx] = None;
                    continue;
                }
            }
        }
        let cand = match storage[entry.idx].take() {
            Some(cand) => cand,
            None => continue,
        };
        if let Some(ref key) = slot_key {
            *slot_counts.entry(key.clone()).or_insert(0) += 1;
        }
        let lane_label = cand.lane_label().to_string();
        *lane_counts.entry(lane_label.clone()).or_insert(0) += 1;
        counter!("arw_context_selected_total", "lane" => lane_label).increment(1);
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
        state_epoch = state_epoch.saturating_add(1);
    }
    (selected, lane_counts, slot_counts)
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

fn build_world_candidate(belief: &Value, project: Option<&str>) -> Option<Candidate> {
    let raw_id = belief.get("id").and_then(|v| v.as_str())?.trim();
    if raw_id.is_empty() {
        return None;
    }
    let id = format!("world:{}", raw_id);
    let lane = belief
        .get("lane")
        .and_then(|v| v.as_str())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| DEFAULT_WORLD_LANE.to_string());
    let updated = belief
        .get("ts")
        .and_then(|v| v.as_str())
        .or_else(|| belief.get("updated").and_then(|v| v.as_str()));

    let mut base = belief
        .get("confidence")
        .and_then(|v| v.as_f64())
        .unwrap_or(0.45) as f32;
    if let Some(score) = belief.get("score").and_then(|v| v.as_f64()) {
        base = base.max(score as f32);
    }
    if let Some(weight) = belief.get("weight").and_then(|v| v.as_f64()) {
        base = base.max(weight as f32);
    }
    let severity_component = belief
        .get("severity")
        .and_then(|v| v.as_f64())
        .map(|s| (s / 5.0) as f32)
        .unwrap_or(0.0);
    let recency = recency_score(updated);
    let mut cscore = (0.6 * base + 0.25 * recency + 0.15 * severity_component).clamp(0.0, 1.0);
    if cscore < 0.05 {
        cscore = 0.05;
    }

    let mut map = Map::new();
    map.insert("id".into(), json!(id.clone()));
    map.insert("lane".into(), json!(lane.clone()));
    if let Some(kind) = belief.get("kind").and_then(|v| v.as_str()) {
        map.insert("kind".into(), json!(kind));
    } else if let Some(action) = belief.get("action").and_then(|v| v.as_str()) {
        map.insert("kind".into(), json!(action));
    } else {
        map.insert("kind".into(), json!("belief"));
    }
    if let Some(ts) = updated {
        map.insert("updated".into(), json!(ts));
    }
    if let Some(project_val) = belief.get("project").and_then(|v| v.as_str()).or(project) {
        map.insert("project".into(), json!(project_val));
    }
    if let Some(slot) = belief
        .get("slot")
        .and_then(|v| v.as_str())
        .or_else(|| belief.get("action").and_then(|v| v.as_str()))
    {
        map.insert("slot".into(), json!(slot));
    }

    let mut tag_components: Vec<String> = Vec::new();
    if let Some(kind) = belief.get("kind").and_then(|v| v.as_str()) {
        tag_components.push(kind.to_ascii_lowercase());
    }
    if let Some(action) = belief.get("action").and_then(|v| v.as_str()) {
        tag_components.push(format!("action:{}", action.to_ascii_lowercase()));
    }
    if let Some(source) = belief.get("source").and_then(|v| v.as_str()) {
        tag_components.push(format!("source:{}", source.to_ascii_lowercase()));
    }
    if !tag_components.is_empty() {
        map.insert("tags".into(), Value::String(tag_components.join(",")));
    }

    let mut value_payload = Map::new();
    value_payload.insert("source".into(), json!("world"));
    if let Some(rationale) = belief.get("rationale") {
        value_payload.insert("rationale".into(), rationale.clone());
    }
    value_payload.insert("record".into(), belief.clone());
    map.insert("value".into(), Value::Object(value_payload));

    let mut value = Value::Object(map);
    let affinity = project.map(|p| project_affinity(&value, p)).unwrap_or(1.0);
    cscore = (cscore * affinity).clamp(0.0, 1.0);

    if let Some(obj) = value.as_object_mut() {
        obj.insert("source".into(), json!("world"));
        obj.insert("cscore".into(), json!(cscore));
        obj.insert(
            "explain".into(),
            json!({
                "kind": "world",
                "components": {
                    "base": base,
                    "recency": recency,
                    "severity": severity_component,
                    "project_affinity": affinity,
                },
                "cscore": cscore,
            }),
        );
    }

    Some(Candidate::from_value(id, Some(lane), value, cscore))
}

fn build_query_expansion_candidate(
    mut value: Value,
    lane: Option<String>,
    project: Option<&str>,
    seeds_used: &[String],
    seed_pool: usize,
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
    let pool = seed_pool.max(1) as f32;
    let support = (seeds_used.len() as f32 / pool).clamp(0.0, 1.0);
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
                "support_detail": {
                    "count": seeds_used.len(),
                    "pool": seed_pool,
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
    slot_counts: &BTreeMap<String, usize>,
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
    let mut root = serde_json::Map::new();
    root.insert("params".into(), spec.snapshot());
    root.insert("counts".into(), counts);
    root.insert("lanes".into(), Value::Object(lanes));
    if !slot_counts.is_empty() || !spec.slot_budgets.is_empty() {
        let mut slots = serde_json::Map::new();
        if !slot_counts.is_empty() {
            let mut counts = serde_json::Map::new();
            for (slot, count) in slot_counts.iter() {
                counts.insert(slot.clone(), json!(count));
            }
            slots.insert("counts".into(), Value::Object(counts));
        }
        if !spec.slot_budgets.is_empty() {
            let mut budgets = serde_json::Map::new();
            for (slot, limit) in spec.slot_budgets.iter() {
                budgets.insert(slot.clone(), json!(limit));
            }
            slots.insert("budgets".into(), Value::Object(budgets));
        }
        root.insert("slots".into(), Value::Object(slots));
    }
    root.insert("had_candidates_above_threshold".into(), json!(has_above));
    root.insert("summary".into(), summary.to_json());
    root.insert(
        "timings_ms".into(),
        json!({
            "retrieve": retrieve_elapsed.as_secs_f64() * 1000.0,
            "query_expand": expand_query_elapsed.as_secs_f64() * 1000.0,
            "link_expand": expand_elapsed.as_secs_f64() * 1000.0,
            "select": select_elapsed.as_secs_f64() * 1000.0,
            "total": total_elapsed.as_secs_f64() * 1000.0,
        }),
    );
    root.insert("scorer".into(), json!(scorer_name));
    root.insert(
        "generated_at".into(),
        json!(chrono::Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true)),
    );
    Value::Object(root)
}

impl Candidate {
    fn from_value(id: String, lane: Option<String>, mut value: Value, cscore: f32) -> Self {
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
        let slot = extract_slot(&value);
        if let (Some(slot_name), Some(obj)) = (slot.as_ref(), value.as_object_mut()) {
            obj.insert("slot".into(), json!(slot_name));
        }
        let slot_key = slot
            .as_deref()
            .map(|s| s.trim().to_ascii_lowercase())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "unslotted".to_string());
        util::attach_memory_ptr(&mut value);
        Candidate {
            id,
            lane,
            key,
            tags,
            embed,
            cscore,
            value,
            slot_key,
        }
    }

    fn slot_key(&self) -> &str {
        &self.slot_key
    }

    fn lane_label(&self) -> &str {
        self.lane.as_deref().unwrap_or("unknown")
    }
}

fn extract_slot(value: &Value) -> Option<String> {
    let slot = value
        .get("slot")
        .and_then(|v| v.as_str())
        .or_else(|| value.get("kind").and_then(|v| v.as_str()))
        .or_else(|| {
            value
                .get("value")
                .and_then(|v| v.get("slot"))
                .and_then(|v| v.as_str())
        })
        .or_else(|| {
            value
                .get("value")
                .and_then(|v| v.get("kind"))
                .and_then(|v| v.as_str())
        })?;
    let slot = slot.trim();
    if slot.is_empty() {
        return None;
    }
    Some(slot.to_ascii_lowercase())
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
        slot_counts: &BTreeMap<String, usize>,
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
            slot_counts: slot_counts.clone(),
            slot_budgets: spec.slot_budgets.clone(),
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

#[cfg(test)]
const CONTEXT_ENV_KEYS: &[&str] = &[
    "ARW_CONTEXT_COVERAGE_MAX_ITERS",
    "ARW_CONTEXT_DIVERSITY_LAMBDA",
    "ARW_CONTEXT_EXPAND_PER_SEED",
    "ARW_CONTEXT_EXPAND_QUERY",
    "ARW_CONTEXT_EXPAND_QUERY_TOP_K",
    "ARW_CONTEXT_K",
    "ARW_CONTEXT_LANE_BONUS",
    "ARW_CONTEXT_LANES_DEFAULT",
    "ARW_CONTEXT_MIN_SCORE",
    "ARW_CONTEXT_SCORER",
    "ARW_CONTEXT_SLOT_BUDGETS",
    "ARW_CONTEXT_STREAM_DEFAULT",
];

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

pub fn default_slot_budgets() -> BTreeMap<String, usize> {
    let mut budgets = BTreeMap::new();
    let raw = match std::env::var("ARW_CONTEXT_SLOT_BUDGETS") {
        Ok(raw) => raw,
        Err(_) => return budgets,
    };
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return budgets;
    }
    if trimmed.starts_with('{') {
        if let Ok(Value::Object(map)) = serde_json::from_str::<Value>(trimmed) {
            for (slot, value) in map.into_iter() {
                if let Some(parsed) = parse_slot_budget_value(value) {
                    let key = normalize_slot_key(&slot);
                    if !key.is_empty() {
                        budgets.insert(key, parsed);
                    }
                }
            }
        }
        return budgets;
    }
    for part in trimmed.split(',') {
        let mut iter = part.splitn(2, '=');
        let key = iter.next().unwrap_or("").trim();
        let value = iter.next().unwrap_or("").trim();
        if key.is_empty() || value.is_empty() {
            continue;
        }
        if let Ok(parsed) = value.parse::<usize>() {
            let key = normalize_slot_key(key);
            if !key.is_empty() {
                budgets.insert(key, parsed);
            }
        }
    }
    budgets
}

fn parse_slot_budget_value(value: Value) -> Option<usize> {
    match value {
        Value::Number(num) => num.as_u64().map(|v| v as usize),
        Value::String(s) => s.trim().parse::<usize>().ok(),
        Value::Bool(b) => Some(if b { 1 } else { 0 }),
        Value::Null => None,
        Value::Array(_) | Value::Object(_) => None,
    }
}

fn normalize_slot_key(input: &str) -> String {
    input.trim().to_ascii_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::env as test_env;
    use chrono::{SecondsFormat, Utc};
    use serde_json::json;

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
        spec.normalize();

        let snap = spec.snapshot();
        assert_eq!(snap["limit"], json!(12));
        assert_eq!(snap["lanes"], json!(vec!["semantic".to_string()]));
        assert_eq!(snap["expand_query"], json!(true));
        assert_eq!(snap["expand_query_top_k"], json!(6));
        assert_eq!(snap["min_score"], json!(spec.min_score));
        assert_eq!(snap["scorer"], json!(spec.scorer));
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
}
