use anyhow::{Context, Result};
use arw_events::Bus;
use arw_topics as topics;
use chrono::{SecondsFormat, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use std::path::PathBuf;
use tokio::fs;
use tracing::warn;
use utoipa::ToSchema;
use uuid::Uuid;

use std::collections::BTreeSet;

use crate::responses;
use crate::util;

fn memory_quarantine_path() -> PathBuf {
    util::state_dir().join("memory.quarantine.json")
}

fn world_diffs_review_path() -> PathBuf {
    util::state_dir().join("world.diffs.review.json")
}

fn default_project_id() -> String {
    std::env::var("ARW_PROJECT_ID").unwrap_or_else(|_| "default".into())
}

fn now_iso() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true)
}

fn clean_opt_string(value: Option<String>) -> Option<String> {
    value.and_then(|raw| {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_string())
        }
    })
}

fn normalize_source(source: Option<String>) -> String {
    match clean_opt_string(source).map(|s| s.to_ascii_lowercase()) {
        Some(slug) if matches!(slug.as_str(), "tool" | "ingest" | "world_diff" | "manual") => slug,
        Some(other) => other,
        None => "tool".to_string(),
    }
}

fn normalize_content_type(content_type: Option<String>) -> String {
    clean_opt_string(content_type).unwrap_or_else(|| "text/plain".to_string())
}

fn normalize_preview(preview: Option<String>) -> String {
    let mut value = preview.unwrap_or_default();
    if value.len() > 2048 {
        value.truncate(2048);
    }
    value
}

fn clamp_evidence_score(score: Option<f64>) -> f64 {
    score.unwrap_or(0.0).clamp(-1.0, 1.0)
}

fn dedupe_markers(markers: Option<Vec<String>>) -> Vec<String> {
    let mut seen = BTreeSet::new();
    let mut out = Vec::new();
    if let Some(items) = markers {
        for marker in items {
            if let Some(clean) = clean_opt_string(Some(marker)) {
                if seen.insert(clean.clone()) {
                    out.push(clean);
                }
            }
        }
    }
    out
}

fn normalize_decision(decision: Option<String>) -> String {
    clean_opt_string(decision)
        .map(|d| d.to_ascii_lowercase())
        .unwrap_or_else(|| "admit".to_string())
}

fn decision_to_state(decision: &str) -> String {
    match decision {
        "reject" => "rejected".to_string(),
        "extract_again" => "needs_extractor".to_string(),
        _ => "admitted".to_string(),
    }
}

async fn read_array(path: &PathBuf) -> Vec<Value> {
    match fs::read(path).await {
        Ok(bytes) => match serde_json::from_slice::<Value>(&bytes) {
            Ok(Value::Array(arr)) => arr,
            Ok(_) => {
                warn!("review: expected array at {:?}, resetting", path);
                Vec::new()
            }
            Err(err) => {
                warn!("review: parse error at {:?}: {}", path, err);
                Vec::new()
            }
        },
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Vec::new(),
        Err(err) => {
            warn!("review: unable to read {:?}: {}", path, err);
            Vec::new()
        }
    }
}

async fn write_array(path: &PathBuf, items: &[Value]) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .await
            .with_context(|| format!("create parent dir for {:?}", path))?;
    }
    let data = serde_json::to_vec_pretty(&Value::Array(items.to_vec()))
        .with_context(|| format!("serialize review entries for {:?}", path))?;
    fs::write(path, data)
        .await
        .with_context(|| format!("write review entries to {:?}", path))?;
    Ok(())
}

#[derive(Debug, Clone, Default, Deserialize, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub struct MemoryQuarantineRequest {
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub project_id: Option<String>,
    #[serde(default)]
    pub episode_id: Option<String>,
    #[serde(default)]
    pub corr_id: Option<String>,
    #[serde(default)]
    pub source: Option<String>,
    #[serde(default)]
    pub content_type: Option<String>,
    #[serde(default)]
    pub content_preview: Option<String>,
    #[serde(default)]
    pub provenance: Option<String>,
    #[serde(default)]
    pub risk_markers: Option<Vec<String>>,
    #[serde(default)]
    pub evidence_score: Option<f64>,
    #[serde(default)]
    pub extractor: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub struct MemoryQuarantineEntry {
    pub id: String,
    pub project_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub episode_id: Option<String>,
    pub corr_id: String,
    pub time: String,
    pub source: String,
    pub content_type: String,
    pub content_preview: String,
    pub provenance: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub risk_markers: Vec<String>,
    pub evidence_score: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(default)]
    pub extractor: Option<String>,
    pub state: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(default)]
    pub review: Option<MemoryQuarantineReview>,
}

impl From<MemoryQuarantineEntry> for Value {
    fn from(entry: MemoryQuarantineEntry) -> Self {
        match serde_json::to_value(&entry) {
            Ok(val) => val,
            Err(err) => {
                warn!(
                    "review: failed to serialize memory quarantine entry: {}",
                    err
                );
                json!({
                    "id": entry.id,
                    "project_id": entry.project_id,
                    "time": entry.time,
                    "content_type": entry.content_type,
                    "content_preview": entry.content_preview,
                    "provenance": entry.provenance,
                    "risk_markers": entry.risk_markers,
                    "evidence_score": entry.evidence_score,
                    "extractor": entry.extractor,
                    "state": entry.state,
                })
            }
        }
    }
}

#[derive(Debug, Clone, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub struct MemoryQuarantineAdmit {
    pub id: String,
    #[serde(default)]
    pub decision: Option<String>,
    #[serde(default)]
    pub note: Option<String>,
    #[serde(default)]
    pub reviewed_by: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub struct MemoryQuarantineReview {
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(default)]
    pub by: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(default)]
    pub time: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(default)]
    pub decision: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(default)]
    pub note: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub struct WorldDiffQueueRequest {
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub project_id: Option<String>,
    #[serde(default)]
    pub from_node: Option<String>,
    #[serde(default)]
    pub summary: Option<String>,
    #[serde(default)]
    pub changes: Option<Value>,
}

#[derive(Debug, Clone, Deserialize, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub struct WorldDiffEntry {
    pub id: String,
    pub project_id: String,
    pub from_node: String,
    pub issued_at: String,
    pub summary: String,
    pub changes: Value,
    pub conflicts: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(default)]
    pub note: Option<String>,
    pub state: String,
}

impl From<WorldDiffEntry> for Value {
    fn from(entry: WorldDiffEntry) -> Self {
        json!({
            "id": entry.id,
            "project_id": entry.project_id,
            "from_node": entry.from_node,
            "issued_at": entry.issued_at,
            "summary": entry.summary,
            "changes": entry.changes,
            "conflicts": entry.conflicts,
            "note": entry.note,
            "state": entry.state,
        })
    }
}

#[derive(Debug, Clone, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub struct WorldDiffDecision {
    pub id: String,
    pub decision: String,
    #[serde(default)]
    pub note: Option<String>,
}

pub async fn memory_quarantine_list() -> Value {
    Value::Array(read_array(&memory_quarantine_path()).await)
}

pub async fn world_diffs_list() -> Value {
    Value::Array(read_array(&world_diffs_review_path()).await)
}

pub async fn memory_quarantine_queue(
    bus: &Bus,
    req: MemoryQuarantineRequest,
) -> Result<MemoryQuarantineEntry> {
    let corr_id = clean_opt_string(req.corr_id).unwrap_or_else(|| Uuid::new_v4().to_string());
    let entry = MemoryQuarantineEntry {
        id: req.id.unwrap_or_else(|| Uuid::new_v4().to_string()),
        project_id: req.project_id.unwrap_or_else(default_project_id),
        episode_id: clean_opt_string(req.episode_id),
        corr_id,
        time: now_iso(),
        source: normalize_source(req.source),
        content_type: normalize_content_type(req.content_type),
        content_preview: normalize_preview(req.content_preview),
        provenance: clean_opt_string(req.provenance).unwrap_or_default(),
        risk_markers: dedupe_markers(req.risk_markers),
        evidence_score: clamp_evidence_score(req.evidence_score),
        extractor: clean_opt_string(req.extractor),
        state: "queued".into(),
        review: None,
    };
    let path = memory_quarantine_path();
    let mut items = read_array(&path).await;
    items.push(Value::from(entry.clone()));
    write_array(&path, &items).await?;
    let mut event_value = Value::from(entry.clone());
    responses::attach_corr(&mut event_value);
    bus.publish(topics::TOPIC_MEMORY_QUARANTINED, &event_value);
    Ok(entry)
}

pub async fn memory_quarantine_admit(
    bus: &Bus,
    req: MemoryQuarantineAdmit,
) -> Result<(usize, Option<Value>)> {
    let path = memory_quarantine_path();
    let mut items = read_array(&path).await;
    let mut removed: Option<Value> = None;
    let MemoryQuarantineAdmit {
        id,
        decision,
        note,
        reviewed_by,
    } = req;
    let decision_slug = normalize_decision(decision);
    let review_state = decision_to_state(&decision_slug);
    let review_time = now_iso();
    let review_note = clean_opt_string(note);
    let review_by = clean_opt_string(reviewed_by);
    items.retain(|v| {
        let keep = v
            .get("id")
            .and_then(|x| x.as_str())
            .map(|item_id| item_id != id)
            .unwrap_or(true);
        if !keep {
            let mut obj = v.as_object().cloned().unwrap_or_else(Map::new);
            obj.insert("state".into(), Value::String(review_state.clone()));
            let mut review_obj = obj
                .remove("review")
                .and_then(|val| val.as_object().cloned())
                .unwrap_or_default();
            review_obj.insert("time".into(), Value::String(review_time.clone()));
            review_obj.insert("decision".into(), Value::String(decision_slug.clone()));
            match review_by.clone() {
                Some(user) => {
                    review_obj.insert("by".into(), Value::String(user));
                }
                None => {
                    review_obj.remove("by");
                }
            }
            match review_note.clone() {
                Some(note_val) => {
                    review_obj.insert("note".into(), Value::String(note_val));
                }
                None => {
                    review_obj.remove("note");
                }
            }
            obj.insert("review".into(), Value::Object(review_obj));
            removed = Some(Value::Object(obj));
        }
        keep
    });
    let removed_count = removed.is_some() as usize;
    write_array(&path, &items).await?;
    if let Some(mut ev) = removed.clone() {
        responses::attach_corr(&mut ev);
        bus.publish(topics::TOPIC_MEMORY_ADMITTED, &ev);
    }
    Ok((removed_count, removed))
}

pub async fn world_diffs_queue(bus: &Bus, req: WorldDiffQueueRequest) -> Result<WorldDiffEntry> {
    let entry = WorldDiffEntry {
        id: req.id.unwrap_or_else(|| Uuid::new_v4().to_string()),
        project_id: req.project_id.unwrap_or_else(default_project_id),
        from_node: req.from_node.unwrap_or_default(),
        issued_at: now_iso(),
        summary: req.summary.unwrap_or_default(),
        changes: req.changes.unwrap_or_else(|| Value::Array(Vec::new())),
        conflicts: Value::Array(Vec::new()),
        note: None,
        state: "queued".into(),
    };
    let path = world_diffs_review_path();
    let mut items = read_array(&path).await;
    items.push(Value::from(entry.clone()));
    write_array(&path, &items).await?;
    let mut ev = Value::from(entry.clone());
    responses::attach_corr(&mut ev);
    bus.publish(topics::TOPIC_WORLDDIFF_QUEUED, &ev);
    Ok(entry)
}

pub async fn world_diffs_decision(bus: &Bus, req: WorldDiffDecision) -> Result<Option<Value>> {
    let path = world_diffs_review_path();
    let mut items = read_array(&path).await;
    let mut updated: Option<Value> = None;
    for item in items.iter_mut() {
        let Some(obj) = item.as_object_mut() else {
            continue;
        };
        if obj
            .get("id")
            .and_then(|x| x.as_str())
            .map(|id| id == req.id)
            .unwrap_or(false)
        {
            let state = match req.decision.as_str() {
                "apply" => "applied",
                "reject" => "rejected",
                _ => "queued",
            };
            obj.insert("state".into(), Value::String(state.into()));
            if let Some(note) = req.note.clone() {
                obj.insert("note".into(), Value::String(note));
            }
            updated = Some(Value::Object(obj.clone()));
            break;
        }
    }
    if updated.is_none() {
        return Ok(None);
    }
    write_array(&path, &items).await?;
    if let Some(mut ev) = updated.clone() {
        responses::attach_corr(&mut ev);
        let topic = match ev.get("state").and_then(|v| v.as_str()).unwrap_or("queued") {
            "applied" => topics::TOPIC_WORLDDIFF_APPLIED,
            "rejected" => topics::TOPIC_WORLDDIFF_REJECTED,
            _ => topics::TOPIC_WORLDDIFF_QUEUED,
        };
        bus.publish(topic, &ev);
    }
    Ok(updated)
}

#[cfg(test)]
mod tests {
    use super::*;
    use arw_topics as topics;
    use tempfile::tempdir;
    use tokio::time::{timeout, Duration};

    #[tokio::test]
    async fn queue_and_admit_enrich_entry_and_events() {
        let temp = tempdir().expect("tempdir");
        let _ctx = crate::test_support::begin_state_env(temp.path());
        let bus = Bus::new(64);
        let mut rx_queue = bus.subscribe();

        let preview_input = "<script>alert(1)</script>".repeat(200);
        let request = MemoryQuarantineRequest {
            id: Some("entry-test".to_string()),
            project_id: Some("proj-A".to_string()),
            episode_id: Some("episode-42".to_string()),
            corr_id: Some("  corr-xyz  ".to_string()),
            source: Some("World_Diff".to_string()),
            content_type: Some("text/html".to_string()),
            content_preview: Some(preview_input.clone()),
            provenance: Some(" https://example.test/page ".to_string()),
            risk_markers: Some(vec!["html".to_string(), "html".to_string()]),
            evidence_score: Some(2.5),
            extractor: Some("dom@1".to_string()),
        };

        let entry = memory_quarantine_queue(&bus, request)
            .await
            .expect("queue entry");

        assert_eq!(entry.project_id, "proj-A");
        assert_eq!(entry.episode_id.as_deref(), Some("episode-42"));
        assert_eq!(entry.corr_id, "corr-xyz");
        assert_eq!(entry.source, "world_diff");
        assert_eq!(entry.content_type, "text/html");
        assert!(entry.content_preview.len() <= 2048);
        assert!(entry.content_preview.contains("<script>"));
        assert_eq!(entry.evidence_score, 1.0, "score is clamped to 1.0");
        assert_eq!(entry.risk_markers.len(), 1, "duplicate markers deduped");
        assert_eq!(entry.risk_markers[0], "html");
        assert_eq!(entry.extractor.as_deref(), Some("dom@1"));
        assert!(entry.review.is_none());
        assert_eq!(entry.provenance, "https://example.test/page");

        let envelope = timeout(Duration::from_secs(1), rx_queue.recv())
            .await
            .expect("queue event timeout")
            .expect("queue event");
        assert_eq!(envelope.kind, topics::TOPIC_MEMORY_QUARANTINED);
        assert_eq!(
            envelope.payload.get("corr_id").and_then(Value::as_str),
            Some(entry.corr_id.as_str())
        );
        assert_eq!(
            envelope.payload.get("source").and_then(Value::as_str),
            Some("world_diff")
        );
        assert_eq!(
            envelope.payload.get("content_type").and_then(Value::as_str),
            Some("text/html")
        );
        assert_eq!(
            envelope.payload.get("provenance").and_then(Value::as_str),
            Some("https://example.test/page")
        );
        assert!(envelope.payload.get("review").is_none());

        let list = memory_quarantine_list().await;
        let arr = list.as_array().expect("array from list");
        assert_eq!(arr.len(), 1);

        let mut rx_admit = bus.subscribe();
        let decision = MemoryQuarantineAdmit {
            id: entry.id.clone(),
            decision: Some("reject".to_string()),
            note: Some(" needs manual review ".to_string()),
            reviewed_by: Some(" analyst@example.com ".to_string()),
        };
        let (removed_count, removed_value) = memory_quarantine_admit(&bus, decision)
            .await
            .expect("admit");
        assert_eq!(removed_count, 1);
        let removed = removed_value.expect("removed payload");
        assert_eq!(
            removed.get("state"),
            Some(&Value::String("rejected".into()))
        );
        let review = removed
            .get("review")
            .and_then(Value::as_object)
            .expect("review object");
        assert_eq!(
            review.get("decision"),
            Some(&Value::String("reject".into()))
        );
        assert_eq!(
            review.get("by"),
            Some(&Value::String("analyst@example.com".into()))
        );
        assert_eq!(
            review.get("note"),
            Some(&Value::String("needs manual review".into()))
        );

        let admit_env = timeout(Duration::from_secs(1), rx_admit.recv())
            .await
            .expect("admit event timeout")
            .expect("admit event");
        assert_eq!(admit_env.kind, topics::TOPIC_MEMORY_ADMITTED);
        assert_eq!(
            admit_env.payload.get("state"),
            Some(&Value::String("rejected".into()))
        );

        let remaining = memory_quarantine_list().await;
        assert!(
            remaining
                .as_array()
                .map(|arr| arr.is_empty())
                .unwrap_or(false),
            "quarantine should be empty after admit"
        );
    }
}
