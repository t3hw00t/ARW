use chrono::SecondsFormat;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use tokio::fs as afs;
use tokio::sync::Mutex;
use tracing::warn;

use crate::{capsule_guard::CAPSULE_EXPIRING_SOON_WINDOW_MS, feedback::FeedbackState, AppState};
use arw_topics as topics;

const LOGIC_HISTORY_LIMIT: usize = 10;

pub async fn telemetry_snapshot(state: &AppState) -> serde_json::Value {
    let metrics = state.metrics().snapshot();
    let bus = state.bus();
    let bus_stats = bus.stats();
    let context = crate::context_metrics::snapshot(&bus);
    let cache = state.tool_cache().stats();
    let now = chrono::Utc::now();
    let generated = now.to_rfc3339_opts(SecondsFormat::Millis, true);
    let generated_ms = now.timestamp_millis();

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

    let logic_history = state.logic_history().recent(LOGIC_HISTORY_LIMIT).await;

    json!({
        "generated": generated,
        "generated_ms": generated_ms,
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
        "logic_history": logic_history,
    })
}

#[derive(Debug)]
pub struct LogicUnitHistoryStore {
    path: PathBuf,
    max_entries: usize,
    inner: Mutex<Vec<Value>>,
}

impl LogicUnitHistoryStore {
    pub fn new(path: PathBuf, max_entries: usize) -> Self {
        let entries = Self::load_existing(&path, max_entries);
        Self {
            path,
            max_entries: max_entries.max(1),
            inner: Mutex::new(entries),
        }
    }

    fn load_existing(path: &Path, max_entries: usize) -> Vec<Value> {
        let mut items: Vec<Value> = Vec::new();
        match std::fs::read(path) {
            Ok(bytes) => match serde_json::from_slice::<Value>(&bytes) {
                Ok(raw) => {
                    let array = raw
                        .get("items")
                        .and_then(Value::as_array)
                        .cloned()
                        .or_else(|| raw.as_array().cloned())
                        .unwrap_or_default();
                    for entry in array.into_iter().filter_map(Self::sanitize_entry) {
                        items.push(entry);
                        if items.len() >= max_entries {
                            break;
                        }
                    }
                }
                Err(err) => warn!(
                    target: "training",
                    error = %err,
                    path = %path.display(),
                    "failed to parse logic history store; starting empty"
                ),
            },
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
            Err(err) => warn!(
                target: "training",
                error = %err,
                path = %path.display(),
                "failed to load logic history store"
            ),
        }
        items
    }

    fn sanitize_entry(entry: Value) -> Option<Value> {
        let obj = entry.as_object()?;
        let kind = obj.get("kind")?.as_str()?.to_string();
        if !kind.starts_with("logic.unit.") {
            return None;
        }
        let time = obj
            .get("time")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        let payload = obj.get("payload").cloned().unwrap_or_else(|| json!({}));
        let mut clean = serde_json::Map::new();
        clean.insert("kind".into(), Value::String(kind));
        if !time.is_empty() {
            clean.insert("time".into(), Value::String(time));
        }
        clean.insert("payload".into(), payload);
        Some(Value::Object(clean))
    }

    async fn persist(&self, snapshot: &[Value]) -> std::io::Result<()> {
        if let Some(parent) = self.path.parent() {
            afs::create_dir_all(parent).await?;
        }
        let body = serde_json::to_vec_pretty(&json!({ "items": snapshot }))
            .map_err(std::io::Error::other)?;
        afs::write(&self.path, body).await
    }

    async fn append_entry(&self, entry: Value) -> std::io::Result<()> {
        let mut guard = self.inner.lock().await;
        guard.insert(0, entry);
        if guard.len() > self.max_entries {
            guard.truncate(self.max_entries);
        }
        let snapshot = guard.clone();
        drop(guard);
        if let Err(err) = self.persist(&snapshot).await {
            warn!(
                target: "training",
                error = %err,
                path = %self.path.display(),
                "failed to persist logic history"
            );
            return Err(err);
        }
        Ok(())
    }

    pub async fn append_envelope(&self, env: &arw_events::Envelope) -> std::io::Result<()> {
        if !env.kind.starts_with("logic.unit.") {
            return Ok(());
        }
        let entry = json!({
            "time": env.time.clone(),
            "kind": env.kind.clone(),
            "payload": env.payload.clone(),
        });
        self.append_entry(entry).await
    }

    pub async fn snapshot(&self, offset: usize, limit: usize) -> (Vec<Value>, usize) {
        let guard = self.inner.lock().await;
        let total = guard.len();
        let start = offset.min(total);
        let end = (start + limit).min(total);
        (guard[start..end].to_vec(), total)
    }

    pub async fn recent(&self, limit: usize) -> Vec<Value> {
        self.snapshot(0, limit).await.0
    }
}

#[cfg(test)]
impl LogicUnitHistoryStore {
    pub async fn append_custom(
        &self,
        kind: &str,
        payload: Value,
        time: Option<String>,
    ) -> std::io::Result<()> {
        if !kind.starts_with("logic.unit.") {
            return Ok(());
        }
        let entry = json!({
            "time": time.unwrap_or_else(|| chrono::Utc::now().to_rfc3339()),
            "kind": kind,
            "payload": payload,
        });
        self.append_entry(entry).await
    }
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

fn summarize_capsules(snapshot: Value) -> Value {
    let count = snapshot.get("count").and_then(Value::as_u64).unwrap_or(0);
    let now_ms = chrono::Utc::now().timestamp_millis() as u64;
    let mut expiring_soon: u64 = 0;
    let mut expired: u64 = 0;
    let mut status_counts: std::collections::BTreeMap<String, u64> =
        std::collections::BTreeMap::new();
    let mut sample: Vec<Value> = Vec::new();
    let mut next_expiry_ms: Option<u64> = None;
    let mut next_expiry_id: Option<String> = None;
    let mut next_expiry_label: Option<String> = None;
    if let Some(items) = snapshot.get("items").and_then(Value::as_array) {
        for item in items.iter() {
            if sample.len() < 5 {
                sample.push(sanitize_capsule(item));
            }
            if let Some(status) = item.get("status").and_then(Value::as_str) {
                *status_counts.entry(status.to_string()).or_insert(0) += 1;
            }
            if let Some(expiry) = item.get("lease_until_ms").and_then(Value::as_u64) {
                if expiry <= now_ms {
                    expired += 1;
                } else if expiry.saturating_sub(now_ms) <= CAPSULE_EXPIRING_SOON_WINDOW_MS {
                    expiring_soon += 1;
                }
                if expiry > now_ms
                    && next_expiry_ms
                        .map(|current| expiry < current)
                        .unwrap_or(true)
                {
                    next_expiry_ms = Some(expiry);
                    next_expiry_id = item
                        .get("id")
                        .and_then(Value::as_str)
                        .map(|s| s.to_string());
                    next_expiry_label = item
                        .get("status_label")
                        .and_then(Value::as_str)
                        .map(|s| s.to_string());
                }
            }
        }
    }

    let renew_due_count = status_counts.get("renew_due").copied().unwrap_or(0);
    let expiring_status_count = status_counts
        .get("expiring")
        .copied()
        .unwrap_or(expiring_soon);

    let status_counts_value = if status_counts.is_empty() {
        Value::Object(serde_json::Map::new())
    } else {
        let mut map = serde_json::Map::with_capacity(status_counts.len());
        for (status, total) in status_counts {
            map.insert(status, Value::Number(total.into()));
        }
        Value::Object(map)
    };

    let next_expiry_in_ms = next_expiry_ms.map(|expiry| expiry.saturating_sub(now_ms));
    let accessible_summary = if count == 0 {
        "No active policy capsules.".to_string()
    } else {
        match (renew_due_count, expiring_status_count, expired) {
            (0, 0, 0) => "All policy capsules are healthy.".to_string(),
            _ => format!(
                "{} capsule{} healthy; {} awaiting renewal; {} expiring; {} expired.",
                count.saturating_sub(renew_due_count + expiring_status_count + expired),
                if count == 1 { "" } else { "s" },
                renew_due_count,
                expiring_status_count,
                expired
            ),
        }
    };

    let mut summary = serde_json::Map::new();
    summary.insert("count".into(), Value::Number(count.into()));
    summary.insert("expiring_soon".into(), Value::Number(expiring_soon.into()));
    summary.insert("expired".into(), Value::Number(expired.into()));
    summary.insert("sample".into(), Value::Array(sample));
    summary.insert("status_counts".into(), status_counts_value);
    summary.insert(
        "accessible_summary".into(),
        Value::String(accessible_summary),
    );
    if let Some(expiry) = next_expiry_ms {
        summary.insert("next_expiry_ms".into(), Value::Number(expiry.into()));
    }
    if let Some(expiry_in) = next_expiry_in_ms {
        summary.insert("next_expiry_in_ms".into(), Value::Number(expiry_in.into()));
    }
    if let Some(id) = next_expiry_id {
        summary.insert("next_expiry_id".into(), Value::String(id));
    }
    if let Some(label) = next_expiry_label {
        summary.insert("next_expiry_label".into(), Value::String(label));
    }

    Value::Object(summary)
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
    if let Some(status) = raw.get("status") {
        obj.insert("status".into(), status.clone());
    }
    if let Some(status_slug) = raw.get("status") {
        obj.insert("status_slug".into(), status_slug.clone());
    }
    if let Some(status_label) = raw.get("status_label") {
        obj.insert("status_label".into(), status_label.clone());
    }
    if let Some(aria_hint) = raw.get("aria_hint") {
        obj.insert("aria_hint".into(), aria_hint.clone());
    }
    if let Some(expires_in) = raw.get("expires_in_ms") {
        obj.insert("expires_in_ms".into(), expires_in.clone());
    }
    if let Some(expired_since) = raw.get("expired_since_ms") {
        obj.insert("expired_since_ms".into(), expired_since.clone());
    }
    if let Some(renew_in) = raw.get("renew_in_ms") {
        obj.insert("renew_in_ms".into(), renew_in.clone());
    }
    if let Some(renew_started) = raw.get("renew_window_started") {
        obj.insert("renew_window_started".into(), renew_started.clone());
    }
    if let Some(renew_start_ms) = raw.get("renew_window_start_ms") {
        obj.insert("renew_window_start_ms".into(), renew_start_ms.clone());
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

pub(crate) fn start_logic_history_recorder(state: AppState) -> crate::tasks::TaskHandle {
    let bus = state.bus();
    let store = state.logic_history();
    crate::tasks::spawn_supervised("training.logic_history", move || {
        let mut rx = bus.subscribe_filtered(
            vec![
                topics::TOPIC_LOGICUNIT_APPLIED.to_string(),
                topics::TOPIC_LOGICUNIT_REVERTED.to_string(),
                topics::TOPIC_LOGICUNIT_INSTALLED.to_string(),
                topics::TOPIC_LOGICUNIT_SUGGESTED.to_string(),
            ],
            Some(128),
        );
        let store = store.clone();
        async move {
            while let Ok(env) = rx.recv().await {
                if let Err(err) = store.append_envelope(&env).await {
                    warn!(
                        target: "training",
                        error = %err,
                        "failed to record logic unit history"
                    );
                }
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::feedback::{FeedbackSignal, Suggestion};
    use arw_events::Envelope;
    use tempfile::tempdir;

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
                {
                    "id": "soon",
                    "lease_until_ms": now_ms + 30_000,
                    "status": "expiring",
                    "status_label": "Expiring soon",
                    "aria_hint": "Capsule expires soon",
                },
                {
                    "id": "later",
                    "lease_until_ms": now_ms + 4 * 60 * 1000,
                    "status": "active",
                    "status_label": "Active",
                    "aria_hint": "Healthy",
                },
                {
                    "id": "expired",
                    "lease_until_ms": now_ms.saturating_sub(2_000),
                    "status": "expired",
                    "status_label": "Expired",
                    "aria_hint": "Expired capsule",
                    "expired_since_ms": 2_000,
                },
                {
                    "id": "renew",
                    "lease_until_ms": Value::Null,
                    "status": "renew_due",
                    "status_label": "Renew now",
                },
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
        assert!(keys.contains(&"status"));
        let status_counts = summary["status_counts"].as_object().unwrap();
        assert_eq!(
            status_counts.get("expiring").and_then(Value::as_u64),
            Some(1)
        );
        assert_eq!(
            status_counts.get("expired").and_then(Value::as_u64),
            Some(1)
        );
        assert!(summary["accessible_summary"].as_str().is_some());
        if let Some(next_expiry_in) = summary["next_expiry_in_ms"].as_u64() {
            assert!(next_expiry_in > 0);
        }
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

    #[tokio::test]
    async fn logic_history_store_persists_entries() {
        let dir = tempdir().expect("tempdir");
        let path = dir.path().join("logic_history.json");
        let store = LogicUnitHistoryStore::new(path.clone(), 3);
        let env = Envelope {
            time: "2024-11-19T10:00:00Z".into(),
            kind: "logic.unit.applied".into(),
            payload: json!({"id": "lu-1", "job_id": "job-1"}),
            policy: None,
            ce: None,
        };
        store.append_envelope(&env).await.expect("append env");
        store
            .append_custom(
                "logic.unit.reverted",
                json!({"snapshot_id": "snap-1"}),
                None,
            )
            .await
            .expect("append custom");

        let (recent, total) = store.snapshot(0, 5).await;
        assert_eq!(total, 2);
        assert_eq!(recent.len(), 2);
        assert_eq!(recent[0]["kind"], json!("logic.unit.reverted"));
        assert_eq!(recent[1]["kind"], json!("logic.unit.applied"));

        let reloaded = LogicUnitHistoryStore::new(path, 5);
        let (again, total_again) = reloaded.snapshot(0, 5).await;
        assert_eq!(total_again, 2);
        assert_eq!(again.len(), 2);
        assert_eq!(again[0]["kind"], json!("logic.unit.reverted"));
    }
}
