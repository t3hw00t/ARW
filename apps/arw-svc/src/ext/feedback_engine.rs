use super::{mem_limit, stats};
use crate::AppState;
use serde_json::{json, Value};
use std::sync::OnceLock;
use tokio::sync::RwLock;

// Lightweight snapshot of current suggestions (reused by API or UI)
static SNAPSHOT: OnceLock<RwLock<Vec<Value>>> = OnceLock::new();
fn snap() -> &'static RwLock<Vec<Value>> {
    SNAPSHOT.get_or_init(|| RwLock::new(Vec::new()))
}

pub fn start_feedback_engine(state: AppState) {
    // Spawn a single actor with short cadence; no blocking on request paths
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(std::time::Duration::from_millis(500));
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            tick.tick().await;
            // Gather minimal features from stats module (bounded, cheap)
            let routes_map = stats::routes_for_analysis().await;
            let mut out: Vec<Value> = Vec::new();

            // Heuristic 1: HTTP timeout hint from worst route EWMA
            if let Some((path, (ewma_ms, _hits, _errs))) = routes_map
                .iter()
                .max_by(|a, b| a.1 .0.partial_cmp(&b.1 .0).unwrap_or(std::cmp::Ordering::Equal))
            {
                if *ewma_ms > 800.0 {
                    let desired = (((ewma_ms / 1000.0) * 2.0) + 10.0)
                        .clamp(20.0, 180.0) as u64;
                    out.push(json!({
                        "id": format!("hint-{}", path),
                        "action": "hint",
                        "params": {"http_timeout_secs": desired},
                        "rationale": format!("High latency on {} (~{:.0} ms)", path, ewma_ms),
                        "confidence": 0.6
                    }));
                }
            }

            // Heuristic 2: Memory pressure
            // Use number of Memory.Applied events as proxy (from stats counters)
            let mem_applied = stats::event_kind_count("Memory.Applied").await;
            if mem_applied > 200 {
                let cur = { *mem_limit().read().await } as u64;
                if cur < 300 {
                    let new = (cur * 3 / 2).clamp(200, 600);
                    out.push(json!({
                        "id": "mem-limit",
                        "action": "mem_limit",
                        "params": {"limit": new},
                        "rationale": format!("Frequent memory updates ({}); suggest {}", mem_applied, new),
                        "confidence": 0.5
                    }));
                }
            }

            // Publish deltas if changed
            {
                let mut s = snap().write().await;
                if *s != out {
                    *s = out.clone();
                    state
                        .bus
                        .publish("Feedback.Suggested", &json!({"suggestions": out}));
                }
            }
        }
    });
}

pub async fn snapshot() -> Vec<Value> {
    snap().read().await.clone()
}

