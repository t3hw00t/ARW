use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use arw_events::Bus;
use arw_topics as topics;
use chrono::{SecondsFormat, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use tokio::sync::RwLock;
use tokio::time::{interval, Duration};
use utoipa::ToSchema;

use crate::{responses, tasks::TaskHandle, AppState};

pub const SNAPSHOT_TTL_SECONDS: u64 = 360;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, ToSchema)]
pub struct ClusterNode {
    pub id: String,
    pub role: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub health: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub capabilities: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub models: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_seen: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_seen_ms: Option<u64>,
}

#[derive(Default)]
struct ClusterStore {
    nodes: HashMap<String, ClusterNode>,
}

pub struct ClusterRegistry {
    store: RwLock<ClusterStore>,
    bus: Bus,
}

impl ClusterRegistry {
    pub fn new(bus: Bus) -> Arc<Self> {
        Arc::new(Self {
            store: RwLock::new(ClusterStore::default()),
            bus,
        })
    }

    pub async fn snapshot(&self) -> Vec<ClusterNode> {
        let now_ms = Utc::now().timestamp_millis();
        let ttl_ms: i64 = match SNAPSHOT_TTL_SECONDS.checked_mul(1_000) {
            Some(0) | None => 0,
            Some(ms) => {
                if ms > i64::MAX as u64 {
                    i64::MAX
                } else {
                    ms as i64
                }
            }
        };

        let mut guard = self.store.write().await;
        if ttl_ms > 0 {
            for node in guard.nodes.values_mut() {
                if let Some(last_seen_ms) = node.last_seen_ms {
                    if last_seen_ms > 0 {
                        let last_i64 = last_seen_ms as i64;
                        let diff = now_ms.saturating_sub(last_i64);
                        if diff > ttl_ms {
                            node.health = Some("stale".into());
                        } else if matches!(node.health.as_deref(), Some("stale")) {
                            node.health = Some("ok".into());
                        }
                    }
                }
            }
        }

        let mut nodes: Vec<ClusterNode> = guard.nodes.values().cloned().collect();
        nodes.sort_by(|a, b| a.id.cmp(&b.id));
        nodes
    }

    pub async fn upsert(&self, node: ClusterNode, emit: bool) -> bool {
        let mut changed = true;
        {
            let mut guard = self.store.write().await;
            if guard.nodes.get(&node.id) == Some(&node) {
                changed = false;
            }
            guard.nodes.insert(node.id.clone(), node.clone());
        }
        if changed && emit {
            if let Ok(mut payload) = serde_json::to_value(&node) {
                responses::attach_corr(&mut payload);
                self.bus
                    .publish(topics::TOPIC_CLUSTER_NODE_CHANGED, &payload);
            }
        }
        changed
    }

    pub async fn advertise_local(&self, state: &AppState) {
        let id = node_id();
        let role = format!("{:?}", arw_core::hierarchy::get_state().self_node.role);
        let mut caps = Map::new();
        let now = Utc::now();
        let advertised = now.to_rfc3339_opts(SecondsFormat::Millis, true);
        let advertised_ms = now.timestamp_millis();
        let advertised_ms = if advertised_ms < 0 {
            0
        } else {
            advertised_ms as u64
        };
        caps.insert("os".into(), Value::String(std::env::consts::OS.into()));
        caps.insert("arch".into(), Value::String(std::env::consts::ARCH.into()));
        caps.insert(
            "arw_version".into(),
            Value::String(env!("CARGO_PKG_VERSION").into()),
        );

        let models_summary = summarize_models(state.models().list().await);
        let node = ClusterNode {
            id: id.clone(),
            role: role.clone(),
            name: None,
            health: Some("ok".into()),
            capabilities: Some(Value::Object(caps.clone())),
            models: Some(models_summary.clone()),
            last_seen: Some(advertised.clone()),
            last_seen_ms: Some(advertised_ms),
        };

        let _ = self.upsert(node, true).await;
        let mut payload = json!({
            "id": id,
            "role": role,
            "capabilities": Value::Object(caps),
            "models": models_summary,
            "advertised": advertised,
            "advertised_ms": advertised_ms,
        });
        responses::attach_corr(&mut payload);
        self.bus
            .publish(topics::TOPIC_CLUSTER_NODE_ADVERTISE, &payload);
    }

    async fn apply_remote_advert(&self, payload: &Value) {
        if let Some(node) = payload_to_node(payload) {
            let _ = self.upsert(node, false).await;
        }
    }
}

pub fn start(state: AppState) -> Vec<TaskHandle> {
    let registry = state.cluster();
    let initial_registry = registry.clone();
    let initial_state = state.clone();
    let mut handles = Vec::new();
    handles.push(TaskHandle::new(
        "cluster.advertise_initial",
        tokio::spawn(async move {
            initial_registry.advertise_local(&initial_state).await;
        }),
    ));

    let periodic_registry = registry.clone();
    let periodic_state = state.clone();
    handles.push(TaskHandle::new(
        "cluster.advertise_periodic",
        tokio::spawn(async move {
            let mut tick = interval(Duration::from_secs(300));
            loop {
                tick.tick().await;
                periodic_registry.advertise_local(&periodic_state).await;
            }
        }),
    ));

    let event_registry = registry.clone();
    let event_state = state.clone();
    let bus = state.bus();
    handles.push(TaskHandle::new(
        "cluster.advertise_on_events",
        tokio::spawn(async move {
            let mut rx = bus.subscribe();
            while let Ok(env) = rx.recv().await {
                match env.kind.as_str() {
                    topics::TOPIC_MODELS_CHANGED | topics::TOPIC_MODELS_REFRESHED => {
                        event_registry.advertise_local(&event_state).await;
                    }
                    topics::TOPIC_GOVERNOR_CHANGED => {
                        event_registry.advertise_local(&event_state).await;
                    }
                    topics::TOPIC_CLUSTER_NODE_ADVERTISE | topics::TOPIC_CLUSTER_NODE_CHANGED => {
                        event_registry.apply_remote_advert(&env.payload).await;
                    }
                    _ => {}
                }
            }
        }),
    ));

    handles
}

fn summarize_models(models: Vec<Value>) -> Value {
    let mut hashes: HashSet<String> = HashSet::new();
    for item in models {
        if let Some(sha) = item.get("sha256").and_then(|v| v.as_str()) {
            if sha.len() == 64 {
                hashes.insert(sha.to_string());
            }
        }
    }
    let mut ordered: Vec<String> = hashes.into_iter().collect();
    ordered.sort();
    let preview: Vec<String> = ordered.iter().take(8).cloned().collect();
    json!({
        "count": ordered.len(),
        "preview": preview,
    })
}

fn node_id() -> String {
    std::env::var("ARW_NODE_ID")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| sysinfo::System::host_name().unwrap_or_else(|| "local".to_string()))
}

fn payload_to_node(payload: &Value) -> Option<ClusterNode> {
    let id = payload.get("id")?.as_str()?.to_string();
    let role = payload.get("role")?.as_str()?.to_string();
    let name = payload
        .get("name")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let health = payload
        .get("health")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let capabilities = payload.get("capabilities").cloned();
    let models = payload.get("models").cloned();
    let advertised_str = payload
        .get("advertised")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let advertised_ms = payload
        .get("advertised_ms")
        .and_then(|v| v.as_i64())
        .map(|raw| if raw < 0 { 0 } else { raw as u64 });
    let now = Utc::now();
    let last_seen =
        advertised_str.unwrap_or_else(|| now.to_rfc3339_opts(SecondsFormat::Millis, true));
    let last_seen_ms = advertised_ms.unwrap_or_else(|| {
        let ts = now.timestamp_millis();
        if ts < 0 {
            0
        } else {
            ts as u64
        }
    });
    Some(ClusterNode {
        id,
        role,
        name,
        health,
        capabilities,
        models,
        last_seen: Some(last_seen),
        last_seen_ms: Some(last_seen_ms),
    })
}
