use crate::{memory_service, story_threads, AppState};
use anyhow::{Context, Result};
use serde::Serialize;
use serde_json::{json, Map, Value};
use std::collections::HashSet;

const STORY_THREAD_LANE: &str = story_threads::STORY_THREAD_LANE;
const DEFAULT_STORY_THREAD_LIMIT: usize = 4;

#[allow(dead_code)]
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum JobCategory {
    AgentTraining,
    ExperimentRun,
    Runtime,
    Modular,
    System,
    Custom(String),
}

impl JobCategory {
    fn as_str(&self) -> &str {
        match self {
            JobCategory::AgentTraining => "agent_training",
            JobCategory::ExperimentRun => "experiment_run",
            JobCategory::Runtime => "runtime",
            JobCategory::Modular => "modular",
            JobCategory::System => "system",
            JobCategory::Custom(value) => value.as_str(),
        }
    }
}

#[derive(Clone, Debug)]
pub struct JobSpec {
    pub category: JobCategory,
    pub kind: String,
    pub label: String,
    pub project: Option<String>,
    pub priority: i32,
    pub tags: Vec<String>,
    pub topics: Vec<String>,
    pub payload: Value,
    pub hints: Option<Value>,
}

impl JobSpec {
    pub fn new(category: JobCategory, kind: impl Into<String>, label: impl Into<String>) -> Self {
        Self {
            category,
            kind: kind.into(),
            label: label.into(),
            project: None,
            priority: 0,
            tags: Vec::new(),
            topics: Vec::new(),
            payload: Value::Null,
            hints: None,
        }
    }
}

pub async fn create_job(state: &AppState, spec: &JobSpec) -> Result<String> {
    let mut data = Map::new();
    data.insert("category".into(), json!(spec.category.as_str()));
    data.insert("kind".into(), json!(spec.kind));
    data.insert("label".into(), json!(spec.label));
    data.insert("priority".into(), json!(spec.priority));
    if let Some(project) = spec.project.as_ref() {
        data.insert("project".into(), json!(project));
    }
    if !spec.tags.is_empty() {
        data.insert(
            "tags".into(),
            Value::Array(spec.tags.iter().map(|tag| json!(tag)).collect()),
        );
    }
    if let Some(hints) = spec.hints.as_ref() {
        data.insert("hints".into(), hints.clone());
    }
    data.insert("payload".into(), spec.payload.clone());
    if !spec.topics.is_empty() {
        data.insert(
            "topics".into(),
            Value::Array(
                spec.topics
                    .iter()
                    .map(|topic| json!({ "name": topic, "slug": memory_service::slugify_topic(topic) }))
                    .collect(),
            ),
        );
    }
    let story_threads =
        collect_story_threads(state, &spec.topics, DEFAULT_STORY_THREAD_LIMIT).await?;
    if !story_threads.is_empty() {
        data.insert("story_threads".into(), Value::Array(story_threads.clone()));
    }
    let id = state
        .kernel()
        .insert_orchestrator_job_async(&spec.label, Some(&Value::Object(data.clone())))
        .await
        .context("insert orchestrator job")?;
    let mut created_event = json!({
        "id": &id,
        "category": spec.category.as_str(),
        "kind": spec.kind,
        "label": spec.label,
        "priority": spec.priority,
    });
    if let serde_json::Value::Object(ref mut map) = created_event {
        if let Some(project) = spec.project.as_ref() {
            map.insert("project".into(), json!(project));
        }
        if !spec.tags.is_empty() {
            map.insert(
                "tags".into(),
                Value::Array(spec.tags.iter().map(|tag| json!(tag)).collect()),
            );
        }
        if let Some(hints) = spec.hints.as_ref() {
            map.insert("hints".into(), hints.clone());
        }
        if !story_threads.is_empty() {
            map.insert("story_threads".into(), Value::Array(story_threads.clone()));
        }
    }
    state
        .bus()
        .publish(arw_topics::TOPIC_ORCHESTRATOR_JOB_CREATED, &created_event);
    Ok(id)
}

pub async fn update_job(
    state: &AppState,
    job_id: &str,
    status: Option<&str>,
    progress: Option<f64>,
    data_patch: Option<Value>,
) -> Result<()> {
    let updated = state
        .kernel()
        .update_orchestrator_job_async(
            job_id.to_string(),
            status.map(|s| s.to_string()),
            progress,
            data_patch,
        )
        .await
        .context("update orchestrator job")?;
    if updated {
        let mut payload = json!({
            "id": job_id,
        });
        if let Some(status) = status {
            if let serde_json::Value::Object(ref mut map) = payload {
                map.insert("status".into(), json!(status));
            }
        }
        if let Some(progress) = progress {
            if let serde_json::Value::Object(ref mut map) = payload {
                map.insert("progress".into(), json!(progress));
            }
        }
        state
            .bus()
            .publish(arw_topics::TOPIC_ORCHESTRATOR_JOB_PROGRESS, &payload);
    }
    Ok(())
}

pub async fn complete_job_ok(
    state: &AppState,
    job_id: &str,
    result: Option<Value>,
    data_patch: Option<Value>,
) -> Result<()> {
    let mut patch = Map::new();
    if let Some(Value::Object(extra)) = data_patch {
        for (k, v) in extra.into_iter() {
            patch.insert(k, v);
        }
    }
    if let Some(result) = result.clone() {
        patch.insert("result".into(), result);
    }
    let patch_value = if patch.is_empty() {
        None
    } else {
        Some(Value::Object(patch))
    };
    update_job(state, job_id, Some("completed"), Some(1.0), patch_value).await?;
    let mut payload = json!({
        "id": job_id,
        "ok": true,
    });
    if let Some(result) = result {
        if let serde_json::Value::Object(ref mut map) = payload {
            map.insert("result".into(), result);
        }
    }
    state
        .bus()
        .publish(arw_topics::TOPIC_ORCHESTRATOR_JOB_COMPLETED, &payload);
    Ok(())
}

pub async fn complete_job_error(
    state: &AppState,
    job_id: &str,
    error: &str,
    data_patch: Option<Value>,
) -> Result<()> {
    let mut patch = Map::new();
    patch.insert("error".into(), json!(error));
    if let Some(Value::Object(extra)) = data_patch {
        for (k, v) in extra.into_iter() {
            patch.insert(k, v);
        }
    }
    update_job(
        state,
        job_id,
        Some("failed"),
        Some(1.0),
        Some(Value::Object(patch.clone())),
    )
    .await?;
    state.bus().publish(
        arw_topics::TOPIC_ORCHESTRATOR_JOB_COMPLETED,
        &json!({
            "id": job_id,
            "ok": false,
            "error": error,
        }),
    );
    Ok(())
}

async fn collect_story_threads(
    state: &AppState,
    topics: &[String],
    limit: usize,
) -> Result<Vec<Value>> {
    if limit == 0 {
        return Ok(Vec::new());
    }
    let mut topic_slugs: HashSet<String> = HashSet::new();
    for topic in topics {
        if let Some(slug) = memory_service::slugify_topic(topic) {
            topic_slugs.insert(slug);
        }
    }
    let records = state
        .kernel()
        .list_recent_memory_async(Some(STORY_THREAD_LANE.to_string()), 64)
        .await
        .unwrap_or_default();
    let mut selected: Vec<Value> = Vec::new();
    for record in records.iter() {
        if selected.len() >= limit {
            break;
        }
        let matches = match record.get("tags") {
            Some(Value::Array(arr)) => arr.iter().filter_map(Value::as_str).any(|tag| {
                if let Some(rest) = tag.strip_prefix("topic:") {
                    topic_slugs.contains(rest)
                } else {
                    false
                }
            }),
            Some(Value::String(s)) => s
                .split(',')
                .map(|part| part.trim())
                .filter_map(|tag| tag.strip_prefix("topic:"))
                .any(|slug| topic_slugs.contains(slug)),
            _ => false,
        };
        if !matches && !topic_slugs.is_empty() {
            continue;
        }
        let mut entry = Map::new();
        if let Some(id) = record.get("id").and_then(Value::as_str) {
            entry.insert("id".into(), json!(id));
        }
        if let Some(updated) = record.get("updated").and_then(Value::as_str) {
            entry.insert("updated".into(), json!(updated));
        }
        if let Some(ptr) = record.get("ptr") {
            entry.insert("ptr".into(), ptr.clone());
        }
        if let Some(value) = record.get("value") {
            if let Some(topic) = value.get("topic").and_then(Value::as_str) {
                entry.insert("topic".into(), json!(topic));
            }
            if let Some(topic_key) = value.get("topic_key").and_then(Value::as_str) {
                entry.insert("topic_key".into(), json!(topic_key));
            }
            if let Some(summary) = value
                .get("summary")
                .and_then(|v| v.get("latest_text"))
                .and_then(Value::as_str)
            {
                entry.insert("summary".into(), json!(summary));
            } else if let Some(text) = value.get("topic").and_then(Value::as_str) {
                entry.insert("summary".into(), json!(text));
            }
            if let Some(weights) = value.get("weights") {
                entry.insert("weights".into(), weights.clone());
            }
        }
        if entry.contains_key("topic") || entry.contains_key("summary") {
            selected.push(Value::Object(entry));
        }
    }
    if selected.is_empty() && topic_slugs.is_empty() {
        for record in records.iter().take(limit) {
            if let Some(value) = record.get("value") {
                if let Some(topic) = value.get("topic").and_then(Value::as_str) {
                    selected.push(json!({
                        "id": record.get("id"),
                        "topic": topic,
                        "summary": value
                            .get("summary")
                            .and_then(|v| v.get("latest_text"))
                            .or_else(|| value.get("summary"))
                    }));
                }
            }
        }
    }
    Ok(selected)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support;
    use serde_json::json;
    use tempfile::tempdir;

    #[tokio::test]
    async fn create_job_inserts_metadata() {
        let temp = tempdir().expect("temp dir");
        let mut ctx = test_support::begin_state_env(temp.path());
        let state = test_support::build_state(temp.path(), &mut ctx.env).await;
        let spec = JobSpec {
            category: JobCategory::AgentTraining,
            kind: "agent.training".into(),
            label: "Train recall mini-agent".into(),
            project: Some("demo".into()),
            priority: 3,
            tags: vec!["training".into(), "recall".into()],
            topics: vec!["Recall improvements".into()],
            payload: json!({"goal": "Improve recall agent"}),
            hints: Some(json!({"diversity": 0.2})),
        };
        let id = create_job(&state, &spec).await.expect("job id");
        let jobs = state
            .kernel()
            .list_orchestrator_jobs_async(5)
            .await
            .expect("jobs");
        let job = jobs
            .into_iter()
            .find(|row| row.get("id").and_then(Value::as_str) == Some(id.as_str()))
            .expect("job row");
        assert_eq!(
            job.get("goal"),
            Some(&Value::String("Train recall mini-agent".into()))
        );
        if let Some(Value::Object(data)) = job.get("data") {
            assert_eq!(data.get("category"), Some(&json!("agent_training")));
            assert_eq!(data.get("project"), Some(&json!("demo")));
        } else {
            panic!("expected data map");
        }
    }
}
