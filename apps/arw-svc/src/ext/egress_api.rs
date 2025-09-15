use arw_macros::{arw_admin, arw_gate};
use axum::{extract::Query, response::IntoResponse};

use crate::AppState;

#[derive(serde::Deserialize)]
pub struct LedgerQuery {
    #[serde(default)]
    pub limit: Option<usize>,
}

// Read a JSONL egress ledger and return the last N entries (best-effort).
#[arw_admin(
    method = "GET",
    path = "/admin/state/egress/ledger",
    summary = "Get egress ledger entries"
)]
#[arw_gate("state:egress_ledger:get")]
pub async fn egress_ledger_get(
    _state: axum::extract::State<AppState>,
    Query(q): Query<LedgerQuery>,
) -> impl IntoResponse {
    let path = super::paths::egress_ledger_path();
    let limit = q.limit.unwrap_or(200).clamp(1, 10_000);
    let mut entries: Vec<serde_json::Value> = Vec::new();
    if let Ok(bytes) = tokio::fs::read(&path).await {
        // Split by lines and parse each line; keep last N
        for line in bytes.split(|b| *b == b'\n') {
            if line.is_empty() {
                continue;
            }
            if let Ok(v) = serde_json::from_slice::<serde_json::Value>(line) {
                entries.push(v);
            }
        }
        if entries.len() > limit {
            entries = entries.split_off(entries.len() - limit);
        }
    }
    super::ok(serde_json::Value::Array(entries)).into_response()
}

#[derive(serde::Deserialize)]
pub struct LedgerSummaryQuery {
    #[serde(default)]
    pub limit: Option<usize>,
    #[serde(default)]
    pub sample: Option<usize>,
    #[serde(default)]
    pub since_ms: Option<u64>,
    #[serde(default)]
    pub decision: Option<String>,
    #[serde(default)]
    pub reason_code: Option<String>,
    #[serde(default)]
    pub project_id: Option<String>,
}

/// Summarize recent egress ledger entries with optional filters; returns counts, bytes, top reasons, and a small sample.
#[arw_admin(
    method = "GET",
    path = "/admin/state/egress/ledger/summary",
    summary = "Summarize egress ledger entries"
)]
#[arw_gate("state:egress_ledger:get")]
pub async fn egress_ledger_summary(
    _state: axum::extract::State<AppState>,
    Query(q): Query<LedgerSummaryQuery>,
) -> impl IntoResponse {
    use std::collections::HashMap;
    let path = super::paths::egress_ledger_path();
    let limit = q.limit.unwrap_or(5000).clamp(1, 100_000);
    let sample_max = q.sample.unwrap_or(30).clamp(0, 200);
    let mut count: u64 = 0;
    let mut scanned: u64 = 0;
    let mut bytes_in: u64 = 0;
    let mut bytes_out: u64 = 0;
    let mut by_decision: HashMap<String, u64> = HashMap::new();
    let mut by_reason: HashMap<String, u64> = HashMap::new();
    let mut sample: Vec<serde_json::Value> = Vec::new();

    // Helper to test filters
    let matches_filters = |v: &serde_json::Value| -> bool {
        if let Some(dec) = q.decision.as_deref() {
            if v.get("decision").and_then(|x| x.as_str()) != Some(dec) {
                return false;
            }
        }
        if let Some(rc) = q.reason_code.as_deref() {
            if v.get("reason_code").and_then(|x| x.as_str()) != Some(rc) {
                return false;
            }
        }
        if let Some(pid) = q.project_id.as_deref() {
            if v.get("project_id").and_then(|x| x.as_str()) != Some(pid) {
                return false;
            }
        }
        if let Some(sms) = q.since_ms {
            // entry time is RFC3339; parse and compare
            if let Some(ts) = v.get("time").and_then(|x| x.as_str()) {
                if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(ts) {
                    let ms = dt.timestamp_millis() as u64;
                    if ms < sms {
                        return false;
                    }
                }
            }
        }
        true
    };

    if let Ok(bytes) = tokio::fs::read(&path).await {
        // Split lines and iterate from the end (most recent first)
        let mut lines: Vec<&[u8]> = bytes
            .split(|b| *b == b'\n')
            .filter(|l| !l.is_empty())
            .collect();
        let total_lines = lines.len();
        let start = total_lines.saturating_sub(limit);
        lines = lines.split_off(start);
        for line in lines.into_iter().rev() {
            scanned += 1;
            if let Ok(v) = serde_json::from_slice::<serde_json::Value>(line) {
                if !matches_filters(&v) {
                    continue;
                }
                count += 1;
                let din = v.get("bytes_in").and_then(|x| x.as_u64()).unwrap_or(0);
                let dout = v.get("bytes_out").and_then(|x| x.as_u64()).unwrap_or(0);
                bytes_in = bytes_in.saturating_add(din);
                bytes_out = bytes_out.saturating_add(dout);
                if let Some(d) = v.get("decision").and_then(|x| x.as_str()) {
                    *by_decision.entry(d.to_string()).or_insert(0) += 1;
                }
                if let Some(r) = v.get("reason_code").and_then(|x| x.as_str()) {
                    *by_reason.entry(r.to_string()).or_insert(0) += 1;
                }
                if sample.len() < sample_max {
                    sample.push(v);
                }
            }
        }
    }
    // Convert reason map to sorted top list
    let mut reasons: Vec<(String, u64)> = by_reason.into_iter().collect();
    reasons.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    let top_reasons: Vec<serde_json::Value> = reasons
        .into_iter()
        .take(10)
        .map(|(k, v)| serde_json::json!({"reason_code": k, "count": v}))
        .collect();

    let out = serde_json::json!({
        "count": count,
        "scanned": scanned,
        "bytes_in": bytes_in,
        "bytes_out": bytes_out,
        "by_decision": by_decision,
        "top_reasons": top_reasons,
        "sample": sample,
        "limit": limit,
    });
    super::ok(out).into_response()
}
