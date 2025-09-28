use std::collections::HashMap;
use std::sync::Arc;

use arw_events::Bus;
use arw_runtime::{
    RegistrySnapshot, RuntimeDescriptor, RuntimeModality, RuntimeRecord, RuntimeSeverity,
    RuntimeState, RuntimeStatus,
};
use arw_topics::TOPIC_RUNTIME_STATE_CHANGED;
use chrono::{DateTime, Utc};
use serde_json::Value;
use tokio::sync::RwLock;

use crate::read_models;
use crate::tasks::TaskHandle;
use crate::AppState;

const READ_MODEL_KEY: &str = "runtime_registry";

#[derive(Default)]
struct RuntimeStore {
    desired: HashMap<String, RuntimeDescriptor>,
    statuses: HashMap<String, RuntimeStatus>,
    updated_at: DateTime<Utc>,
}

impl RuntimeStore {
    fn new() -> Self {
        Self {
            desired: HashMap::new(),
            statuses: HashMap::new(),
            updated_at: Utc::now(),
        }
    }
}

#[derive(Clone)]
pub(crate) struct RuntimeRegistry {
    state: Arc<RwLock<RuntimeStore>>,
    bus: Bus,
}

impl RuntimeRegistry {
    pub fn new(bus: Bus) -> Self {
        Self {
            state: Arc::new(RwLock::new(RuntimeStore::new())),
            bus,
        }
    }

    #[allow(dead_code)]
    pub async fn register_descriptor(&self, descriptor: RuntimeDescriptor) {
        let mut guard = self.state.write().await;
        let id = descriptor.id.clone();
        guard.desired.insert(id.clone(), descriptor);
        guard.updated_at = Utc::now();
        drop(guard);
        self.publish_snapshot().await;
    }

    #[allow(dead_code)]
    pub async fn remove_descriptor(&self, id: &str) {
        let mut guard = self.state.write().await;
        guard.desired.remove(id);
        guard.statuses.remove(id);
        guard.updated_at = Utc::now();
        drop(guard);
        self.publish_snapshot().await;
    }

    pub async fn apply_status(&self, mut status: RuntimeStatus) {
        if status.summary.is_empty() {
            status.summary = format!("state set to {:?}", status.state);
        }
        status.updated_at = Utc::now();
        let payload = serde_json::json!({
            "id": status.id,
            "state": status.state,
            "severity": status.severity,
            "summary": status.summary,
            "detail": status.detail,
            "updated": status.updated_at.to_rfc3339(),
        });
        let mut guard = self.state.write().await;
        guard.statuses.insert(status.id.clone(), status.clone());
        guard.updated_at = Utc::now();
        drop(guard);
        self.bus.publish(TOPIC_RUNTIME_STATE_CHANGED, &payload);
        self.publish_snapshot().await;
    }

    #[allow(dead_code)]
    pub async fn set_offline(&self, id: &str, reason: impl Into<String>) {
        let mut status = RuntimeStatus::new(id.to_string(), RuntimeState::Offline);
        status.severity = RuntimeSeverity::Warn;
        status.summary = "Runtime marked offline".to_string();
        status.detail.push(reason.into());
        self.apply_status(status).await;
    }

    pub async fn snapshot(&self) -> RegistrySnapshot {
        let guard = self.state.read().await;
        let mut runtimes: Vec<RuntimeRecord> = Vec::new();
        for (id, descriptor) in guard.desired.iter() {
            let status = guard
                .statuses
                .get(id)
                .cloned()
                .unwrap_or_else(|| RuntimeStatus::new(id.clone(), RuntimeState::Unknown));
            runtimes.push(RuntimeRecord {
                descriptor: descriptor.clone(),
                status,
            });
        }
        for (id, status) in guard.statuses.iter() {
            if !guard.desired.contains_key(id) {
                let descriptor = RuntimeDescriptor::new(id.clone(), "external");
                runtimes.push(RuntimeRecord {
                    descriptor,
                    status: status.clone(),
                });
            }
        }
        runtimes.sort_by(|a, b| a.descriptor.id.cmp(&b.descriptor.id));
        RegistrySnapshot {
            updated_at: guard.updated_at,
            runtimes,
        }
    }

    async fn publish_snapshot(&self) {
        let snapshot = self.snapshot().await;
        if let Ok(value) = serde_json::to_value(&snapshot) {
            read_models::publish_read_model_patch(&self.bus, READ_MODEL_KEY, &value);
        }
    }

    async fn ensure_descriptor(&self, id: &str) {
        let mut guard = self.state.write().await;
        guard.desired.entry(id.to_string()).or_insert_with(|| {
            let mut descriptor = RuntimeDescriptor::new(id.to_string(), "health-probe");
            descriptor.modalities.push(RuntimeModality::Text);
            descriptor
        });
        guard.updated_at = Utc::now();
    }

    pub async fn handle_health_event(&self, payload: Value) {
        let Some(target) = payload
            .get("target")
            .and_then(|v| v.as_str())
            .map(str::to_owned)
        else {
            return;
        };
        self.ensure_descriptor(&target).await;
        if let Some(status) = RuntimeStatus::from_health_payload(&target, &payload) {
            self.apply_status(status).await;
        }
    }
}

pub(crate) fn start(state: AppState) -> Vec<TaskHandle> {
    let registry = state.runtime();
    let mut tasks = Vec::new();

    // Seed the read-model on startup so listeners have an immediate view.
    tasks.push(TaskHandle::new(
        "runtime.registry.seed",
        tokio::spawn(async move {
            registry.publish_snapshot().await;
        }),
    ));

    let registry = state.runtime();
    let mut rx = state.bus().subscribe();
    tasks.push(TaskHandle::new(
        "runtime.registry.health_listener",
        tokio::spawn(async move {
            while let Ok(env) = rx.recv().await {
                if env.kind.as_str() == "runtime.health" {
                    registry.handle_health_event(env.payload.clone()).await;
                }
            }
        }),
    ));

    tasks
}
