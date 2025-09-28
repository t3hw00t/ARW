use std::sync::Arc;

use arw_events::Bus;
use arw_policy::{AbacRequest, Decision, PolicyEngine};
use arw_topics as topics;
use serde_json::json;
use tokio::sync::Mutex;

/// Shared policy state with helper methods for evaluation and reloads.
#[derive(Clone)]
pub struct PolicyHandle {
    engine: Arc<Mutex<PolicyEngine>>,
    bus: Bus,
}

impl PolicyHandle {
    pub fn new(engine: PolicyEngine, bus: Bus) -> Arc<Self> {
        Self::from_shared(Arc::new(Mutex::new(engine)), bus)
    }

    pub fn from_shared(engine: Arc<Mutex<PolicyEngine>>, bus: Bus) -> Arc<Self> {
        Arc::new(Self { engine, bus })
    }

    pub async fn snapshot(&self) -> serde_json::Value {
        self.engine.lock().await.snapshot()
    }

    pub async fn evaluate_action(&self, kind: &str) -> Decision {
        self.engine.lock().await.evaluate_action(kind)
    }

    pub async fn evaluate_abac(&self, req: &AbacRequest) -> Decision {
        self.engine.lock().await.evaluate_abac(req)
    }

    pub async fn reload_from_env(&self) -> PolicyEngine {
        let updated = PolicyEngine::load_from_env();
        self.replace(updated.clone(), true).await;
        updated
    }

    pub async fn replace(&self, updated: PolicyEngine, emit_event: bool) {
        {
            let mut guard = self.engine.lock().await;
            *guard = updated.clone();
        }
        if emit_event {
            let snapshot = updated.snapshot();
            self.bus
                .publish(topics::TOPIC_POLICY_RELOADED, &json!(snapshot));
        }
    }
}
