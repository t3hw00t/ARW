use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use super::DownloadTuning;

/// API-facing counters surfaced by the models service.
#[derive(Clone, Serialize, Deserialize, ToSchema, Default)]
pub struct ModelsMetricsCounters {
    pub started: u64,
    pub queued: u64,
    pub admitted: u64,
    pub resumed: u64,
    pub canceled: u64,
    pub completed: u64,
    pub completed_cached: u64,
    pub errors: u64,
    pub bytes_total: u64,
    pub ewma_mbps: Option<f64>,
    pub preflight_ok: u64,
    pub preflight_denied: u64,
    pub preflight_skipped: u64,
    pub coalesced: u64,
}

/// Snapshot of inflight model downloads keyed by checksum.
#[derive(Clone, Serialize, Deserialize, ToSchema, Default)]
pub struct ModelsInflightEntry {
    pub sha256: String,
    pub primary: String,
    #[serde(default)]
    pub followers: Vec<String>,
    pub count: u64,
}

/// Destination metadata for a model download job.
#[derive(Clone, Serialize, Deserialize, ToSchema, Default)]
pub struct ModelsJobDestination {
    pub host: String,
    pub port: u16,
    pub protocol: String,
}

/// Snapshot of an active download job.
#[derive(Clone, Serialize, Deserialize, ToSchema, Default)]
pub struct ModelsJobSnapshot {
    pub model_id: String,
    pub job_id: String,
    pub url: String,
    pub corr_id: String,
    pub dest: ModelsJobDestination,
    pub started_at: u64,
}

/// Concurrency limits reported to callers.
#[derive(Clone, Serialize, Deserialize, ToSchema, Default)]
pub struct ModelsConcurrencySnapshot {
    pub configured_max: u64,
    pub available_permits: u64,
    pub held_permits: u64,
    #[serde(default)]
    pub hard_cap: Option<u64>,
    #[serde(default)]
    pub pending_shrink: Option<u64>,
}

/// Runtime tuning knobs applied to download jobs.
#[derive(Clone, Serialize, Deserialize, ToSchema, Default)]
pub struct ModelsRuntimeConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub idle_timeout_secs: Option<u64>,
    pub send_retries: u32,
    pub stream_retries: u32,
    pub retry_backoff_ms: u64,
    pub preflight_enabled: bool,
}

/// Complete payload returned by the metrics endpoint.
#[derive(Clone, Serialize, Deserialize, ToSchema, Default)]
pub struct ModelsMetricsResponse {
    pub started: u64,
    pub queued: u64,
    pub admitted: u64,
    pub resumed: u64,
    pub canceled: u64,
    pub completed: u64,
    pub completed_cached: u64,
    pub errors: u64,
    pub bytes_total: u64,
    pub ewma_mbps: Option<f64>,
    pub preflight_ok: u64,
    pub preflight_denied: u64,
    pub preflight_skipped: u64,
    pub coalesced: u64,
    #[serde(default)]
    pub inflight: Vec<ModelsInflightEntry>,
    pub concurrency: ModelsConcurrencySnapshot,
    #[serde(default)]
    pub jobs: Vec<ModelsJobSnapshot>,
    #[serde(default)]
    pub runtime: ModelsRuntimeConfig,
}

impl ModelsMetricsResponse {
    pub(super) fn from_parts(
        counters: ModelsMetricsCounters,
        inflight: Vec<ModelsInflightEntry>,
        concurrency: ModelsConcurrencySnapshot,
        jobs: Vec<ModelsJobSnapshot>,
        runtime: ModelsRuntimeConfig,
    ) -> Self {
        Self {
            started: counters.started,
            queued: counters.queued,
            admitted: counters.admitted,
            resumed: counters.resumed,
            canceled: counters.canceled,
            completed: counters.completed,
            completed_cached: counters.completed_cached,
            errors: counters.errors,
            bytes_total: counters.bytes_total,
            ewma_mbps: counters.ewma_mbps,
            preflight_ok: counters.preflight_ok,
            preflight_denied: counters.preflight_denied,
            preflight_skipped: counters.preflight_skipped,
            coalesced: counters.coalesced,
            inflight,
            concurrency,
            jobs,
            runtime,
        }
    }
}

impl ModelsRuntimeConfig {
    pub(super) fn from_tuning(tuning: &DownloadTuning, preflight_enabled: bool) -> Self {
        Self {
            idle_timeout_secs: tuning.idle_timeout_secs(),
            send_retries: tuning.send_retries,
            stream_retries: tuning.stream_retries,
            retry_backoff_ms: tuning.retry_backoff_ms,
            preflight_enabled,
        }
    }
}
