use serde_json::json;

use crate::app_state::AppState;

#[derive(Default)]
pub struct GovernorService;

impl GovernorService {
    pub fn new() -> Self {
        Self
    }

    pub async fn profile_get(&self) -> String {
        crate::ext::governor_profile().read().await.clone()
    }

    pub async fn profile_set(&self, state: &AppState, name: String) {
        {
            let mut g = crate::ext::governor_profile().write().await;
            *g = name.clone();
        }
        state
            .bus
            .publish("governor.changed", &json!({"profile": name}));
        crate::ext::persist_orch().await;
    }

    pub async fn hints_get(&self) -> serde_json::Value {
        let h = crate::ext::hints().read().await.clone();
        serde_json::to_value(h).unwrap_or(json!({}))
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn hints_set_values(
        &self,
        state: &AppState,
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
            let mut h = crate::ext::hints().write().await;
            if max_concurrency.is_some() {
                h.max_concurrency = max_concurrency;
            }
            if event_buffer.is_some() {
                h.event_buffer = event_buffer;
            }
            if http_timeout_secs.is_some() {
                h.http_timeout_secs = http_timeout_secs;
            }
            if mode.is_some() {
                h.mode = mode.clone();
            }
            if slo_ms.is_some() {
                h.slo_ms = slo_ms;
            }
            if retrieval_k.is_some() {
                h.retrieval_k = retrieval_k;
            }
            if retrieval_div.is_some() {
                h.retrieval_div = retrieval_div;
            }
            if mmr_lambda.is_some() {
                h.mmr_lambda = mmr_lambda;
            }
            if compression_aggr.is_some() {
                h.compression_aggr = compression_aggr;
            }
            if vote_k.is_some() {
                h.vote_k = vote_k;
            }
            if context_budget_tokens.is_some() {
                h.context_budget_tokens = context_budget_tokens;
            }
            if context_item_budget_tokens.is_some() {
                h.context_item_budget_tokens = context_item_budget_tokens;
            }
            if context_format.is_some() {
                h.context_format = context_format.clone();
            }
            if include_provenance.is_some() {
                h.include_provenance = include_provenance;
            }
            if context_item_template.is_some() {
                h.context_item_template = context_item_template.clone();
            }
            if context_header.is_some() {
                h.context_header = context_header.clone();
            }
            if context_footer.is_some() {
                h.context_footer = context_footer.clone();
            }
            if joiner.is_some() {
                h.joiner = joiner.clone();
            }
        }
        // Apply dynamic HTTP timeout: prefer explicit, else derive from SLO
        let applied = if let Some(secs) = http_timeout_secs {
            Some(secs)
        } else {
            slo_ms.map(|ms| ms.div_ceil(1000).max(1))
        };
        if let Some(secs) = applied {
            crate::dyn_timeout::set_global_timeout_secs(secs);
            let mut payload = json!({"action":"hint","params":{"http_timeout_secs": secs, "source": "slo|mode"},"ok": true});
            crate::ext::corr::ensure_corr(&mut payload);
            state.bus.publish("actions.hint.applied", &payload);
        }
        // Optional mode-policy side effects (light-touch): expose as event for UI/recipes
        if let Some(m) = mode {
            let mut payload = json!({"action":"mode","mode": m});
            crate::ext::corr::ensure_corr(&mut payload);
            state.bus.publish("governor.changed", &payload);
        }
        crate::ext::persist_orch().await;
    }
}
