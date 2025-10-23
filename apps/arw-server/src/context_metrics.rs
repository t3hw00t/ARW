use arw_topics as topics;
use serde_json::{json, Number, Value};
use std::cmp::Ordering;
use std::collections::BTreeMap;

const REPLAY_DEPTH: usize = 128;
const RECENT_LIMIT: usize = 5;

#[derive(Clone, Default)]
struct RunningStats {
    sum: f64,
    count: u64,
    max: f64,
    has_value: bool,
}

impl RunningStats {
    fn add(&mut self, value: f64) {
        if !value.is_finite() {
            return;
        }
        self.sum += value;
        self.count += 1;
        if !self.has_value || value > self.max {
            self.max = value;
            self.has_value = true;
        }
    }

    fn average(&self) -> Option<f64> {
        if self.count == 0 {
            None
        } else {
            Some(self.sum / self.count as f64)
        }
    }

    fn max(&self) -> Option<f64> {
        if self.has_value {
            Some(self.max)
        } else {
            None
        }
    }

    fn samples(&self) -> u64 {
        self.count
    }
}

fn stats_entry(stats: &RunningStats) -> Option<Value> {
    if stats.samples() == 0 {
        return None;
    }
    let mut map = serde_json::Map::new();
    if let Some(avg) = stats.average() {
        if let Some(num) = Number::from_f64(avg) {
            map.insert("avg".into(), Value::Number(num));
        }
    }
    if let Some(max) = stats.max() {
        if let Some(num) = Number::from_f64(max) {
            map.insert("max".into(), Value::Number(num));
        }
    }
    map.insert("samples".into(), Value::Number(stats.samples().into()));
    Some(Value::Object(map))
}

fn update_stat(map: &mut BTreeMap<String, RunningStats>, key: &str, value: f64) {
    if !value.is_finite() {
        return;
    }
    map.entry(key.to_string()).or_default().add(value);
}

fn stats_map_to_object(map: &BTreeMap<String, RunningStats>) -> Value {
    let mut obj = serde_json::Map::new();
    for (key, stats) in map.iter() {
        if let Some(entry) = stats_entry(stats) {
            obj.insert(key.clone(), entry);
        }
    }
    Value::Object(obj)
}

fn stats_map_to_ranked_array(map: &BTreeMap<String, RunningStats>, key_name: &str) -> Value {
    let mut items: Vec<Value> = Vec::new();
    for (key, stats) in map.iter() {
        if stats.samples() == 0 {
            continue;
        }
        let mut obj = serde_json::Map::new();
        obj.insert(key_name.into(), Value::String(key.clone()));
        if let Some(avg) = stats.average() {
            if let Some(num) = Number::from_f64(avg) {
                obj.insert("avg".into(), Value::Number(num));
            }
        }
        if let Some(max) = stats.max() {
            if let Some(num) = Number::from_f64(max) {
                obj.insert("max".into(), Value::Number(num));
            }
        }
        obj.insert("samples".into(), Value::Number(stats.samples().into()));
        items.push(Value::Object(obj));
    }
    items.sort_by(|a, b| {
        let avg_a = a.get("avg").and_then(Value::as_f64).unwrap_or(0.0);
        let avg_b = b.get("avg").and_then(Value::as_f64).unwrap_or(0.0);
        avg_b
            .partial_cmp(&avg_a)
            .unwrap_or(Ordering::Equal)
            .then_with(|| {
                let key_a = a.get(key_name).and_then(Value::as_str).unwrap_or("");
                let key_b = b.get(key_name).and_then(Value::as_str).unwrap_or("");
                key_a.cmp(key_b)
            })
    });
    Value::Array(items)
}

fn is_working_set_iteration_summary(kind: &str) -> bool {
    kind == topics::TOPIC_WORKING_SET_ITERATION_SUMMARY || kind == "working._set.iteration.summary"
}

fn is_working_set_completed(kind: &str) -> bool {
    kind == topics::TOPIC_WORKING_SET_COMPLETED || kind == "working._set.completed"
}

pub(crate) fn snapshot(bus: &arw_events::Bus) -> Value {
    let replay = bus.replay(REPLAY_DEPTH);
    let mut coverage_latest: Option<Value> = None;
    let mut coverage_recent: Vec<Value> = Vec::new();
    let mut coverage_needs_more = 0usize;
    let mut coverage_reasons: BTreeMap<String, u64> = BTreeMap::new();
    let mut recall_latest: Option<Value> = None;
    let mut recall_recent: Vec<Value> = Vec::new();
    let mut recall_level_counts: BTreeMap<String, u64> = BTreeMap::new();
    let mut recall_score_total = 0.0f64;
    let mut recall_samples = 0usize;
    let mut recall_at_risk = 0usize;
    let mut recall_slot_stats: BTreeMap<String, SlotStats> = BTreeMap::new();
    let mut coverage_slot_reasons: BTreeMap<String, u64> = BTreeMap::new();

    let mut assembly_latest: Option<Value> = None;
    let mut assembly_recent: Vec<Value> = Vec::new();
    let mut assembly_total = 0usize;
    let mut assembly_needs_more = 0usize;
    let mut assembly_selected_stats = RunningStats::default();
    let mut assembly_candidate_stats = RunningStats::default();
    let mut assembly_duration_stats = RunningStats::default();
    let mut assembly_lane_stats: BTreeMap<String, RunningStats> = BTreeMap::new();
    let mut assembly_slot_stats: BTreeMap<String, RunningStats> = BTreeMap::new();

    let mut retriever_latest: Option<Value> = None;
    let mut retriever_recent: Vec<Value> = Vec::new();
    let mut retriever_samples = 0usize;
    let mut retriever_count_stats: BTreeMap<String, RunningStats> = BTreeMap::new();
    let mut retriever_timing_stats: BTreeMap<String, RunningStats> = BTreeMap::new();
    let mut retriever_lane_stats: BTreeMap<String, RunningStats> = BTreeMap::new();
    let mut retriever_slot_stats: BTreeMap<String, RunningStats> = BTreeMap::new();

    for env in replay.iter().rev() {
        let kind = env.kind.as_str();
        match kind {
            topics::TOPIC_CONTEXT_COVERAGE => {
                let sanitized = sanitize_coverage_event(env);
                if sanitized
                    .get("needs_more")
                    .and_then(Value::as_bool)
                    .unwrap_or(false)
                {
                    coverage_needs_more += 1;
                }
                if let Some(reasons) = sanitized.get("reasons").and_then(Value::as_array) {
                    for reason in reasons.iter().filter_map(Value::as_str) {
                        *coverage_reasons.entry(reason.to_string()).or_default() += 1;
                        if let Some(slot) = reason.strip_prefix("slot_underfilled:") {
                            *coverage_slot_reasons.entry(slot.to_string()).or_default() += 1;
                        }
                    }
                }
                if coverage_latest.is_none() {
                    coverage_latest = Some(sanitized.clone());
                }
                coverage_recent.push(sanitized);
                if coverage_recent.len() >= RECENT_LIMIT {
                    coverage_recent.truncate(RECENT_LIMIT);
                }
            }
            topics::TOPIC_CONTEXT_RECALL_RISK => {
                let sanitized = sanitize_recall_risk_event(env);
                if sanitized
                    .get("at_risk")
                    .and_then(Value::as_bool)
                    .unwrap_or(false)
                {
                    recall_at_risk += 1;
                }
                if let Some(level) = sanitized.get("level").and_then(Value::as_str) {
                    *recall_level_counts.entry(level.to_string()).or_default() += 1;
                }
                if let Some(score) = sanitized.get("score").and_then(Value::as_f64) {
                    if score.is_finite() {
                        recall_score_total += score;
                        recall_samples += 1;
                    }
                }
                if let Some(components) = sanitized
                    .get("components")
                    .and_then(|c| c.get("slots"))
                    .and_then(Value::as_object)
                {
                    for (slot, value) in components {
                        if let Some(gap) = value.as_f64() {
                            if !gap.is_finite() {
                                continue;
                            }
                            let stats = recall_slot_stats.entry(slot.to_string()).or_default();
                            stats.sum += gap;
                            stats.count += 1;
                            if gap > stats.max {
                                stats.max = gap;
                            }
                        }
                    }
                }
                if recall_latest.is_none() {
                    recall_latest = Some(sanitized.clone());
                }
                recall_recent.push(sanitized);
                if recall_recent.len() >= RECENT_LIMIT {
                    recall_recent.truncate(RECENT_LIMIT);
                }
            }
            _ if is_working_set_iteration_summary(kind) => {
                let sanitized = sanitize_iteration_summary_event(env);
                if assembly_latest.is_none() {
                    assembly_latest = Some(sanitized.clone());
                }
                assembly_recent.push(sanitized.clone());
                if assembly_recent.len() > RECENT_LIMIT {
                    assembly_recent.truncate(RECENT_LIMIT);
                }
                assembly_total += 1;
                let needs_more = sanitized
                    .get("needs_more")
                    .or_else(|| sanitized.get("coverage").and_then(|c| c.get("needs_more")))
                    .and_then(Value::as_bool)
                    .unwrap_or(false);
                if needs_more {
                    assembly_needs_more += 1;
                }
                if let Some(summary) = sanitized.get("summary").and_then(Value::as_object) {
                    if let Some(selected) = summary.get("selected").and_then(Value::as_f64) {
                        assembly_selected_stats.add(selected);
                    }
                    if let Some(candidates) =
                        summary.get("total_candidates").and_then(Value::as_f64)
                    {
                        assembly_candidate_stats.add(candidates);
                    }
                    if let Some(lane_counts) = summary.get("lane_counts").and_then(Value::as_object)
                    {
                        for (lane, value) in lane_counts {
                            if let Some(count) = value.as_f64() {
                                update_stat(&mut assembly_lane_stats, lane, count);
                            }
                        }
                    }
                    if let Some(slot_counts) = summary
                        .get("slots")
                        .and_then(Value::as_object)
                        .and_then(|obj| obj.get("counts"))
                        .and_then(Value::as_object)
                    {
                        for (slot, value) in slot_counts {
                            if let Some(count) = value.as_f64() {
                                update_stat(&mut assembly_slot_stats, slot, count);
                            }
                        }
                    }
                }
                if let Some(duration) = sanitized.get("duration_ms").and_then(Value::as_f64) {
                    assembly_duration_stats.add(duration);
                }
            }
            _ if is_working_set_completed(kind) => {
                let sanitized = sanitize_working_set_completed_event(env);
                if retriever_latest.is_none() {
                    retriever_latest = Some(sanitized.clone());
                }
                retriever_recent.push(sanitized.clone());
                if retriever_recent.len() > RECENT_LIMIT {
                    retriever_recent.truncate(RECENT_LIMIT);
                }
                retriever_samples += 1;
                if let Some(counts) = sanitized.get("counts").and_then(Value::as_object) {
                    for (key, value) in counts {
                        if let Some(num) = value.as_f64() {
                            update_stat(&mut retriever_count_stats, key, num);
                        }
                    }
                } else if let Some(summary) = sanitized.get("summary").and_then(Value::as_object) {
                    if let Some(selected) = summary.get("selected").and_then(Value::as_f64) {
                        update_stat(&mut retriever_count_stats, "selected", selected);
                    }
                    if let Some(total) = summary.get("total_candidates").and_then(Value::as_f64) {
                        update_stat(&mut retriever_count_stats, "candidates", total);
                    }
                }
                if let Some(timings) = sanitized.get("timings_ms").and_then(Value::as_object) {
                    for (key, value) in timings {
                        if let Some(num) = value.as_f64() {
                            update_stat(&mut retriever_timing_stats, key, num);
                        }
                    }
                }
                if let Some(lanes) = sanitized.get("lanes").and_then(Value::as_object) {
                    for (lane, value) in lanes {
                        if let Some(num) = value.as_f64() {
                            update_stat(&mut retriever_lane_stats, lane, num);
                        }
                    }
                }
                if let Some(slot_counts) = sanitized
                    .get("slots")
                    .and_then(Value::as_object)
                    .and_then(|obj| obj.get("counts"))
                    .and_then(Value::as_object)
                {
                    for (slot, value) in slot_counts {
                        if let Some(num) = value.as_f64() {
                            update_stat(&mut retriever_slot_stats, slot, num);
                        }
                    }
                }
            }
            _ => {}
        }

        if coverage_recent.len() >= RECENT_LIMIT
            && recall_recent.len() >= RECENT_LIMIT
            && assembly_recent.len() >= RECENT_LIMIT
            && retriever_recent.len() >= RECENT_LIMIT
        {
            break;
        }
    }

    let mut reason_counts: Vec<Value> = coverage_reasons
        .into_iter()
        .map(|(reason, count)| json!({"reason": reason, "count": count}))
        .collect();
    reason_counts.sort_by(|a, b| b["count"].as_u64().cmp(&a["count"].as_u64()));
    if reason_counts.len() > RECENT_LIMIT {
        reason_counts.truncate(RECENT_LIMIT);
    }

    let mut slot_reason_counts: Vec<Value> = coverage_slot_reasons
        .into_iter()
        .map(|(slot, count)| json!({"slot": slot, "count": count}))
        .collect();
    slot_reason_counts.sort_by(|a, b| {
        let count_b = b["count"].as_u64().unwrap_or(0);
        let count_a = a["count"].as_u64().unwrap_or(0);
        count_b.cmp(&count_a).then_with(|| {
            let slot_a = a["slot"].as_str().unwrap_or("");
            let slot_b = b["slot"].as_str().unwrap_or("");
            slot_a.cmp(slot_b)
        })
    });
    if slot_reason_counts.len() > RECENT_LIMIT {
        slot_reason_counts.truncate(RECENT_LIMIT);
    }

    let coverage_section = coverage_latest
        .map(|latest| {
            let total = coverage_recent.len();
            let ratio = if total > 0 {
                Number::from_f64(coverage_needs_more as f64 / total as f64)
            } else {
                None
            };
            let mut obj = serde_json::Map::new();
            obj.insert("latest".into(), latest);
            if !coverage_recent.is_empty() {
                obj.insert("recent".into(), Value::Array(coverage_recent));
            }
            if let Some(number) = ratio {
                obj.insert("needs_more_ratio".into(), Value::Number(number));
            }
            if !reason_counts.is_empty() {
                obj.insert("top_reasons".into(), Value::Array(reason_counts));
            }
            if !slot_reason_counts.is_empty() {
                obj.insert("top_slots".into(), Value::Array(slot_reason_counts.clone()));
            }
            Value::Object(obj)
        })
        .unwrap_or(Value::Null);

    let mut top_slots: Vec<Value> = recall_slot_stats
        .into_iter()
        .filter_map(|(slot, stats)| {
            if stats.count == 0 {
                return None;
            }
            let avg = stats.sum / stats.count as f64;
            Number::from_f64(avg).map(|avg_num| {
                json!({
                    "slot": slot,
                    "avg_gap": avg_num,
                    "max_gap": stats.max,
                    "samples": stats.count,
                })
            })
        })
        .collect();
    top_slots.sort_by(|a, b| {
        let left = a["avg_gap"].as_f64().unwrap_or(0.0);
        let right = b["avg_gap"].as_f64().unwrap_or(0.0);
        let order = right.partial_cmp(&left).unwrap_or(Ordering::Equal);
        if order != Ordering::Equal {
            return order;
        }
        let slot_a = a["slot"].as_str().unwrap_or("");
        let slot_b = b["slot"].as_str().unwrap_or("");
        slot_a.cmp(slot_b)
    });
    if top_slots.len() > RECENT_LIMIT {
        top_slots.truncate(RECENT_LIMIT);
    }

    let recall_section = recall_latest
        .map(|latest| {
            let total = recall_recent.len();
            let avg_score = if recall_samples > 0 {
                Number::from_f64(recall_score_total / recall_samples as f64).map(Value::Number)
            } else {
                None
            };
            let risk_ratio = if recall_samples > 0 {
                Number::from_f64(recall_at_risk as f64 / recall_samples as f64).map(Value::Number)
            } else {
                None
            };
            let mut level_counts: Vec<Value> = recall_level_counts
                .into_iter()
                .map(|(level, count)| json!({"level": level, "count": count}))
                .collect();
            level_counts.sort_by(|a, b| b["count"].as_u64().cmp(&a["count"].as_u64()));
            if level_counts.len() > RECENT_LIMIT {
                level_counts.truncate(RECENT_LIMIT);
            }
            let mut obj = serde_json::Map::new();
            obj.insert("latest".into(), latest);
            if !recall_recent.is_empty() {
                obj.insert("recent".into(), Value::Array(recall_recent));
            }
            if let Some(avg) = avg_score {
                obj.insert("avg_score".into(), avg);
            }
            if let Some(ratio) = risk_ratio {
                obj.insert("at_risk_ratio".into(), ratio);
            }
            if total > 0 {
                obj.insert("sampled".into(), json!(total));
            }
            if !level_counts.is_empty() {
                obj.insert("levels".into(), Value::Array(level_counts));
            }
            if !top_slots.is_empty() {
                obj.insert("top_slots".into(), Value::Array(top_slots.clone()));
            }
            Value::Object(obj)
        })
        .unwrap_or(Value::Null);

    let assembly_section = if assembly_latest.is_none() && assembly_recent.is_empty() {
        Value::Null
    } else {
        let mut obj = serde_json::Map::new();
        if let Some(latest) = assembly_latest {
            obj.insert("latest".into(), latest);
        }
        if !assembly_recent.is_empty() {
            obj.insert("recent".into(), Value::Array(assembly_recent));
        }
        if assembly_total > 0 {
            obj.insert(
                "samples".into(),
                Value::Number((assembly_total as u64).into()),
            );
            if let Some(ratio) =
                Number::from_f64(assembly_needs_more as f64 / assembly_total as f64)
            {
                obj.insert("needs_more_ratio".into(), Value::Number(ratio));
            }
        }
        let mut metrics = serde_json::Map::new();
        if let Some(entry) = stats_entry(&assembly_selected_stats) {
            metrics.insert("selected".into(), entry);
        }
        if let Some(entry) = stats_entry(&assembly_candidate_stats) {
            metrics.insert("candidates".into(), entry);
        }
        if let Some(entry) = stats_entry(&assembly_duration_stats) {
            metrics.insert("duration_ms".into(), entry);
        }
        if !metrics.is_empty() {
            obj.insert("metrics".into(), Value::Object(metrics));
        }
        if !assembly_lane_stats.is_empty() {
            obj.insert(
                "lanes".into(),
                stats_map_to_ranked_array(&assembly_lane_stats, "lane"),
            );
        }
        if !assembly_slot_stats.is_empty() {
            obj.insert(
                "slots".into(),
                stats_map_to_ranked_array(&assembly_slot_stats, "slot"),
            );
        }
        Value::Object(obj)
    };

    let retriever_section = if retriever_latest.is_none() && retriever_recent.is_empty() {
        Value::Null
    } else {
        let mut obj = serde_json::Map::new();
        if let Some(latest) = retriever_latest {
            obj.insert("latest".into(), latest);
        }
        if !retriever_recent.is_empty() {
            obj.insert("recent".into(), Value::Array(retriever_recent));
        }
        if retriever_samples > 0 {
            obj.insert(
                "samples".into(),
                Value::Number((retriever_samples as u64).into()),
            );
        }
        if !retriever_count_stats.is_empty() {
            obj.insert("counts".into(), stats_map_to_object(&retriever_count_stats));
        }
        if !retriever_timing_stats.is_empty() {
            obj.insert(
                "timings_ms".into(),
                stats_map_to_object(&retriever_timing_stats),
            );
        }
        if !retriever_lane_stats.is_empty() {
            obj.insert(
                "lanes".into(),
                stats_map_to_ranked_array(&retriever_lane_stats, "lane"),
            );
        }
        if !retriever_slot_stats.is_empty() {
            obj.insert(
                "slots".into(),
                stats_map_to_ranked_array(&retriever_slot_stats, "slot"),
            );
        }
        Value::Object(obj)
    };

    let assembled_latest = replay
        .iter()
        .rev()
        .find(|env| env.kind.as_str() == topics::TOPIC_CONTEXT_ASSEMBLED)
        .map(|env| sanitize_context_assembled(&env.payload));

    json!({
        "coverage": coverage_section,
        "recall_risk": recall_section,
        "assembled": assembled_latest,
        "assembly": assembly_section,
        "retriever": retriever_section,
    })
}

#[derive(Default)]
struct SlotStats {
    sum: f64,
    count: u64,
    max: f64,
}

fn sanitize_coverage_event(env: &arw_events::Envelope) -> Value {
    let mut obj = serde_json::Map::new();
    obj.insert("time".into(), json!(env.time.clone()));
    for key in ["needs_more", "reasons", "duration_ms"] {
        if let Some(value) = env.payload.get(key) {
            obj.insert(key.into(), value.clone());
        }
    }
    if let Some(bias) = env.payload.get("persona_bias").cloned() {
        obj.insert("persona_bias".into(), bias);
    }
    if let Some(summary) = env.payload.get("summary") {
        obj.insert("summary".into(), summary.clone());
    }
    if let Some(spec) = env.payload.get("spec") {
        obj.insert("spec".into(), sanitize_spec(spec));
    }
    if let Some(project) = env.payload.get("project") {
        obj.insert("project".into(), project.clone());
    }
    if let Some(query) = env.payload.get("query") {
        obj.insert("query".into(), query.clone());
    }
    Value::Object(obj)
}

fn sanitize_recall_risk_event(env: &arw_events::Envelope) -> Value {
    let mut obj = serde_json::Map::new();
    obj.insert("time".into(), json!(env.time.clone()));
    for key in [
        "score",
        "level",
        "at_risk",
        "selected_ratio",
        "desired_lanes",
        "lane_count",
        "needs_more",
        "duration_ms",
    ] {
        if let Some(value) = env.payload.get(key) {
            obj.insert(key.into(), value.clone());
        }
    }
    if let Some(components) = env.payload.get("components") {
        obj.insert("components".into(), components.clone());
    }
    if let Some(reasons) = env.payload.get("reasons") {
        obj.insert("reasons".into(), reasons.clone());
    }
    if let Some(summary) = env.payload.get("summary") {
        obj.insert("summary".into(), summary.clone());
    }
    if let Some(spec) = env.payload.get("spec") {
        obj.insert("spec".into(), sanitize_spec(spec));
    }
    if let Some(project) = env.payload.get("project") {
        obj.insert("project".into(), project.clone());
    }
    if let Some(query) = env.payload.get("query") {
        obj.insert("query".into(), query.clone());
    }
    if let Some(bias) = env.payload.get("persona_bias").cloned() {
        obj.insert("persona_bias".into(), bias);
    }
    Value::Object(obj)
}

pub(crate) fn sanitize_context_assembled(payload: &Value) -> Value {
    let mut out = serde_json::Map::new();
    if let Some(obj) = payload.as_object() {
        for key in [
            "query",
            "project",
            "persona",
            "persona_bias",
            "lanes",
            "limit",
            "expand_per_seed",
            "diversity_lambda",
            "min_score",
            "scorer",
            "expand_query",
            "expand_query_top_k",
            "max_iterations",
            "context_preview",
        ] {
            if let Some(value) = obj.get(key) {
                out.insert(key.into(), value.clone());
            }
        }
        if let Some(ws) = obj.get("working_set").and_then(Value::as_object) {
            let mut ws_obj = serde_json::Map::new();
            if let Some(counts) = ws.get("counts") {
                ws_obj.insert("counts".into(), counts.clone());
            }
            if let Some(summary) = ws.get("summary") {
                ws_obj.insert("summary".into(), summary.clone());
            }
            if let Some(coverage) = ws.get("coverage") {
                ws_obj.insert("coverage".into(), coverage.clone());
            }
            if let Some(final_spec) = ws.get("final_spec") {
                ws_obj.insert("final_spec".into(), sanitize_spec(final_spec));
            }
            if let Some(iterations) = ws.get("iterations") {
                ws_obj.insert("iterations".into(), iterations.clone());
            }
            out.insert("working_set".into(), Value::Object(ws_obj));
        }
    }
    Value::Object(out)
}

fn sanitize_iteration_summary_event(env: &arw_events::Envelope) -> Value {
    let mut obj = serde_json::Map::new();
    obj.insert("time".into(), json!(env.time.clone()));
    let payload = &env.payload;
    if let Some(iteration) = payload
        .get("iteration")
        .cloned()
        .or_else(|| payload.get("index").cloned())
    {
        if !iteration.is_null() {
            obj.insert("iteration".into(), iteration);
        }
    }
    for key in ["corr_id", "project", "query"] {
        if let Some(value) = payload.get(key).cloned() {
            if !value.is_null() {
                obj.insert(key.into(), value);
            }
        }
    }
    if let Some(duration) = payload.get("duration_ms").cloned() {
        if !duration.is_null() {
            obj.insert("duration_ms".into(), duration);
        }
    }
    if let Some(coverage) = payload.get("coverage").cloned() {
        obj.insert("coverage".into(), coverage);
    }
    if let Some(reasons) = payload.get("reasons").cloned() {
        obj.insert("reasons".into(), reasons);
    }
    if let Some(summary) = payload.get("summary").cloned() {
        obj.insert("summary".into(), summary);
    }
    if let Some(bias) = payload.get("persona_bias").cloned() {
        obj.insert("persona_bias".into(), bias);
    }
    if let Some(spec) = payload.get("spec").cloned() {
        obj.insert("spec".into(), spec);
    }
    if let Some(next_spec) = payload.get("next_spec").cloned() {
        obj.insert("next_spec".into(), next_spec);
    }
    if let Some(needs_more) = payload
        .get("needs_more")
        .cloned()
        .or_else(|| {
            payload
                .get("coverage")
                .and_then(|c| c.get("needs_more"))
                .cloned()
        })
        .or_else(|| payload.get("coverage_gap").cloned())
    {
        obj.insert("needs_more".into(), needs_more);
    }
    Value::Object(obj)
}

fn sanitize_working_set_completed_event(env: &arw_events::Envelope) -> Value {
    let mut obj = serde_json::Map::new();
    obj.insert("time".into(), json!(env.time.clone()));
    let payload = &env.payload;
    for key in ["iteration", "corr_id", "project", "query"] {
        if let Some(value) = payload.get(key).cloned() {
            if !value.is_null() {
                obj.insert(key.into(), value);
            }
        }
    }
    if let Some(summary) = payload.get("summary").cloned() {
        obj.insert("summary".into(), summary);
    }
    if let Some(bias) = payload.get("persona_bias").cloned() {
        obj.insert("persona_bias".into(), bias);
    }
    if let Some(diag) = payload.get("diagnostics").and_then(Value::as_object) {
        if let Some(counts) = diag.get("counts").cloned() {
            obj.insert("counts".into(), counts);
        }
        if let Some(lanes) = diag.get("lanes").cloned() {
            obj.insert("lanes".into(), lanes);
        }
        if let Some(slots) = diag.get("slots").cloned() {
            obj.insert("slots".into(), slots);
        }
        if let Some(timings) = diag.get("timings_ms").cloned() {
            obj.insert("timings_ms".into(), timings);
        }
        if let Some(params) = diag.get("params").cloned() {
            obj.insert("params".into(), params);
        }
        if let Some(scorer) = diag.get("scorer").cloned() {
            obj.insert("scorer".into(), scorer);
        }
        if let Some(flag) = diag.get("had_candidates_above_threshold").cloned() {
            obj.insert("had_candidates_above_threshold".into(), flag);
        }
        if let Some(generated) = diag.get("generated_at").cloned() {
            obj.insert("generated_at".into(), generated);
        }
    }
    if !obj.contains_key("counts") {
        let mut counts = serde_json::Map::new();
        if let Some(items) = payload.get("items").and_then(Value::as_array) {
            counts.insert(
                "selected".into(),
                Value::Number((items.len() as u64).into()),
            );
        }
        if let Some(seeds) = payload.get("seeds").and_then(Value::as_array) {
            counts.insert("seeds".into(), Value::Number((seeds.len() as u64).into()));
        }
        if let Some(expanded) = payload.get("expanded").and_then(Value::as_array) {
            counts.insert(
                "expanded".into(),
                Value::Number((expanded.len() as u64).into()),
            );
        }
        if !counts.is_empty() {
            obj.insert("counts".into(), Value::Object(counts));
        }
    }
    Value::Object(obj)
}

fn sanitize_spec(spec: &Value) -> Value {
    let mut out = serde_json::Map::new();
    if let Some(obj) = spec.as_object() {
        for key in [
            "lanes",
            "limit",
            "expand_per_seed",
            "diversity_lambda",
            "min_score",
            "project",
            "persona",
            "lane_bonus",
            "lane_priorities",
            "scorer",
            "expand_query",
            "expand_query_top_k",
            "slot_budgets",
        ] {
            if let Some(value) = obj.get(key) {
                out.insert(key.into(), value.clone());
            }
        }
        if let Some(query_flag) = obj.get("query_provided") {
            out.insert("query_provided".into(), query_flag.clone());
        }
    }
    Value::Object(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use arw_events::{Bus, Envelope};

    #[test]
    fn sanitize_context_assembled_strips_heavy_fields() {
        let payload = json!({
            "query": "foo",
            "lanes": ["semantic"],
            "limit": 10,
            "working_set": {
                "items": [json!({"id": 1})],
                "counts": {"items": 1},
                "summary": {"selected": 1},
                "coverage": {"needs_more": false},
                "final_spec": {
                    "lanes": ["semantic"],
                    "limit": 10,
                    "slot_budgets": {"notes": 1}
                },
                "iterations": [json!({"index": 0})]
            },
            "context_preview": "line one"
        });

        let sanitized = sanitize_context_assembled(&payload);
        assert_eq!(sanitized["query"], json!("foo"));
        assert!(sanitized["working_set"].get("items").is_none());
        assert!(sanitized["working_set"]["summary"].is_object());
        assert!(sanitized["working_set"]["final_spec"].is_object());
        assert!(sanitized["context_preview"].is_string());
    }

    #[test]
    fn sanitize_recall_risk_event_filters_heavy_fields() {
        let env = Envelope {
            time: "2025-09-27T00:00:00Z".into(),
            kind: topics::TOPIC_CONTEXT_RECALL_RISK.into(),
            payload: json!({
                "score": 0.42,
                "level": "medium",
                "at_risk": true,
                "components": {"coverage_shortfall": 0.4, "slots": {"instructions": 0.5}},
                "summary": {"selected": 5, "target_limit": 8},
                "spec": {"lanes": ["semantic"], "limit": 8, "slot_budgets": {"instructions": 2}},
                "extra_noise": "drop me",
                "project": "project-123",
                "query": "why"
            }),
            policy: None,
            ce: None,
        };

        let sanitized = sanitize_recall_risk_event(&env);
        assert_eq!(sanitized["level"], json!("medium"));
        assert!(sanitized.get("extra_noise").is_none());
        assert!(sanitized["components"].is_object());
        assert!(sanitized["spec"].is_object());
        assert!(sanitized["spec"].get("slot_budgets").is_some());
        assert_eq!(sanitized["project"], json!("project-123"));
    }

    #[test]
    fn sanitize_iteration_summary_event_extracts_core_fields() {
        let env = Envelope {
            time: "2025-10-10T07:00:00Z".into(),
            kind: topics::TOPIC_WORKING_SET_ITERATION_SUMMARY.into(),
            payload: json!({
                "iteration": 2,
                "summary": {"selected": 4, "total_candidates": 9},
                "coverage": {"needs_more": false, "reasons": []},
                "spec": {"lanes": ["semantic"], "limit": 6},
                "duration_ms": 142.0,
                "project": "alpha",
                "query": "demo"
            }),
            policy: None,
            ce: None,
        };

        let sanitized = sanitize_iteration_summary_event(&env);
        assert_eq!(sanitized["iteration"], json!(2));
        assert_eq!(sanitized["project"], json!("alpha"));
        assert!(sanitized["summary"].is_object());
        assert!(sanitized["coverage"].is_object());
        assert!(sanitized.get("items").is_none());
    }

    #[test]
    fn sanitize_working_set_completed_event_drops_bulk_payloads() {
        let env = Envelope {
            time: "2025-10-10T07:05:00Z".into(),
            kind: topics::TOPIC_WORKING_SET_COMPLETED.into(),
            payload: json!({
                "iteration": 1,
                "items": [1, 2, 3],
                "seeds": [1, 2, 3, 4],
                "expanded": [1, 2],
                "summary": {"selected": 3, "total_candidates": 5},
                "diagnostics": {
                    "counts": {"seeds": 4, "expanded": 2, "selected": 3},
                    "timings_ms": {"total": 210.0}
                }
            }),
            policy: None,
            ce: None,
        };

        let sanitized = sanitize_working_set_completed_event(&env);
        assert_eq!(sanitized["iteration"], json!(1));
        assert!(sanitized["counts"].is_object());
        assert!(sanitized["timings_ms"].is_object());
        assert!(sanitized.get("items").is_none());
        assert!(sanitized.get("seeds").is_none());
        assert!(sanitized.get("expanded").is_none());
    }

    #[test]
    fn snapshot_collects_recent_events() {
        let bus = Bus::new_with_replay(8, 16);
        bus.publish(
            topics::TOPIC_CONTEXT_COVERAGE,
            &json!({
                "needs_more": true,
                "reasons": ["below_target_limit", "slot_underfilled:instructions"],
                "summary": {"selected": 2},
                "spec": {"lanes": ["semantic"], "limit": 8}
            }),
        );
        bus.publish(
            topics::TOPIC_CONTEXT_RECALL_RISK,
            &json!({
                "score": 0.65,
                "level": "medium",
                "at_risk": true,
                "components": {
                    "coverage_shortfall": 0.35,
                    "slots": {"instructions": 0.9, "analysis": 0.4}
                },
                "summary": {"selected": 2},
                "spec": {"lanes": ["semantic"], "limit": 8},
                "reasons": ["below_target_limit"],
                "project": "alpha"
            }),
        );
        bus.publish(
            topics::TOPIC_CONTEXT_ASSEMBLED,
            &json!({
                "query": "foo",
                "lanes": ["semantic"],
                "limit": 8,
                "working_set": {
                    "counts": {"items": 3},
                    "summary": {"selected": 3},
                    "coverage": {"needs_more": false},
                    "final_spec": {"lanes": ["semantic"], "limit": 8}
                }
            }),
        );
        bus.publish(
            topics::TOPIC_WORKING_SET_ITERATION_SUMMARY,
            &json!({
                "iteration": 0,
                "summary": {
                    "selected": 3,
                    "total_candidates": 6,
                    "lane_counts": {"semantic": 2, "episodic": 1},
                    "slots": {
                        "counts": {"instructions": 1, "analysis": 1},
                        "budgets": {"instructions": 2}
                    }
                },
                "coverage": {"needs_more": true, "reasons": ["slot_underfilled:instructions"]},
                "duration_ms": 180.0,
                "project": "alpha",
                "query": "why"
            }),
        );
        bus.publish(
            topics::TOPIC_WORKING_SET_COMPLETED,
            &json!({
                "iteration": 0,
                "summary": {
                    "selected": 3,
                    "total_candidates": 6,
                    "lane_counts": {"semantic": 2, "episodic": 1},
                    "slot_counts": {"instructions": 1},
                    "slot_budgets": {"instructions": 2},
                    "min_score": 0.4,
                    "target_limit": 6,
                    "avg_cscore": 0.55,
                    "max_cscore": 0.72,
                    "min_cscore": 0.42,
                    "threshold_hits": 2,
                    "scorer": "mmr"
                },
                "diagnostics": {
                    "counts": {"seeds": 4, "expanded": 2, "selected": 3, "candidates": 6},
                    "lanes": {"semantic": 3, "episodic": 1},
                    "slots": {
                        "counts": {"instructions": 1, "analysis": 1},
                        "budgets": {"instructions": 2}
                    },
                    "timings_ms": {"retrieve": 120.0, "select": 80.0, "total": 220.0},
                    "scorer": "mmr",
                    "generated_at": "2025-10-10T12:00:00Z",
                    "had_candidates_above_threshold": true
                }
            }),
        );

        let context = snapshot(&bus);
        assert!(context["coverage"].is_object());
        assert_eq!(
            context["coverage"]["latest"]["reasons"][0],
            json!("below_target_limit")
        );
        assert!(context["recall_risk"].is_object());
        assert_eq!(context["recall_risk"]["latest"]["level"], json!("medium"));
        assert_eq!(
            context["recall_risk"]["levels"][0]["level"],
            json!("medium")
        );
        assert_eq!(
            context["recall_risk"]["top_slots"][0]["slot"],
            json!("instructions")
        );
        assert!(context["coverage"]["top_slots"]
            .as_array()
            .unwrap()
            .iter()
            .any(|slot| slot["slot"] == json!("instructions")));
        assert!(context["assembled"].is_object());
        assert_eq!(context["assembled"]["lanes"], json!(["semantic"]));
        let assembly_value = &context["assembly"];
        let assembly = assembly_value.as_object().unwrap_or_else(|| {
            panic!("expected assembly object, got {}", assembly_value);
        });
        assert_eq!(assembly.get("samples"), Some(&json!(1)));
        assert_eq!(assembly["latest"]["summary"]["selected"], json!(3));
        let retriever_value = &context["retriever"];
        let retriever = retriever_value.as_object().unwrap_or_else(|| {
            panic!("expected retriever object, got {}", retriever_value);
        });
        assert_eq!(retriever.get("samples"), Some(&json!(1)));
        assert_eq!(retriever["latest"]["counts"]["seeds"], json!(4));
        assert!(retriever["counts"]["seeds"]["avg"].as_f64().unwrap_or(0.0) > 0.0);
    }
}
