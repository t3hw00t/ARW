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
    ) {
        {
            let mut guard = self.hints.write().await;
            if max_concurrency.is_some() {
                guard.max_concurrency = max_concurrency;
            }
            if event_buffer.is_some() {
                guard.event_buffer = event_buffer;
            }
            if http_timeout_secs.is_some() {
                guard.http_timeout_secs = http_timeout_secs;
            }
            if mode.is_some() {
                guard.mode = mode.clone();
            }
            if slo_ms.is_some() {
                guard.slo_ms = slo_ms;
            }
            if retrieval_k.is_some() {
                guard.retrieval_k = retrieval_k;
            }
            if retrieval_div.is_some() {
                guard.retrieval_div = retrieval_div;
            }
            if mmr_lambda.is_some() {
                guard.mmr_lambda = mmr_lambda;
            }
            if compression_aggr.is_some() {
                guard.compression_aggr = compression_aggr;
            }
            if vote_k.is_some() {
                guard.vote_k = vote_k;
            }
            if context_budget_tokens.is_some() {
                guard.context_budget_tokens = context_budget_tokens;
            }
            if context_item_budget_tokens.is_some() {
                guard.context_item_budget_tokens = context_item_budget_tokens;
            }
            if context_format.is_some() {
                guard.context_format = context_format.clone();
            }
            if include_provenance.is_some() {
                guard.include_provenance = include_provenance;
            }
            if context_item_template.is_some() {
                guard.context_item_template = context_item_template.clone();
            }
            if context_header.is_some() {
                guard.context_header = context_header.clone();
            }
            if context_footer.is_some() {
                guard.context_footer = context_footer.clone();
            }
            if joiner.is_some() {
                guard.joiner = joiner.clone();
            }
        }

        if let Some(secs) = http_timeout_secs.or_else(|| slo_ms.map(|ms| ms.div_ceil(1000).max(1)))
        {
            http_timeout::set_secs(secs);
            let mut payload = json!({
                "action": "hint",
                "params": {"http_timeout_secs": secs, "source": "slo|mode"},
                "ok": true
            });
            crate::responses::attach_corr(&mut payload);
            bus.publish(topics::TOPIC_ACTIONS_HINT_APPLIED, &payload);
        }

        if let Some(m) = mode {
            let mut payload = json!({"action": "mode", "mode": m});
            crate::responses::attach_corr(&mut payload);
            bus.publish(topics::TOPIC_GOVERNOR_CHANGED, &payload);
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
