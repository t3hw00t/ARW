use arw_topics as topics;
use serde_json::{json, Number, Value};
use std::cmp::Ordering;
use std::collections::BTreeMap;

const REPLAY_DEPTH: usize = 128;
const RECENT_LIMIT: usize = 5;

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

    for env in replay.iter().rev() {
        match env.kind.as_str() {
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
            _ => {}
        }

        if coverage_recent.len() >= RECENT_LIMIT && recall_recent.len() >= RECENT_LIMIT {
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

    let assembled_latest = replay
        .iter()
        .rev()
        .find(|env| env.kind.as_str() == topics::TOPIC_CONTEXT_ASSEMBLED)
        .map(|env| sanitize_context_assembled(&env.payload));

    json!({
        "coverage": coverage_section,
        "recall_risk": recall_section,
        "assembled": assembled_latest,
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
    Value::Object(obj)
}

pub(crate) fn sanitize_context_assembled(payload: &Value) -> Value {
    let mut out = serde_json::Map::new();
    if let Some(obj) = payload.as_object() {
        for key in [
            "query",
            "project",
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
            "lane_bonus",
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
    }
}
