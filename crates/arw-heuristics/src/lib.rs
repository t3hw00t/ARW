use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RouteStat {
    pub ewma_ms: f64,
    pub p95_ms: f64,
    pub hits: u64,
    pub errors: u64,
}

impl RouteStat {
    pub fn error_rate(&self) -> f64 {
        if self.hits == 0 {
            return 0.0;
        }
        (self.errors as f64) / (self.hits as f64)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Features {
    pub routes: HashMap<String, RouteStat>,
    pub mem_applied_count: u64,
    pub cur_mem_limit: Option<u64>,
    pub bus_lagged: u64,
    pub bus_receivers: usize,
    pub pending_suggestions: usize,
    pub pending_signals: usize,
    pub current_profile: Option<String>,
    pub auto_apply_enabled: bool,
}

/// Evaluate lightweight heuristics and return a list of suggestion JSON objects.
/// Each suggestion follows the shape:
/// { id, action, params: {..}, rationale, confidence }
pub fn evaluate(features: &Features) -> Vec<Value> {
    let mut out: Vec<Value> = Vec::new();

    // Heuristic 1: HTTP timeout hint driven by composite latency (p95 + EWMA).
    if let Some((path, rs)) = features
        .routes
        .iter()
        .filter(|(_, rs)| rs.hits > 10)
        .max_by(|a, b| {
            let a_score = a.1.p95_ms.max(a.1.ewma_ms);
            let b_score = b.1.p95_ms.max(b.1.ewma_ms);
            a_score
                .partial_cmp(&b_score)
                .unwrap_or(std::cmp::Ordering::Equal)
        })
    {
        let composite_ms = rs.p95_ms.max(rs.ewma_ms);
        if composite_ms > 850.0 {
            let target_secs = (((composite_ms / 1000.0) * 1.6) + 6.0).clamp(15.0, 240.0);
            out.push(serde_json::json!({
                "id": format!("hint-http-timeout-{}", path),
                "action": "hint",
                "params": {
                    "http_timeout_secs": target_secs.round() as u64,
                    "route": path,
                    "source": "heuristics.latency"
                },
                "rationale": format!(
                    "Route {} p95 ≈ {:.0} ms, EWMA ≈ {:.0} ms; recommend raising timeout",
                    path,
                    rs.p95_ms,
                    rs.ewma_ms
                ),
                "confidence": if features.auto_apply_enabled { 0.68 } else { 0.55 }
            }));
        }
    }

    // Heuristic 2: Memory pressure (based on number of memory.applied events)
    if features.mem_applied_count > 250 {
        let cur = features.cur_mem_limit.unwrap_or(200);
        let ceiling = 800u64;
        if cur < ceiling {
            let proposed = ((cur as f64) * 1.4).round() as u64;
            let new_limit = proposed.clamp(200, ceiling);
            out.push(serde_json::json!({
                "id": "mem-limit",
                "action": "mem_limit",
                "params": {"limit": new_limit},
                "rationale": format!(
                    "{} memory.apply events this window; suggest bumping limit to {}",
                    features.mem_applied_count,
                    new_limit
                ),
                "confidence": if features.auto_apply_enabled { 0.62 } else { 0.5 }
            }));
        }
    }

    // Heuristic 3: Event bus lag awareness.
    if features.bus_lagged > 100 {
        out.push(serde_json::json!({
            "id": "hint-bus-lag",
            "action": "hint",
            "params": {
                "topic": "event.bus",
                "lagged": features.bus_lagged,
                "receivers": features.bus_receivers,
                "source": "heuristics.bus"
            },
            "rationale": format!(
                "Event bus observed {} lagged deliveries; inspect slow subscribers ({} receivers)",
                features.bus_lagged,
                features.bus_receivers
            ),
            "confidence": 0.42
        }));
    }

    // Aggregate stats for responsive governor hints.
    let total_hits: u64 = features.routes.values().map(|r| r.hits).sum();
    let total_errors: u64 = features.routes.values().map(|r| r.errors).sum();
    let aggregate_error_rate = if total_hits > 0 {
        total_errors as f64 / total_hits as f64
    } else {
        0.0
    };
    let peak_latency = features
        .routes
        .values()
        .map(|r| r.p95_ms.max(r.ewma_ms))
        .fold(0.0f64, f64::max);

    let profile = features.current_profile.as_deref().unwrap_or("balance");
    if features.bus_lagged > 200 && peak_latency > 1100.0 && profile != "safe" {
        let suggested_concurrency = if total_hits > 400 { 6 } else { 4 };
        let timeout_secs = ((peak_latency / 1000.0).max(1.0).ceil() as u64).clamp(12, 240);
        out.push(serde_json::json!({
            "id": "governor-hints-overload",
            "action": "governor.hints",
            "params": {
                "max_concurrency": suggested_concurrency,
                "http_timeout_secs": timeout_secs,
                "source": "heuristics.overload"
            },
            "rationale": format!(
                "Bus lag {} with peak latency ≈ {:.0} ms and error rate {:.1}% ; trial max concurrency {} and timeout {}s",
                features.bus_lagged,
                peak_latency,
                aggregate_error_rate * 100.0,
                suggested_concurrency,
                timeout_secs
            ),
            "confidence": 0.52
        }));
    }

    // Heuristic 4: Elevated error rate on dominant route.
    if let Some((path, rs)) = features
        .routes
        .iter()
        .filter(|(_, rs)| rs.hits >= 20)
        .max_by(|a, b| {
            a.1.error_rate()
                .partial_cmp(&b.1.error_rate())
                .unwrap_or(std::cmp::Ordering::Equal)
        })
    {
        let err_rate = rs.error_rate();
        if err_rate > 0.18 {
            out.push(serde_json::json!({
                "id": format!("hint-route-errors-{}", path),
                "action": "hint",
                "params": {
                    "route": path,
                    "error_rate": err_rate,
                    "source": "heuristics.errors"
                },
                "rationale": format!(
                    "{} error rate {:.1}% across {} calls; probe upstream or throttle",
                    path,
                    err_rate * 100.0,
                    rs.hits
                ),
                "confidence": 0.58
            }));
        }
    }

    // Heuristic 5: Suggest enabling auto-apply when backlog grows.
    if !features.auto_apply_enabled
        && features.pending_suggestions >= 3
        && features.pending_signals >= 5
    {
        out.push(serde_json::json!({
            "id": "hint-auto-apply",
            "action": "hint",
            "params": {
                "setting": "feedback.auto_apply",
                "source": "heuristics.feedback"
            },
            "rationale": format!(
                "{} queued suggestions / {} signals; enable auto-apply to trial guards",
                features.pending_suggestions,
                features.pending_signals
            ),
            "confidence": 0.45
        }));
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn produces_latency_hint() {
        let mut features = Features {
            auto_apply_enabled: true,
            ..Features::default()
        };
        features.routes.insert(
            "/chat".into(),
            RouteStat {
                ewma_ms: 900.0,
                p95_ms: 1500.0,
                hits: 64,
                errors: 2,
            },
        );

        let suggestions = evaluate(&features);
        assert!(suggestions.iter().any(|value| {
            value["id"]
                .as_str()
                .map(|s| s.contains("hint-http-timeout"))
                .unwrap_or(false)
                && value["params"]["http_timeout_secs"].is_u64()
        }));
    }

    #[test]
    fn suggests_governor_hints_when_overloaded() {
        let mut features = Features {
            current_profile: Some("balance".into()),
            bus_lagged: 320,
            ..Features::default()
        };
        features.routes.insert(
            "/chat".into(),
            RouteStat {
                ewma_ms: 1200.0,
                p95_ms: 1600.0,
                hits: 600,
                errors: 30,
            },
        );

        let suggestions = evaluate(&features);
        assert!(suggestions.iter().any(|value| {
            value["action"].as_str() == Some("governor.hints")
                && value["params"]["max_concurrency"].is_u64()
        }));
    }
}
