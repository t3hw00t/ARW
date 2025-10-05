use std::path::PathBuf;
use std::sync::Arc;

use arw_events::Bus;
use arw_topics as topics;
use serde::{Deserialize, Serialize};
use serde_json::json;
use tokio::sync::RwLock;
use utoipa::ToSchema;

use crate::{http_timeout, util};

#[derive(Clone, Serialize, Deserialize, Default, ToSchema)]
pub struct Hints {
    #[serde(default)]
    pub max_concurrency: Option<usize>,
    #[serde(default)]
    pub event_buffer: Option<usize>,
    #[serde(default)]
    pub http_timeout_secs: Option<u64>,
    #[serde(default)]
    pub mode: Option<String>,
    #[serde(default)]
    pub slo_ms: Option<u64>,
    #[serde(default)]
    pub retrieval_k: Option<usize>,
    #[serde(default)]
    pub retrieval_div: Option<f64>,
    #[serde(default)]
    pub mmr_lambda: Option<f64>,
    #[serde(default)]
    pub compression_aggr: Option<f64>,
    #[serde(default)]
    pub vote_k: Option<u8>,
    #[serde(default)]
    pub context_budget_tokens: Option<usize>,
    #[serde(default)]
    pub context_item_budget_tokens: Option<usize>,
    #[serde(default)]
    pub context_format: Option<String>,
    #[serde(default)]
    pub include_provenance: Option<bool>,
    #[serde(default)]
    pub context_item_template: Option<String>,
    #[serde(default)]
    pub context_header: Option<String>,
    #[serde(default)]
    pub context_footer: Option<String>,
    #[serde(default)]
    pub joiner: Option<String>,
}

#[derive(Serialize, Deserialize, Default)]
struct PersistedOrchestration {
    #[serde(default)]
    profile: Option<String>,
    #[serde(default)]
    hints: Option<Hints>,
    #[serde(default)]
    memory_limit: Option<u64>,
}

pub struct GovernorState {
    profile: RwLock<String>,
    hints: RwLock<Hints>,
    memory_limit: RwLock<Option<u64>>,
    path: PathBuf,
}

impl GovernorState {
    pub async fn new() -> Arc<Self> {
        let path = util::state_dir().join("orchestration.json");
        let (profile, hints, memory_limit) = match tokio::fs::read(&path).await {
            Ok(bytes) => serde_json::from_slice::<PersistedOrchestration>(&bytes)
                .map(|p| {
                    (
                        p.profile.unwrap_or_else(|| "balance".into()),
                        p.hints.unwrap_or_default(),
                        p.memory_limit,
                    )
                })
                .unwrap_or_else(|_| ("balance".into(), Hints::default(), None)),
            Err(_) => ("balance".into(), Hints::default(), None),
        };

        if let Some(secs) = hints
            .http_timeout_secs
            .or_else(|| hints.slo_ms.map(|ms| ms.div_ceil(1000).max(1)))
        {
            http_timeout::set_secs(secs);
        }

        Arc::new(Self {
            profile: RwLock::new(profile),
            hints: RwLock::new(hints),
            memory_limit: RwLock::new(memory_limit),
            path,
        })
    }

    pub async fn profile(&self) -> String {
        self.profile.read().await.clone()
    }

    pub async fn set_profile(&self, bus: &Bus, name: String) {
        {
            let mut guard = self.profile.write().await;
            *guard = name.clone();
        }
        bus.publish(topics::TOPIC_GOVERNOR_CHANGED, &json!({"profile": name}));
        self.persist().await;
    }

    pub async fn hints(&self) -> Hints {
        self.hints.read().await.clone()
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn apply_hints(
        &self,
        bus: &Bus,
        max_concurrency: Option<usize>,
        event_buffer: Option<usize>,
        http_timeout_secs: Option<u64>,
        mode: Option<String>,
        slo_ms: Option<u64>,
        retrieval_k: Option<usize>,
        retrieval_div: Option<f64>,
        mmr_lambda: Option<f64>,
        compression_aggr: Option<f64>,
        vote_k: Option<u8>,
        context_budget_tokens: Option<usize>,
        context_item_budget_tokens: Option<usize>,
        context_format: Option<String>,
        include_provenance: Option<bool>,
        context_item_template: Option<String>,
        context_header: Option<String>,
        context_footer: Option<String>,
        joiner: Option<String>,
        source: Option<&str>,
    ) {
        let mut applied_map = serde_json::Map::new();
        {
            let mut guard = self.hints.write().await;
            if let Some(value) = max_concurrency {
                guard.max_concurrency = Some(value);
                applied_map.insert("max_concurrency".into(), serde_json::json!(value));
            }
            if let Some(value) = event_buffer {
                guard.event_buffer = Some(value);
                applied_map.insert("event_buffer".into(), serde_json::json!(value));
            }
            if let Some(value) = http_timeout_secs {
                guard.http_timeout_secs = Some(value);
                applied_map.insert("http_timeout_secs".into(), serde_json::json!(value));
            }
            if let Some(ref value) = mode {
                guard.mode = Some(value.clone());
                applied_map.insert("mode".into(), serde_json::json!(value));
            }
            if let Some(value) = slo_ms {
                guard.slo_ms = Some(value);
                applied_map.insert("slo_ms".into(), serde_json::json!(value));
            }
            if let Some(value) = retrieval_k {
                guard.retrieval_k = Some(value);
                applied_map.insert("retrieval_k".into(), serde_json::json!(value));
            }
            if let Some(value) = retrieval_div {
                guard.retrieval_div = Some(value);
                applied_map.insert("retrieval_div".into(), serde_json::json!(value));
            }
            if let Some(value) = mmr_lambda {
                guard.mmr_lambda = Some(value);
                applied_map.insert("mmr_lambda".into(), serde_json::json!(value));
            }
            if let Some(value) = compression_aggr {
                guard.compression_aggr = Some(value);
                applied_map.insert("compression_aggr".into(), serde_json::json!(value));
            }
            if let Some(value) = vote_k {
                guard.vote_k = Some(value);
                applied_map.insert("vote_k".into(), serde_json::json!(value));
            }
            if let Some(value) = context_budget_tokens {
                guard.context_budget_tokens = Some(value);
                applied_map.insert("context_budget_tokens".into(), serde_json::json!(value));
            }
            if let Some(value) = context_item_budget_tokens {
                guard.context_item_budget_tokens = Some(value);
                applied_map.insert(
                    "context_item_budget_tokens".into(),
                    serde_json::json!(value),
                );
            }
            if let Some(ref value) = context_format {
                guard.context_format = Some(value.clone());
                applied_map.insert("context_format".into(), serde_json::json!(value));
            }
            if let Some(value) = include_provenance {
                guard.include_provenance = Some(value);
                applied_map.insert("include_provenance".into(), serde_json::json!(value));
            }
            if let Some(ref value) = context_item_template {
                guard.context_item_template = Some(value.clone());
                applied_map.insert("context_item_template".into(), serde_json::json!(value));
            }
            if let Some(ref value) = context_header {
                guard.context_header = Some(value.clone());
                applied_map.insert("context_header".into(), serde_json::json!(value));
            }
            if let Some(ref value) = context_footer {
                guard.context_footer = Some(value.clone());
                applied_map.insert("context_footer".into(), serde_json::json!(value));
            }
            if let Some(ref value) = joiner {
                guard.joiner = Some(value.clone());
                applied_map.insert("joiner".into(), serde_json::json!(value));
            }
        }

        if let Some(secs) = http_timeout_secs.or_else(|| slo_ms.map(|ms| ms.div_ceil(1000).max(1)))
        {
            http_timeout::set_secs(secs);
            applied_map.insert("http_timeout_secs".into(), serde_json::json!(secs));
            let mut payload = json!({
                "action": "hint",
                "params": {"http_timeout_secs": secs, "source": "slo|mode"},
                "ok": true
            });
            crate::responses::attach_corr(&mut payload);
            bus.publish(topics::TOPIC_ACTIONS_HINT_APPLIED, &payload);
        }

        if let Some(m) = mode.clone() {
            let mut payload = json!({"action": "mode", "mode": m});
            crate::responses::attach_corr(&mut payload);
            bus.publish(topics::TOPIC_GOVERNOR_CHANGED, &payload);
        }

        if !applied_map.is_empty() {
            let mut payload = json!({
                "action": "governor.hints",
                "params": serde_json::Value::Object(applied_map),
                "ok": true
            });
            if let Some(src) = source {
                if let Some(obj) = payload.as_object_mut() {
                    obj.insert("source".into(), serde_json::json!(src));
                }
            }
            crate::responses::attach_corr(&mut payload);
            bus.publish(topics::TOPIC_ACTIONS_HINT_APPLIED, &payload);
        }

        self.persist().await;
    }

    pub async fn set_memory_limit(&self, value: Option<u64>) {
        let mut guard = self.memory_limit.write().await;
        *guard = value;
        self.persist().await;
    }

    pub async fn memory_limit(&self) -> Option<u64> {
        *self.memory_limit.read().await
    }

    async fn persist(&self) {
        let profile = self.profile().await;
        let hints = self.hints().await;
        let memory_limit = *self.memory_limit.read().await;
        let payload = PersistedOrchestration {
            profile: Some(profile),
            hints: Some(hints),
            memory_limit,
        };
        if let Ok(bytes) = serde_json::to_vec_pretty(&payload) {
            if let Some(parent) = self.path.parent() {
                let _ = tokio::fs::create_dir_all(parent).await;
            }
            let _ = tokio::fs::write(&self.path, bytes).await;
        }
    }
}
