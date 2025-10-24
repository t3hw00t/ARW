use std::cmp::Ordering;
use std::collections::{BTreeMap, BinaryHeap};
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::Result;
use arw_kernel::KernelSession;
use chrono::SecondsFormat;
use metrics::{counter, histogram};
use rustc_hash::{FxHashMap, FxHashSet};
use serde_json::{json, Map, Value};

use crate::{
    context_capability::{self, ContextCapabilityPlan, PlanApplicationHints},
    util, AppState,
};

use super::{
    default_expand_per_seed, default_limit,
    models::{WorkingSet, WorkingSetObserver, WorkingSetSummary},
    spec::WorkingSetSpec,
    SharedValue, DEFAULT_WORLD_LANE, METRIC_WORLD_CANDIDATES, STREAM_EVENT_COMPLETED,
    STREAM_EVENT_EXPANDED, STREAM_EVENT_QUERY_EXPANDED, STREAM_EVENT_SEED, STREAM_EVENT_SELECTED,
    STREAM_EVENT_STARTED,
};

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
    let mut builder = WorkingSetBuilder::new(state, spec.clone(), world_beliefs)?;
    builder.build(observer)
}

struct WorkingSetBuilder {
    spec: WorkingSetSpec,
    world_beliefs: Arc<[Value]>,
    kernel_session: KernelSession,
    capability_plan: ContextCapabilityPlan,
    plan_hints: PlanApplicationHints,
}

struct PendingExpansion {
    dst_id: String,
    seed: SeedInfo,
    link: Value,
}

impl WorkingSetBuilder {
    fn new(state: &AppState, spec: WorkingSetSpec, world_beliefs: Arc<[Value]>) -> Result<Self> {
        let kernel_session = state.kernel().session()?;
        let capability_profile = state.capability().maybe_refresh(false);
        let capability_plan = context_capability::plan_for_profile(&capability_profile);
        counter!(
            "arw_context_capability_plan_total",
            "tier" => capability_plan.tier.as_str()
        )
        .increment(1);
        let plan_hints = PlanApplicationHints {
            apply_default_limit: spec.limit == 0 || spec.limit == default_limit(),
            apply_default_expand: spec.expand_per_seed == 0
                || spec.expand_per_seed == default_expand_per_seed(),
        };
        Ok(Self {
            spec,
            world_beliefs,
            kernel_session,
            capability_plan,
            plan_hints,
        })
    }

    fn build<O: WorkingSetObserver>(&mut self, observer: &mut O) -> Result<WorkingSet> {
        self.spec.normalize();
        context_capability::apply_plan_to_spec(
            &mut self.spec,
            &self.capability_plan,
            self.plan_hints,
        );
        self.plan_hints = PlanApplicationHints::default();
        let spec = &self.spec;
        let scorer_label = spec.scorer_label();
        let scorer = resolve_scorer(Some(scorer_label.as_str()));
        observer.emit(
            STREAM_EVENT_STARTED,
            Arc::new(json!({
                "spec": spec.snapshot(),
                "scorer": scorer.name(),
            })),
        );
        counter!("arw_context_scorer_used_total", "scorer" => scorer.name()).increment(1);

        let total_start = Instant::now();
        let mut lanes: Vec<Option<String>> = if spec.lanes.is_empty() {
            vec![None]
        } else {
            spec.lanes.iter().cloned().map(Some).collect()
        };
        lanes.dedup_by(|a, b| a.as_ref().map(|s| s.as_str()) == b.as_ref().map(|s| s.as_str()));

        let mut candidates: FxHashMap<String, Candidate> = FxHashMap::default();
        let mut seeds_raw: Vec<SharedValue> = Vec::new();
        let mut expanded_raw: Vec<SharedValue> = Vec::new();
        let mut seed_infos: Vec<SeedInfo> = Vec::new();

        let retrieve_start = Instant::now();
        let effective_lane_count = if lanes.len() == 1 && lanes[0].is_none() {
            1
        } else {
            lanes.len().max(1)
        };
        for lane in lanes.iter() {
            let fetch_k = self.compute_retrieve_limit(spec, lane, effective_lane_count);
            let mut items = self.kernel_session.select_memory_hybrid(
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
                    let payload = Arc::clone(&candidate.value);
                    let lane_for_event = candidate.lane.clone();
                    observer.emit(
                        STREAM_EVENT_SEED,
                        Arc::new(json!({
                            "item": payload.as_ref().clone(),
                            "lane": lane_for_event.clone(),
                        })),
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
                spec,
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
        if spec.expand_per_seed > 0 && !seed_infos.is_empty() {
            let mut pending: Vec<PendingExpansion> = Vec::new();
            let mut fetch_ids: Vec<String> = Vec::new();
            let mut seen_dst: FxHashSet<String> = FxHashSet::default();
            let seed_ids_for_links: Vec<String> =
                seed_infos.iter().map(|seed| seed.id.clone()).collect();
            let links_map = self
                .kernel_session
                .list_memory_links_many(&seed_ids_for_links, spec.expand_per_seed as i64)
                .unwrap_or_default();
            for seed in seed_infos.iter().cloned() {
                if let Some(links) = links_map.get(&seed.id) {
                    for link in links.iter().cloned() {
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
            }
            if !pending.is_empty() {
                let fetched = self
                    .kernel_session
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
                            let payload = Arc::clone(&candidate.value);
                            let lane_for_event = candidate.lane.clone();
                            observer.emit(
                                STREAM_EVENT_EXPANDED,
                                Arc::new(json!({
                                    "item": payload.as_ref().clone(),
                                    "lane": lane_for_event.clone(),
                                })),
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

        let world_cap = world_candidate_cap(spec);
        self.ingest_world_beliefs(
            spec,
            &mut candidates,
            &mut expanded_raw,
            observer,
            world_cap,
        );

        let candidate_total = candidates.len();
        let has_above = candidates
            .values()
            .any(|candidate| candidate.cscore >= spec.min_score);
        let all_candidates: Vec<Candidate> = candidates.into_values().collect();

        let select_start = Instant::now();
        let (selected, lane_counts, slot_counts) =
            select_candidates(all_candidates, spec, has_above, scorer.as_ref(), observer);
        let select_elapsed = select_start.elapsed();
        histogram!(
            "arw_context_phase_duration_ms",
            "phase" => "select"
        )
        .record(select_elapsed.as_secs_f64() * 1000.0);

        let items: Vec<SharedValue> = selected.iter().map(|c| Arc::clone(&c.value)).collect();
        let summary = WorkingSetSummary::from_selection(
            spec,
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

        let diagnostics = Arc::new(build_diagnostics(
            spec,
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
        ));

        let completed_payload = Arc::new(json!({
            "items": clone_shared_values(&items),
            "seeds": clone_shared_values(&seeds_raw),
            "expanded": clone_shared_values(&expanded_raw),
            "summary": summary.to_json(),
            "diagnostics": diagnostics.as_ref().clone(),
        }));
        observer.emit(STREAM_EVENT_COMPLETED, Arc::clone(&completed_payload));

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
        candidates: &mut FxHashMap<String, Candidate>,
        expanded_raw: &mut Vec<SharedValue>,
        observer: &mut O,
        cap: usize,
    ) {
        if cap == 0 {
            return;
        }
        let mut world_candidates: Vec<Candidate> = Vec::new();
        for belief in self.world_beliefs.iter() {
            if let Some(candidate) = build_world_candidate(belief, spec.project.as_deref()) {
                insert_ranked_candidate(&mut world_candidates, candidate, cap);
            }
        }
        if world_candidates.is_empty() {
            return;
        }

        for candidate in world_candidates {
            let payload = Arc::clone(&candidate.value);
            let lane_for_event = candidate.lane.clone();
            observer.emit(
                STREAM_EVENT_EXPANDED,
                Arc::new(json!({
                    "item": payload.as_ref().clone(),
                    "lane": lane_for_event.clone(),
                    "source": "world",
                })),
            );
            counter!(METRIC_WORLD_CANDIDATES).increment(1);
            expanded_raw.push(payload);
            insert_candidate(candidates, candidate);
        }
    }

    fn pseudo_relevance_expand<O: WorkingSetObserver>(
        &self,
        spec: &WorkingSetSpec,
        lanes: &[Option<String>],
        seed_infos: &[SeedInfo],
        candidates: &mut FxHashMap<String, Candidate>,
        expanded_raw: &mut Vec<SharedValue>,
        observer: &mut O,
    ) -> Result<usize> {
        let seed_pool = seed_infos.len();
        let mut seeds_with_embed: Vec<(&SeedInfo, &[f32])> = seed_infos
            .iter()
            .filter_map(|seed| seed.embed.as_deref().map(|embed| (seed, embed)))
            .collect();
        if seeds_with_embed.is_empty() {
            return Ok(0);
        }
        let top_k = spec.expand_query_top_k.min(seeds_with_embed.len());
        if top_k == 0 {
            return Ok(0);
        }
        let cmp = |a: &(&SeedInfo, &[f32]), b: &(&SeedInfo, &[f32])| {
            b.0.cscore
                .partial_cmp(&a.0.cscore)
                .unwrap_or(Ordering::Equal)
        };
        if seeds_with_embed.len() > top_k {
            let nth = top_k.saturating_sub(1);
            seeds_with_embed.select_nth_unstable_by(nth, cmp);
            seeds_with_embed.truncate(top_k);
        }
        seeds_with_embed.sort_unstable_by(cmp);
        let dims = seeds_with_embed[0].1.len();
        if dims == 0 {
            return Ok(0);
        }
        let mut avg = vec![0f32; dims];
        let mut weight_sum = 0f32;
        let mut seed_ids: Vec<String> = Vec::new();
        let mut lane_sums: FxHashMap<String, (Vec<f32>, f32)> = FxHashMap::default();
        let mut lane_seed_ids: FxHashMap<String, Vec<String>> = FxHashMap::default();
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
        let mut lane_vectors: FxHashMap<String, Vec<f32>> = FxHashMap::default();
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
            let mut items = self.kernel_session.select_memory_hybrid(
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
                    let payload = Arc::clone(&candidate.value);
                    let lane_for_event = candidate.lane.clone();
                    observer.emit(
                        STREAM_EVENT_QUERY_EXPANDED,
                        Arc::new(json!({
                            "item": payload.as_ref().clone(),
                            "lane": lane_for_event.clone(),
                            "seeds_used": seeds_for_lane,
                        })),
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
pub(super) struct Candidate {
    pub(super) id: String,
    pub(super) lane: Option<String>,
    pub(super) key: Option<String>,
    pub(super) tags: Vec<String>,
    pub(super) embed: Option<Arc<[f32]>>,
    pub(super) cscore: f32,
    pub(super) value: SharedValue,
    pub(super) slot_key: String,
}

#[derive(Clone)]
struct SeedInfo {
    id: String,
    cscore: f32,
    lane: Option<String>,
    embed: Option<Arc<[f32]>>,
}

pub(super) trait CandidateScorer: Send + Sync {
    fn name(&self) -> &'static str;
    fn score(&self, candidate: &Candidate, ctx: &SelectionContext) -> f32;
}

pub(super) struct SelectionContext<'a> {
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
            .map(|lane| lane_bonus(ctx.lane_counts, lane, ctx.spec))
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
            .map(|lane| lane_bonus(ctx.lane_counts, lane, ctx.spec))
            .unwrap_or(0.0);
        candidate.cscore + lane_bonus
    }
}

pub(super) fn resolve_scorer(name: Option<&str>) -> Box<dyn CandidateScorer + Send + Sync> {
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

pub(super) fn select_candidates<O: WorkingSetObserver>(
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
    let mut slot_limit_cache: FxHashMap<String, Option<usize>> = FxHashMap::default();
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
        if let (true, Some(key)) = (use_slots, slot_key.as_ref()) {
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
        if let Some(key) = slot_key.as_ref() {
            *slot_counts.entry(key.clone()).or_insert(0) += 1;
        }
        let lane_label = cand.lane_label().to_string();
        *lane_counts.entry(lane_label.clone()).or_insert(0) += 1;
        counter!("arw_context_selected_total", "lane" => lane_label).increment(1);
        observer.emit(
            STREAM_EVENT_SELECTED,
            Arc::new(json!({
                "rank": selected.len(),
                "item": cand.value.as_ref().clone(),
                "score": cand.cscore,
                "scorer": scorer.name(),
            })),
        );
        selected.push(cand);
        state_epoch = state_epoch.saturating_add(1);
    }
    (selected, lane_counts, slot_counts)
}

fn lane_bonus(counts: &BTreeMap<String, usize>, lane: &str, spec: &WorkingSetSpec) -> f32 {
    let mut total = spec.lane_priority(lane);
    if counts.get(lane).copied().unwrap_or(0) == 0 {
        total += spec.lane_bonus;
    }
    total
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
        let sim = cosine(ea.as_ref(), eb.as_ref());
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
    let embed = parse_embed(&value);
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
    let candidate =
        Candidate::from_value_with_embed(id.clone(), lane.clone(), value, cscore, embed.clone());
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

pub(super) fn build_world_candidate(belief: &Value, project: Option<&str>) -> Option<Candidate> {
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

fn world_candidate_cap(spec: &WorkingSetSpec) -> usize {
    std::env::var("ARW_CONTEXT_WORLD_MAX")
        .ok()
        .and_then(|s| s.trim().parse::<usize>().ok())
        .filter(|cap| *cap > 0)
        .unwrap_or_else(|| spec.limit.saturating_mul(2).max(64))
}

fn clone_shared_values(values: &[SharedValue]) -> Vec<Value> {
    values.iter().map(|v| v.as_ref().clone()).collect()
}

fn insert_candidate(map: &mut FxHashMap<String, Candidate>, candidate: Candidate) {
    let id = candidate.id.clone();
    if let Some(existing) = map.get_mut(&id) {
        if candidate.cscore > existing.cscore {
            *existing = candidate;
        }
    } else {
        map.insert(id, candidate);
    }
}

impl WorkingSetBuilder {
    fn compute_retrieve_limit(
        &self,
        spec: &WorkingSetSpec,
        lane: &Option<String>,
        lane_count: usize,
    ) -> i64 {
        let mut fetch = self.estimate_lane_goal(spec, lane, lane_count);
        fetch = fetch.saturating_mul(3);
        fetch = fetch.saturating_add(spec.expand_per_seed);
        let mut min_fetch = spec.limit.saturating_add(spec.expand_per_seed).max(10);
        let mut cap = fetch_cap_override();
        if cap.is_none() && self.capability_plan.fetch_cap > 0 {
            cap = Some(self.capability_plan.fetch_cap);
        }
        if let Some(cap) = cap {
            fetch = fetch.min(cap);
            min_fetch = min_fetch.min(cap);
        }
        fetch = fetch.max(min_fetch);
        let legacy = ((spec.limit * 3) + spec.expand_per_seed).max(10);
        fetch = fetch.min(legacy);
        fetch as i64
    }

    fn estimate_lane_goal(
        &self,
        spec: &WorkingSetSpec,
        _lane: &Option<String>,
        lane_count: usize,
    ) -> usize {
        if lane_count <= 1 || _lane.is_none() {
            return spec.limit.max(1);
        }
        spec.limit.div_ceil(lane_count).max(1)
    }
}

fn insert_ranked_candidate(candidates: &mut Vec<Candidate>, candidate: Candidate, cap: usize) {
    if cap == 0 {
        return;
    }
    let pos = candidates
        .iter()
        .position(|existing| candidate.cscore > existing.cscore);
    match pos {
        Some(idx) => candidates.insert(idx, candidate),
        None => candidates.push(candidate),
    }
    if candidates.len() > cap {
        candidates.pop();
    }
}

fn fetch_cap_override() -> Option<usize> {
    std::env::var("ARW_CONTEXT_FETCH_MAX")
        .ok()
        .and_then(|raw| raw.trim().parse::<usize>().ok())
        .filter(|cap| *cap > 0)
}

#[allow(clippy::too_many_arguments)]
fn build_diagnostics(
    spec: &WorkingSetSpec,
    seeds: &[SeedInfo],
    expanded: &[SharedValue],
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
    pub(super) fn from_value(id: String, lane: Option<String>, value: Value, cscore: f32) -> Self {
        Self::from_value_with_embed(id, lane, value, cscore, None)
    }

    pub(super) fn from_value_with_embed(
        id: String,
        lane: Option<String>,
        mut value: Value,
        cscore: f32,
        embed_override: Option<Arc<[f32]>>,
    ) -> Self {
        let key = value
            .get("key")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .filter(|s| !s.is_empty());
        let mut tags = value
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
        if !tags.is_empty() {
            tags.sort_unstable();
            tags.dedup();
        }
        let embed = embed_override.or_else(|| parse_embed(&value));
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
            value: Arc::new(value),
            slot_key,
        }
    }

    pub(super) fn slot_key(&self) -> &str {
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

fn parse_embed(value: &Value) -> Option<Arc<[f32]>> {
    fn from_array(raw: &Value) -> Option<Arc<[f32]>> {
        if let Value::Array(arr) = raw {
            let mut out = Vec::with_capacity(arr.len());
            for v in arr {
                if let Some(f) = v.as_f64() {
                    out.push(f as f32);
                }
            }
            if out.is_empty() {
                None
            } else {
                Some(Arc::from(out.into_boxed_slice()))
            }
        } else {
            None
        }
    }

    let raw = value.get("embed")?;
    if let Some(s) = raw.as_str() {
        if let Ok(parsed) = serde_json::from_str::<Value>(s) {
            return from_array(&parsed);
        }
        return None;
    }
    from_array(raw)
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
    let mut i = 0usize;
    let mut j = 0usize;
    let mut intersection = 0usize;
    let mut union = 0usize;
    while i < a.len() && j < b.len() {
        match a[i].cmp(&b[j]) {
            Ordering::Less => {
                union += 1;
                i += 1;
            }
            Ordering::Greater => {
                union += 1;
                j += 1;
            }
            Ordering::Equal => {
                intersection += 1;
                union += 1;
                i += 1;
                j += 1;
            }
        }
    }
    union += a.len().saturating_sub(i) + b.len().saturating_sub(j);
    if union == 0 {
        0.0
    } else {
        intersection as f32 / union as f32
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
            lane_priorities: spec.lane_priorities.clone(),
            slot_counts: slot_counts.clone(),
            slot_budgets: spec.slot_budgets.clone(),
            min_score: spec.min_score,
            scorer: scorer.to_string(),
        }
    }
}
