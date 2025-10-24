use axum::extract::MatchedPath;
use axum::http::{HeaderName, HeaderValue, Request};
use axum::middleware::Next;
use axum::response::Response;
use serde::Serialize;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use arw_events::BusStats;

use crate::tool_cache::ToolCacheStats;

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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub latency_histogram: Option<RouteHistogram>,
}

#[derive(Clone, Serialize, Default)]
pub struct RoutesSummary {
    pub by_path: BTreeMap<String, RouteSummary>,
}

#[derive(Clone, Serialize, Default)]
pub struct RouteHistogram {
    pub sum_ms: f64,
    pub count: u64,
    pub buckets: Vec<RouteHistogramBucket>,
}

#[derive(Clone, Serialize)]
pub struct RouteHistogramBucket {
    pub le_ms: Option<f64>,
    pub count: u64,
}

#[derive(Default)]
struct CompressionPromptCounters {
    requests: u64,
    successes: u64,
    errors: u64,
    primary: u64,
    fallback: u64,
    sum_latency_ms: f64,
    sum_ratio: f64,
    sum_pre_chars: f64,
    sum_post_chars: f64,
    sum_pre_bytes: f64,
    sum_post_bytes: f64,
}

pub struct CompressionPromptSample {
    pub latency_ms: f64,
    pub ratio: f64,
    pub pre_chars: u64,
    pub post_chars: u64,
    pub pre_bytes: u64,
    pub post_bytes: u64,
    pub fallback: bool,
}

impl CompressionPromptCounters {
    fn record_success(&mut self, sample: &CompressionPromptSample) {
        self.requests = self.requests.saturating_add(1);
        self.successes = self.successes.saturating_add(1);
        if sample.fallback {
            self.fallback = self.fallback.saturating_add(1);
        } else {
            self.primary = self.primary.saturating_add(1);
        }
        self.sum_latency_ms += sample.latency_ms;
        self.sum_ratio += sample.ratio;
        self.sum_pre_chars += sample.pre_chars as f64;
        self.sum_post_chars += sample.post_chars as f64;
        self.sum_pre_bytes += sample.pre_bytes as f64;
        self.sum_post_bytes += sample.post_bytes as f64;
    }

    fn record_error(&mut self) {
        self.requests = self.requests.saturating_add(1);
        self.errors = self.errors.saturating_add(1);
    }

    fn summary(&self) -> CompressionPromptSummary {
        if self.successes == 0 {
            return CompressionPromptSummary {
                requests: self.requests,
                successes: self.successes,
                errors: self.errors,
                primary: self.primary,
                fallback: self.fallback,
                avg_latency_ms: None,
                avg_ratio: None,
                avg_pre_chars: None,
                avg_post_chars: None,
                avg_pre_bytes: None,
                avg_post_bytes: None,
            };
        }
        let successes = self.successes as f64;
        let avg = |sum: f64| Some(sum / successes);
        CompressionPromptSummary {
            requests: self.requests,
            successes: self.successes,
            errors: self.errors,
            primary: self.primary,
            fallback: self.fallback,
            avg_latency_ms: avg(self.sum_latency_ms),
            avg_ratio: avg(self.sum_ratio),
            avg_pre_chars: avg(self.sum_pre_chars),
            avg_post_chars: avg(self.sum_post_chars),
            avg_pre_bytes: avg(self.sum_pre_bytes),
            avg_post_bytes: avg(self.sum_post_bytes),
        }
    }
}

#[derive(Default)]
struct PlanCounters {
    total: u64,
    last_target_tokens: Option<u32>,
    last_engine: Option<String>,
    mode_counts: BTreeMap<String, u64>,
    kv_policy_counts: BTreeMap<String, u64>,
    guard_failures: u64,
}

#[derive(Clone)]
pub struct PlanMetricsSample {
    pub target_tokens: u32,
    pub engine: String,
    pub applied_modes: Vec<String>,
    pub kv_policy: Option<String>,
    pub guard_failures: Option<u8>,
}

impl PlanCounters {
    fn record(&mut self, sample: &PlanMetricsSample) {
        self.total = self.total.saturating_add(1);
        self.last_target_tokens = Some(sample.target_tokens);
        self.last_engine = Some(sample.engine.clone());
        for mode in &sample.applied_modes {
            *self.mode_counts.entry(mode.clone()).or_default() += 1;
        }
        if let Some(policy) = &sample.kv_policy {
            *self.kv_policy_counts.entry(policy.clone()).or_default() += 1;
        }
        if let Some(failures) = sample.guard_failures {
            if failures > 0 {
                self.guard_failures = self.guard_failures.saturating_add(failures as u64);
            }
        }
    }

    fn summary(&self) -> PlanSummary {
        PlanSummary {
            total: self.total,
            last_target_tokens: self.last_target_tokens,
            last_engine: self.last_engine.clone(),
            mode_counts: self.mode_counts.clone(),
            kv_policy_counts: self.kv_policy_counts.clone(),
            guard_failures: self.guard_failures,
        }
    }
}

#[derive(Clone, Serialize, Default)]
pub struct AutonomySummary {
    pub interrupts: BTreeMap<String, u64>,
}

#[derive(Clone, Serialize, Default)]
pub struct ModularSummary {
    pub agent_totals: BTreeMap<String, u64>,
    pub tool_totals: BTreeMap<String, u64>,
}

#[derive(Clone, Serialize, Default)]
pub struct MemoryGcSummary {
    pub expired_total: u64,
    pub evicted_total: u64,
}

#[derive(Clone, Serialize, Default)]
pub struct MemoryEmbedBackfillSummary {
    pub total_updated: u64,
    pub runs: u64,
    pub last_batch: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pending_estimate: Option<u64>,
}

#[derive(Clone, Serialize, Default)]
pub struct PersonaTelemetrySummary {
    pub total: u64,
    pub by_persona: BTreeMap<String, PersonaTelemetryPersonaSummary>,
}

#[derive(Clone, Serialize, Default)]
pub struct PersonaTelemetryPersonaSummary {
    pub total: u64,
    pub by_signal: BTreeMap<String, u64>,
    #[serde(skip_serializing_if = "BTreeMap::is_empty", default)]
    pub signal_strength: BTreeMap<String, f32>,
    #[serde(skip_serializing_if = "BTreeMap::is_empty", default)]
    pub lane_priorities: BTreeMap<String, f32>,
    #[serde(skip_serializing_if = "BTreeMap::is_empty", default)]
    pub slot_overrides: BTreeMap<String, u64>,
}

#[derive(Clone, Serialize, Default)]
pub struct CompressionPromptSummary {
    pub requests: u64,
    pub successes: u64,
    pub errors: u64,
    pub primary: u64,
    pub fallback: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub avg_latency_ms: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub avg_ratio: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub avg_pre_chars: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub avg_post_chars: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub avg_pre_bytes: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub avg_post_bytes: Option<f64>,
}

#[derive(Clone, Serialize, Default)]
pub struct CompressionSummary {
    pub prompt: CompressionPromptSummary,
}

#[derive(Clone, Serialize, Default)]
pub struct PlanSummary {
    pub total: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_target_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_engine: Option<String>,
    pub mode_counts: BTreeMap<String, u64>,
    pub kv_policy_counts: BTreeMap<String, u64>,
    pub guard_failures: u64,
}
#[derive(Clone, Serialize)]
pub struct MetricsSummary {
    pub events: EventsSummary,
    pub routes: RoutesSummary,
    pub tasks: BTreeMap<String, TaskStatus>,
    pub compatibility: CompatibilitySummary,
    pub memory_gc: MemoryGcSummary,
    pub memory_embed_backfill: MemoryEmbedBackfillSummary,
    pub autonomy: AutonomySummary,
    pub modular: ModularSummary,
    pub worker: WorkerSummary,
    pub egress: EgressSummary,
    pub persona: PersonaTelemetrySummary,
    pub compression: CompressionSummary,
    pub plan: PlanSummary,
}

#[derive(Clone, Serialize, Default)]
pub struct CompatibilitySummary {
    pub legacy_capsule_headers: u64,
}

#[derive(Default)]
struct RouteStat {
    hits: u64,
    errors: u64,
    ewma_ms: f64,
    max_ms: u64,
    hist: Vec<u64>,
    total_ms: u128,
}

impl RouteStat {
    fn new(hist_size: usize) -> Self {
        Self {
            hits: 0,
            errors: 0,
            ewma_ms: 0.0,
            max_ms: 0,
            hist: vec![0; hist_size],
            total_ms: 0,
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
        if let Some(bin) = self.hist.get_mut(bucket) {
            *bin = bin.saturating_add(1);
        }
        self.total_ms = self.total_ms.saturating_add(ms as u128);
    }

    fn summary(&self, bounds_ms: &[u64]) -> RouteSummary {
        let histogram = if self.hits == 0 {
            None
        } else {
            let mut cumulative = 0u64;
            let mut buckets = Vec::with_capacity(self.hist.len());
            for (idx, count) in self.hist.iter().enumerate() {
                cumulative = cumulative.saturating_add(*count);
                let le_ms = if idx < bounds_ms.len() {
                    Some(bounds_ms[idx] as f64)
                } else {
                    None
                };
                buckets.push(RouteHistogramBucket {
                    le_ms,
                    count: cumulative,
                });
            }
            Some(RouteHistogram {
                sum_ms: self.total_ms as f64,
                count: self.hits,
                buckets,
            })
        };
        let p95 = self.percentile_from_hist(0.95, bounds_ms);
        RouteSummary {
            hits: self.hits,
            errors: self.errors,
            ewma_ms: (self.ewma_ms * 10.0).round() / 10.0,
            p95_ms: p95,
            max_ms: self.max_ms,
            latency_histogram: histogram,
        }
    }

    fn percentile_from_hist(&self, percentile: f64, bounds_ms: &[u64]) -> u64 {
        if self.hits == 0 {
            return 0;
        }
        let percentile = percentile.clamp(0.0, 1.0);
        let rank = ((self.hits as f64) * percentile).ceil().max(1.0) as u64;
        let mut cumulative = 0u64;
        for (idx, count) in self.hist.iter().enumerate() {
            cumulative = cumulative.saturating_add(*count);
            if cumulative >= rank {
                return if idx < bounds_ms.len() {
                    bounds_ms[idx]
                } else {
                    self.max_ms
                };
            }
        }
        self.max_ms
    }
}

#[derive(Default)]
struct RouteStats {
    by_path: BTreeMap<String, RouteStat>,
}

#[derive(Clone, Serialize, Default)]
pub struct TaskStatus {
    pub started: u64,
    pub completed: u64,
    pub aborted: u64,
    pub inflight: u64,
    pub last_start: Option<String>,
    pub last_stop: Option<String>,
    pub restarts_window: u64,
}

#[derive(Default, Clone)]
struct TaskStat {
    started: u64,
    completed: u64,
    aborted: u64,
    inflight: u64,
    last_start: Option<String>,
    last_stop: Option<String>,
    restarts_window: u64,
}

struct MemoryGcCounters {
    expired: AtomicU64,
    evicted: AtomicU64,
}

impl Default for MemoryGcCounters {
    fn default() -> Self {
        Self {
            expired: AtomicU64::new(0),
            evicted: AtomicU64::new(0),
        }
    }
}

impl MemoryGcCounters {
    fn record(&self, expired: u64, evicted: u64) {
        if expired > 0 {
            self.expired.fetch_add(expired, Ordering::Relaxed);
        }
        if evicted > 0 {
            self.evicted.fetch_add(evicted, Ordering::Relaxed);
        }
    }

    fn snapshot(&self) -> MemoryGcSummary {
        MemoryGcSummary {
            expired_total: self.expired.load(Ordering::Relaxed),
            evicted_total: self.evicted.load(Ordering::Relaxed),
        }
    }
}

#[derive(Default)]
struct MemoryEmbedBackfillCounters {
    total_updated: u64,
    runs: u64,
    last_batch: u64,
    last_at: Option<String>,
    last_error: Option<String>,
    pending_estimate: Option<u64>,
}

impl MemoryEmbedBackfillCounters {
    fn record_success(&mut self, updated: u64, pending: Option<u64>) {
        self.runs = self.runs.saturating_add(1);
        self.total_updated = self.total_updated.saturating_add(updated);
        self.last_batch = updated;
        self.last_at = Some(now_rfc3339());
        self.last_error = None;
        if pending.is_some() {
            self.pending_estimate = pending;
        }
    }

    fn record_error(&mut self, err: &str) {
        let truncated: String = err.chars().take(256).collect();
        self.last_error = Some(truncated);
        self.last_at = Some(now_rfc3339());
    }

    fn snapshot(&self) -> MemoryEmbedBackfillSummary {
        MemoryEmbedBackfillSummary {
            total_updated: self.total_updated,
            runs: self.runs,
            last_batch: self.last_batch,
            last_at: self.last_at.clone(),
            last_error: self.last_error.clone(),
            pending_estimate: self.pending_estimate,
        }
    }
}

#[derive(Default)]
struct PersonaTelemetryCounters {
    total: u64,
    by_persona: BTreeMap<String, PersonaTelemetryPersonaCounters>,
}

#[derive(Default)]
struct PersonaTelemetryPersonaCounters {
    total: u64,
    by_signal: BTreeMap<String, u64>,
    strength_sum: BTreeMap<String, f64>,
    strength_count: BTreeMap<String, u64>,
    lane_priorities: BTreeMap<String, f32>,
    slot_overrides: BTreeMap<String, u64>,
}

impl PersonaTelemetryCounters {
    fn record(&mut self, persona_id: &str, signal: &str, strength: Option<f32>) {
        let entry = self.by_persona.entry(persona_id.to_string()).or_default();
        entry.total = entry.total.saturating_add(1);
        let label = if signal.is_empty() {
            "unspecified".to_string()
        } else {
            signal.to_string()
        };
        *entry.by_signal.entry(label.clone()).or_default() += 1;
        if let Some(value) = strength {
            let clamped = value.clamp(0.0, 1.0) as f64;
            *entry.strength_sum.entry(label.clone()).or_default() += clamped;
            *entry.strength_count.entry(label.clone()).or_default() += 1;
        }
        self.total = self.total.saturating_add(1);
    }

    fn record_bias(
        &mut self,
        persona_id: &str,
        lanes: &BTreeMap<String, f32>,
        slots: &BTreeMap<String, usize>,
    ) {
        let entry = self.by_persona.entry(persona_id.to_string()).or_default();
        entry.lane_priorities = lanes
            .iter()
            .map(|(lane, value)| (lane.clone(), (*value).clamp(-1.0, 1.0)))
            .collect();
        entry.slot_overrides = slots
            .iter()
            .map(|(slot, count)| (slot.clone(), *count as u64))
            .collect();
    }

    fn summary(&self) -> PersonaTelemetrySummary {
        let mut by_persona = BTreeMap::new();
        for (persona, counters) in &self.by_persona {
            let mut signal_strength = BTreeMap::new();
            for (signal, sum) in counters.strength_sum.iter() {
                if let Some(count) = counters.strength_count.get(signal) {
                    if *count > 0 {
                        signal_strength.insert(signal.clone(), (*sum / *count as f64) as f32);
                    }
                }
            }
            by_persona.insert(
                persona.clone(),
                PersonaTelemetryPersonaSummary {
                    total: counters.total,
                    by_signal: counters.by_signal.clone(),
                    signal_strength,
                    lane_priorities: counters.lane_priorities.clone(),
                    slot_overrides: counters.slot_overrides.clone(),
                },
            );
        }
        PersonaTelemetrySummary {
            total: self.total,
            by_persona,
        }
    }
}

#[derive(Default)]
struct ModularCounters {
    agents: BTreeMap<String, u64>,
    tools: BTreeMap<String, u64>,
}

#[derive(Clone, Serialize, Default)]
pub struct WorkerSummary {
    pub configured: u64,
    pub busy: u64,
    pub started: u64,
    pub completed: u64,
    pub queue_depth: u64,
}

#[derive(Clone, Serialize, Default)]
pub struct EgressSummary {
    pub minted_total: u64,
    pub refreshed_total: u64,
    pub scope_leases: BTreeMap<String, ScopeLeaseSummary>,
}

#[derive(Clone, Serialize, Default)]
pub struct ScopeLeaseSummary {
    pub minted: u64,
    pub refreshed: u64,
    pub last_capability: Option<String>,
    pub last_reason: Option<String>,
    pub last_ttl_until: Option<String>,
    pub last_minted_at: Option<String>,
    pub last_minted_epoch: Option<i64>,
}

impl ModularCounters {
    fn record_agent(&mut self, agent: &str) {
        *self.agents.entry(agent.to_string()).or_default() += 1;
    }

    fn record_tool(&mut self, tool: &str) {
        *self.tools.entry(tool.to_string()).or_default() += 1;
    }

    fn snapshot(&self) -> ModularSummary {
        ModularSummary {
            agent_totals: self.agents.clone(),
            tool_totals: self.tools.clone(),
        }
    }
}

impl TaskStat {
    fn on_start(&mut self) {
        self.started = self.started.saturating_add(1);
        self.inflight = self.inflight.saturating_add(1);
        self.last_start = Some(now_rfc3339());
    }

    fn on_finish(&mut self, outcome: TaskOutcome) {
        if self.inflight > 0 {
            self.inflight -= 1;
        }
        match outcome {
            TaskOutcome::Completed => {
                self.completed = self.completed.saturating_add(1);
            }
            TaskOutcome::Aborted => {
                self.aborted = self.aborted.saturating_add(1);
            }
        }
        self.last_stop = Some(now_rfc3339());
    }

    fn summary(&self) -> TaskStatus {
        TaskStatus {
            started: self.started,
            completed: self.completed,
            aborted: self.aborted,
            inflight: self.inflight,
            last_start: self.last_start.clone(),
            last_stop: self.last_stop.clone(),
            restarts_window: self.restarts_window,
        }
    }
}

enum TaskOutcome {
    Completed,
    Aborted,
}

#[derive(Default, Clone)]
struct ScopeLeaseCounters {
    minted: u64,
    refreshed: u64,
    last_capability: Option<String>,
    last_reason: Option<String>,
    last_ttl_until: Option<String>,
    last_minted_at: Option<String>,
    last_minted_epoch: Option<i64>,
}

#[derive(Default)]
struct EgressCounters {
    minted_total: u64,
    refreshed_total: u64,
    by_scope: BTreeMap<String, ScopeLeaseCounters>,
}

impl ScopeLeaseCounters {
    fn update(
        &mut self,
        capability: &str,
        ttl_until: Option<&str>,
        reason: &str,
        minted_at_iso: String,
        minted_at_epoch: i64,
        refreshed: bool,
    ) {
        if refreshed {
            self.refreshed = self.refreshed.saturating_add(1);
        } else {
            self.minted = self.minted.saturating_add(1);
        }
        self.last_capability = Some(capability.to_string());
        self.last_reason = Some(reason.to_string());
        self.last_ttl_until = ttl_until.map(|s| s.to_string());
        self.last_minted_at = Some(minted_at_iso);
        self.last_minted_epoch = Some(minted_at_epoch);
    }

    fn summary(&self) -> ScopeLeaseSummary {
        ScopeLeaseSummary {
            minted: self.minted,
            refreshed: self.refreshed,
            last_capability: self.last_capability.clone(),
            last_reason: self.last_reason.clone(),
            last_ttl_until: self.last_ttl_until.clone(),
            last_minted_at: self.last_minted_at.clone(),
            last_minted_epoch: self.last_minted_epoch,
        }
    }
}

impl EgressCounters {
    fn record(
        &mut self,
        scope: Option<&str>,
        capability: &str,
        ttl_until: Option<&str>,
        refreshed: bool,
    ) {
        let scope_label = scope
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .unwrap_or("(unknown)");
        let now = chrono::Utc::now();
        let now_iso = now.to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
        let now_epoch = now.timestamp();
        let reason = if refreshed { "refresh" } else { "mint" };

        let entry = self.by_scope.entry(scope_label.to_string()).or_default();
        entry.update(capability, ttl_until, reason, now_iso, now_epoch, refreshed);

        if refreshed {
            self.refreshed_total = self.refreshed_total.saturating_add(1);
        } else {
            self.minted_total = self.minted_total.saturating_add(1);
        }
    }

    fn summary(&self) -> EgressSummary {
        let mut scopes = BTreeMap::new();
        for (scope, counters) in self.by_scope.iter() {
            scopes.insert(scope.clone(), counters.summary());
        }
        EgressSummary {
            minted_total: self.minted_total,
            refreshed_total: self.refreshed_total,
            scope_leases: scopes,
        }
    }
}

pub struct Metrics {
    events: Mutex<EventStats>,
    routes: Mutex<RouteStats>,
    hist_buckets: Vec<u64>,
    tasks: Mutex<BTreeMap<String, TaskStat>>,
    tasks_version: AtomicU64,
    routes_version: AtomicU64,
    legacy_capsule_headers: AtomicU64,
    memory_gc: MemoryGcCounters,
    memory_embed_backfill: Mutex<MemoryEmbedBackfillCounters>,
    autonomy_interrupts: Mutex<BTreeMap<String, u64>>,
    modular: Mutex<ModularCounters>,
    egress: Mutex<EgressCounters>,
    persona_telemetry: Mutex<PersonaTelemetryCounters>,
    compression_prompt: Mutex<CompressionPromptCounters>,
    plan: Mutex<PlanCounters>,
    worker_configured: AtomicU64,
    worker_busy: AtomicU64,
    worker_started: AtomicU64,
    worker_completed: AtomicU64,
    queue_depth: AtomicU64,
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
            tasks: Mutex::new(BTreeMap::new()),
            tasks_version: AtomicU64::new(0),
            routes_version: AtomicU64::new(0),
            legacy_capsule_headers: AtomicU64::new(0),
            memory_gc: MemoryGcCounters::default(),
            memory_embed_backfill: Mutex::new(MemoryEmbedBackfillCounters::default()),
            autonomy_interrupts: Mutex::new(BTreeMap::new()),
            modular: Mutex::new(ModularCounters::default()),
            egress: Mutex::new(EgressCounters::default()),
            persona_telemetry: Mutex::new(PersonaTelemetryCounters::default()),
            compression_prompt: Mutex::new(CompressionPromptCounters::default()),
            plan: Mutex::new(PlanCounters::default()),
            worker_configured: AtomicU64::new(0),
            worker_busy: AtomicU64::new(0),
            worker_started: AtomicU64::new(0),
            worker_completed: AtomicU64::new(0),
            queue_depth: AtomicU64::new(0),
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
        self.routes_version.fetch_add(1, Ordering::Relaxed);
    }

    pub fn record_scope_lease_mint(
        &self,
        scope: Option<&str>,
        capability: &str,
        ttl_until: Option<&str>,
        refreshed: bool,
    ) {
        if let Ok(mut counters) = self.egress.lock() {
            counters.record(scope, capability, ttl_until, refreshed);
        }
    }

    pub fn record_persona_feedback(
        &self,
        persona_id: &str,
        signal: Option<&str>,
        strength: Option<f32>,
    ) {
        let label = signal.filter(|s| !s.is_empty()).unwrap_or("unspecified");
        if let Ok(mut counters) = self.persona_telemetry.lock() {
            counters.record(persona_id, label, strength);
        }
    }

    pub fn record_persona_bias(
        &self,
        persona_id: &str,
        lanes: &BTreeMap<String, f32>,
        slots: &BTreeMap<String, usize>,
    ) {
        if let Ok(mut counters) = self.persona_telemetry.lock() {
            counters.record_bias(persona_id, lanes, slots);
        }
    }

    pub fn record_prompt_compression_success(&self, sample: CompressionPromptSample) {
        if let Ok(mut counters) = self.compression_prompt.lock() {
            counters.record_success(&sample);
        }
    }

    pub fn record_prompt_compression_error(&self) {
        if let Ok(mut counters) = self.compression_prompt.lock() {
            counters.record_error();
        }
    }

    pub fn record_plan_sample(&self, sample: PlanMetricsSample) {
        if let Ok(mut counters) = self.plan.lock() {
            counters.record(&sample);
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
                    out.insert(path.clone(), stat.summary(&self.hist_buckets));
                }
                RoutesSummary { by_path: out }
            })
            .unwrap_or_default();
        let tasks = self.tasks_snapshot();
        let compatibility = CompatibilitySummary {
            legacy_capsule_headers: self.legacy_capsule_headers.load(Ordering::Relaxed),
        };
        let memory_gc = self.memory_gc.snapshot();
        let memory_embed_backfill = self
            .memory_embed_backfill
            .lock()
            .map(|counters| counters.snapshot())
            .unwrap_or_default();
        let autonomy = self
            .autonomy_interrupts
            .lock()
            .map(|map| AutonomySummary {
                interrupts: map.clone(),
            })
            .unwrap_or_default();
        let modular = self
            .modular
            .lock()
            .map(|counters| counters.snapshot())
            .unwrap_or_default();
        let worker = WorkerSummary {
            configured: self.worker_configured.load(Ordering::Relaxed),
            busy: self.worker_busy.load(Ordering::Relaxed),
            started: self.worker_started.load(Ordering::Relaxed),
            completed: self.worker_completed.load(Ordering::Relaxed),
            queue_depth: self.queue_depth.load(Ordering::Relaxed),
        };
        let egress = self
            .egress
            .lock()
            .map(|counters| counters.summary())
            .unwrap_or_default();
        let persona = self
            .persona_telemetry
            .lock()
            .map(|counters| counters.summary())
            .unwrap_or_default();
        let compression = self
            .compression_prompt
            .lock()
            .map(|counters| CompressionSummary {
                prompt: counters.summary(),
            })
            .unwrap_or_default();
        let plan = self
            .plan
            .lock()
            .map(|counters| counters.summary())
            .unwrap_or_default();
        MetricsSummary {
            events,
            routes,
            tasks,
            compatibility,
            memory_gc,
            memory_embed_backfill,
            autonomy,
            modular,
            worker,
            egress,
            persona,
            compression,
            plan,
        }
    }

    pub fn egress_summary(&self) -> EgressSummary {
        self.egress
            .lock()
            .map(|counters| counters.summary())
            .unwrap_or_default()
    }

    pub fn routes_version(&self) -> u64 {
        self.routes_version.load(Ordering::Relaxed)
    }

    pub fn task_started(&self, name: &str) {
        if let Ok(mut map) = self.tasks.lock() {
            map.entry(name.to_string()).or_default().on_start();
        }
        self.tasks_version.fetch_add(1, Ordering::Relaxed);
    }

    pub fn task_completed(&self, name: &str) {
        self.record_task_outcome(name, TaskOutcome::Completed);
    }

    pub fn task_aborted(&self, name: &str) {
        self.record_task_outcome(name, TaskOutcome::Aborted);
    }

    pub fn task_restarts_window_set(&self, name: &str, count: u64) {
        if let Ok(mut map) = self.tasks.lock() {
            map.entry(name.to_string()).or_default().restarts_window = count;
        }
    }

    fn record_task_outcome(&self, name: &str, outcome: TaskOutcome) {
        if let Ok(mut map) = self.tasks.lock() {
            map.entry(name.to_string()).or_default().on_finish(outcome);
        }
        self.tasks_version.fetch_add(1, Ordering::Relaxed);
    }

    pub fn tasks_snapshot(&self) -> BTreeMap<String, TaskStatus> {
        self.tasks
            .lock()
            .map(|map| {
                map.iter()
                    .map(|(name, stat)| (name.clone(), stat.summary()))
                    .collect()
            })
            .unwrap_or_default()
    }

    pub fn tasks_snapshot_with_version(&self) -> (u64, BTreeMap<String, TaskStatus>) {
        let version = self.tasks_version.load(Ordering::Relaxed);
        let items = self.tasks_snapshot();
        (version, items)
    }

    pub fn record_legacy_capsule_header(&self) {
        self.legacy_capsule_headers.fetch_add(1, Ordering::Relaxed);
    }

    pub fn record_memory_gc(&self, expired: u64, evicted: u64) {
        if expired == 0 && evicted == 0 {
            return;
        }
        self.memory_gc.record(expired, evicted);
    }

    pub fn record_embed_backfill(&self, batch_updated: u64, pending: Option<u64>) {
        if let Ok(mut counters) = self.memory_embed_backfill.lock() {
            counters.record_success(batch_updated, pending);
        }
    }

    pub fn record_embed_backfill_error(&self, err: &str) {
        if let Ok(mut counters) = self.memory_embed_backfill.lock() {
            counters.record_error(err);
        }
    }

    pub fn event_kind_count(&self, kind: &str) -> u64 {
        self.events
            .lock()
            .map(|stats| stats.kinds.get(kind).copied().unwrap_or(0))
            .unwrap_or(0)
    }

    pub fn record_autonomy_interrupt(&self, reason: &str) {
        let key = if reason.is_empty() { "unknown" } else { reason };
        if let Ok(mut map) = self.autonomy_interrupts.lock() {
            *map.entry(key.to_string()).or_default() += 1;
        }
    }

    pub fn record_modular_agent(&self, agent_id: &str) {
        if let Ok(mut counters) = self.modular.lock() {
            counters.record_agent(agent_id);
        }
    }

    pub fn record_modular_tool(&self, tool_id: &str) {
        if let Ok(mut counters) = self.modular.lock() {
            counters.record_tool(tool_id);
        }
    }

    pub fn set_worker_configured(&self, count: u64) {
        self.worker_configured.store(count, Ordering::Relaxed);
    }

    pub fn worker_job_started(&self) {
        self.worker_started.fetch_add(1, Ordering::Relaxed);
        self.worker_busy.fetch_add(1, Ordering::Relaxed);
    }

    pub fn worker_job_finished(&self) {
        self.worker_completed.fetch_add(1, Ordering::Relaxed);
        let _ = self
            .worker_busy
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |current| {
                Some(current.saturating_sub(1))
            });
    }

    pub fn queue_enqueued(&self) {
        self.queue_depth.fetch_add(1, Ordering::Relaxed);
    }

    pub fn queue_dequeued(&self) {
        let _ = self
            .queue_depth
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |current| {
                Some(current.saturating_sub(1))
            });
    }

    pub fn queue_reset(&self, depth: u64) {
        self.queue_depth.store(depth, Ordering::Relaxed);
    }
}

pub fn route_stats_snapshot(
    summary: &MetricsSummary,
    bus: &BusStats,
    cache: &ToolCacheStats,
) -> Value {
    json!({
        "bus": {
            "published": bus.published,
            "delivered": bus.delivered,
            "receivers": bus.receivers,
            "lagged": bus.lagged,
            "no_receivers": bus.no_receivers,
        },
        "events": summary.events,
        "routes": summary.routes,
        "tasks": summary.tasks,
        "cache": cache_stats_snapshot(cache),
        "memory_gc": summary.memory_gc,
        "memory_embed_backfill": summary.memory_embed_backfill,
        "autonomy": summary.autonomy,
        "modular": summary.modular,
        "worker": summary.worker,
        "persona": summary.persona,
    })
}

pub fn cache_stats_snapshot(cache: &ToolCacheStats) -> Value {
    json!({
        "hit": cache.hit,
        "miss": cache.miss,
        "coalesced": cache.coalesced,
        "errors": cache.errors,
        "bypass": cache.bypass,
        "payload_too_large": cache.payload_too_large,
        "capacity": cache.capacity,
        "ttl_secs": cache.ttl_secs,
        "entries": cache.entries,
        "max_payload_bytes": cache.max_payload_bytes,
        "latency_saved_ms_total": cache.latency_saved_ms_total,
        "latency_saved_samples": cache.latency_saved_samples,
        "avg_latency_saved_ms": cache.avg_latency_saved_ms,
        "payload_bytes_saved_total": cache.payload_bytes_saved_total,
        "payload_saved_samples": cache.payload_saved_samples,
        "avg_payload_bytes_saved": cache.avg_payload_bytes_saved,
        "avg_hit_age_secs": cache.avg_hit_age_secs,
        "hit_age_samples": cache.hit_age_samples,
        "last_hit_age_secs": cache.last_hit_age_secs,
        "max_hit_age_secs": cache.max_hit_age_secs,
        "stampede_suppression_rate": cache.stampede_suppression_rate,
        "last_latency_saved_ms": cache.last_latency_saved_ms,
        "last_payload_bytes": cache.last_payload_bytes,
    })
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compression_prompt_counters_success_snapshot() {
        let mut counters = CompressionPromptCounters::default();
        let sample = CompressionPromptSample {
            latency_ms: 40.0,
            ratio: 0.55,
            pre_chars: 200,
            post_chars: 110,
            pre_bytes: 600,
            post_bytes: 330,
            fallback: false,
        };
        counters.record_success(&sample);
        let summary = counters.summary();

        assert_eq!(summary.requests, 1);
        assert_eq!(summary.successes, 1);
        assert_eq!(summary.errors, 0);
        assert_eq!(summary.primary, 1);
        assert_eq!(summary.fallback, 0);
        assert_eq!(summary.avg_latency_ms, Some(40.0));
        assert_eq!(summary.avg_ratio, Some(0.55));
        assert_eq!(summary.avg_pre_chars, Some(200.0));
        assert_eq!(summary.avg_post_chars, Some(110.0));
        assert_eq!(summary.avg_pre_bytes, Some(600.0));
        assert_eq!(summary.avg_post_bytes, Some(330.0));
    }

    #[test]
    fn compression_prompt_counters_errors_and_fallbacks() {
        let mut counters = CompressionPromptCounters::default();
        counters.record_success(&CompressionPromptSample {
            latency_ms: 10.0,
            ratio: 0.9,
            pre_chars: 180,
            post_chars: 162,
            pre_bytes: 512,
            post_bytes: 460,
            fallback: true,
        });
        counters.record_error();
        counters.record_error();

        let summary = counters.summary();
        assert_eq!(summary.requests, 3); // 1 success + 2 errors
        assert_eq!(summary.successes, 1);
        assert_eq!(summary.errors, 2);
        assert_eq!(summary.primary, 0);
        assert_eq!(summary.fallback, 1);
        assert_eq!(summary.avg_ratio, Some(0.9));
    }

    #[test]
    fn plan_counters_record_snapshot() {
        let metrics = Metrics::new();
        metrics.record_plan_sample(PlanMetricsSample {
            target_tokens: 1024,
            engine: "llama.cpp".into(),
            applied_modes: vec!["transclude".into(), "delta".into()],
            kv_policy: Some("snapkv".into()),
            guard_failures: Some(1),
        });

        let summary = metrics.snapshot();
        assert_eq!(summary.plan.total, 1);
        assert_eq!(summary.plan.last_target_tokens, Some(1024));
        assert_eq!(summary.plan.last_engine.as_deref(), Some("llama.cpp"));
        assert_eq!(summary.plan.mode_counts.get("transclude"), Some(&1u64));
        assert_eq!(summary.plan.kv_policy_counts.get("snapkv"), Some(&1u64));
        assert_eq!(summary.plan.guard_failures, 1);
    }

    #[test]
    fn route_summary_carries_histogram() {
        let metrics = Metrics::new();
        metrics.record_route("/foo", 200, 8);
        metrics.record_route("/foo", 200, 42);
        metrics.record_route("/foo", 200, 1200);

        let summary = metrics.snapshot();
        let route = summary
            .routes
            .by_path
            .get("/foo")
            .expect("missing route summary");

        let histogram = route
            .latency_histogram
            .as_ref()
            .expect("expected histogram data");
        assert_eq!(histogram.count, 3);
        assert!((histogram.sum_ms - 1250.0).abs() < 1e-6);

        let bucket_10 = histogram
            .buckets
            .iter()
            .find(|bucket| bucket.le_ms == Some(10.0))
            .expect("bucket <=10ms");
        assert_eq!(bucket_10.count, 1);

        let bucket_50 = histogram
            .buckets
            .iter()
            .find(|bucket| bucket.le_ms == Some(50.0))
            .expect("bucket <=50ms");
        assert_eq!(bucket_50.count, 2);

        let bucket_2000 = histogram
            .buckets
            .iter()
            .find(|bucket| bucket.le_ms == Some(2000.0))
            .expect("bucket <=2000ms");
        assert_eq!(bucket_2000.count, 3);

        let last_bucket = histogram.buckets.last().expect("expected terminal bucket");
        assert!(last_bucket.le_ms.is_none());
        assert_eq!(last_bucket.count, 3);
    }
}
