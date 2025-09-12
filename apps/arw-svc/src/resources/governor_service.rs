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
            .publish("Governor.Changed", &json!({"profile": name}));
        crate::ext::persist_orch().await;
    }

    pub async fn hints_get(&self) -> serde_json::Value {
        let h = crate::ext::hints().read().await.clone();
        serde_json::to_value(h).unwrap_or(json!({}))
    }

    pub async fn hints_set_values(
        &self,
        state: &AppState,
        max_concurrency: Option<usize>,
        event_buffer: Option<usize>,
        http_timeout_secs: Option<u64>,
        mode: Option<String>,
        slo_ms: Option<u64>,
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
            state.bus.publish("Actions.HintApplied", &payload);
        }
        // Optional mode-policy side effects (light-touch): expose as event for UI/recipes
        if let Some(m) = mode {
            let mut payload = json!({"action":"mode","mode": m});
            crate::ext::corr::ensure_corr(&mut payload);
            state.bus.publish("Governor.Changed", &payload);
        }
        crate::ext::persist_orch().await;
    }
}
