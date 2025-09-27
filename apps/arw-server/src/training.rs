use chrono::SecondsFormat;
use serde_json::{json, Number, Value};
use std::collections::BTreeMap;

use crate::{feedback::FeedbackState, AppState};

pub async fn telemetry_snapshot(state: &AppState) -> serde_json::Value {
    let metrics = state.metrics().snapshot();
    let bus = state.bus();
    let bus_stats = bus.stats();
    let context = derive_context_section(&bus);
    let cache = state.tool_cache().stats();
    let generated = chrono::Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true);

    let routes: Vec<Value> = metrics
        .routes
        .by_path
        .iter()
        .map(|(path, summary)| {
            json!({
                "path": path,
                "hits": summary.hits,
                "errors": summary.errors,
                "ewma_ms": summary.ewma_ms,
                "p95_ms": summary.p95_ms,
                "max_ms": summary.max_ms,
            })
        })
        .collect();

    let mut kinds: Vec<(String, u64)> = metrics
        .events
        .kinds
        .iter()
        .map(|(k, v)| (k.clone(), *v))
        .collect();
    kinds.sort_by(|a, b| b.1.cmp(&a.1));

    let completed = metrics
        .events
        .kinds
        .get("actions.completed")
        .copied()
        .unwrap_or(0);
    let failed = metrics
        .events
        .kinds
        .get("actions.failed")
        .copied()
        .unwrap_or(0);
    let total_runs = completed + failed;
    let success_rate = if total_runs > 0 {
        Some((completed as f64) / (total_runs as f64))
    } else {
        None
    };

    let tasks = serde_json::to_value(&metrics.tasks).unwrap_or_else(|_| json!({}));
    let compatibility = serde_json::to_value(&metrics.compatibility)
        .unwrap_or_else(|_| json!({"legacy_capsule_headers": 0}));
    let cache_snapshot = crate::metrics::cache_stats_snapshot(&cache);

    let governor = state.governor();
    let capsules = state.capsules();
    let feedback = state.feedback();

    let (profile, hints, memory_limit, capsule_view, feedback_state) = tokio::join!(
        governor.profile(),
        governor.hints(),
        governor.memory_limit(),
        capsules.snapshot(),
        feedback.snapshot()
    );

    let governor_hints = compact_options(serde_json::to_value(hints).unwrap_or(Value::Null));
    let capsules_summary = summarize_capsules(capsule_view);
    let feedback_summary = summarize_feedback(feedback_state);

    json!({
        "generated": generated,
        "events": {
            "start": metrics.events.start,
            "total": metrics.events.total,
            "kinds": kinds,
        },
        "routes": routes,
        "bus": {
            "published": bus_stats.published,
            "delivered": bus_stats.delivered,
            "receivers": bus_stats.receivers,
            "lagged": bus_stats.lagged,
            "no_receivers": bus_stats.no_receivers,
        },
        "tools": {
            "completed": completed,
            "failed": failed,
            "total_runs": total_runs,
            "success_rate": success_rate,
        },
        "tasks": tasks,
        "cache": cache_snapshot,
        "governor": {
            "profile": profile,
            "memory_limit": memory_limit,
            "hints": governor_hints,
        },
        "capsules": capsules_summary,
        "feedback": feedback_summary,
        "compatibility": compatibility,
        "context": context,
    })
}

fn compact_options(value: Value) -> Value {
    match value {
        Value::Object(map) => {
            let mut cleaned = serde_json::Map::new();
            for (key, value) in map.into_iter() {
                let compacted = compact_options(value);
                match &compacted {
                    Value::Null => continue,
                    Value::Object(obj) if obj.is_empty() => continue,
                    Value::Array(items) if items.is_empty() => continue,
                    _ => {
                        cleaned.insert(key, compacted);
                    }
                }
            }
            Value::Object(cleaned)
        }
        Value::Array(items) => {
            let mut cleaned = Vec::new();
            for entry in items.into_iter() {
                let compacted = compact_options(entry);
                match &compacted {
                    Value::Null => continue,
                    Value::Object(obj) if obj.is_empty() => continue,
                    Value::Array(values) if values.is_empty() => continue,
                    _ => cleaned.push(compacted),
                }
            }
            Value::Array(cleaned)
        }
        other => other,
    }
}

const CAPSULE_EXPIRING_SOON_WINDOW_MS: u64 = 60_000;

fn summarize_capsules(snapshot: Value) -> Value {
    let count = snapshot.get("count").and_then(Value::as_u64).unwrap_or(0);
    let now_ms = chrono::Utc::now().timestamp_millis() as u64;
    let mut expiring_soon: u64 = 0;
    let mut expired: u64 = 0;
    let mut sample: Vec<Value> = Vec::new();
    if let Some(items) = snapshot.get("items").and_then(Value::as_array) {
        for item in items.iter() {
            if sample.len() < 5 {
                sample.push(sanitize_capsule(item));
            }
            if let Some(expiry) = item.get("lease_until_ms").and_then(Value::as_u64) {
                if expiry <= now_ms {
                    expired += 1;
                } else if expiry
                    .saturating_sub(now_ms)
                    <= CAPSULE_EXPIRING_SOON_WINDOW_MS
                {
                    expiring_soon += 1;
                }
            }
        }
    }

    json!({
        "count": count,
        "expiring_soon": expiring_soon,
        "expired": expired,
        "sample": sample,
    })
}

fn sanitize_capsule(raw: &Value) -> Value {
    let mut obj = serde_json::Map::new();
    obj.insert("id".into(), raw.get("id").cloned().unwrap_or(Value::Null));
    obj.insert(
        "version".into(),
        raw.get("version").cloned().unwrap_or(Value::Null),
    );
    if let Some(issuer) = raw.get("issuer") {
        obj.insert("issuer".into(), issuer.clone());
    }
    if let Some(applied_ms) = raw.get("applied_ms") {
        obj.insert("applied_ms".into(), applied_ms.clone());
    }
    if let Some(hop_ttl) = raw.get("hop_ttl") {
        obj.insert("hop_ttl".into(), hop_ttl.clone());
    }
    if let Some(remaining_hops) = raw.get("remaining_hops") {
        obj.insert("remaining_hops".into(), remaining_hops.clone());
    }
    if let Some(lease_until) = raw.get("lease_until_ms") {
        obj.insert("lease_until_ms".into(), lease_until.clone());
    }
    if let Some(renew_within) = raw.get("renew_within_ms") {
        obj.insert("renew_within_ms".into(), renew_within.clone());
    }

    Value::Object(obj)
}

fn summarize_feedback(feedback: FeedbackState) -> Value {
    let signals_count = feedback.signals.len();
    let suggestions_count = feedback.suggestions.len();

    let recent_signals: Vec<Value> = feedback
        .signals
        .iter()
        .rev()
        .take(5)
        .map(|sig| {
            json!({
                "id": sig.id,
                "ts": sig.ts,
                "kind": sig.kind,
                "target": sig.target,
                "confidence": sig.confidence,
                "severity": sig.severity,
            })
        })
        .collect();

    let suggestion_sample: Vec<Value> = feedback
        .suggestions
        .iter()
        .take(3)
        .map(|suggestion| {
            json!({
                "id": suggestion.id,
                "action": suggestion.action,
                "confidence": suggestion.confidence,
                "rationale": suggestion.rationale,
                "params": suggestion.params,
            })
        })
        .collect();

    json!({
        "auto_apply": feedback.auto_apply,
        "signals": {
            "count": signals_count,
            "recent": recent_signals,
        },
        "suggestions": {
            "count": suggestions_count,
            "sample": suggestion_sample,
        }
    })
}

fn derive_context_section(bus: &arw_events::Bus) -> Value {
    const REPLAY_DEPTH: usize = 128;
    let replay = bus.replay(REPLAY_DEPTH);
    let mut coverage_latest: Option<Value> = None;
    let mut coverage_recent: Vec<Value> = Vec::new();
    let mut coverage_needs_more = 0usize;
    let mut coverage_reasons: BTreeMap<String, u64> = BTreeMap::new();

    for env in replay.iter().rev() {
        match env.kind.as_str() {
            arw_topics::TOPIC_CONTEXT_COVERAGE => {
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
                    }
                }
                if coverage_latest.is_none() {
                    coverage_latest = Some(sanitized.clone());
                }
                coverage_recent.push(sanitized);
                if coverage_recent.len() == 5 {
                    break;
                }
            }
            _ => {}
        }
    }

    let mut reason_counts: Vec<Value> = coverage_reasons
        .into_iter()
        .map(|(reason, count)| json!({"reason": reason, "count": count}))
        .collect();
    reason_counts.sort_by(|a, b| b["count"].as_u64().cmp(&a["count"].as_u64()));
    if reason_counts.len() > 5 {
        reason_counts.truncate(5);
    }

    let coverage_section = if coverage_latest.is_some() {
        let total = coverage_recent.len();
        let ratio = if total > 0 {
            Number::from_f64(coverage_needs_more as f64 / total as f64)
        } else {
            None
        };
        let mut obj = serde_json::Map::new();
        obj.insert("latest".into(), coverage_latest.unwrap());
        if !coverage_recent.is_empty() {
            obj.insert("recent".into(), Value::Array(coverage_recent));
        }
        if let Some(number) = ratio {
            obj.insert("needs_more_ratio".into(), Value::Number(number));
        }
        if !reason_counts.is_empty() {
            obj.insert("top_reasons".into(), Value::Array(reason_counts));
        }
        Value::Object(obj)
    } else {
        Value::Null
    };

    let assembled_latest = replay
        .iter()
        .rev()
        .find(|env| env.kind.as_str() == arw_topics::TOPIC_CONTEXT_ASSEMBLED)
        .map(|env| sanitize_context_assembled(&env.payload));

    json!({
        "coverage": coverage_section,
        "assembled": assembled_latest,
    })
}

fn sanitize_coverage_event(env: &arw_events::Envelope) -> Value {
    let mut obj = serde_json::Map::new();
    obj.insert("time".into(), json!(env.time.clone()));
    if let Some(needs_more) = env.payload.get("needs_more") {
        obj.insert("needs_more".into(), needs_more.clone());
    }
    if let Some(reasons) = env.payload.get("reasons") {
        obj.insert("reasons".into(), reasons.clone());
    }
    if let Some(duration) = env.payload.get("duration_ms") {
        obj.insert("duration_ms".into(), duration.clone());
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

fn sanitize_context_assembled(payload: &Value) -> Value {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::feedback::{FeedbackSignal, Suggestion};
    use arw_events::{Bus, Envelope};

    #[test]
    fn compact_options_drops_null_entries() {
        let value = json!({
            "keep": "value",
            "drop": null,
            "nested": {"inner": null, "keep": 1},
        });
        let compacted = compact_options(value);
        let obj = compacted.as_object().expect("object");
        assert!(obj.contains_key("keep"));
        assert!(!obj.contains_key("drop"));
        let nested = obj
            .get("nested")
            .and_then(Value::as_object)
            .expect("nested object");
        assert!(!nested.contains_key("inner"));
        assert_eq!(nested.get("keep"), Some(&json!(1)));
    }

    #[test]
    fn summarize_capsules_counts_expiring_and_expired() {
        let now_ms = chrono::Utc::now().timestamp_millis() as u64;
        let snapshot = json!({
            "count": 3,
            "items": [
                {"lease_until_ms": now_ms + 30_000},
                {"lease_until_ms": now_ms + 4 * 60 * 1000},
                {"lease_until_ms": now_ms.saturating_sub(2_000)},
                {"lease_until_ms": Value::Null},
            ]
        });

        let summary = summarize_capsules(snapshot);
        assert_eq!(summary["count"], json!(3));
        assert_eq!(summary["expiring_soon"], json!(1));
        assert_eq!(summary["expired"], json!(1));
        let sample = summary["sample"].as_array().unwrap();
        assert!(sample.len() <= 5);
        let first = sample.first().unwrap();
        let keys: Vec<&str> = first
            .as_object()
            .unwrap()
            .keys()
            .map(|k| k.as_str())
            .collect();
        assert!(keys.contains(&"id"));
        assert!(keys.contains(&"version"));
        assert!(!keys.contains(&"denies"));
    }

    #[test]
    fn summarize_feedback_limits_history() {
        let mut state = FeedbackState::default();
        for idx in 0..7 {
            state.signals.push(FeedbackSignal {
                id: format!("sig-{idx}"),
                ts: format!("2025-09-{:02}", idx + 1),
                kind: "lag".into(),
                target: "tool.cache".into(),
                confidence: 0.5,
                severity: 2,
                note: None,
            });
        }
        for idx in 0..4 {
            state.suggestions.push(Suggestion {
                id: format!("suggest-{idx}"),
                action: "governor.apply".into(),
                params: json!({"mode": "balance"}),
                rationale: String::new(),
                confidence: 0.7,
            });
        }
        state.auto_apply = true;

        let summary = summarize_feedback(state);
        assert_eq!(summary["auto_apply"], json!(true));
        assert_eq!(summary["signals"]["count"], json!(7));
        let recent = summary["signals"]["recent"].as_array().unwrap();
        assert_eq!(recent.len(), 5);
        assert_eq!(recent.first().unwrap()["id"], json!("sig-6"));
        assert_eq!(summary["suggestions"]["count"], json!(4));
        assert_eq!(
            summary["suggestions"]["sample"].as_array().unwrap().len(),
            3
        );
    }

    #[test]
    fn sanitize_coverage_event_retains_core_fields() {
        let env = Envelope {
            time: "2025-09-28T10:00:00Z".into(),
            kind: arw_topics::TOPIC_CONTEXT_COVERAGE.into(),
            payload: json!({
                "needs_more": true,
                "reasons": ["below_target_limit", "weak_average_score"],
                "duration_ms": 32.5,
                "summary": {
                    "target_limit": 12,
                    "selected": 9,
                },
                "spec": {
                    "lanes": ["semantic"],
                    "limit": 12,
                    "slot_budgets": {"evidence": 2}
                },
                "project": "demo",
                "query": "foo"
            }),
            policy: None,
            ce: None,
        };

        let sanitized = sanitize_coverage_event(&env);
        assert_eq!(sanitized["time"], json!("2025-09-28T10:00:00Z"));
        assert_eq!(sanitized["needs_more"], json!(true));
        assert_eq!(sanitized["reasons"].as_array().unwrap().len(), 2);
        assert_eq!(sanitized["summary"]["selected"], json!(9));
        assert_eq!(sanitized["spec"]["limit"], json!(12));
        assert!(sanitized["spec"].get("slot_budgets").is_some());
        assert!(sanitized.get("items").is_none());
    }

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
    fn derive_context_section_collects_recent_events() {
        let bus = Bus::new_with_replay(8, 16);
        bus.publish(
            arw_topics::TOPIC_CONTEXT_COVERAGE,
            &json!({"needs_more": true, "reasons": ["below_target_limit"], "summary": {"selected": 2}, "spec": {"lanes": ["semantic"], "limit": 8}}),
        );
        bus.publish(
            arw_topics::TOPIC_CONTEXT_ASSEMBLED,
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

        let context = derive_context_section(&bus);
        assert!(context["coverage"].is_object());
        assert_eq!(
            context["coverage"]["latest"]["reasons"][0],
            json!("below_target_limit")
        );
        assert!(context["assembled"].is_object());
        assert_eq!(context["assembled"]["lanes"], json!(["semantic"]));
    }
}
