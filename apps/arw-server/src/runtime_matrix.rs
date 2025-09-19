use chrono::SecondsFormat;
use once_cell::sync::OnceCell;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::RwLock;
use tokio::time::{interval, Duration};

use crate::{read_models, AppState};

fn store() -> &'static RwLock<HashMap<String, Value>> {
    static MATRIX: OnceCell<RwLock<HashMap<String, Value>>> = OnceCell::new();
    MATRIX.get_or_init(|| RwLock::new(HashMap::new()))
}

fn node_id() -> String {
    std::env::var("ARW_NODE_ID")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| sysinfo::System::host_name().unwrap_or_else(|| "local".to_string()))
}

pub(crate) fn snapshot() -> HashMap<String, Value> {
    store().read().unwrap().clone()
}

pub(crate) fn start(state: AppState) {
    let subscriber_state = state.clone();
    let bus_for_patch = state.bus();
    tokio::spawn(async move {
        let mut rx = subscriber_state.bus().subscribe();
        while let Ok(env) = rx.recv().await {
            if env.kind.as_str() == "runtime.health" {
                let key = env
                    .payload
                    .get("target")
                    .and_then(|v| v.as_str())
                    .unwrap_or("runtime")
                    .to_string();
                let snapshot = {
                    let mut guard = store().write().unwrap();
                    guard.insert(key, env.payload.clone());
                    guard.clone()
                };
                read_models::publish_read_model_patch(
                    &bus_for_patch,
                    "runtime_matrix",
                    &json!({"items": snapshot}),
                );
            }
        }
    });

    let publisher_state = state.clone();
    tokio::spawn(async move {
        let mut tick = interval(Duration::from_secs(5));
        loop {
            tick.tick().await;
            if let Some(payload) = build_local_health_payload(&publisher_state).await {
                publisher_state.bus().publish("runtime.health", &payload);
            }
        }
    });
}

async fn build_local_health_payload(state: &AppState) -> Option<Value> {
    let generated = chrono::Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true);
    let metrics = state.metrics().snapshot();
    let bus_stats = state.bus().stats();

    let mut total_hits = 0u64;
    let mut total_errors = 0u64;
    let mut ewma_sum = 0.0f64;
    let mut ewma_count = 0u64;
    for summary in metrics.routes.by_path.values() {
        total_hits = total_hits.saturating_add(summary.hits);
        total_errors = total_errors.saturating_add(summary.errors);
        if summary.ewma_ms.is_finite() && summary.ewma_ms > 0.0 {
            ewma_sum += summary.ewma_ms;
            ewma_count = ewma_count.saturating_add(1);
        }
    }
    let avg_ewma = if ewma_count == 0 {
        None
    } else {
        Some((ewma_sum / ewma_count as f64).round())
    };

    let payload = json!({
        "target": node_id(),
        "status": if state.kernel_enabled() { "ok" } else { "disabled" },
        "generated": generated,
        "kernel": {
            "enabled": state.kernel_enabled(),
        },
        "bus": {
            "published": bus_stats.published,
            "delivered": bus_stats.delivered,
            "receivers": bus_stats.receivers,
            "lagged": bus_stats.lagged,
            "no_receivers": bus_stats.no_receivers,
        },
        "events": {
            "total": metrics.events.total,
            "kinds": metrics.events.kinds.len(),
        },
        "http": {
            "routes": metrics.routes.by_path.len(),
            "hits": total_hits,
            "errors": total_errors,
            "avg_ewma_ms": avg_ewma,
        }
    });
    Some(payload)
}
