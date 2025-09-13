use crate::AppState;
use arw_macros::{arw_admin, arw_gate};
use axum::extract::State;
use axum::response::IntoResponse;
use serde_json::json;
use std::collections::HashMap;
use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering as AOrder};
use std::sync::OnceLock;
use std::sync::OnceLock as StdOnceLock;
use tokio::sync::{Notify, RwLock};

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
    // Mark read-model dirty and signal coalescer
    route_stats_mark_dirty();
}

async fn route_stats_read_model() -> serde_json::Value {
    let rs = route_stats_cell().read().await;
    let mut by_path = serde_json::Map::new();
    for (p, st) in rs.by_path.iter() {
        let v = json!({
            "hits": st.hits,
            "errors": st.errors,
            "ewma_ms": st.ewma_ms,
            "p95_ms": st.p95_ms,
            "max_ms": st.max_ms,
        });
        by_path.insert(p.clone(), v);
    }
    serde_json::Value::Object(serde_json::Map::from_iter([(
        String::from("by_path"),
        serde_json::Value::Object(by_path),
    )]))
}

/// Periodically publish JSON Patch deltas for route stats under a generic and specific topic
pub async fn start_route_stats_publisher(bus: arw_events::Bus) {
    let idle_ms: u64 = std::env::var("ARW_ROUTE_STATS_PUBLISH_MS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(2000)
        .max(200);
    let coalesce_ms: u64 = std::env::var("ARW_ROUTE_STATS_COALESCE_MS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(250)
        .max(10);
    let notify = route_stats_notify();
    let mut idle = tokio::time::interval(std::time::Duration::from_millis(idle_ms));
    loop {
        tokio::select! {
            _ = notify.notified() => {
                // Coalesce bursts
                tokio::time::sleep(std::time::Duration::from_millis(coalesce_ms)).await;
                // Clear dirty flag once per publish
                let _ = ROUTE_DIRTY.swap(false, AOrder::Relaxed);
                publish_route_stats(&bus).await;
            }
            _ = idle.tick() => {
                // Idle publish (diff will be empty if unchanged)
                publish_route_stats(&bus).await;
            }
        }
    }
}

async fn publish_route_stats(bus: &arw_events::Bus) {
    let cur = route_stats_read_model().await;
    crate::ext::read_model::emit_patch_dual(
        bus,
        "State.RouteStats.Patch",
        "State.ReadModel.Patch",
        "route_stats",
        &cur,
    );
}

static ROUTE_DIRTY: AtomicBool = AtomicBool::new(false);
static ROUTE_NOTIFY: StdOnceLock<Notify> = StdOnceLock::new();
fn route_stats_notify() -> &'static Notify {
    ROUTE_NOTIFY.get_or_init(Notify::new)
}
fn route_stats_mark_dirty() {
    ROUTE_DIRTY.store(true, AOrder::Relaxed);
    route_stats_notify().notify_one();
}

#[arw_admin(
    method = "GET",
    path = "/admin/state/route_stats",
    summary = "Get route stats (read-model)"
)]
#[arw_gate("state:route_stats:get")]
pub(crate) async fn route_stats_get() -> impl IntoResponse {
    super::ok(route_stats_read_model().await).into_response()
}
#[arw_admin(
    method = "GET",
    path = "/admin/introspect/stats",
    summary = "Runtime stats and route metrics"
)]
#[arw_gate("introspect:stats")]
pub(crate) async fn stats_get(State(state): State<AppState>) -> impl IntoResponse {
    let events = stats_cell().read().await.clone();
    let routes = route_stats_cell().read().await.clone();
    let bus = state.bus.stats();
    super::ok(json!({ "events": events, "routes": routes, "bus": bus })).into_response()
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

    // Models download metrics
    out.push_str("# HELP arw_models_download_started_total Models download started\n# TYPE arw_models_download_started_total counter\n");
    out.push_str("# HELP arw_models_download_queued_total Models download queued\n# TYPE arw_models_download_queued_total counter\n");
    out.push_str("# HELP arw_models_download_admitted_total Models download admitted\n# TYPE arw_models_download_admitted_total counter\n");
    out.push_str("# HELP arw_models_download_resumed_total Models download resumed\n# TYPE arw_models_download_resumed_total counter\n");
    out.push_str("# HELP arw_models_download_canceled_total Models download canceled\n# TYPE arw_models_download_canceled_total counter\n");
    out.push_str("# HELP arw_models_download_completed_total Models download completed\n# TYPE arw_models_download_completed_total counter\n");
    out.push_str("# HELP arw_models_download_completed_cached_total Models completed via cache\n# TYPE arw_models_download_completed_cached_total counter\n");
    out.push_str("# HELP arw_models_download_error_total Models download errors\n# TYPE arw_models_download_error_total counter\n");
    out.push_str("# HELP arw_models_download_bytes_total Network bytes downloaded for models\n# TYPE arw_models_download_bytes_total counter\n");
    out.push_str("# HELP arw_models_download_ewma_mbps EWMA throughput MB/s\n# TYPE arw_models_download_ewma_mbps gauge\n");
    let mm = crate::resources::models_service::models_metrics_value();
    let get = |k: &str| mm.get(k).and_then(|x| x.as_u64()).unwrap_or(0);
    let _ = writeln!(out, "arw_models_download_started_total {}", get("started"));
    let _ = writeln!(out, "arw_models_download_queued_total {}", get("queued"));
    let _ = writeln!(
        out,
        "arw_models_download_admitted_total {}",
        get("admitted")
    );
    let _ = writeln!(out, "arw_models_download_resumed_total {}", get("resumed"));
    let _ = writeln!(
        out,
        "arw_models_download_canceled_total {}",
        get("canceled")
    );
    let _ = writeln!(
        out,
        "arw_models_download_completed_total {}",
        get("completed")
    );
    let _ = writeln!(
        out,
        "arw_models_download_completed_cached_total {}",
        get("completed_cached")
    );
    let _ = writeln!(out, "arw_models_download_error_total {}", get("errors"));
    let _ = writeln!(
        out,
        "arw_models_download_bytes_total {}",
        get("bytes_total")
    );
    // EWMA MB/s from state file (best-effort)
    let ewma = crate::ext::io::load_json_file(&crate::ext::paths::downloads_metrics_path())
        .and_then(|v| v.get("ewma_mbps").and_then(|x| x.as_f64()))
        .unwrap_or(0.0);
    let _ = writeln!(out, "arw_models_download_ewma_mbps {}", ewma);

    // Tool Action Cache metrics (best-effort)
    out.push_str("# HELP arw_tools_cache_hits_total Action cache hits\n# TYPE arw_tools_cache_hits_total counter\n");
    out.push_str("# HELP arw_tools_cache_miss_total Action cache misses\n# TYPE arw_tools_cache_miss_total counter\n");
    out.push_str("# HELP arw_tools_cache_coalesced_total Coalesced requests\n# TYPE arw_tools_cache_coalesced_total counter\n");
    out.push_str(
        "# HELP arw_tools_cache_entries Current entries\n# TYPE arw_tools_cache_entries gauge\n",
    );
    out.push_str("# HELP arw_tools_cache_ttl_seconds TTL seconds\n# TYPE arw_tools_cache_ttl_seconds gauge\n");
    out.push_str("# HELP arw_tools_cache_capacity_max Max capacity\n# TYPE arw_tools_cache_capacity_max gauge\n");
    let c = super::tools_exec::cache_stats_value();
    let hit = c.get("hit").and_then(|x| x.as_u64()).unwrap_or(0);
    let miss = c.get("miss").and_then(|x| x.as_u64()).unwrap_or(0);
    let coal = c.get("coalesced").and_then(|x| x.as_u64()).unwrap_or(0);
    let entries = c.get("entries").and_then(|x| x.as_u64()).unwrap_or(0);
    let ttl = c.get("ttl_secs").and_then(|x| x.as_u64()).unwrap_or(0);
    let cap = c.get("capacity").and_then(|x| x.as_u64()).unwrap_or(0);
    let _ = writeln!(out, "arw_tools_cache_hits_total {}", hit);
    let _ = writeln!(out, "arw_tools_cache_miss_total {}", miss);
    let _ = writeln!(out, "arw_tools_cache_coalesced_total {}", coal);
    let _ = writeln!(out, "arw_tools_cache_entries {}", entries);
    let _ = writeln!(out, "arw_tools_cache_ttl_seconds {}", ttl);
    let _ = writeln!(out, "arw_tools_cache_capacity_max {}", cap);

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

/// Snapshot of p95 latency per path (ms)
pub(crate) async fn routes_p95_by_path() -> HashMap<String, u64> {
    let rs = route_stats_cell().read().await.clone();
    rs.by_path
        .into_iter()
        .map(|(k, v)| (k, v.p95_ms))
        .collect()
}

pub(crate) async fn event_kind_count(kind: &str) -> u64 {
    let s = stats_cell().read().await;
    s.kinds.get(kind).cloned().unwrap_or(0)
}
