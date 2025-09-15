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

// ---- Probe metrics aggregation (GPU/NPU) ----
#[derive(Clone, Default)]
struct ProbeAgg {
    gpu_count: u64,
    gpu_mem_used: u64,
    gpu_mem_total: u64,
    npu_count: u64,
    gpus: Vec<GpuAdapter>,
    // CPU and Memory snapshot
    cpu_avg: f64,
    cpu_cores: Vec<f64>,
    mem_total: u64,
    mem_used: u64,
    swap_total: u64,
    swap_used: u64,
}

#[derive(Clone, Default)]
struct GpuAdapter {
    index: String,
    name: String,
    vendor: String,
    vendor_id: String,
    mem_total: u64,
    mem_used: Option<u64>,
    busy_percent: Option<u64>,
}
static PROBE: OnceLock<RwLock<ProbeAgg>> = OnceLock::new();
fn probe_cell() -> &'static RwLock<ProbeAgg> {
    PROBE.get_or_init(|| RwLock::new(ProbeAgg::default()))
}
pub async fn start_probe_metrics_collector(bus: arw_events::Bus) {
    let mut rx = bus.subscribe();
    while let Ok(env) = rx.recv().await {
        if env.kind == crate::ext::topics::TOPIC_PROBE_METRICS {
            // payload is JSON with cpu/memory/disk and arrays gpus/npus
            let p = env.payload;
            // Initialize with CPU avg; fill the rest below
            let mut agg = ProbeAgg {
                cpu_avg: p
                    .get("cpu")
                    .and_then(|c| c.get("avg"))
                    .and_then(|v| v.as_f64())
                    .unwrap_or(0.0),
                ..Default::default()
            };
            // CPU cores
            if let Some(arr) = p
                .get("cpu")
                .and_then(|c| c.get("per_core"))
                .and_then(|v| v.as_array())
            {
                agg.cpu_cores = arr.iter().filter_map(|x| x.as_f64()).collect();
            }
            // Memory
            if let Some(m) = p.get("memory").and_then(|v| v.as_object()) {
                agg.mem_total = m.get("total").and_then(|x| x.as_u64()).unwrap_or(0);
                agg.mem_used = m.get("used").and_then(|x| x.as_u64()).unwrap_or(0);
                agg.swap_total = m.get("swap_total").and_then(|x| x.as_u64()).unwrap_or(0);
                agg.swap_used = m.get("swap_used").and_then(|x| x.as_u64()).unwrap_or(0);
            }
            if let Some(gpus) = p.get("gpus").and_then(|v| v.as_array()) {
                agg.gpu_count = gpus.len() as u64;
                for g in gpus {
                    let mt = g.get("mem_total").and_then(|x| x.as_u64()).unwrap_or(0);
                    let mu = g.get("mem_used").and_then(|x| x.as_u64());
                    agg.gpu_mem_total = agg.gpu_mem_total.saturating_add(mt);
                    if let Some(n) = mu {
                        agg.gpu_mem_used = agg.gpu_mem_used.saturating_add(n);
                    }
                    let idx = g
                        .get("index")
                        .and_then(|x| x.as_str())
                        .unwrap_or("")
                        .to_string();
                    let name = g
                        .get("name")
                        .and_then(|x| x.as_str())
                        .unwrap_or("")
                        .to_string();
                    let vendor = g
                        .get("vendor")
                        .and_then(|x| x.as_str())
                        .unwrap_or("")
                        .to_string();
                    let vendor_id = g
                        .get("vendor_id")
                        .and_then(|x| x.as_str())
                        .unwrap_or("")
                        .to_string();
                    let busy = g.get("busy_percent").and_then(|x| x.as_u64());
                    agg.gpus.push(GpuAdapter {
                        index: idx,
                        name,
                        vendor,
                        vendor_id,
                        mem_total: mt,
                        mem_used: mu,
                        busy_percent: busy,
                    });
                }
            }
            if let Some(npus) = p.get("npus").and_then(|v| v.as_array()) {
                agg.npu_count = npus.len() as u64;
            }
            {
                let mut w = probe_cell().write().await;
                *w = agg;
            }
        }
    }
}
#[derive(Clone, serde::Serialize)]
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
    #[serde(skip_serializing)]
    sum_ms: u64,
    #[serde(skip_serializing)]
    hist: Vec<u64>, // per-bucket counts (+Inf as last)
}
#[derive(Clone, Default, serde::Serialize)]
struct RouteStats {
    by_path: HashMap<String, RouteStat>,
}
static ROUTE_STATS: OnceLock<RwLock<RouteStats>> = OnceLock::new();
fn route_stats_cell() -> &'static RwLock<RouteStats> {
    ROUTE_STATS.get_or_init(|| RwLock::new(RouteStats::default()))
}

// Histogram buckets (ms). Final bucket stores +Inf cumulative.
static HIST_MS: once_cell::sync::OnceCell<Vec<u64>> = once_cell::sync::OnceCell::new();
fn hist_buckets() -> &'static [u64] {
    HIST_MS
        .get_or_init(|| {
            if let Ok(s) = std::env::var("ARW_ROUTE_HIST_MS") {
                let mut v: Vec<u64> = s
                    .split(',')
                    .filter_map(|t| t.trim().parse::<u64>().ok())
                    .collect();
                v.sort_unstable();
                v.dedup();
                if !v.is_empty() {
                    return v;
                }
            }
            vec![5, 10, 25, 50, 100, 200, 500, 1000, 2000, 5000, 10000]
        })
        .as_slice()
}
fn new_hist() -> Vec<u64> {
    vec![0; hist_buckets().len() + 1]
}
fn hist_index(ms: u64) -> usize {
    for (i, &b) in hist_buckets().iter().enumerate() {
        if ms <= b {
            return i;
        }
    }
    hist_buckets().len() // +Inf bucket
}
pub(crate) async fn route_obs(path: &str, status: u16, ms: u64) {
    let mut rs = route_stats_cell().write().await;
    let ent = rs
        .by_path
        .entry(path.to_string())
        .or_insert_with(|| RouteStat {
            hits: 0,
            errors: 0,
            ewma_ms: 0.0,
            last_ms: 0,
            max_ms: 0,
            last_status: 0,
            p95_ms: 0,
            sample: VecDeque::with_capacity(50),
            sum_ms: 0,
            hist: new_hist(),
        });
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
    ent.sum_ms = ent.sum_ms.saturating_add(ms);
    let idx = hist_index(ms);
    if let Some(c) = ent.hist.get_mut(idx) {
        *c = c.saturating_add(1);
    }
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
    use crate::ext::topics::TOPIC_READMODEL_PATCH;
    crate::ext::read_model::emit_patch(bus, TOPIC_READMODEL_PATCH, "route_stats", &cur);
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

    // RPU trust
    out.push_str("# HELP arw_rpu_trust_last_reload_ms Epoch ms of last trust store reload\n# TYPE arw_rpu_trust_last_reload_ms gauge\n");
    let _ = writeln!(
        out,
        "arw_rpu_trust_last_reload_ms {}",
        arw_core::rpu::trust_last_reload_ms()
    );
    out.push_str("# HELP arw_rpu_trust_issuers Number of trust issuers\n# TYPE arw_rpu_trust_issuers gauge\n");
    let _ = writeln!(
        out,
        "arw_rpu_trust_issuers {}",
        arw_core::rpu::trust_snapshot().len()
    );

    // GPU/NPU aggregated metrics from last probe
    out.push_str("# HELP arw_gpu_adapters GPU adapters count\n# TYPE arw_gpu_adapters gauge\n");
    out.push_str("# HELP arw_gpu_mem_bytes_total Total GPU memory across adapters\n# TYPE arw_gpu_mem_bytes_total gauge\n");
    out.push_str("# HELP arw_gpu_mem_bytes_used GPU memory used across adapters\n# TYPE arw_gpu_mem_bytes_used gauge\n");
    out.push_str("# HELP arw_npu_adapters NPU adapters count\n# TYPE arw_npu_adapters gauge\n");
    let snap = probe_cell().read().await.clone();
    let _ = writeln!(out, "arw_gpu_adapters {}", snap.gpu_count);
    let _ = writeln!(out, "arw_gpu_mem_bytes_total {}", snap.gpu_mem_total);
    let _ = writeln!(out, "arw_gpu_mem_bytes_used {}", snap.gpu_mem_used);
    let _ = writeln!(out, "arw_npu_adapters {}", snap.npu_count);
    // CPU/Mem
    out.push_str(
        "# HELP arw_cpu_percent_avg Average CPU usage percent\n# TYPE arw_cpu_percent_avg gauge\n",
    );
    let _ = writeln!(out, "arw_cpu_percent_avg {}", snap.cpu_avg);
    out.push_str("# HELP arw_cpu_percent_core CPU usage percent by core (labels: core)\n# TYPE arw_cpu_percent_core gauge\n");
    for (i, v) in snap.cpu_cores.iter().enumerate() {
        let _ = writeln!(out, "arw_cpu_percent_core{{core=\"{}\"}} {}", i, v);
    }
    out.push_str(
        "# HELP arw_mem_bytes_total Total system memory bytes\n# TYPE arw_mem_bytes_total gauge\n",
    );
    out.push_str(
        "# HELP arw_mem_bytes_used Used system memory bytes\n# TYPE arw_mem_bytes_used gauge\n",
    );
    out.push_str(
        "# HELP arw_swap_bytes_total Total system swap bytes\n# TYPE arw_swap_bytes_total gauge\n",
    );
    out.push_str(
        "# HELP arw_swap_bytes_used Used system swap bytes\n# TYPE arw_swap_bytes_used gauge\n",
    );
    let _ = writeln!(out, "arw_mem_bytes_total {}", snap.mem_total);
    let _ = writeln!(out, "arw_mem_bytes_used {}", snap.mem_used);
    let _ = writeln!(out, "arw_swap_bytes_total {}", snap.swap_total);
    let _ = writeln!(out, "arw_swap_bytes_used {}", snap.swap_used);

    // Per-adapter metrics
    out.push_str("# HELP arw_gpu_adapter_info GPU adapter info (labels: index,vendor_id,vendor,name)\n# TYPE arw_gpu_adapter_info gauge\n");
    out.push_str("# HELP arw_gpu_adapter_memory_bytes GPU adapter memory bytes by kind (labels: index,kind)\n# TYPE arw_gpu_adapter_memory_bytes gauge\n");
    out.push_str("# HELP arw_gpu_adapter_busy_percent GPU adapter busy percent (labels: index)\n# TYPE arw_gpu_adapter_busy_percent gauge\n");
    for g in snap.gpus.iter() {
        let _ = writeln!(
            out,
            "arw_gpu_adapter_info{{index=\"{}\",vendor_id=\"{}\",vendor=\"{}\",name=\"{}\"}} 1",
            esc(&g.index),
            esc(&g.vendor_id),
            esc(&g.vendor),
            esc(&g.name)
        );
        let _ = writeln!(
            out,
            "arw_gpu_adapter_memory_bytes{{index=\"{}\",kind=\"total\"}} {}",
            esc(&g.index),
            g.mem_total
        );
        if let Some(mu) = g.mem_used {
            let _ = writeln!(
                out,
                "arw_gpu_adapter_memory_bytes{{index=\"{}\",kind=\"used\"}} {}",
                esc(&g.index),
                mu
            );
        }
        if let Some(bp) = g.busy_percent {
            let _ = writeln!(
                out,
                "arw_gpu_adapter_busy_percent{{index=\"{}\"}} {}",
                esc(&g.index),
                bp
            );
        }
    }

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
    out.push_str(
        "# HELP arw_http_route_latency_ms Histogram of HTTP latencies in ms\n# TYPE arw_http_route_latency_ms histogram\n",
    );
    // Global histogram aggregation
    let mut g_hist = new_hist();
    let mut g_sum: u64 = 0;
    let mut g_count: u64 = 0;
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
        // Histogram (cumulative buckets)
        let mut cum: u64 = 0;
        for (i, &b) in hist_buckets().iter().enumerate() {
            let c = *st.hist.get(i).unwrap_or(&0);
            cum = cum.saturating_add(c);
            let _ = writeln!(
                out,
                "arw_http_route_latency_ms_bucket{{path=\"{}\",le=\"{}\"}} {}",
                p, b, cum
            );
        }
        // +Inf bucket (includes overflow)
        let c = *st.hist.last().unwrap_or(&0);
        cum = cum.saturating_add(c);
        let _ = writeln!(
            out,
            "arw_http_route_latency_ms_bucket{{path=\"{}\",le=\"+Inf\"}} {}",
            p, cum
        );
        let _ = writeln!(
            out,
            "arw_http_route_latency_ms_sum{{path=\"{}\"}} {}",
            p, st.sum_ms
        );
        let _ = writeln!(
            out,
            "arw_http_route_latency_ms_count{{path=\"{}\"}} {}",
            p, st.hits
        );
        // Aggregate
        for (i, &v) in st.hist.iter().enumerate() {
            if let Some(g) = g_hist.get_mut(i) {
                *g = (*g).saturating_add(v);
            }
        }
        g_sum = g_sum.saturating_add(st.sum_ms);
        g_count = g_count.saturating_add(st.hits);
    }
    // Global histogram exposition
    let mut cum: u64 = 0;
    for (i, &b) in hist_buckets().iter().enumerate() {
        let c = *g_hist.get(i).unwrap_or(&0);
        cum = cum.saturating_add(c);
        let _ = writeln!(out, "arw_http_latency_ms_bucket{{le=\"{}\"}} {}", b, cum);
    }
    let c = *g_hist.last().unwrap_or(&0);
    cum = cum.saturating_add(c);
    let _ = writeln!(out, "arw_http_latency_ms_bucket{{le=\"+Inf\"}} {}", cum);
    let _ = writeln!(out, "arw_http_latency_ms_sum {}", g_sum);
    let _ = writeln!(out, "arw_http_latency_ms_count {}", g_count);
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
    rs.by_path.into_iter().map(|(k, v)| (k, v.p95_ms)).collect()
}

pub(crate) async fn event_kind_count(kind: &str) -> u64 {
    let s = stats_cell().read().await;
    s.kinds.get(kind).cloned().unwrap_or(0)
}
