use anyhow::{Context, Result};
use arw_events::Bus;
use arw_topics as topics;
use chrono::{SecondsFormat, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::path::PathBuf;
use tokio::fs;
use tracing::warn;
use utoipa::ToSchema;
use uuid::Uuid;

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

#[derive(Debug, Clone, Deserialize, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub struct MemoryQuarantineRequest {
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub project_id: Option<String>,
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
    pub time: String,
    pub content_type: String,
    pub content_preview: String,
    pub provenance: String,
    pub risk_markers: Vec<String>,
    pub evidence_score: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(default)]
    pub extractor: Option<String>,
    pub state: String,
}

impl From<MemoryQuarantineEntry> for Value {
    fn from(entry: MemoryQuarantineEntry) -> Self {
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

#[derive(Debug, Clone, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub struct MemoryQuarantineAdmit {
    pub id: String,
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
    let mut preview = req.content_preview.unwrap_or_default();
    if preview.len() > 2048 {
        preview.truncate(2048);
    }
    let entry = MemoryQuarantineEntry {
        id: req.id.unwrap_or_else(|| Uuid::new_v4().to_string()),
        project_id: req.project_id.unwrap_or_else(default_project_id),
        time: now_iso(),
        content_type: req.content_type.unwrap_or_else(|| "text/plain".into()),
        content_preview: preview,
        provenance: req.provenance.unwrap_or_default(),
        risk_markers: req.risk_markers.unwrap_or_default(),
        evidence_score: req.evidence_score.unwrap_or(0.0),
        extractor: req.extractor,
        state: "queued".into(),
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
    items.retain(|v| {
        let keep = v
            .get("id")
            .and_then(|x| x.as_str())
            .map(|id| id != req.id)
            .unwrap_or(true);
        if !keep {
            removed = Some(v.clone());
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
