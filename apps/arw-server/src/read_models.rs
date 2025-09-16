use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::time::Duration;
use tokio::time;

use crate::AppState;
use arw_topics as topics;

pub(crate) fn start_read_models(state: AppState) {
    spawn_read_model(&state, "logic_units", Duration::from_millis(1500), |st| {
        st.kernel
            .list_logic_units(200)
            .ok()
            .map(|items| json!({ "items": items }))
    });

    spawn_read_model(
        &state,
        "orchestrator_jobs",
        Duration::from_millis(2000),
        |st| {
            st.kernel
                .list_orchestrator_jobs(200)
                .ok()
                .map(|items| json!({ "items": items }))
        },
    );

    spawn_read_model(&state, "memory_recent", Duration::from_millis(2500), |st| {
        st.kernel
            .list_recent_memory(None, 200)
            .ok()
            .map(|items| json!({ "items": items }))
    });

    spawn_read_model(&state, "route_stats", Duration::from_millis(2000), |st| {
        let bus = st.bus.stats();
        let metrics = st.metrics.snapshot();
        Some(json!({
            "bus": {
                "published": bus.published,
                "delivered": bus.delivered,
                "receivers": bus.receivers,
                "lagged": bus.lagged,
                "no_receivers": bus.no_receivers,
            },
            "events": metrics.events,
            "routes": metrics.routes,
        }))
    });
}

fn spawn_read_model<F>(state: &AppState, id: &'static str, period: Duration, builder: F)
where
    F: FnMut(&AppState) -> Option<Value> + Send + 'static,
{
    let bus = state.bus.clone();
    let state = state.clone();
    tokio::spawn(async move {
        let mut builder = builder;
        let mut last_hash: Option<[u8; 32]> = None;
        let mut tick = time::interval(period);
        loop {
            tick.tick().await;
            if let Some(value) = builder(&state) {
                if let Some(hash) = hash_value(&value) {
                    let is_changed = last_hash.map(|prev| prev != hash).unwrap_or(true);
                    if is_changed {
                        last_hash = Some(hash);
                        bus.publish(
                            topics::TOPIC_READMODEL_PATCH,
                            &json!({
                                "id": id,
                                "patch": [
                                    {"op": "replace", "path": "/", "value": value}
                                ]
                            }),
                        );
                    }
                }
            }
        }
    });
}

fn hash_value(value: &Value) -> Option<[u8; 32]> {
    let bytes = serde_json::to_vec(value).ok()?;
    let digest = Sha256::digest(&bytes);
    let mut out = [0u8; 32];
    out.copy_from_slice(&digest);
    Some(out)
}
