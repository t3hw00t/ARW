use chrono::SecondsFormat;
use serde_json::{json, Value};
use std::path::PathBuf;
use tokio::fs as afs;
use tokio::time::{interval, Duration};
use uuid::Uuid;

use crate::{tasks::TaskHandle, AppState};
use arw_topics as topics;

fn base_dir() -> PathBuf {
    crate::util::state_dir().join("self")
}

fn proposals_dir() -> PathBuf {
    base_dir().join("_proposals")
}

fn sanitize_agent_id(agent: &str) -> Option<String> {
    let trimmed = agent.trim();
    if trimmed.is_empty() || trimmed.len() > 120 {
        return None;
    }
    if trimmed.starts_with('.') {
        return None;
    }
    let valid = trimmed
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, ' ' | '-' | '_' | '.'));
    if !valid {
        return None;
    }
    Some(trimmed.to_string())
}

fn agent_path(agent: &str) -> Option<PathBuf> {
    sanitize_agent_id(agent).map(|id| base_dir().join(format!("{}.json", id)))
}

fn agent_path_from_sanitized(id: &str) -> PathBuf {
    base_dir().join(format!("{}.json", id))
}

fn agent_id() -> String {
    std::env::var("ARW_SELF_AGENT_ID")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| {
            std::env::var("ARW_SELF_SEED_ID")
                .ok()
                .filter(|s| !s.trim().is_empty())
                .unwrap_or_else(|| "dev-assistant".to_string())
        })
}

fn aggregator_interval_secs() -> u64 {
    std::env::var("ARW_SELF_AGG_INTERVAL_SECS")
        .ok()
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(20)
        .max(5)
}

#[derive(Debug)]
pub enum SelfModelError {
    InvalidAgent,
    MissingProposal,
    InvalidProposal,
    Io(std::io::Error),
    Serde(serde_json::Error),
}

impl From<std::io::Error> for SelfModelError {
    fn from(value: std::io::Error) -> Self {
        SelfModelError::Io(value)
    }
}

impl From<serde_json::Error> for SelfModelError {
    fn from(value: serde_json::Error) -> Self {
        SelfModelError::Serde(value)
    }
}

impl std::fmt::Display for SelfModelError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SelfModelError::InvalidAgent => write!(f, "invalid_agent"),
            SelfModelError::MissingProposal => write!(f, "proposal_not_found"),
            SelfModelError::InvalidProposal => write!(f, "invalid_proposal"),
            SelfModelError::Io(e) => write!(f, "io_error: {}", e),
            SelfModelError::Serde(e) => write!(f, "serde_error: {}", e),
        }
    }
}

impl std::error::Error for SelfModelError {}

pub async fn list_agents() -> Vec<String> {
    let dir = base_dir();
    let mut out: Vec<String> = Vec::new();
    if let Ok(mut rd) = afs::read_dir(&dir).await {
        while let Ok(Some(entry)) = rd.next_entry().await {
            if let Some(name) = entry.file_name().to_str() {
                if let Some(stem) = name.strip_suffix(".json") {
                    if sanitize_agent_id(stem).is_some() {
                        out.push(stem.to_string());
                    }
                }
            }
        }
    }
    out.sort();
    out.dedup();
    out
}

pub async fn load(agent: &str) -> Result<Option<Value>, SelfModelError> {
    let Some(path) = agent_path(agent) else {
        return Ok(None);
    };
    match afs::read(&path).await {
        Ok(bytes) => {
            let value = serde_json::from_slice::<Value>(&bytes)?;
            Ok(Some(value))
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(SelfModelError::Io(e)),
    }
}

pub async fn save(agent: &str, model: &Value) -> Result<(), SelfModelError> {
    let Some(safe) = sanitize_agent_id(agent) else {
        return Err(SelfModelError::InvalidAgent);
    };
    let dir = base_dir();
    afs::create_dir_all(&dir).await?;
    let path = agent_path_from_sanitized(&safe);
    let body = serde_json::to_vec_pretty(model)?;
    afs::write(path, body).await?;
    Ok(())
}

async fn load_sanitized(agent: &str) -> Result<Value, SelfModelError> {
    let path = agent_path_from_sanitized(agent);
    match afs::read(&path).await {
        Ok(bytes) => Ok(serde_json::from_slice::<Value>(&bytes)?),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(json!({})),
        Err(e) => Err(SelfModelError::Io(e)),
    }
}

pub async fn update_merge<F>(agent: &str, mutator: F) -> Result<(), SelfModelError>
where
    F: FnOnce(&mut Value),
{
    let Some(safe) = sanitize_agent_id(agent) else {
        return Err(SelfModelError::InvalidAgent);
    };
    let mut current = load_sanitized(&safe).await?;
    if !current.is_object() {
        current = json!({});
    }
    mutator(&mut current);
    save(&safe, &current).await
}

fn merge_json(into: &mut Value, patch: &Value) {
    match (into, patch) {
        (Value::Object(dst), Value::Object(src)) => {
            for (k, v) in src.iter() {
                merge_json(dst.entry(k.clone()).or_insert(Value::Null), v);
            }
        }
        (dst, src) => *dst = src.clone(),
    }
}

pub async fn propose_update(
    agent: &str,
    patch: Value,
    rationale: Option<String>,
) -> Result<Value, SelfModelError> {
    let Some(safe) = sanitize_agent_id(agent) else {
        return Err(SelfModelError::InvalidAgent);
    };
    let current = load_sanitized(&safe).await?;
    let mut proposed = current.clone();
    merge_json(&mut proposed, &patch);
    afs::create_dir_all(base_dir()).await?;
    let prop_dir = proposals_dir();
    afs::create_dir_all(&prop_dir).await?;
    let prop_id = Uuid::new_v4().to_string();
    let now = chrono::Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true);
    let envelope = json!({
        "id": prop_id,
        "time": now,
        "agent": safe,
        "rationale": rationale,
        "patch": patch,
        "current": current,
        "proposed": proposed,
    });
    let path = prop_dir.join(format!(
        "{}.json",
        envelope["id"].as_str().unwrap_or("proposal")
    ));
    afs::write(&path, serde_json::to_vec_pretty(&envelope)?).await?;
    Ok(envelope)
}

pub async fn apply_proposal(id: &str) -> Result<Value, SelfModelError> {
    if id.trim().is_empty() {
        return Err(SelfModelError::InvalidProposal);
    }
    let path = proposals_dir().join(format!("{}.json", id));
    let bytes = match afs::read(&path).await {
        Ok(b) => b,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Err(SelfModelError::MissingProposal)
        }
        Err(e) => return Err(SelfModelError::Io(e)),
    };
    let env = serde_json::from_slice::<Value>(&bytes)?;
    let agent = env
        .get("agent")
        .and_then(Value::as_str)
        .and_then(sanitize_agent_id)
        .ok_or(SelfModelError::InvalidProposal)?;
    let proposed = env
        .get("proposed")
        .cloned()
        .ok_or(SelfModelError::InvalidProposal)?;
    save(&agent, &proposed).await?;
    Ok(json!({"id": id, "agent": agent, "applied": true}))
}

pub fn start_aggregators(state: AppState) -> Vec<TaskHandle> {
    vec![
        spawn_tool_competence(state.clone()),
        spawn_resource_forecaster(state),
    ]
}

fn spawn_tool_competence(state: AppState) -> TaskHandle {
    TaskHandle::new(
        "self_model.tool_competence",
        tokio::spawn(async move {
            let bus = state.bus();
            let mut rx = bus.subscribe();
            while let Some(env) =
                crate::util::next_bus_event(&mut rx, &bus, "self_model.tool_competence").await
            {
                if env.kind.as_str() == topics::TOPIC_TOOL_RAN {
                    if let Some(tool_id) = env.payload.get("id").and_then(|v| v.as_str()) {
                        let now = chrono::Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true);
                        let agent = agent_id();
                        if let Err(err) = update_merge(&agent, |model| {
                            if !model.is_object() {
                                *model = json!({});
                            }
                            let obj = model.as_object_mut().unwrap();
                            let lane = obj.entry("competence_map").or_insert_with(|| json!({}));
                            let entry = lane
                                .as_object_mut()
                                .unwrap()
                                .entry(tool_id.to_string())
                                .or_insert_with(|| json!({"count": 0}));
                            if let Some(map) = entry.as_object_mut() {
                                let cur = map.get("count").and_then(|v| v.as_u64()).unwrap_or(0);
                                map.insert("count".into(), json!(cur.saturating_add(1)));
                                map.insert("last".into(), json!(now.clone()));
                            }
                        })
                        .await
                        {
                            tracing::debug!("self_model competence merge failed: {}", err);
                        }
                    }
                }
            }
        }),
    )
}

fn spawn_resource_forecaster(state: AppState) -> TaskHandle {
    TaskHandle::new(
        "self_model.resource_forecaster",
        tokio::spawn(async move {
            let mut ticker = interval(Duration::from_secs(aggregator_interval_secs()));
            loop {
                ticker.tick().await;
                let snapshot = state.metrics().snapshot();
                let routes = snapshot.routes.by_path;
                let chat = routes.get("/admin/chat/send").cloned();
                let tools = routes.get("/admin/tools/run").cloned();
                if chat.is_none() && tools.is_none() {
                    continue;
                }
                let mut patch = json!({"resource_curve": {"recipes": {}}});
                if let Some(route) = chat {
                    if route.ewma_ms.is_finite() && route.ewma_ms > 0.0 {
                        if let Some(num) = serde_json::Number::from_f64(route.ewma_ms) {
                            patch["resource_curve"]["recipes"]["chat"]["latency_ms_mean"] =
                                Value::Number(num);
                        }
                    }
                }
                if let Some(route) = tools {
                    if route.ewma_ms.is_finite() && route.ewma_ms > 0.0 {
                        if let Some(num) = serde_json::Number::from_f64(route.ewma_ms) {
                            patch["resource_curve"]["recipes"]["tools"]["latency_ms_mean"] =
                                Value::Number(num);
                        }
                    }
                }
                let recipes_empty = patch["resource_curve"]["recipes"]
                    .as_object()
                    .map(|o| o.is_empty())
                    .unwrap_or(true);
                if recipes_empty {
                    continue;
                }
                let agent = agent_id();
                if let Err(err) = update_merge(&agent, |model| {
                    merge_json(model, &patch);
                })
                .await
                {
                    tracing::debug!("self_model resource merge failed: {}", err);
                }
            }
        }),
    )
}
