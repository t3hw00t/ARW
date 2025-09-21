use chrono::SecondsFormat;
use serde_json::json;

use crate::AppState;

pub fn telemetry_snapshot(state: &AppState) -> serde_json::Value {
    let metrics = state.metrics().snapshot();
    let bus = state.bus().stats();
    let generated = chrono::Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true);

    let routes: Vec<serde_json::Value> = metrics
        .routes
        .by_path
        .iter()
        .map(|(path, summary)| {
            json!({
                "path": path,
                "hits": summary.hits,
                "errors": summary.errors,
                "ewma_ms": summary.ewma_ms,
                "p95_ms": summary.p95_ms,
                "max_ms": summary.max_ms,
            })
        })
        .collect();

    let mut kinds: Vec<(String, u64)> = metrics
        .events
        .kinds
        .iter()
        .map(|(k, v)| (k.clone(), *v))
        .collect();
    kinds.sort_by(|a, b| b.1.cmp(&a.1));

    let completed = metrics
        .events
        .kinds
        .get("actions.completed")
        .copied()
        .unwrap_or(0);
    let failed = metrics
        .events
        .kinds
        .get("actions.failed")
        .copied()
        .unwrap_or(0);
    let total_runs = completed + failed;
    let success_rate = if total_runs > 0 {
        Some((completed as f64) / (total_runs as f64))
    } else {
        None
    };

    json!({
        "generated": generated,
        "events": {
            "start": metrics.events.start,
            "total": metrics.events.total,
            "kinds": kinds,
        },
        "routes": routes,
        "bus": {
            "published": bus.published,
            "delivered": bus.delivered,
            "receivers": bus.receivers,
            "lagged": bus.lagged,
            "no_receivers": bus.no_receivers,
        },
        "tools": {
            "completed": completed,
            "failed": failed,
            "success_rate": success_rate,
        }
    })
}
