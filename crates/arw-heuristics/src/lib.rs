use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RouteStat {
    pub ewma_ms: f64,
    pub hits: u64,
    pub errors: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Features {
    pub routes: HashMap<String, RouteStat>,
    pub mem_applied_count: u64,
    pub cur_mem_limit: Option<u64>,
}

/// Evaluate lightweight heuristics and return a list of suggestion JSON objects.
/// Each suggestion follows the shape:
/// { id, action, params: {..}, rationale, confidence }
pub fn evaluate(features: &Features) -> Vec<Value> {
    let mut out: Vec<Value> = Vec::new();

    // Heuristic 1: HTTP timeout hint from worst route EWMA
    if let Some((path, rs)) = features.routes.iter().max_by(|a, b| {
        a.1.ewma_ms
            .partial_cmp(&b.1.ewma_ms)
            .unwrap_or(std::cmp::Ordering::Equal)
    }) {
        if rs.ewma_ms > 800.0 {
            let desired = (((rs.ewma_ms / 1000.0) * 2.0) + 10.0).clamp(20.0, 180.0) as u64;
            out.push(serde_json::json!({
                "id": format!("hint-{}", path),
                "action": "hint",
                "params": {"http_timeout_secs": desired},
                "rationale": format!("High latency on {} (~{:.0} ms)", path, rs.ewma_ms),
                "confidence": 0.6
            }));
        }
    }

    // Heuristic 2: Memory pressure (based on number of memory.applied events)
    if features.mem_applied_count > 200 {
        let cur = features.cur_mem_limit.unwrap_or(200);
        if cur < 300 {
            let new = (cur.saturating_mul(3) / 2).clamp(200, 600);
            out.push(serde_json::json!({
                "id": "mem-limit",
                "action": "mem_limit",
                "params": {"limit": new},
                "rationale": format!("Frequent memory updates ({}); suggest {}", features.mem_applied_count, new),
                "confidence": 0.5
            }));
        }
    }

    out
}
