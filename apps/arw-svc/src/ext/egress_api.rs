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
    let limit = q.limit.unwrap_or(200).max(1).min(10_000);
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
