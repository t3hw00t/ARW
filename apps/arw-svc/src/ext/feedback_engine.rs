use super::{mem_limit, stats};
use crate::AppState;
use serde::Deserialize;
use serde_json::{json, Value};
use std::sync::{
    atomic::{AtomicU64, Ordering},
    OnceLock,
};
use tokio::sync::RwLock;

// Lightweight snapshot of current suggestions (reused by API or UI)
static SNAPSHOT: OnceLock<RwLock<Vec<Value>>> = OnceLock::new();
static VERSION: OnceLock<AtomicU64> = OnceLock::new();
fn snap() -> &'static RwLock<Vec<Value>> {
    SNAPSHOT.get_or_init(|| RwLock::new(Vec::new()))
}
fn ver() -> &'static AtomicU64 {
    VERSION.get_or_init(|| AtomicU64::new(0))
}

pub fn start_feedback_engine(state: AppState) {
    // Spawn a single actor with short cadence; no blocking on request paths
    tokio::spawn(async move {
        let tick_ms: u64 = load_cfg_tick_ms().unwrap_or_else(|| {
            std::env::var("ARW_FEEDBACK_TICK_MS")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(500)
        });
        let mut tick = tokio::time::interval(std::time::Duration::from_millis(tick_ms));
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            tick.tick().await;
            // Gather minimal features from stats module (bounded, cheap)
            let routes_map = stats::routes_for_analysis().await;
            let mut out: Vec<Value> = Vec::new();

            // Heuristic 1: HTTP timeout hint from worst route EWMA
            if let Some((path, (ewma_ms, _hits, _errs))) = routes_map.iter().max_by(|a, b| {
                a.1 .0
                    .partial_cmp(&b.1 .0)
                    .unwrap_or(std::cmp::Ordering::Equal)
            }) {
                if *ewma_ms > 800.0 {
                    let desired = (((ewma_ms / 1000.0) * 2.0) + 10.0).clamp(20.0, 180.0) as u64;
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
                    let v = ver().fetch_add(1, Ordering::Relaxed) + 1;
                    state.bus.publish(
                        "Feedback.Suggested",
                        &json!({"version": v, "suggestions": out}),
                    );
                }
            }
        }
    });
}

pub async fn snapshot() -> (u64, Vec<Value>) {
    let v = ver().load(Ordering::Relaxed);
    let s = snap().read().await.clone();
    (v, s)
}

pub async fn updates_since(since: u64) -> Option<(u64, Vec<Value>)> {
    let cur = ver().load(Ordering::Relaxed);
    if cur > since {
        Some((cur, snap().read().await.clone()))
    } else {
        None
    }
}

// --- Optional config loader (configs/feedback.toml) ---
#[derive(Deserialize, Default)]
struct FbCfg {
    tick_ms: Option<u64>,
}
fn load_cfg_tick_ms() -> Option<u64> {
    static CFG: OnceLock<Option<FbCfg>> = OnceLock::new();
    let cfg = CFG.get_or_init(|| {
        let p = std::path::Path::new("configs/feedback.toml");
        if let Ok(s) = std::fs::read_to_string(p) {
            toml::from_str::<FbCfg>(&s).ok()
        } else {
            None
        }
    });
    cfg.as_ref().and_then(|c| c.tick_ms)
}
