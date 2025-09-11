use crate::AppState;
use arw_macros::{arw_gate, arw_admin};
use axum::extract::State;
use axum::response::IntoResponse;
use axum::Json;
use serde_json::json;
use std::collections::HashMap;
use std::collections::VecDeque;
use std::sync::OnceLock;
use tokio::sync::RwLock;

#[derive(Clone, Default, serde::Serialize)]
struct Stats {
    start: String,
    total: u64,
    kinds: HashMap<String, u64>,
}
static STATS: OnceLock<RwLock<Stats>> = OnceLock::new();
fn stats_cell() -> &'static RwLock<Stats> {
    STATS.get_or_init(|| {
        let start = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
        RwLock::new(Stats {
            start,
            total: 0,
            kinds: HashMap::new(),
        })
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
struct RouteStats {
    by_path: HashMap<String, RouteStat>,
}
static ROUTE_STATS: OnceLock<RwLock<RouteStats>> = OnceLock::new();
fn route_stats_cell() -> &'static RwLock<RouteStats> {
    ROUTE_STATS.get_or_init(|| RwLock::new(RouteStats::default()))
}
pub(crate) async fn route_obs(path: &str, status: u16, ms: u64) {
    let mut rs = route_stats_cell().write().await;
    let ent = rs.by_path.entry(path.to_string()).or_default();
    ent.hits += 1;
    if status >= 400 {
        ent.errors += 1;
    }
    // EWMA with alpha=0.2
    let a = 0.2f64;
    let v = ms as f64;
    ent.ewma_ms = if ent.ewma_ms == 0.0 {
        v
    } else {
        (1.0 - a) * ent.ewma_ms + a * v
    };
    ent.last_ms = ms;
    ent.max_ms = ent.max_ms.max(ms);
    ent.last_status = status;
    // p95 with small sliding sample
    if ent.sample.len() >= 50 {
        ent.sample.pop_front();
    }
    ent.sample.push_back(ms);
    let mut tmp: Vec<u64> = ent.sample.iter().copied().collect();
    if !tmp.is_empty() {
        tmp.sort_unstable();
        let idx = ((tmp.len() as f64) * 0.95).ceil() as usize;
        let idx = idx.saturating_sub(1).min(tmp.len() - 1);
        ent.p95_ms = tmp[idx];
    }
}
#[arw_admin(method="GET", path="/admin/introspect/stats", summary="Runtime stats and route metrics")]
#[arw_gate("introspect:stats")]
pub(crate) async fn stats_get(State(state): State<AppState>) -> impl IntoResponse {
    let events = stats_cell().read().await.clone();
    let routes = route_stats_cell().read().await.clone();
    let bus = state.bus.stats();
    Json(json!({ "events": events, "routes": routes, "bus": bus })).into_response()
}

// Simple Prometheus exposition for core counters and route timings
pub(crate) async fn metrics_get(State(state): State<AppState>) -> impl IntoResponse {
    let events = stats_cell().read().await.clone();
    let routes = route_stats_cell().read().await.clone();
    let bus = state.bus.stats();
    fn esc(s: &str) -> String {
        s.replace('\\', "\\\\").replace('"', "\\\"")
    }
    let mut out = String::new();
    use std::fmt::Write as _;
    // Bus
    out.push_str(
        "# HELP arw_bus_published_total Events published\n# TYPE arw_bus_published_total counter\n",
    );
    let _ = writeln!(out, "arw_bus_published_total {}", bus.published);
    out.push_str("# HELP arw_bus_delivered_total Events delivered to receivers\n# TYPE arw_bus_delivered_total counter\n");
    let _ = writeln!(out, "arw_bus_delivered_total {}", bus.delivered);
    out.push_str(
        "# HELP arw_bus_lagged_total Lag signals observed\n# TYPE arw_bus_lagged_total counter\n",
    );
    let _ = writeln!(out, "arw_bus_lagged_total {}", bus.lagged);
    out.push_str("# HELP arw_bus_no_receivers_total Publishes with no receivers\n# TYPE arw_bus_no_receivers_total counter\n");
    let _ = writeln!(out, "arw_bus_no_receivers_total {}", bus.no_receivers);
    out.push_str(
        "# HELP arw_bus_receivers Current receiver count\n# TYPE arw_bus_receivers gauge\n",
    );
    let _ = writeln!(out, "arw_bus_receivers {}", bus.receivers);

    // Build info
    out.push_str("# HELP arw_build_info Build info\n# TYPE arw_build_info gauge\n");
    let name: &'static str = env!("CARGO_PKG_NAME");
    let ver: &'static str = env!("CARGO_PKG_VERSION");
    let sha: &str = option_env!("ARW_BUILD_SHA").unwrap_or("unknown");
    let _ = writeln!(
        out,
        "arw_build_info{{service=\"{}\",version=\"{}\",sha=\"{}\"}} 1",
        name, ver, sha
    );

    // Events by kind
    out.push_str("# HELP arw_events_total Total events by kind\n# TYPE arw_events_total counter\n");
    for (k, v) in events.kinds.iter() {
        let _ = writeln!(out, "arw_events_total{{kind=\"{}\"}} {}", esc(k), v);
    }

    // Route stats
    out.push_str("# HELP arw_http_route_hits_total HTTP hits by route\n# TYPE arw_http_route_hits_total counter\n");
    out.push_str("# HELP arw_http_route_errors_total HTTP errors by route\n# TYPE arw_http_route_errors_total counter\n");
    out.push_str(
        "# HELP arw_http_route_ewma_ms EWMA latency ms\n# TYPE arw_http_route_ewma_ms gauge\n",
    );
    out.push_str(
        "# HELP arw_http_route_p95_ms p95 latency ms\n# TYPE arw_http_route_p95_ms gauge\n",
    );
    out.push_str(
        "# HELP arw_http_route_max_ms max latency ms\n# TYPE arw_http_route_max_ms gauge\n",
    );
    for (path, st) in routes.by_path.iter() {
        let p = esc(path);
        let _ = writeln!(
            out,
            "arw_http_route_hits_total{{path=\"{}\"}} {}",
            p, st.hits
        );
        let _ = writeln!(
            out,
            "arw_http_route_errors_total{{path=\"{}\"}} {}",
            p, st.errors
        );
        let _ = writeln!(
            out,
            "arw_http_route_ewma_ms{{path=\"{}\"}} {}",
            p, st.ewma_ms
        );
        let _ = writeln!(out, "arw_http_route_p95_ms{{path=\"{}\"}} {}", p, st.p95_ms);
        let _ = writeln!(out, "arw_http_route_max_ms{{path=\"{}\"}} {}", p, st.max_ms);
    }
    (
        axum::http::StatusCode::OK,
        [(
            axum::http::header::CONTENT_TYPE,
            "text/plain; version=0.0.4",
        )],
        out,
    )
}

// Lightweight snapshot for analysis: path -> (ewma_ms, hits, errors)
pub(crate) async fn routes_for_analysis() -> HashMap<String, (f64, u64, u64)> {
    let rs = route_stats_cell().read().await.clone();
    rs.by_path
        .into_iter()
        .map(|(k, v)| (k, (v.ewma_ms, v.hits, v.errors)))
        .collect()
}

pub(crate) async fn event_kind_count(kind: &str) -> u64 {
    let s = stats_cell().read().await;
    s.kinds.get(kind).cloned().unwrap_or(0)
}
