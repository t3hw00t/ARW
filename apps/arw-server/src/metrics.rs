use axum::extract::MatchedPath;
use axum::http::{HeaderName, HeaderValue, Request};
use axum::middleware::Next;
use axum::response::Response;
use serde::Serialize;
use std::collections::{BTreeMap, VecDeque};
use std::sync::{Arc, Mutex};
use std::time::Instant;

const SAMPLE_WINDOW: usize = 50;
const EWMA_ALPHA: f64 = 0.2;

fn now_rfc3339() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

#[derive(Clone, Serialize)]
pub struct EventsSummary {
    pub start: String,
    pub total: u64,
    pub kinds: BTreeMap<String, u64>,
}

#[derive(Default)]
struct EventStats {
    start: String,
    total: u64,
    kinds: BTreeMap<String, u64>,
}

impl EventStats {
    fn new() -> Self {
        Self {
            start: now_rfc3339(),
            total: 0,
            kinds: BTreeMap::new(),
        }
    }

    fn record(&mut self, kind: &str) {
        self.total = self.total.saturating_add(1);
        if !kind.is_empty() {
            *self.kinds.entry(kind.to_string()).or_default() += 1;
        }
    }

    fn summary(&self) -> EventsSummary {
        EventsSummary {
            start: self.start.clone(),
            total: self.total,
            kinds: self.kinds.clone(),
        }
    }
}

#[derive(Clone, Serialize, Default)]
pub struct RouteSummary {
    pub hits: u64,
    pub errors: u64,
    pub ewma_ms: f64,
    pub p95_ms: u64,
    pub max_ms: u64,
}

#[derive(Clone, Serialize, Default)]
pub struct RoutesSummary {
    pub by_path: BTreeMap<String, RouteSummary>,
}

#[derive(Clone, Serialize)]
pub struct MetricsSummary {
    pub events: EventsSummary,
    pub routes: RoutesSummary,
}

#[derive(Default)]
struct RouteStat {
    hits: u64,
    errors: u64,
    ewma_ms: f64,
    p95_ms: u64,
    max_ms: u64,
    sample: VecDeque<u64>,
    hist: Vec<u64>,
}

impl RouteStat {
    fn new(hist_size: usize) -> Self {
        Self {
            hits: 0,
            errors: 0,
            ewma_ms: 0.0,
            p95_ms: 0,
            max_ms: 0,
            sample: VecDeque::with_capacity(SAMPLE_WINDOW),
            hist: vec![0; hist_size],
        }
    }

    fn update(&mut self, status: u16, ms: u64, bucket: usize) {
        self.hits = self.hits.saturating_add(1);
        if status >= 400 {
            self.errors = self.errors.saturating_add(1);
        }
        let value = ms as f64;
        self.ewma_ms = if self.ewma_ms == 0.0 {
            value
        } else {
            (1.0 - EWMA_ALPHA) * self.ewma_ms + EWMA_ALPHA * value
        };
        self.max_ms = self.max_ms.max(ms);
        if self.sample.len() >= SAMPLE_WINDOW {
            self.sample.pop_front();
        }
        self.sample.push_back(ms);
        if let Some(bin) = self.hist.get_mut(bucket) {
            *bin = bin.saturating_add(1);
        }
        if !self.sample.is_empty() {
            let mut tmp: Vec<u64> = self.sample.iter().copied().collect();
            tmp.sort_unstable();
            let idx = ((tmp.len() as f64) * 0.95).ceil() as usize;
            let idx = idx.saturating_sub(1).min(tmp.len() - 1);
            self.p95_ms = tmp[idx];
        }
    }

    fn summary(&self) -> RouteSummary {
        RouteSummary {
            hits: self.hits,
            errors: self.errors,
            ewma_ms: (self.ewma_ms * 10.0).round() / 10.0,
            p95_ms: self.p95_ms,
            max_ms: self.max_ms,
        }
    }
}

#[derive(Default)]
struct RouteStats {
    by_path: BTreeMap<String, RouteStat>,
}

pub struct Metrics {
    events: Mutex<EventStats>,
    routes: Mutex<RouteStats>,
    hist_buckets: Vec<u64>,
}

impl Default for Metrics {
    fn default() -> Self {
        Self::new()
    }
}

impl Metrics {
    pub fn new() -> Self {
        let hist_buckets = std::env::var("ARW_ROUTE_HIST_MS")
            .ok()
            .and_then(|s| {
                let mut buckets: Vec<u64> = s
                    .split(',')
                    .filter_map(|t| t.trim().parse::<u64>().ok())
                    .collect();
                if buckets.is_empty() {
                    None
                } else {
                    buckets.sort_unstable();
                    buckets.dedup();
                    Some(buckets)
                }
            })
            .unwrap_or_else(|| vec![5, 10, 25, 50, 100, 200, 500, 1000, 2000, 5000, 10000]);
        Self {
            events: Mutex::new(EventStats::new()),
            routes: Mutex::new(RouteStats::default()),
            hist_buckets,
        }
    }

    fn hist_index(&self, ms: u64) -> usize {
        for (idx, bound) in self.hist_buckets.iter().enumerate() {
            if ms <= *bound {
                return idx;
            }
        }
        self.hist_buckets.len()
    }

    pub fn record_event(&self, kind: &str) {
        if let Ok(mut stats) = self.events.lock() {
            stats.record(kind);
        }
    }

    pub fn record_route(&self, path: &str, status: u16, ms: u64) {
        let bucket = self.hist_index(ms);
        if let Ok(mut stats) = self.routes.lock() {
            let entry = stats
                .by_path
                .entry(path.to_string())
                .or_insert_with(|| RouteStat::new(self.hist_buckets.len() + 1));
            entry.update(status, ms, bucket);
        }
    }

    pub fn snapshot(&self) -> MetricsSummary {
        let events = self
            .events
            .lock()
            .map(|stats| stats.summary())
            .unwrap_or_else(|_| EventStats::new().summary());
        let routes = self
            .routes
            .lock()
            .map(|stats| {
                let mut out = BTreeMap::new();
                for (path, stat) in stats.by_path.iter() {
                    out.insert(path.clone(), stat.summary());
                }
                RoutesSummary { by_path: out }
            })
            .unwrap_or_default();
        MetricsSummary { events, routes }
    }
}

pub async fn track_http(
    metrics: Arc<Metrics>,
    req: Request<axum::body::Body>,
    next: Next,
) -> Response {
    let path = req
        .extensions()
        .get::<MatchedPath>()
        .map(|m| m.as_str().to_string())
        .unwrap_or_else(|| req.uri().path().to_string());
    let started = Instant::now();
    let mut res = next.run(req).await;
    let elapsed_ms = started.elapsed().as_millis() as u64;
    let status = res.status().as_u16();
    metrics.record_route(&path, status, elapsed_ms);
    let name = HeaderName::from_static("server-timing");
    if !res.headers().contains_key(&name) {
        if let Ok(value) = HeaderValue::from_str(&format!("total;dur={}", elapsed_ms)) {
            res.headers_mut().insert(name, value);
        }
    }
    res
}
