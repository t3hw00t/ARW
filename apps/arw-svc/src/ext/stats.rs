use axum::response::IntoResponse;
use axum::Json;
use serde_json::json;
use std::collections::HashMap;
use std::collections::VecDeque;
use std::sync::OnceLock;
use tokio::sync::RwLock;

#[derive(Clone, Default, serde::Serialize)]
struct Stats { start: String, total: u64, kinds: HashMap<String, u64> }
static STATS: OnceLock<RwLock<Stats>> = OnceLock::new();
fn stats_cell() -> &'static RwLock<Stats> {
    STATS.get_or_init(|| {
        let start = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
        RwLock::new(Stats{ start, total: 0, kinds: HashMap::new() })
    })
}
pub(crate) async fn stats_on_event(kind: &str) {
    let mut s = stats_cell().write().await;
    s.total += 1;
    *s.kinds.entry(kind.to_string()).or_default() += 1;
}
#[derive(Clone, Default, serde::Serialize)]
struct RouteStat {
    hits: u64,
    errors: u64,
    ewma_ms: f64,
    last_ms: u64,
    max_ms: u64,
    last_status: u16,
    p95_ms: u64,
    #[serde(skip_serializing)]
    sample: VecDeque<u64>,
}
#[derive(Clone, Default, serde::Serialize)]
struct RouteStats { by_path: HashMap<String, RouteStat> }
static ROUTE_STATS: OnceLock<RwLock<RouteStats>> = OnceLock::new();
fn route_stats_cell() -> &'static RwLock<RouteStats> { ROUTE_STATS.get_or_init(|| RwLock::new(RouteStats::default())) }
pub(crate) async fn route_obs(path: &str, status: u16, ms: u64) {
    let mut rs = route_stats_cell().write().await;
    let ent = rs.by_path.entry(path.to_string()).or_default();
    ent.hits += 1;
    if status >= 400 { ent.errors += 1; }
    // EWMA with alpha=0.2
    let a = 0.2f64; let v = ms as f64; ent.ewma_ms = if ent.ewma_ms == 0.0 { v } else { (1.0 - a) * ent.ewma_ms + a * v };
    ent.last_ms = ms; ent.max_ms = ent.max_ms.max(ms); ent.last_status = status;
    // p95 with small sliding sample
    if ent.sample.len() >= 50 { ent.sample.pop_front(); }
    ent.sample.push_back(ms);
    let mut tmp: Vec<u64> = ent.sample.iter().copied().collect();
    if !tmp.is_empty() {
        tmp.sort_unstable();
        let idx = ((tmp.len() as f64) * 0.95).ceil() as usize;
        let idx = idx.saturating_sub(1).min(tmp.len()-1);
        ent.p95_ms = tmp[idx];
    }
}
pub(crate) async fn stats_get() -> impl IntoResponse {
    let events = stats_cell().read().await.clone();
    let routes = route_stats_cell().read().await.clone();
    Json(json!({ "events": events, "routes": routes }))
}

