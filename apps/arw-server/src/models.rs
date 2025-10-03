use arw_events::Bus;
use arw_topics as topics;
use chrono::{DateTime, Utc};
use fs2::available_space;
use futures_util::StreamExt;
use reqwest::{header, Client, StatusCode};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Number, Value};
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex as StdMutex};
use std::time::{Duration, Instant};
use tokio::fs;
use tokio::io::{AsyncReadExt, AsyncWriteExt, BufReader, BufWriter};
use tokio::sync::{Mutex, Notify, RwLock};
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;
use tracing::{debug, error, warn};

use crate::{http_timeout, read_models, util};
use once_cell::sync::OnceCell;
use utoipa::ToSchema;

const DEFAULT_CONCURRENCY: u64 = 2;
const METRIC_MANIFEST_INDEX_REBUILDS: &str = "models.manifest_index.rebuilds";
const GAUGE_MANIFEST_INDEX_ENTRIES: &str = "models.manifest_index.entries";
const DOWNLOAD_EVENT_KIND: &str = "models.download.progress";
const PROGRESS_EMIT_BYTES: u64 = 5 * 1024 * 1024; // 5 MiB
const PROGRESS_EMIT_INTERVAL: Duration = Duration::from_millis(750);

#[derive(Clone, Copy, Debug)]
struct DownloadTuning {
    idle_timeout: Option<Duration>,
    send_retries: u32,
    stream_retries: u32,
    retry_backoff_ms: u64,
}

impl DownloadTuning {
    fn from_env() -> Self {
        let idle_timeout = std::env::var("ARW_DL_IDLE_TIMEOUT_SECS")
            .ok()
            .and_then(|s| s.parse::<u64>().ok());
        let idle_timeout = match idle_timeout {
            Some(0) => None,
            Some(secs) => Some(Duration::from_secs(secs)),
            None => Some(Duration::from_secs(300)),
        };
        let send_retries = std::env::var("ARW_DL_SEND_RETRIES")
            .ok()
            .and_then(|s| s.parse::<u32>().ok())
            .unwrap_or(2);
        let stream_retries = std::env::var("ARW_DL_STREAM_RETRIES")
            .ok()
            .and_then(|s| s.parse::<u32>().ok())
            .unwrap_or(2);
        let retry_backoff_ms = std::env::var("ARW_DL_RETRY_BACKOFF_MS")
            .ok()
            .and_then(|s| s.parse::<u64>().ok())
            .unwrap_or(500)
            .clamp(50, 60_000);
        Self {
            idle_timeout,
            send_retries,
            stream_retries,
            retry_backoff_ms,
        }
    }

    fn idle_timeout_secs(&self) -> Option<u64> {
        self.idle_timeout.map(|d| d.as_secs())
    }

    fn backoff_delay(&self, attempt: u32) -> Duration {
        let step = attempt.max(1);
        let base = Duration::from_millis(self.retry_backoff_ms);
        base.checked_mul(step).unwrap_or(base)
    }
}

#[derive(Clone, Default)]
struct ConcurrencyState {
    configured_max: u64,
    hard_cap: Option<u64>,
}

impl ConcurrencyState {
    fn new() -> Self {
        let configured_max = std::env::var("ARW_MODELS_MAX_CONC")
            .ok()
            .and_then(|s| s.parse::<u64>().ok())
            .unwrap_or(DEFAULT_CONCURRENCY)
            .max(1);
        let hard_cap = std::env::var("ARW_MODELS_MAX_CONC_HARD")
            .ok()
            .and_then(|s| s.parse::<u64>().ok())
            .filter(|v| *v > 0);
        Self {
            configured_max,
            hard_cap,
        }
    }

    fn configured(&self) -> u64 {
        self.hard_cap
            .map(|cap| cap.min(self.configured_max))
            .unwrap_or(self.configured_max)
            .max(1)
    }
}

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

#[derive(Clone, Serialize, Deserialize, ToSchema, Default)]
pub struct ModelsInflightEntry {
    pub sha256: String,
    pub primary: String,
    #[serde(default)]
    pub followers: Vec<String>,
    pub count: u64,
}

#[derive(Clone, Serialize, Deserialize, ToSchema, Default)]
pub struct ModelsJobDestination {
    pub host: String,
    pub port: u16,
    pub protocol: String,
}

#[derive(Clone, Serialize, Deserialize, ToSchema, Default)]
pub struct ModelsJobSnapshot {
    pub model_id: String,
    pub job_id: String,
    pub url: String,
    pub corr_id: String,
    pub dest: ModelsJobDestination,
    pub started_at: u64,
}

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

#[derive(Clone, Serialize, Deserialize, ToSchema, Default)]
pub struct ModelsRuntimeConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub idle_timeout_secs: Option<u64>,
    pub send_retries: u32,
    pub stream_retries: u32,
    pub retry_backoff_ms: u64,
    pub preflight_enabled: bool,
}

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
    fn from_parts(
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
    fn from_tuning(tuning: &DownloadTuning, preflight_enabled: bool) -> Self {
        Self {
            idle_timeout_secs: tuning.idle_timeout_secs(),
            send_retries: tuning.send_retries,
            stream_retries: tuning.stream_retries,
            retry_backoff_ms: tuning.retry_backoff_ms,
            preflight_enabled,
        }
    }
}

#[derive(Clone, Default)]
struct MetricsState {
    started: u64,
    queued: u64,
    admitted: u64,
    resumed: u64,
    canceled: u64,
    completed: u64,
    completed_cached: u64,
    errors: u64,
    bytes_total: u64,
    ewma_mbps: Option<f64>,
    preflight_ok: u64,
    preflight_denied: u64,
    preflight_skipped: u64,
    coalesced: u64,
}

impl MetricsState {
    fn snapshot(&self) -> ModelsMetricsCounters {
        ModelsMetricsCounters {
            started: self.started,
            queued: self.queued,
            admitted: self.admitted,
            resumed: self.resumed,
            canceled: self.canceled,
            completed: self.completed,
            completed_cached: self.completed_cached,
            errors: self.errors,
            bytes_total: self.bytes_total,
            ewma_mbps: self.ewma_mbps,
            preflight_ok: self.preflight_ok,
            preflight_denied: self.preflight_denied,
            preflight_skipped: self.preflight_skipped,
            coalesced: self.coalesced,
        }
    }

    fn record_started(&mut self) {
        self.started = self.started.saturating_add(1);
        self.queued = self.queued.saturating_add(1);
    }

    fn record_admitted(&mut self) {
        if self.queued > 0 {
            self.queued -= 1;
        }
        self.admitted = self.admitted.saturating_add(1);
    }

    fn record_resumed(&mut self) {
        self.resumed = self.resumed.saturating_add(1);
    }

    fn record_completed(&mut self, bytes: u64, mbps: Option<f64>, cached: bool) {
        if cached {
            self.completed_cached = self.completed_cached.saturating_add(1);
        } else {
            self.completed = self.completed.saturating_add(1);
        }
        self.bytes_total = self.bytes_total.saturating_add(bytes);
        if let Some(speed) = mbps {
            self.ewma_mbps = Some(match self.ewma_mbps {
                Some(prev) => (prev * 0.6) + (speed * 0.4),
                None => speed,
            });
        }
    }

    fn record_error(&mut self) {
        self.errors = self.errors.saturating_add(1);
    }

    fn record_canceled(&mut self) {
        self.canceled = self.canceled.saturating_add(1);
    }

    fn record_preflight_ok(&mut self) {
        self.preflight_ok = self.preflight_ok.saturating_add(1);
    }

    fn record_preflight_denied(&mut self) {
        self.preflight_denied = self.preflight_denied.saturating_add(1);
    }

    fn record_preflight_skipped(&mut self) {
        self.preflight_skipped = self.preflight_skipped.saturating_add(1);
    }

    fn record_coalesced(&mut self) {
        self.coalesced = self.coalesced.saturating_add(1);
    }
}

#[derive(Clone)]
struct DestInfo {
    host: String,
    port: u16,
    protocol: String,
}

struct DownloadHandle {
    cancel: CancellationToken,
    task: Option<JoinHandle<()>>,
    job_id: String,
    url_display: String,
    corr_id: String,
    dest: DestInfo,
    started_at: Instant,
}

struct DownloadsState {
    jobs: Mutex<HashMap<String, DownloadHandle>>,
    notify: Notify,
}

impl DownloadsState {
    fn new() -> Self {
        Self {
            jobs: Mutex::new(HashMap::new()),
            notify: Notify::new(),
        }
    }

    async fn contains(&self, id: &str) -> bool {
        self.jobs.lock().await.contains_key(id)
    }

    async fn active_count(&self) -> usize {
        self.jobs.lock().await.len()
    }

    async fn wait_for_slot(&self, max: u64, cancel: &CancellationToken) -> Result<(), ()> {
        let max = max.max(1);
        loop {
            let current = self.active_count().await as u64;
            if current < max {
                return Ok(());
            }
            tokio::select! {
                _ = cancel.cancelled() => return Err(()),
                _ = self.notify.notified() => {}
            }
        }
    }

    async fn wait_until_at_most(&self, limit: u64) {
        let limit = limit.max(1);
        loop {
            let current = self.active_count().await as u64;
            if current <= limit {
                return;
            }
            self.notify.notified().await;
        }
    }

    async fn insert_job(&self, model_id: &str, handle: DownloadHandle) -> Result<(), ()> {
        let mut jobs = self.jobs.lock().await;
        if jobs.contains_key(model_id) {
            return Err(());
        }
        jobs.insert(model_id.to_string(), handle);
        Ok(())
    }

    async fn remove_job(&self, model_id: &str) -> Option<DownloadHandle> {
        let mut jobs = self.jobs.lock().await;
        let removed = jobs.remove(model_id);
        if removed.is_some() {
            self.notify.notify_waiters();
        }
        removed
    }

    async fn cancel_job(&self, model_id: &str) -> Option<(String, DestInfo)> {
        let handle = {
            let mut jobs = self.jobs.lock().await;
            jobs.remove(model_id)
        };
        if let Some(mut handle) = handle {
            let corr_id = handle.corr_id.clone();
            let dest = handle.dest.clone();
            handle.cancel.cancel();
            self.notify.notify_waiters();
            if let Some(task) = handle.task.take() {
                tokio::spawn(async move {
                    if let Err(err) = task.await {
                        warn!("cancelled download join err: {err}");
                    }
                });
            }
            Some((corr_id, dest))
        } else {
            None
        }
    }

    async fn job_snapshot(&self) -> Vec<ModelsJobSnapshot> {
        let jobs = self.jobs.lock().await;
        jobs.iter()
            .map(|(model_id, handle)| {
                let dest = &handle.dest;
                ModelsJobSnapshot {
                    model_id: model_id.clone(),
                    job_id: handle.job_id.clone(),
                    url: handle.url_display.clone(),
                    corr_id: handle.corr_id.clone(),
                    dest: ModelsJobDestination {
                        host: dest.host.clone(),
                        port: dest.port,
                        protocol: dest.protocol.clone(),
                    },
                    started_at: handle.started_at.elapsed().as_secs(),
                }
            })
            .collect()
    }
}

#[derive(Default)]
struct HashGuardEntry {
    primary: String,
    followers: HashSet<String>,
}

#[derive(Default)]
struct HashGuardState {
    by_sha: HashMap<String, HashGuardEntry>,
    model_to_sha: HashMap<String, String>,
}

enum HashGuardRole {
    Primary,
    Coalesced { primary: String },
}

#[derive(Debug, Clone, Default)]
struct PreflightInfo {
    content_length: Option<u64>,
    etag: Option<String>,
    last_modified: Option<String>,
}

enum PreflightError {
    Skip(String),
    Denied { code: String, message: String },
}

impl HashGuardState {
    fn register(&mut self, model_id: &str, sha: &str) -> HashGuardRole {
        match self.by_sha.get_mut(sha) {
            Some(entry) => {
                entry.followers.insert(model_id.to_string());
                self.model_to_sha
                    .insert(model_id.to_string(), sha.to_string());
                HashGuardRole::Coalesced {
                    primary: entry.primary.clone(),
                }
            }
            None => {
                let entry = HashGuardEntry {
                    primary: model_id.to_string(),
                    followers: HashSet::new(),
                };
                self.by_sha.insert(sha.to_string(), entry);
                self.model_to_sha
                    .insert(model_id.to_string(), sha.to_string());
                HashGuardRole::Primary
            }
        }
    }

    fn release_primary(&mut self, model_id: &str) -> Vec<String> {
        let Some(sha) = self.model_to_sha.remove(model_id) else {
            return Vec::new();
        };
        let Some(entry) = self.by_sha.remove(&sha) else {
            return Vec::new();
        };
        for follower in &entry.followers {
            self.model_to_sha.remove(follower);
        }
        entry.followers.into_iter().collect()
    }

    fn release_model(&mut self, model_id: &str) {
        let Some(sha) = self.model_to_sha.remove(model_id) else {
            return;
        };
        let mut remove_entry = false;
        if let Some(entry) = self.by_sha.get_mut(&sha) {
            entry.followers.remove(model_id);
            if entry.primary == model_id {
                if let Some(next_primary) = entry.followers.iter().next().cloned() {
                    entry.followers.remove(&next_primary);
                    entry.primary = next_primary;
                } else {
                    remove_entry = true;
                }
            }
        }
        if remove_entry {
            self.by_sha.remove(&sha);
        }
    }

    fn progress_targets(&self, model_id: &str) -> Vec<String> {
        let mut targets = vec![model_id.to_string()];
        if let Some(sha) = self.model_to_sha.get(model_id) {
            if let Some(entry) = self.by_sha.get(sha) {
                if entry.primary == model_id {
                    targets.extend(entry.followers.iter().cloned());
                }
            }
        }
        targets
    }

    fn inflight_snapshot(&self) -> Vec<ModelsInflightEntry> {
        self.by_sha
            .iter()
            .map(|(sha, entry)| ModelsInflightEntry {
                sha256: sha.clone(),
                primary: entry.primary.clone(),
                followers: entry.followers.iter().cloned().collect(),
                count: 1 + entry.followers.len() as u64,
            })
            .collect()
    }

    fn followers_of_primary(&self, model_id: &str) -> Vec<String> {
        self.by_sha
            .values()
            .find(|entry| entry.primary == model_id)
            .map(|entry| entry.followers.iter().cloned().collect())
            .unwrap_or_default()
    }
}

pub struct ModelStore {
    items: RwLock<Vec<Value>>,
    manifest_index: RwLock<Option<Arc<ManifestHashIndex>>>,
    default_id: RwLock<String>,
    concurrency: RwLock<ConcurrencyState>,
    metrics: RwLock<MetricsState>,
    downloads: DownloadsState,
    hash_guard: StdMutex<HashGuardState>,
    http_client: Client,
    bus: Bus,
    kernel: Option<arw_kernel::Kernel>,
}

impl ModelStore {
    pub fn new(bus: Bus, kernel: Option<arw_kernel::Kernel>) -> Self {
        Self {
            items: RwLock::new(Vec::new()),
            manifest_index: RwLock::new(None),
            default_id: RwLock::new(String::new()),
            concurrency: RwLock::new(ConcurrencyState::new()),
            metrics: RwLock::new(MetricsState::default()),
            downloads: DownloadsState::new(),
            hash_guard: StdMutex::new(HashGuardState::default()),
            http_client: crate::http_client::client().clone(),
            bus,
            kernel,
        }
    }

    pub async fn bootstrap(&self) {
        if let Err(err) = fs::create_dir_all(self.models_dir()).await {
            warn!("failed to create models dir: {err}");
        }
        let items = self
            .load_from_disk()
            .await
            .unwrap_or_else(|_| util::default_models());
        self.replace_items(items).await;
        self.load_metrics_from_disk().await;
        self.emit_metrics_patch().await;
    }

    pub async fn summary(&self) -> Value {
        let items = self.items.read().await.clone();
        let default = self.default_id.read().await.clone();
        let metrics = self.metrics.read().await.clone().snapshot();
        let metrics_value = serde_json::to_value(metrics).unwrap_or(Value::Null);
        let concurrency =
            serde_json::to_value(self.concurrency_snapshot().await).unwrap_or(Value::Null);
        json!({
            "items": items,
            "default": default,
            "concurrency": concurrency,
            "metrics": metrics_value,
        })
    }

    pub async fn list(&self) -> Vec<Value> {
        self.items.read().await.clone()
    }

    pub async fn refresh(&self) -> Vec<Value> {
        let fresh = util::default_models();
        self.replace_items(fresh.clone()).await;
        if let Err(err) = self.write_to_disk(fresh.clone()).await {
            warn!("models refresh persist failed: {err}");
        }
        self.bus.publish(
            topics::TOPIC_MODELS_REFRESHED,
            &json!({"count": fresh.len()}),
        );
        fresh
    }

    pub async fn save(&self) -> Result<(), String> {
        let items = self.items.read().await.clone();
        let res = self.write_to_disk(items.clone()).await;
        if res.is_ok() {
            self.bus.publish(
                topics::TOPIC_MODELS_CHANGED,
                &json!({"op":"save","count": items.len()}),
            );
        }
        res
    }

    pub async fn load(&self) -> Result<Vec<Value>, String> {
        let items = self.load_from_disk().await?;
        self.replace_items(items.clone()).await;
        self.bus.publish(
            topics::TOPIC_MODELS_CHANGED,
            &json!({"op":"load","count": items.len()}),
        );
        Ok(items)
    }

    pub async fn add_model(&self, entry: Value) -> Result<(), String> {
        let id = entry
            .get("id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| "model id required".to_string())?
            .to_string();
        {
            let mut items = self.items.write().await;
            if let Some(pos) = items
                .iter()
                .position(|m| m.get("id").and_then(|v| v.as_str()) == Some(id.as_str()))
            {
                items[pos] = entry;
            } else {
                items.push(entry);
            }
        }
        self.invalidate_manifest_index().await;
        self.emit_patch().await;
        self.bus
            .publish(topics::TOPIC_MODELS_CHANGED, &json!({"op":"add","id": id}));
        Ok(())
    }

    pub async fn remove_model(&self, id: &str) -> bool {
        let mut removed = false;
        {
            let mut items = self.items.write().await;
            items.retain(|m| {
                let keep = m.get("id").and_then(|v| v.as_str()) != Some(id);
                if !keep {
                    removed = true;
                }
                keep
            });
        }
        if removed {
            self.invalidate_manifest_index().await;
            self.emit_patch().await;
            self.bus.publish(
                topics::TOPIC_MODELS_CHANGED,
                &json!({"op":"remove","id": id}),
            );
        }
        removed
    }

    pub async fn default_get(&self) -> String {
        self.default_id.read().await.clone()
    }

    pub async fn default_set(&self, id: String) -> Result<(), String> {
        let exists = self
            .items
            .read()
            .await
            .iter()
            .any(|m| m.get("id").and_then(|v| v.as_str()) == Some(id.as_str()));
        if !exists {
            return Err(format!("unknown model id: {}", id));
        }
        *self.default_id.write().await = id.clone();
        self.emit_patch().await;
        self.bus.publish(
            topics::TOPIC_MODELS_CHANGED,
            &json!({"op":"default","id": id}),
        );
        Ok(())
    }

    pub async fn concurrency_get(&self) -> ModelsConcurrencySnapshot {
        self.concurrency_snapshot().await
    }

    pub async fn concurrency_set(
        &self,
        configured_max: Option<u64>,
        hard_cap: Option<u64>,
        block: Option<bool>,
    ) -> ModelsConcurrencySnapshot {
        let before = {
            let state = self.concurrency.read().await;
            state.configured()
        };
        {
            let mut state = self.concurrency.write().await;
            if let Some(max) = configured_max {
                state.configured_max = max.max(1);
            }
            state.hard_cap = hard_cap.filter(|v| *v > 0);
        }
        self.downloads.notify.notify_waiters();
        let after = {
            let state = self.concurrency.read().await;
            state.configured()
        };
        let should_block = block.unwrap_or(true) && after < before;
        if should_block {
            self.downloads.wait_until_at_most(after).await;
        }
        self.concurrency_snapshot().await
    }

    pub async fn metrics_value(&self) -> ModelsMetricsResponse {
        let counters = self.metrics.read().await.clone().snapshot();
        let inflight = self.inflight_snapshot();
        let concurrency = self.concurrency_snapshot().await;
        let jobs = self.downloads.job_snapshot().await;
        let runtime =
            ModelsRuntimeConfig::from_tuning(Self::download_tuning(), Self::preflight_enabled());
        ModelsMetricsResponse::from_parts(counters, inflight, concurrency, jobs, runtime)
    }

    pub async fn jobs_snapshot(&self) -> Value {
        let active = self.downloads.job_snapshot().await;
        let inflight = self.inflight_snapshot();
        let concurrency = self.concurrency_snapshot().await;
        json!({
            "active": active,
            "inflight": inflight,
            "concurrency": concurrency,
        })
    }

    pub async fn hashes_page(
        &self,
        limit: usize,
        offset: usize,
        provider: Option<String>,
        model: Option<String>,
        sort: Option<String>,
        order: Option<String>,
    ) -> HashPage {
        let index = self.manifest_hash_index().await;
        let mut rows: Vec<HashItem> = index
            .iter()
            .map(|(sha256, refs)| refs.to_hash_item(sha256))
            .collect();
        if let Some(filter) = provider.as_ref() {
            rows.retain(|row| row.providers.iter().any(|p| p == filter));
        }
        if let Some(filter) = model.as_ref() {
            rows.retain(|row| row.models.iter().any(|m| m == filter));
        }
        let sort_key = sort
            .map(|s| s.to_ascii_lowercase())
            .unwrap_or_else(|| "bytes".to_string());
        let desc_default = sort_key == "bytes";
        let desc = match order.as_deref() {
            Some("asc") => false,
            Some("desc") => true,
            _ => desc_default,
        };
        rows.sort_by(|a, b| {
            let ord = match sort_key.as_str() {
                "sha256" => a.sha256.cmp(&b.sha256),
                "path" => a.path.cmp(&b.path),
                "providers_count" => a.providers.len().cmp(&b.providers.len()),
                _ => a.bytes.cmp(&b.bytes),
            };
            if desc {
                ord.reverse()
            } else {
                ord
            }
        });
        let total = rows.len();
        let limit = limit.clamp(1, 10_000);
        let pages = if total == 0 {
            0
        } else {
            ((total - 1) / limit) + 1
        };
        let max_offset = if pages == 0 { 0 } else { (pages - 1) * limit };
        let offset = if total == 0 {
            0
        } else {
            offset.min(max_offset)
        };
        let end = offset.saturating_add(limit).min(total);
        let slice = rows[offset..end].to_vec();
        let count = end.saturating_sub(offset);
        let page = if pages == 0 { 0 } else { (offset / limit) + 1 };
        let prev_offset = if page <= 1 {
            None
        } else {
            Some(offset.saturating_sub(limit))
        };
        let next_offset = if page == 0 || page >= pages {
            None
        } else {
            Some(end)
        };
        HashPage {
            items: slice,
            total,
            count,
            limit,
            offset,
            prev_offset,
            next_offset,
            page,
            pages,
            last_offset: max_offset,
        }
    }

    async fn manifest_hash_index(&self) -> Arc<ManifestHashIndex> {
        if let Some(cached) = self.manifest_index.read().await.as_ref().cloned() {
            return cached;
        }

        let items_snapshot = {
            let guard = self.items.read().await;
            guard.clone()
        };
        let built = Arc::new(Self::collect_manifest_hash_index(&items_snapshot));
        let entries = built.len();

        let mut guard = self.manifest_index.write().await;
        if let Some(existing) = guard.as_ref() {
            return existing.clone();
        }
        metrics::counter!(METRIC_MANIFEST_INDEX_REBUILDS).increment(1);
        metrics::gauge!(GAUGE_MANIFEST_INDEX_ENTRIES).set(entries as f64);
        debug!(entries, "manifest hash index rebuilt");
        *guard = Some(built.clone());
        built
    }

    async fn invalidate_manifest_index(&self) {
        self.manifest_index.write().await.take();
    }

    fn collect_manifest_hash_index(items: &[Value]) -> ManifestHashIndex {
        let mut index = ManifestHashIndex::new();
        for entry in items {
            let Some(hash) = entry.get("sha256").and_then(|v| v.as_str()) else {
                continue;
            };
            if hash.len() != 64 {
                continue;
            }
            let bucket = index.entry(hash.to_string()).or_default();
            bucket.ingest_manifest(entry);
        }
        index
    }

    pub async fn start_download(self: &Arc<Self>, req: DownloadRequest) -> Result<(), String> {
        let id = req.id.trim();
        if id.is_empty() {
            return Err("id is required".into());
        }
        let mut url = req.url.clone();
        if url.is_none() {
            url = self.find_model_url(id).await;
        }
        let url = url.ok_or_else(|| "url is required".to_string())?;
        if !(url.starts_with("http://") || url.starts_with("https://")) {
            return Err("unsupported url scheme".into());
        }

        let provider = req.provider.clone();
        let sha_hint = req.sha256.trim();
        if sha_hint.len() != 64 || !sha_hint.chars().all(|c| c.is_ascii_hexdigit()) {
            return Err("invalid sha256".into());
        }
        let sha_hint = sha_hint.to_ascii_lowercase();

        if self.downloads.contains(id).await {
            return Err("download already in progress".into());
        }

        let dest = Self::dest_info(&url);
        let started_at = Instant::now();
        let corr_id = uuid::Uuid::new_v4().to_string();

        let role = self.register_hash_role(id, &sha_hint);
        if let HashGuardRole::Coalesced { primary } = role {
            self.with_metrics(|m| m.record_coalesced()).await;
            self.upsert_model_status(id, "coalesced", provider.clone(), Some(url.clone()))
                .await;
            let extra = self.progress_extra_with_hints(
                Some(json!({"mode": "coalesced", "primary": primary})),
                Some(Duration::from_secs(0)),
                None,
                None,
            );
            self.publish_progress(id, Some("coalesced"), Some("hash-guard"), extra, None, None);
            return Ok(());
        }

        let mut preflight_bytes: Option<u64> = None;
        if Self::preflight_enabled() {
            match self.run_preflight(id, &url).await {
                Ok(info) => {
                    preflight_bytes = info.content_length;
                    self.with_metrics(|m| m.record_preflight_ok()).await;
                    let extra = self.progress_extra_with_hints(
                        Some(json!({
                            "mode": "ok",
                            "content_length": info.content_length,
                            "etag": info.etag,
                            "last_modified": info.last_modified
                        })),
                        Some(Duration::from_secs(0)),
                        None,
                        info.content_length,
                    );
                    self.publish_progress(
                        id,
                        Some("preflight"),
                        None,
                        extra,
                        None,
                        Some(corr_id.clone()),
                    );
                }
                Err(PreflightError::Skip(reason)) => {
                    self.with_metrics(|m| m.record_preflight_skipped()).await;
                    let extra = self.progress_extra_with_hints(
                        Some(json!({"mode": "skip", "reason": reason})),
                        Some(Duration::from_secs(0)),
                        None,
                        None,
                    );
                    self.publish_progress(
                        id,
                        Some("preflight"),
                        Some("skipped"),
                        extra,
                        None,
                        Some(corr_id.clone()),
                    );
                }
                Err(PreflightError::Denied { code, message }) => {
                    self.with_metrics(|m| m.record_preflight_denied()).await;
                    let extra = self.progress_extra_with_hints(
                        Some(json!({"error": message.clone()})),
                        Some(Duration::from_secs(0)),
                        None,
                        None,
                    );
                    self.publish_progress(
                        id,
                        Some("error"),
                        Some(&code),
                        extra,
                        Some(code.clone()),
                        Some(corr_id.clone()),
                    );
                    self.mark_error(id, &code, &message).await;
                    self.append_egress_event(
                        "deny",
                        &code,
                        &dest,
                        &corr_id,
                        None,
                        Some(Duration::from_secs(0)),
                    )
                    .await;
                    self.release_primary_hash(id);
                    return Err(message);
                }
            }
        }

        self.with_metrics(|m| m.record_started()).await;
        let start_extra = self.progress_extra_with_hints(
            Some(json!({"url": url, "content_length": preflight_bytes})),
            Some(Duration::from_secs(0)),
            None,
            preflight_bytes,
        );
        self.publish_progress(
            id,
            Some("started"),
            None,
            start_extra,
            None,
            Some(corr_id.clone()),
        );
        self.upsert_model_status(id, "queued", provider.clone(), Some(url.clone()))
            .await;

        let cancel = CancellationToken::new();
        let max = self.concurrency.read().await.configured();
        if self.downloads.wait_for_slot(max, &cancel).await.is_err() {
            self.with_metrics(|m| {
                if m.queued > 0 {
                    m.queued -= 1;
                }
            })
            .await;
            self.release_primary_hash(id);
            return Err("download canceled".into());
        }

        let job_id = uuid::Uuid::new_v4().to_string();
        let model_id = id.to_string();
        let url_clone = url.clone();
        let runner = Arc::clone(self);
        let finisher = Arc::clone(self);
        let provider_clone = provider.clone();
        let sha_clone = sha_hint.clone();
        let cancel_clone = cancel.clone();
        let dest_clone = dest.clone();
        let corr_clone = corr_id.clone();

        let handle = DownloadHandle {
            cancel,
            task: None,
            job_id: job_id.clone(),
            url_display: Self::redact_url_for_logs(&url),
            corr_id: corr_id.clone(),
            dest: dest.clone(),
            started_at,
        };

        if self.downloads.insert_job(id, handle).await.is_err() {
            self.release_primary_hash(id);
            return Err("download already in progress".into());
        }

        self.publish_preview(id, &url, provider.as_deref(), &dest, &corr_id);

        let job_handle = tokio::spawn(async move {
            let outcome = runner
                .run_download_job(
                    model_id.clone(),
                    url_clone,
                    provider_clone,
                    sha_clone,
                    cancel_clone,
                    started_at,
                    corr_clone,
                    dest_clone,
                )
                .await;
            finisher.finish_download(model_id, outcome).await;
        });

        {
            let mut jobs = self.downloads.jobs.lock().await;
            if let Some(entry) = jobs.get_mut(id) {
                entry.task = Some(job_handle);
            }
        }

        Ok(())
    }

    pub async fn cancel_download(&self, id: &str) -> Result<(), String> {
        if id.trim().is_empty() {
            return Err("id is required".into());
        }
        if let Some((corr_id, _dest)) = self.downloads.cancel_job(id).await {
            let extra = self.progress_extra_with_hints(None, None, None, None);
            self.publish_progress(
                id,
                Some("canceled"),
                None,
                extra,
                None,
                Some(corr_id.clone()),
            );
            self.with_metrics(|m| m.record_canceled()).await;
            self.upsert_model_status(id, "canceled", None, None).await;
            Ok(())
        } else {
            let extra = self.progress_extra_with_hints(None, None, None, None);
            self.release_hash_for_model(id);
            self.publish_progress(id, Some("no-active-job"), None, extra, None, None);
            Err("no active download".into())
        }
    }

    pub async fn cas_gc(&self, req: CasGcRequest) -> Result<Value, String> {
        let ttl_hours = req.ttl_hours.unwrap_or(24);
        let verbose = req.verbose.unwrap_or(false);
        let cutoff = chrono::Utc::now() - chrono::Duration::hours(ttl_hours as i64);
        let manifest_index = self.manifest_hash_index().await;
        let cas_dir = self.cas_dir();
        let mut scanned = 0u64;
        let mut kept = 0u64;
        let mut deleted = 0u64;
        let mut deleted_bytes = 0u64;
        let mut deleted_items = if verbose { Some(Vec::new()) } else { None };

        if let Ok(mut entries) = fs::read_dir(&cas_dir).await {
            while let Ok(Some(entry)) = entries.next_entry().await {
                let path = entry.path();
                if !path.is_file() {
                    continue;
                }
                scanned += 1;
                let fname = path
                    .file_name()
                    .and_then(|s| s.to_str())
                    .unwrap_or_default()
                    .to_string();
                if manifest_index.contains_key(&fname) {
                    kept += 1;
                    continue;
                }
                let meta = match entry.metadata().await {
                    Ok(m) => m,
                    Err(err) => {
                        warn!("cas gc metadata failed for {:?}: {err}", path);
                        continue;
                    }
                };
                let (modified, modified_str) = match meta.modified() {
                    Ok(time) => {
                        let dt = DateTime::<Utc>::from(time);
                        (dt, Some(dt.to_rfc3339()))
                    }
                    Err(_) => {
                        let fallback = Utc::now();
                        (fallback, None)
                    }
                };
                if modified > cutoff {
                    kept += 1;
                    continue;
                }
                let size = meta.len();
                if let Err(err) = fs::remove_file(&path).await {
                    warn!("cas gc remove failed {:?}: {err}", path);
                    kept += 1;
                    continue;
                }
                deleted += 1;
                deleted_bytes = deleted_bytes.saturating_add(size);
                if let Some(ref mut list) = deleted_items {
                    let rel_path = path.strip_prefix(&cas_dir).unwrap_or(&path).to_path_buf();
                    list.push(CasGcDeletedItem {
                        sha256: fname.clone(),
                        path: rel_path.to_string_lossy().into_owned(),
                        bytes: size,
                        last_modified: modified_str,
                    });
                }
            }
        }

        let mut payload = json!({
            "scanned": scanned,
            "kept": kept,
            "deleted": deleted,
            "deleted_bytes": deleted_bytes,
            "ttl_hours": ttl_hours,
        });
        if let Some(list) = deleted_items {
            payload.as_object_mut().expect("payload object").insert(
                "deleted_items".into(),
                serde_json::to_value(list).unwrap_or(Value::Null),
            );
        }
        self.bus.publish(topics::TOPIC_MODELS_CAS_GC, &payload);
        Ok(payload)
    }

    #[allow(clippy::too_many_arguments)]
    async fn run_download_job(
        self: Arc<Self>,
        model_id: String,
        url: String,
        provider: Option<String>,
        sha_hint: String,
        cancel: CancellationToken,
        started_at: Instant,
        corr_id: String,
        dest: DestInfo,
    ) -> DownloadOutcome {
        self.with_metrics(|m| m.record_admitted()).await;

        let tmp_dir = self.models_dir().join("tmp");
        if let Err(err) = fs::create_dir_all(&tmp_dir).await {
            error!("models tmp dir create failed: {err}");
            return DownloadOutcome::Failed {
                code: "io".into(),
                message: err.to_string(),
                elapsed: started_at.elapsed(),
                dest,
                corr_id,
                bytes_in: 0,
            };
        }

        let (tmp_path, meta_path) = Self::tmp_paths(&tmp_dir, &model_id, &sha_hint);
        let mut resume_from = fs::metadata(&tmp_path)
            .await
            .ok()
            .map(|m| m.len())
            .unwrap_or(0);
        let mut downloaded = resume_from;
        let mut hasher = Sha256::new();
        let mut last_emit_bytes = downloaded;
        let mut last_emit_at = Instant::now();

        if resume_from > 0 {
            if let Err(err) = Self::hash_existing(&tmp_path, &mut hasher).await {
                warn!("failed to hash existing partial download: {err}; restarting");
                let _ = Self::remove_resume_artifacts(&tmp_path, &meta_path).await;
                resume_from = 0;
                downloaded = 0;
                hasher = Sha256::new();
            }
        }

        let tuning = Self::download_tuning();
        let if_range_header = if resume_from > 0 {
            Self::load_resume_ifrange(&meta_path).await
        } else {
            None
        };

        let mut attempt = 0u32;
        let response = loop {
            let mut request = self
                .http_client
                .get(&url)
                .timeout(http_timeout::get_duration());

            if resume_from > 0 {
                request = request.header(reqwest::header::RANGE, format!("bytes={}-", resume_from));
                if let Some(ref if_range) = if_range_header {
                    request = request.header(reqwest::header::IF_RANGE, if_range.clone());
                }
            }

            match request.send().await {
                Ok(resp) => match resp.error_for_status() {
                    Ok(ok) => break ok,
                    Err(err) => {
                        error!("model download http error: {err}");
                        let code = if err.is_timeout() {
                            "request-timeout"
                        } else {
                            "http"
                        };
                        let _ = Self::remove_resume_artifacts(&tmp_path, &meta_path).await;
                        return DownloadOutcome::Failed {
                            code: code.into(),
                            message: err.to_string(),
                            elapsed: started_at.elapsed(),
                            dest,
                            corr_id,
                            bytes_in: downloaded,
                        };
                    }
                },
                Err(err) => {
                    if attempt < tuning.send_retries {
                        let delay = tuning.backoff_delay(attempt + 1);
                        warn!(
                            "model download send error (attempt {attempt}): {err}; retrying in {:?}",
                            delay
                        );
                        tokio::time::sleep(delay).await;
                        attempt += 1;
                        continue;
                    }
                    error!("model download request error: {err}");
                    let code = if err.is_timeout() {
                        "request-timeout"
                    } else {
                        "http"
                    };
                    let _ = Self::remove_resume_artifacts(&tmp_path, &meta_path).await;
                    return DownloadOutcome::Failed {
                        code: code.into(),
                        message: err.to_string(),
                        elapsed: started_at.elapsed(),
                        dest,
                        corr_id,
                        bytes_in: downloaded,
                    };
                }
            }
        };

        Self::save_resume_validators(&meta_path, response.headers()).await;

        let status = response.status();
        let content_len = response.content_length();
        let mut total = content_len.map(|len| resume_from + len);

        let file_base = if resume_from > 0 && status == reqwest::StatusCode::PARTIAL_CONTENT {
            if !Self::validate_resume_content_range(resume_from, response.headers()) {
                warn!("Content-Range mismatch when resuming model {model_id}; aborting resume");
                let _ = Self::remove_resume_artifacts(&tmp_path, &meta_path).await;
                return DownloadOutcome::Failed {
                    code: "resume-content-range".into(),
                    message: "server provided mismatched Content-Range for resume".into(),
                    elapsed: started_at.elapsed(),
                    dest,
                    corr_id,
                    bytes_in: resume_from,
                };
            }
            self.with_metrics(|m| m.record_resumed()).await;
            let extra = self.progress_extra_with_hints(
                Some(json!({"offset": resume_from})),
                Some(started_at.elapsed()),
                Some(resume_from),
                total,
            );
            self.publish_progress(
                &model_id,
                Some("resumed"),
                Some("resumed"),
                extra,
                None,
                Some(corr_id.clone()),
            );
            match tokio::fs::OpenOptions::new()
                .append(true)
                .open(&tmp_path)
                .await
            {
                Ok(file) => file,
                Err(err) => {
                    error!("failed to open tmp for append: {err}");
                    let _ = Self::remove_resume_artifacts(&tmp_path, &meta_path).await;
                    return DownloadOutcome::Failed {
                        code: "io".into(),
                        message: err.to_string(),
                        elapsed: started_at.elapsed(),
                        dest,
                        corr_id,
                        bytes_in: resume_from,
                    };
                }
            }
        } else {
            if resume_from > 0 && status == reqwest::StatusCode::OK {
                let _ = Self::remove_resume_artifacts(&tmp_path, &meta_path).await;
                downloaded = 0;
                total = content_len;
            } else if resume_from > 0 && status != reqwest::StatusCode::PARTIAL_CONTENT {
                let _ = Self::remove_resume_artifacts(&tmp_path, &meta_path).await;
                return DownloadOutcome::Failed {
                    code: "resume-http-status".into(),
                    message: format!("unexpected status {} for resume", status),
                    elapsed: started_at.elapsed(),
                    dest,
                    corr_id,
                    bytes_in: resume_from,
                };
            }
            match tokio::fs::File::create(&tmp_path).await {
                Ok(file) => {
                    hasher = Sha256::new();
                    file
                }
                Err(err) => {
                    error!("model download file create failed: {err}");
                    let _ = Self::remove_resume_artifacts(&tmp_path, &meta_path).await;
                    return DownloadOutcome::Failed {
                        code: "io".into(),
                        message: err.to_string(),
                        elapsed: started_at.elapsed(),
                        dest,
                        corr_id,
                        bytes_in: downloaded,
                    };
                }
            }
        };

        let mut file = BufWriter::new(file_base);
        let mut stream = response.bytes_stream();
        let reserve_bytes = Self::disk_reserve_bytes();
        let max_bytes = Self::max_download_bytes();
        let quota_bytes = Self::quota_bytes();
        let cas_usage_bytes = if quota_bytes.is_some() {
            self.cas_usage_bytes().await.unwrap_or(0)
        } else {
            0
        };

        if let Some(max) = max_bytes {
            if let Some(total_bytes) = total {
                if total_bytes > max {
                    let _ = Self::remove_resume_artifacts(&tmp_path, &meta_path).await;
                    return DownloadOutcome::Failed {
                        code: "size_limit".into(),
                        message: format!(
                            "download size {} exceeds max {} (ARW_MODELS_MAX_MB)",
                            total_bytes, max
                        ),
                        elapsed: started_at.elapsed(),
                        dest,
                        corr_id,
                        bytes_in: downloaded,
                    };
                }
            }
        }
        if let Some(quota) = quota_bytes {
            if let Some(total_bytes) = total {
                if cas_usage_bytes.saturating_add(total_bytes) > quota {
                    let _ = Self::remove_resume_artifacts(&tmp_path, &meta_path).await;
                    return DownloadOutcome::Failed {
                        code: "quota_exceeded".into(),
                        message: format!(
                            "quota {} bytes would be exceeded by download ({} bytes)",
                            quota, total_bytes
                        ),
                        elapsed: started_at.elapsed(),
                        dest,
                        corr_id,
                        bytes_in: downloaded,
                    };
                }
            }
        }
        if reserve_bytes > 0 {
            if let Ok(avail) = Self::available_space(self.state_dir()) {
                if avail <= reserve_bytes {
                    let _ = Self::remove_resume_artifacts(&tmp_path, &meta_path).await;
                    return DownloadOutcome::Failed {
                        code: "disk_insufficient".into(),
                        message: format!(
                            "available disk {} <= reserve {} (ARW_MODELS_DISK_RESERVE_MB)",
                            avail, reserve_bytes
                        ),
                        elapsed: started_at.elapsed(),
                        dest,
                        corr_id,
                        bytes_in: downloaded,
                    };
                }
                if let Some(total_bytes) = total {
                    if avail.saturating_sub(reserve_bytes) < total_bytes {
                        let _ = Self::remove_resume_artifacts(&tmp_path, &meta_path).await;
                        return DownloadOutcome::Failed {
                            code: "disk_insufficient".into(),
                            message: format!(
                                "not enough free space for download: need {} + reserve {}, only {} available",
                                total_bytes, reserve_bytes, avail
                            ),
                            elapsed: started_at.elapsed(),
                            dest,
                            corr_id,
                            bytes_in: downloaded,
                        };
                    }
                }
            }
        }

        let limits = DownloadBudgetLimits::global();
        let mut budget_notifier = BudgetNotifier::new(limits);
        let start = started_at;
        let idle_timeout = tuning.idle_timeout;

        loop {
            let next = if let Some(idle) = idle_timeout {
                tokio::select! {
                    chunk = stream.next() => chunk,
                    _ = cancel.cancelled() => {
                        let _ = Self::remove_resume_artifacts(&tmp_path, &meta_path).await;
                        return DownloadOutcome::Canceled {
                            elapsed: start.elapsed(),
                            dest,
                            corr_id,
                        };
                    }
                    _ = tokio::time::sleep(idle) => {
                        warn!("model download idle-timeout after {:?}", idle);
                        let _ = Self::remove_resume_artifacts(&tmp_path, &meta_path).await;
                        return DownloadOutcome::Failed {
                            code: "idle-timeout".into(),
                            message: format!(
                                "no data received for {} seconds (ARW_DL_IDLE_TIMEOUT_SECS)",
                                idle.as_secs()
                            ),
                            elapsed: start.elapsed(),
                            dest,
                            corr_id,
                            bytes_in: downloaded,
                        };
                    }
                }
            } else {
                tokio::select! {
                    chunk = stream.next() => chunk,
                    _ = cancel.cancelled() => {
                        let _ = Self::remove_resume_artifacts(&tmp_path, &meta_path).await;
                        return DownloadOutcome::Canceled {
                            elapsed: start.elapsed(),
                            dest,
                            corr_id,
                        };
                    }
                }
            };
            let Some(next) = next else {
                break;
            };
            let chunk = match next {
                Ok(c) => c,
                Err(err) => {
                    error!("model download chunk error: {err}");
                    let _ = Self::remove_resume_artifacts(&tmp_path, &meta_path).await;
                    return DownloadOutcome::Failed {
                        code: "http".into(),
                        message: err.to_string(),
                        elapsed: start.elapsed(),
                        dest,
                        corr_id,
                        bytes_in: downloaded,
                    };
                }
            };
            if let Err(err) = file.write_all(&chunk).await {
                error!("model download write error: {err}");
                let _ = Self::remove_resume_artifacts(&tmp_path, &meta_path).await;
                return DownloadOutcome::Failed {
                    code: "io".into(),
                    message: err.to_string(),
                    elapsed: start.elapsed(),
                    dest,
                    corr_id,
                    bytes_in: downloaded,
                };
            }
            hasher.update(&chunk);
            downloaded = downloaded.saturating_add(chunk.len() as u64);

            if let Some(max) = max_bytes {
                if downloaded > max {
                    let _ = Self::remove_resume_artifacts(&tmp_path, &meta_path).await;
                    return DownloadOutcome::Failed {
                        code: "size_limit".into(),
                        message: format!("download exceeded max size {} (ARW_MODELS_MAX_MB)", max),
                        elapsed: start.elapsed(),
                        dest,
                        corr_id,
                        bytes_in: downloaded,
                    };
                }
            }
            if let Some(quota) = quota_bytes {
                if cas_usage_bytes.saturating_add(downloaded) > quota {
                    let _ = Self::remove_resume_artifacts(&tmp_path, &meta_path).await;
                    return DownloadOutcome::Failed {
                        code: "quota_exceeded".into(),
                        message: format!(
                            "quota {} bytes exceeded (downloaded {}, existing {})",
                            quota, downloaded, cas_usage_bytes
                        ),
                        elapsed: start.elapsed(),
                        dest,
                        corr_id,
                        bytes_in: downloaded,
                    };
                }
            }
            if reserve_bytes > 0 {
                if let Ok(avail) = Self::available_space(self.state_dir()) {
                    if avail <= reserve_bytes {
                        let _ = Self::remove_resume_artifacts(&tmp_path, &meta_path).await;
                        return DownloadOutcome::Failed {
                            code: "disk_insufficient".into(),
                            message: format!(
                                "download aborted: free space {} <= reserve {}",
                                avail, reserve_bytes
                            ),
                            elapsed: start.elapsed(),
                            dest,
                            corr_id,
                            bytes_in: downloaded,
                        };
                    }
                }
            }

            if downloaded.saturating_sub(last_emit_bytes) >= PROGRESS_EMIT_BYTES
                || last_emit_at.elapsed() >= PROGRESS_EMIT_INTERVAL
            {
                let mut extra = json!({
                    "bytes": downloaded,
                    "downloaded": downloaded,
                });
                if let Some(total_bytes) = total {
                    if let Value::Object(ref mut map) = extra {
                        map.insert("total".into(), Value::from(total_bytes));
                        if total_bytes > 0 {
                            let pct =
                                ((downloaded as f64) / (total_bytes as f64) * 100.0).min(100.0);
                            if let Some(num) = Number::from_f64(pct) {
                                map.insert("percent".into(), Value::Number(num));
                            }
                        }
                    }
                }
                let extra = self.progress_extra_with_hints(
                    Some(extra),
                    Some(start.elapsed()),
                    Some(downloaded),
                    total,
                );
                self.publish_progress(
                    &model_id,
                    Some("downloading"),
                    None,
                    extra,
                    None,
                    Some(corr_id.clone()),
                );
                last_emit_bytes = downloaded;
                last_emit_at = Instant::now();
            }

            let elapsed_now = start.elapsed();
            budget_notifier.maybe_emit(
                self.as_ref(),
                &model_id,
                elapsed_now,
                Some(downloaded),
                total,
                &corr_id,
            );

            if let Some(hard_ms) = limits.hard_ms {
                if duration_to_millis(elapsed_now) >= hard_ms {
                    let _ = Self::remove_resume_artifacts(&tmp_path, &meta_path).await;
                    return DownloadOutcome::Failed {
                        code: "hard-budget".into(),
                        message: format!(
                            "download exceeded hard budget {} ms (ARW_BUDGET_DOWNLOAD_HARD_MS)",
                            hard_ms
                        ),
                        elapsed: elapsed_now,
                        dest,
                        corr_id,
                        bytes_in: downloaded,
                    };
                }
            }
        }

        if let Err(err) = file.flush().await {
            error!("model download flush error: {err}");
            let _ = Self::remove_resume_artifacts(&tmp_path, &meta_path).await;
            return DownloadOutcome::Failed {
                code: "io".into(),
                message: err.to_string(),
                elapsed: start.elapsed(),
                dest,
                corr_id,
                bytes_in: downloaded,
            };
        }
        if let Err(err) = file.get_mut().sync_all().await {
            error!("model download sync error: {err}");
            let _ = Self::remove_resume_artifacts(&tmp_path, &meta_path).await;
            return DownloadOutcome::Failed {
                code: "io".into(),
                message: err.to_string(),
                elapsed: start.elapsed(),
                dest,
                corr_id,
                bytes_in: downloaded,
            };
        }

        let sha256 = format!("{:x}", hasher.finalize());
        if !sha_hint.eq_ignore_ascii_case(&sha256) {
            let _ = Self::remove_resume_artifacts(&tmp_path, &meta_path).await;
            return DownloadOutcome::Failed {
                code: "sha256_mismatch".into(),
                message: format!("expected {}, got {}", sha_hint, sha256),
                elapsed: start.elapsed(),
                dest,
                corr_id,
                bytes_in: downloaded,
            };
        }

        let cas_path = self.cas_dir().join(&sha256);
        let cas_exists = cas_path.exists();
        if !cas_exists {
            if let Some(parent) = cas_path.parent() {
                if let Err(err) = fs::create_dir_all(parent).await {
                    error!("cas dir create failed: {err}");
                    let _ = Self::remove_resume_artifacts(&tmp_path, &meta_path).await;
                    return DownloadOutcome::Failed {
                        code: "io".into(),
                        message: err.to_string(),
                        elapsed: start.elapsed(),
                        dest,
                        corr_id,
                        bytes_in: downloaded,
                    };
                }
            }
            if let Err(err) = fs::rename(&tmp_path, &cas_path).await {
                error!("cas rename failed: {err}");
                let _ = Self::remove_resume_artifacts(&tmp_path, &meta_path).await;
                return DownloadOutcome::Failed {
                    code: "io".into(),
                    message: err.to_string(),
                    elapsed: start.elapsed(),
                    dest,
                    corr_id,
                    bytes_in: downloaded,
                };
            }
        } else if let Err(err) = fs::remove_file(&tmp_path).await {
            warn!("tmp cleanup failed: {err}");
        }
        let _ = fs::remove_file(&meta_path).await;

        let elapsed = start.elapsed();
        let success = CompletedDownload {
            sha256,
            bytes: downloaded,
            provider,
            url: Some(url),
            elapsed,
            cached: cas_exists,
        };
        if cas_exists {
            DownloadOutcome::Cached {
                info: success,
                dest,
                corr_id,
            }
        } else {
            DownloadOutcome::Completed {
                info: success,
                dest,
                corr_id,
            }
        }
    }
    async fn finish_download(&self, model_id: String, outcome: DownloadOutcome) {
        let followers = self.followers_for(&model_id);
        match outcome {
            DownloadOutcome::Completed {
                info,
                dest,
                corr_id,
            } => {
                let info_clone = info.clone();
                let corr_clone = corr_id.clone();
                self.handle_success(&model_id, info, false, dest, corr_id.clone())
                    .await;
                for follower in &followers {
                    self.handle_coalesced_success(follower, &info_clone, &corr_clone)
                        .await;
                }
            }
            DownloadOutcome::Cached {
                info,
                dest,
                corr_id,
            } => {
                let info_clone = info.clone();
                let corr_clone = corr_id.clone();
                self.handle_success(&model_id, info, true, dest, corr_id.clone())
                    .await;
                for follower in &followers {
                    self.handle_coalesced_success(follower, &info_clone, &corr_clone)
                        .await;
                }
            }
            DownloadOutcome::Canceled {
                elapsed,
                dest,
                corr_id,
            } => {
                self.with_metrics(|m| m.record_canceled()).await;
                let extra = self.progress_extra_with_hints(None, Some(elapsed), None, None);
                self.publish_progress(
                    &model_id,
                    Some("canceled"),
                    None,
                    extra,
                    None,
                    Some(corr_id.clone()),
                );
                self.upsert_model_status(&model_id, "canceled", None, None)
                    .await;
                self.append_egress_event(
                    "deny",
                    "canceled",
                    &dest,
                    &corr_id,
                    Some(0),
                    Some(elapsed),
                )
                .await;
                for follower in &followers {
                    self.handle_coalesced_canceled(follower, elapsed, &corr_id)
                        .await;
                }
            }
            DownloadOutcome::Failed {
                code,
                message,
                elapsed,
                dest,
                corr_id,
                bytes_in,
            } => {
                self.with_metrics(|m| m.record_error()).await;
                let extra = self.progress_extra_with_hints(
                    Some(json!({"error": message.clone()})),
                    Some(elapsed),
                    None,
                    None,
                );
                self.publish_progress(
                    &model_id,
                    Some("error"),
                    Some(&code),
                    extra,
                    Some(code.clone()),
                    Some(corr_id.clone()),
                );
                self.mark_error(&model_id, &code, &message).await;
                self.append_egress_event(
                    "deny",
                    &code,
                    &dest,
                    &corr_id,
                    Some(bytes_in),
                    Some(elapsed),
                )
                .await;
                for follower in &followers {
                    self.handle_coalesced_error(follower, &code, &message, elapsed, &corr_id)
                        .await;
                }
            }
        }
        self.release_primary_hash(&model_id);
        self.downloads.remove_job(&model_id).await;
        self.emit_patch().await;
    }

    async fn handle_success(
        &self,
        model_id: &str,
        info: CompletedDownload,
        cached: bool,
        dest: DestInfo,
        corr_id: String,
    ) {
        let mbps = if info.elapsed.as_secs_f64() > 0.0 {
            Some((info.bytes as f64 / 1_048_576.0) / info.elapsed.as_secs_f64())
        } else {
            None
        };
        self.with_metrics(|m| m.record_completed(info.bytes, mbps, cached))
            .await;
        if let Some(speed) = mbps {
            if let Err(err) = self.persist_download_metrics(speed).await {
                warn!("persist download metrics failed: {err}");
            }
        }

        let mut extra = json!({
            "sha256": info.sha256,
            "bytes": info.bytes,
            "downloaded": info.bytes,
            "cached": cached,
        });
        if let Value::Object(ref mut map) = extra {
            map.insert("total".into(), Value::from(info.bytes));
            if let Some(num) = Number::from_f64(100.0) {
                map.insert("percent".into(), Value::Number(num));
            }
        }
        let extra = self.progress_extra_with_hints(
            Some(extra),
            Some(info.elapsed),
            Some(info.bytes),
            Some(info.bytes),
        );
        self.publish_progress(
            model_id,
            Some("complete"),
            None,
            extra,
            None,
            Some(corr_id.clone()),
        );
        let elapsed = info.elapsed;
        let bytes_for_ledger = if cached { 0 } else { info.bytes };
        self.append_egress_event(
            "allow",
            "models.download",
            &dest,
            &corr_id,
            Some(bytes_for_ledger),
            Some(elapsed),
        )
        .await;
        if let Err(err) = self.record_success(model_id, &info, &corr_id).await {
            warn!("record success failed: {err}");
        }
    }

    async fn handle_coalesced_success(
        &self,
        model_id: &str,
        info: &CompletedDownload,
        corr_id: &str,
    ) {
        let mut follower_info = info.clone();
        follower_info.cached = true;
        let mut extra = json!({
            "sha256": follower_info.sha256,
            "bytes": follower_info.bytes,
            "downloaded": follower_info.bytes,
            "cached": true,
            "source": "coalesced",
        });
        if let Value::Object(ref mut map) = extra {
            map.insert("total".into(), Value::from(follower_info.bytes));
            if let Some(num) = Number::from_f64(100.0) {
                map.insert("percent".into(), Value::Number(num));
            }
        }
        let extra = self.progress_extra_with_hints(
            Some(extra),
            Some(follower_info.elapsed),
            Some(follower_info.bytes),
            Some(follower_info.bytes),
        );
        self.publish_progress(
            model_id,
            Some("complete"),
            None,
            extra,
            None,
            Some(corr_id.to_string()),
        );
        if let Err(err) = self.record_success(model_id, &follower_info, corr_id).await {
            warn!("record success follower failed: {err}");
        }
    }

    async fn handle_coalesced_canceled(&self, model_id: &str, elapsed: Duration, corr_id: &str) {
        let extra = self.progress_extra_with_hints(None, Some(elapsed), None, None);
        self.publish_progress(
            model_id,
            Some("canceled"),
            None,
            extra,
            None,
            Some(corr_id.to_string()),
        );
        self.upsert_model_status(model_id, "canceled", None, None)
            .await;
    }

    async fn handle_coalesced_error(
        &self,
        model_id: &str,
        code: &str,
        message: &str,
        elapsed: Duration,
        corr_id: &str,
    ) {
        let extra = self.progress_extra_with_hints(
            Some(json!({"error": message})),
            Some(elapsed),
            None,
            None,
        );
        self.publish_progress(
            model_id,
            Some("error"),
            Some(code),
            extra,
            Some(code.to_string()),
            Some(corr_id.to_string()),
        );
        self.mark_error(model_id, code, message).await;
    }

    async fn record_success(
        &self,
        model_id: &str,
        info: &CompletedDownload,
        corr_id: &str,
    ) -> Result<(), String> {
        let cas_path = self.cas_dir().join(&info.sha256);
        let manifest = json!({
            "id": model_id,
            "sha256": info.sha256,
            "bytes": info.bytes,
            "path": cas_path.to_string_lossy(),
            "provider": info.provider,
            "url": info.url,
            "downloaded_at": Utc::now().to_rfc3339(),
            "verified": true,
        });
        self.upsert_model_available(model_id, &manifest, info.cached)
            .await;
        self.write_manifest(model_id, &manifest).await?;
        let mut event_body = manifest.clone();
        if !corr_id.is_empty() {
            event_body
                .as_object_mut()
                .map(|map| map.insert("corr_id".into(), Value::String(corr_id.to_string())));
        }
        self.bus
            .publish(topics::TOPIC_MODELS_MANIFEST_WRITTEN, &event_body);
        Ok(())
    }

    async fn mark_error(&self, model_id: &str, code: &str, message: &str) {
        let mut created = false;
        {
            let mut items = self.items.write().await;
            let entry = items
                .iter_mut()
                .find(|m| m.get("id").and_then(|v| v.as_str()) == Some(model_id));
            if let Some(value) = entry {
                if let Some(obj) = value.as_object_mut() {
                    obj.insert("status".into(), Value::String("error".into()));
                    obj.insert("error_code".into(), Value::String(code.into()));
                    obj.insert("error".into(), Value::String(message.into()));
                }
            } else {
                created = true;
                let mut obj = Map::new();
                obj.insert("id".into(), Value::String(model_id.into()));
                obj.insert("status".into(), Value::String("error".into()));
                obj.insert("error_code".into(), Value::String(code.into()));
                obj.insert("error".into(), Value::String(message.into()));
                items.push(Value::Object(obj));
            }
        }
        if created {
            self.bus.publish(
                topics::TOPIC_MODELS_CHANGED,
                &json!({"op":"add","id": model_id}),
            );
        }
        self.invalidate_manifest_index().await;
        if let Err(err) = self.persist_items_snapshot().await {
            warn!("persist models after error failed: {err}");
        }
        self.emit_patch().await;
    }

    async fn upsert_model_status(
        &self,
        id: &str,
        status: &str,
        provider: Option<String>,
        url: Option<String>,
    ) {
        let mut created = false;
        {
            let mut items = self.items.write().await;
            let entry = items
                .iter_mut()
                .find(|m| m.get("id").and_then(|v| v.as_str()) == Some(id));
            if let Some(value) = entry {
                if let Some(obj) = value.as_object_mut() {
                    obj.insert("status".into(), Value::String(status.into()));
                    if let Some(provider) = provider.as_ref() {
                        obj.insert("provider".into(), Value::String(provider.clone()));
                    }
                    if let Some(url) = url.as_ref() {
                        obj.insert("url".into(), Value::String(url.clone()));
                    }
                }
            } else {
                created = true;
                let mut obj = Map::new();
                obj.insert("id".into(), Value::String(id.into()));
                obj.insert("status".into(), Value::String(status.into()));
                if let Some(provider) = provider.as_ref() {
                    obj.insert("provider".into(), Value::String(provider.clone()));
                }
                if let Some(url) = url.as_ref() {
                    obj.insert("url".into(), Value::String(url.clone()));
                }
                items.push(Value::Object(obj));
            }
        }
        if created {
            self.bus
                .publish(topics::TOPIC_MODELS_CHANGED, &json!({"op":"add","id": id}));
        }
        self.invalidate_manifest_index().await;
        if let Err(err) = self.persist_items_snapshot().await {
            warn!("persist models after status update failed: {err}");
        }
        self.emit_patch().await;
    }

    async fn upsert_model_available(&self, id: &str, manifest: &Value, cached: bool) {
        let mut created = false;
        {
            let mut items = self.items.write().await;
            let entry = items
                .iter_mut()
                .find(|m| m.get("id").and_then(|v| v.as_str()) == Some(id));
            if let Some(value) = entry {
                if let Some(obj) = value.as_object_mut() {
                    obj.insert("status".into(), Value::String("available".into()));
                    if let Some(sha) = manifest.get("sha256").cloned() {
                        obj.insert("sha256".into(), sha);
                    }
                    if let Some(bytes) = manifest.get("bytes").cloned() {
                        obj.insert("bytes".into(), bytes);
                    }
                    if let Some(path) = manifest.get("path").cloned() {
                        obj.insert("path".into(), path);
                    }
                    if let Some(provider) = manifest.get("provider").cloned() {
                        obj.insert("provider".into(), provider);
                    }
                    if let Some(url) = manifest.get("url").cloned() {
                        obj.insert("url".into(), url);
                    }
                    if let Some(verified) = manifest.get("verified").cloned() {
                        obj.insert("verified".into(), verified);
                    }
                    obj.insert("cached".into(), Value::Bool(cached));
                    obj.remove("error_code");
                    obj.remove("error");
                }
            } else {
                created = true;
                let mut obj = Map::new();
                obj.insert("id".into(), Value::String(id.into()));
                obj.insert("status".into(), Value::String("available".into()));
                if let Some(sha) = manifest.get("sha256").cloned() {
                    obj.insert("sha256".into(), sha);
                }
                if let Some(bytes) = manifest.get("bytes").cloned() {
                    obj.insert("bytes".into(), bytes);
                }
                if let Some(path) = manifest.get("path").cloned() {
                    obj.insert("path".into(), path);
                }
                if let Some(provider) = manifest.get("provider").cloned() {
                    obj.insert("provider".into(), provider);
                }
                if let Some(url) = manifest.get("url").cloned() {
                    obj.insert("url".into(), url);
                }
                if let Some(verified) = manifest.get("verified").cloned() {
                    obj.insert("verified".into(), verified);
                }
                obj.insert("cached".into(), Value::Bool(cached));
                items.push(Value::Object(obj));
            }
        }
        if created {
            self.bus
                .publish(topics::TOPIC_MODELS_CHANGED, &json!({"op":"add","id": id}));
        } else {
            self.bus.publish(
                topics::TOPIC_MODELS_CHANGED,
                &json!({"op":"update","id": id}),
            );
        }
        self.invalidate_manifest_index().await;
        if let Err(err) = self.persist_items_snapshot().await {
            warn!("persist models after success failed: {err}");
        }
    }

    async fn persist_items_snapshot(&self) -> Result<(), String> {
        let snapshot = self.items.read().await.clone();
        self.write_to_disk(snapshot).await
    }

    async fn load_metrics_from_disk(&self) {
        let path = self.models_dir().join("downloads.metrics.json");
        if let Ok(bytes) = fs::read(&path).await {
            if let Ok(value) = serde_json::from_slice::<Value>(&bytes) {
                if let Some(ewma) = value.get("ewma_mbps").and_then(|v| v.as_f64()) {
                    let mut metrics = self.metrics.write().await;
                    metrics.ewma_mbps = Some(ewma);
                }
            }
        }
    }

    async fn emit_metrics_patch(&self) {
        let snapshot = self.metrics.read().await.clone().snapshot();
        match serde_json::to_value(&snapshot) {
            Ok(value) => read_models::publish_read_model_patch(&self.bus, "models_metrics", &value),
            Err(err) => warn!("serialize models metrics snapshot failed: {err}"),
        }
    }

    fn progress_extra_with_hints(
        &self,
        extra: Option<Value>,
        elapsed: Option<Duration>,
        downloaded: Option<u64>,
        total: Option<u64>,
    ) -> Option<Value> {
        let hints = *ProgressHintsConfig::global();
        if !hints.include_budget && !hints.include_disk {
            return extra;
        }

        let mut map = match extra {
            Some(Value::Object(map)) => map,
            Some(other) => return Some(other),
            None => Map::new(),
        };

        if hints.include_budget {
            if let Some(budget) = DownloadBudgetLimits::global().snapshot(elapsed) {
                map.insert("budget".into(), budget);
            }
        }

        if hints.include_disk {
            if let Some(disk) = self.disk_snapshot(downloaded, total) {
                map.insert("disk".into(), disk);
            }
        }

        if map.is_empty() {
            None
        } else {
            Some(Value::Object(map))
        }
    }

    fn disk_snapshot(&self, downloaded: Option<u64>, total: Option<u64>) -> Option<Value> {
        let reserve = Self::disk_reserve_bytes();
        let available = Self::available_space(self.state_dir()).ok();
        let need = match (total, downloaded) {
            (Some(total), Some(done)) => Some(total.saturating_sub(done)),
            (Some(total), None) => Some(total),
            _ => None,
        };

        if reserve == 0 && available.is_none() && need.is_none() {
            return None;
        }

        let mut map = Map::new();
        map.insert("reserve".into(), Value::from(reserve));
        match available {
            Some(bytes) => {
                map.insert("available".into(), Value::from(bytes));
            }
            None => {
                map.insert("available".into(), Value::Null);
            }
        }
        if let Some(bytes) = need {
            map.insert("need".into(), Value::from(bytes));
        }

        Some(Value::Object(map))
    }

    fn disk_reserve_bytes() -> u64 {
        static BYTES: OnceCell<u64> = OnceCell::new();
        *BYTES.get_or_init(|| {
            std::env::var("ARW_MODELS_DISK_RESERVE_MB")
                .ok()
                .and_then(|s| s.parse::<u64>().ok())
                .unwrap_or(256)
                .saturating_mul(1024 * 1024)
        })
    }

    fn max_download_bytes() -> Option<u64> {
        static BYTES: OnceCell<Option<u64>> = OnceCell::new();
        *BYTES.get_or_init(|| {
            std::env::var("ARW_MODELS_MAX_MB")
                .ok()
                .and_then(|s| s.parse::<u64>().ok())
                .map(|mb| mb.saturating_mul(1024 * 1024))
                .filter(|b| *b > 0)
        })
    }

    fn quota_bytes() -> Option<u64> {
        static BYTES: OnceCell<Option<u64>> = OnceCell::new();
        *BYTES.get_or_init(|| {
            std::env::var("ARW_MODELS_QUOTA_MB")
                .ok()
                .and_then(|s| s.parse::<u64>().ok())
                .map(|mb| mb.saturating_mul(1024 * 1024))
                .filter(|b| *b > 0)
        })
    }

    fn download_tuning() -> &'static DownloadTuning {
        static TUNING: OnceCell<DownloadTuning> = OnceCell::new();
        TUNING.get_or_init(DownloadTuning::from_env)
    }

    fn available_space(path: PathBuf) -> Result<u64, String> {
        available_space(path).map_err(|e| e.to_string())
    }

    fn state_dir(&self) -> PathBuf {
        util::state_dir()
    }

    async fn cas_usage_bytes(&self) -> Result<u64, String> {
        let mut total = 0u64;
        let dir = self.cas_dir();
        let mut entries = match fs::read_dir(&dir).await {
            Ok(it) => it,
            Err(_) => return Ok(0),
        };
        while let Ok(Some(entry)) = entries.next_entry().await {
            if let Ok(meta) = entry.metadata().await {
                if meta.is_file() {
                    total = total.saturating_add(meta.len());
                }
            }
        }
        Ok(total)
    }

    async fn write_manifest(&self, id: &str, manifest: &Value) -> Result<(), String> {
        let path = self.manifest_path(id);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .await
                .map_err(|e| format!("create manifest dir failed: {e}"))?;
        }
        fs::write(
            &path,
            serde_json::to_vec_pretty(manifest).map_err(|e| e.to_string())?,
        )
        .await
        .map_err(|e| format!("write manifest failed: {e}"))
    }

    async fn with_metrics<F>(&self, f: F)
    where
        F: FnOnce(&mut MetricsState),
    {
        let mut metrics = self.metrics.write().await;
        f(&mut metrics);
        let snapshot = metrics.snapshot();
        drop(metrics);
        match serde_json::to_value(&snapshot) {
            Ok(value) => read_models::publish_read_model_patch(&self.bus, "models_metrics", &value),
            Err(err) => warn!("serialize models metrics snapshot failed: {err}"),
        }
    }

    fn register_hash_role(&self, model_id: &str, sha: &str) -> HashGuardRole {
        let mut guard = self.hash_guard.lock().expect("hash guard poisoned");
        guard.register(model_id, sha)
    }

    fn release_primary_hash(&self, model_id: &str) -> Vec<String> {
        let mut guard = self.hash_guard.lock().expect("hash guard poisoned");
        guard.release_primary(model_id)
    }

    fn release_hash_for_model(&self, model_id: &str) {
        let mut guard = self.hash_guard.lock().expect("hash guard poisoned");
        guard.release_model(model_id);
    }

    fn progress_targets(&self, model_id: &str) -> Vec<String> {
        let guard = self.hash_guard.lock().expect("hash guard poisoned");
        guard.progress_targets(model_id)
    }

    fn inflight_snapshot(&self) -> Vec<ModelsInflightEntry> {
        let guard = self.hash_guard.lock().expect("hash guard poisoned");
        guard.inflight_snapshot()
    }

    fn followers_for(&self, model_id: &str) -> Vec<String> {
        let guard = self.hash_guard.lock().expect("hash guard poisoned");
        guard.followers_of_primary(model_id)
    }

    fn preflight_enabled() -> bool {
        match std::env::var("ARW_DL_PREFLIGHT") {
            Ok(value) => matches!(
                value.trim().to_ascii_lowercase().as_str(),
                "1" | "true" | "on"
            ),
            Err(_) => true,
        }
    }

    async fn run_preflight(
        &self,
        model_id: &str,
        url: &str,
    ) -> Result<PreflightInfo, PreflightError> {
        let request = self
            .http_client
            .head(url)
            .timeout(http_timeout::get_duration());
        let response = match request.send().await {
            Ok(resp) => resp,
            Err(err) => {
                warn!("model preflight HEAD failed for {model_id}: {err}; skipping preflight");
                return Err(PreflightError::Skip(err.to_string()));
            }
        };

        if matches!(
            response.status(),
            StatusCode::METHOD_NOT_ALLOWED | StatusCode::NOT_IMPLEMENTED
        ) {
            return Err(PreflightError::Skip(format!(
                "server does not support HEAD (status {})",
                response.status()
            )));
        }

        if !response.status().is_success() {
            return Err(PreflightError::Denied {
                code: "preflight-http".into(),
                message: format!("HEAD {} failed with status {}", url, response.status()),
            });
        }

        let headers = response.headers();
        let content_length = headers
            .get(header::CONTENT_LENGTH)
            .and_then(|v| v.to_str().ok())
            .and_then(|s| s.parse::<u64>().ok());
        let etag = headers
            .get(header::ETAG)
            .and_then(|v| v.to_str().ok())
            .map(|s| s.trim().to_string());
        let last_modified = headers
            .get(header::LAST_MODIFIED)
            .and_then(|v| v.to_str().ok())
            .map(|s| s.to_string());

        if let Some(max) = Self::max_download_bytes() {
            if let Some(len) = content_length {
                if len > max {
                    return Err(PreflightError::Denied {
                        code: "size_limit".into(),
                        message: format!(
                            "download size {} exceeds max {} (ARW_MODELS_MAX_MB)",
                            len, max
                        ),
                    });
                }
            }
        }

        if let Some(quota) = Self::quota_bytes() {
            if let Some(len) = content_length {
                let usage = self.cas_usage_bytes().await.unwrap_or(0);
                if usage.saturating_add(len) > quota {
                    return Err(PreflightError::Denied {
                        code: "quota_exceeded".into(),
                        message: format!(
                            "quota {} bytes would be exceeded by download ({} bytes in CAS)",
                            quota, usage
                        ),
                    });
                }
            }
        }

        if let Some(len) = content_length {
            let reserve = Self::disk_reserve_bytes();
            match Self::available_space(self.state_dir()) {
                Ok(avail) => {
                    if avail <= reserve || avail.saturating_sub(reserve) < len {
                        return Err(PreflightError::Denied {
                            code: "disk_insufficient".into(),
                            message: format!(
                                "not enough free space (available {}, reserve {}, need {})",
                                avail, reserve, len
                            ),
                        });
                    }
                }
                Err(err) => {
                    warn!("disk space check failed during preflight: {err}");
                }
            }
        }

        Ok(PreflightInfo {
            content_length,
            etag,
            last_modified,
        })
    }
    fn publish_progress(
        &self,
        id: &str,
        status: Option<&str>,
        code: Option<&str>,
        extra: Option<Value>,
        error_code: Option<String>,
        corr_id: Option<String>,
    ) {
        let targets = self.progress_targets(id);
        for target in targets {
            let mut obj = Map::new();
            obj.insert("id".into(), Value::String(target));
            if let Some(status) = status {
                obj.insert("status".into(), Value::String(status.into()));
            }
            if let Some(code) = code {
                obj.insert("code".into(), Value::String(code.into()));
            }
            if let Some(extra_val) = extra.clone() {
                match extra_val {
                    Value::Object(map) => {
                        for (k, v) in map {
                            obj.insert(k, v);
                        }
                    }
                    other => {
                        obj.insert("extra".into(), other);
                    }
                }
            }
            if let Some(err) = error_code.clone() {
                obj.insert("error_code".into(), Value::String(err));
            }
            if let Some(cid) = corr_id.clone() {
                if !cid.is_empty() {
                    obj.insert("corr_id".into(), Value::String(cid));
                }
            }
            self.bus.publish(DOWNLOAD_EVENT_KIND, &Value::Object(obj));
        }
    }

    fn publish_preview(
        &self,
        id: &str,
        url: &str,
        provider: Option<&str>,
        dest: &DestInfo,
        corr_id: &str,
    ) {
        let mut obj = Map::new();
        obj.insert("id".into(), Value::String(id.to_string()));
        obj.insert("url".into(), Value::String(Self::redact_url_for_logs(url)));
        if let Some(provider) = provider {
            obj.insert("provider".into(), Value::String(provider.to_string()));
        }
        obj.insert(
            "dest".into(),
            json!({
                "host": dest.host.clone(),
                "port": dest.port,
                "protocol": dest.protocol.clone(),
            }),
        );
        if !corr_id.is_empty() {
            obj.insert("corr_id".into(), Value::String(corr_id.to_string()));
        }
        obj.insert("posture".into(), Value::String(util::effective_posture()));
        self.bus
            .publish(topics::TOPIC_EGRESS_PREVIEW, &Value::Object(obj));
    }

    async fn append_egress_event(
        &self,
        decision: &str,
        reason: &str,
        dest: &DestInfo,
        corr_id: &str,
        bytes_in: Option<u64>,
        elapsed: Option<Duration>,
    ) {
        let posture = util::effective_posture();
        let project_id = std::env::var("ARW_PROJECT_ID").ok();
        let ledger_id = if let Some(kernel) = &self.kernel {
            let dest_host = if dest.host.is_empty() {
                None
            } else {
                Some(dest.host.clone())
            };
            let dest_port = if dest.port > 0 {
                Some(dest.port as i64)
            } else {
                None
            };
            let protocol = if dest.protocol.is_empty() {
                None
            } else {
                Some(dest.protocol.clone())
            };
            let corr = if corr_id.is_empty() {
                None
            } else {
                Some(corr_id.to_string())
            };
            let bytes_in_i64 = bytes_in.map(|b| b as i64);
            match kernel
                .clone()
                .append_egress_async(
                    decision.to_string(),
                    Some(reason.to_string()),
                    dest_host,
                    dest_port,
                    protocol,
                    bytes_in_i64,
                    Some(0),
                    corr,
                    project_id.clone(),
                    Some(posture.clone()),
                    Some(json!({"source": "model_steward"})),
                )
                .await
            {
                Ok(id) => Some(id),
                Err(err) => {
                    warn!("append egress ledger failed: {err}");
                    None
                }
            }
        } else {
            None
        };

        let mut payload = Map::new();
        payload.insert("id".into(), json!(ledger_id));
        payload.insert("decision".into(), json!(decision));
        if !reason.is_empty() {
            payload.insert("reason".into(), json!(reason));
        }
        if !dest.host.is_empty() {
            payload.insert("dest_host".into(), json!(dest.host.clone()));
        }
        if dest.port > 0 {
            payload.insert("dest_port".into(), json!(dest.port as i64));
        }
        if !dest.protocol.is_empty() {
            payload.insert("protocol".into(), json!(dest.protocol.clone()));
        }
        if let Some(bytes) = bytes_in {
            payload.insert("bytes_in".into(), json!(bytes as i64));
        }
        payload.insert("bytes_out".into(), json!(0));
        if !corr_id.is_empty() {
            payload.insert("corr_id".into(), json!(corr_id));
        }
        if let Some(ms) = elapsed.map(|d| d.as_millis() as u64) {
            payload.insert("duration_ms".into(), json!(ms));
        }
        payload.insert("posture".into(), json!(posture));
        if let Some(proj) = project_id {
            payload.insert("project_id".into(), json!(proj));
        }
        payload.insert("tool_id".into(), json!("models.download"));
        payload.insert("meta".into(), json!({"source": "model_steward"}));
        self.bus.publish(
            topics::TOPIC_EGRESS_LEDGER_APPENDED,
            &Value::Object(payload),
        );
    }

    fn tmp_paths(tmp_dir: &Path, model_id: &str, sha_hint: &str) -> (PathBuf, PathBuf) {
        let base = if !sha_hint.is_empty() {
            sha_hint.to_string()
        } else {
            format!("{}-{}", model_id, uuid::Uuid::new_v4())
        };
        let tmp = tmp_dir.join(format!("{}.part", base));
        let meta = tmp.with_extension("part.meta");
        (tmp, meta)
    }

    async fn hash_existing(path: &Path, hasher: &mut Sha256) -> Result<(), String> {
        let file = tokio::fs::File::open(path)
            .await
            .map_err(|e| e.to_string())?;
        let mut reader = BufReader::new(file);
        let mut buf = [0u8; 64 * 1024];
        loop {
            let n = reader.read(&mut buf).await.map_err(|e| e.to_string())?;
            if n == 0 {
                break;
            }
            hasher.update(&buf[..n]);
        }
        Ok(())
    }

    async fn remove_resume_artifacts(tmp_path: &Path, meta_path: &Path) {
        if tokio::fs::remove_file(tmp_path).await.is_err() {
            // ignore missing file
        }
        let _ = tokio::fs::remove_file(meta_path).await;
    }

    async fn save_resume_validators(meta_path: &Path, headers: &header::HeaderMap) {
        let etag = headers
            .get(header::ETAG)
            .and_then(|v| v.to_str().ok())
            .map(|s| s.to_string());
        let last_modified = headers
            .get(header::LAST_MODIFIED)
            .and_then(|v| v.to_str().ok())
            .map(|s| s.to_string());
        if etag.is_none() && last_modified.is_none() {
            return;
        }
        let mut map = Map::new();
        if let Some(e) = etag {
            map.insert("etag".into(), Value::String(e));
        }
        if let Some(lm) = last_modified {
            map.insert("last_modified".into(), Value::String(lm));
        }
        let value = Value::Object(map);
        if let Some(parent) = meta_path.parent() {
            let _ = fs::create_dir_all(parent).await;
        }
        if let Ok(bytes) = serde_json::to_vec(&value) {
            let _ = fs::write(meta_path, bytes).await;
        }
    }

    async fn load_resume_ifrange(meta_path: &Path) -> Option<String> {
        let bytes = fs::read(meta_path).await.ok()?;
        let value: Value = serde_json::from_slice(&bytes).ok()?;
        if let Some(etag) = value.get("etag").and_then(|v| v.as_str()) {
            Some(etag.to_string())
        } else {
            value
                .get("last_modified")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
        }
    }

    fn validate_resume_content_range(resume_from: u64, headers: &header::HeaderMap) -> bool {
        let Some(value) = headers
            .get(header::CONTENT_RANGE)
            .and_then(|v| v.to_str().ok())
        else {
            return false;
        };
        let value = value.trim();
        let Some(rest) = value.strip_prefix("bytes ") else {
            return false;
        };
        let Some(range_part) = rest.split('/').next() else {
            return false;
        };
        let mut parts = range_part.split('-');
        let Some(start) = parts.next() else {
            return false;
        };
        match start.parse::<u64>() {
            Ok(start_offset) => start_offset == resume_from,
            Err(_) => false,
        }
    }

    fn dest_info(url: &str) -> DestInfo {
        if let Ok(parsed) = reqwest::Url::parse(url) {
            let host = parsed.host_str().unwrap_or("").to_string();
            let port = parsed.port().unwrap_or_else(|| match parsed.scheme() {
                "https" => 443,
                "http" => 80,
                _ => 0,
            });
            let protocol = parsed.scheme().to_string();
            DestInfo {
                host,
                port,
                protocol,
            }
        } else {
            DestInfo {
                host: String::new(),
                port: 0,
                protocol: "http".into(),
            }
        }
    }

    fn redact_url_for_logs(u: &str) -> String {
        if let Ok(mut url) = reqwest::Url::parse(u) {
            let _ = url.set_username("");
            let _ = url.set_password(None);
            url.set_query(None);
            url.set_fragment(None);
            url.to_string()
        } else {
            let no_fragment = u.split('#').next().unwrap_or(u);
            no_fragment
                .split('?')
                .next()
                .unwrap_or(no_fragment)
                .to_string()
        }
    }

    async fn concurrency_snapshot(&self) -> ModelsConcurrencySnapshot {
        let state = self.concurrency.read().await.clone();
        let configured = state.configured();
        let active = self.downloads.active_count().await as u64;
        let available = configured.saturating_sub(active);
        let pending = active.saturating_sub(configured);
        ModelsConcurrencySnapshot {
            configured_max: configured,
            available_permits: available,
            held_permits: active.min(configured),
            hard_cap: state.hard_cap,
            pending_shrink: (pending > 0).then_some(pending),
        }
    }

    async fn replace_items(&self, items: Vec<Value>) {
        {
            let mut guard = self.items.write().await;
            *guard = items;
        }
        self.invalidate_manifest_index().await;
        let mut default_guard = self.default_id.write().await;
        if default_guard.is_empty() {
            *default_guard = self
                .items
                .read()
                .await
                .iter()
                .find_map(|m| m.get("id").and_then(|v| v.as_str()))
                .unwrap_or_default()
                .to_string();
        }
        drop(default_guard);
        self.emit_patch().await;
    }

    async fn emit_patch(&self) {
        let snapshot = {
            let items = self.items.read().await.clone();
            let default = self.default_id.read().await.clone();
            json!({"items": items, "default": default})
        };
        read_models::publish_read_model_patch(&self.bus, "models", &snapshot);
    }

    async fn write_to_disk(&self, items: Vec<Value>) -> Result<(), String> {
        let path = self.models_file();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .await
                .map_err(|e| format!("create dir failed: {e}"))?;
        }
        let data = serde_json::to_vec_pretty(&items).map_err(|e| e.to_string())?;
        fs::write(&path, data)
            .await
            .map_err(|e| format!("write models.json failed: {e}"))
    }

    async fn load_from_disk(&self) -> Result<Vec<Value>, String> {
        let path = self.models_file();
        let bytes = fs::read(&path)
            .await
            .map_err(|e| format!("read models.json failed: {e}"))?;
        serde_json::from_slice(&bytes).map_err(|e| e.to_string())
    }

    fn models_dir(&self) -> PathBuf {
        util::state_dir().join("models")
    }

    fn cas_dir(&self) -> PathBuf {
        self.models_dir().join("by-hash")
    }

    pub fn cas_blob_path(&self, hash: &str) -> PathBuf {
        self.cas_dir().join(hash)
    }

    fn manifest_path(&self, id: &str) -> PathBuf {
        self.models_dir().join(format!("{id}.json"))
    }

    fn models_file(&self) -> PathBuf {
        self.models_dir().join("models.json")
    }

    async fn persist_download_metrics(&self, ewma: f64) -> Result<(), String> {
        let metrics_path = self.models_dir().join("downloads.metrics.json");
        let body = json!({"ewma_mbps": ewma});
        if let Some(parent) = metrics_path.parent() {
            fs::create_dir_all(parent)
                .await
                .map_err(|e| format!("metrics dir create failed: {e}"))?;
        }
        fs::write(
            &metrics_path,
            serde_json::to_vec_pretty(&body).map_err(|e| e.to_string())?,
        )
        .await
        .map_err(|e| format!("persist download metrics failed: {e}"))
    }

    async fn find_model_url(&self, id: &str) -> Option<String> {
        self.items
            .read()
            .await
            .iter()
            .find(|m| m.get("id").and_then(|v| v.as_str()) == Some(id))
            .and_then(|m| m.get("url").and_then(|v| v.as_str()).map(|s| s.to_string()))
    }
}

#[derive(Clone, Copy)]
struct ProgressHintsConfig {
    include_budget: bool,
    include_disk: bool,
}

impl ProgressHintsConfig {
    fn global() -> &'static Self {
        static CONFIG: OnceCell<ProgressHintsConfig> = OnceCell::new();
        CONFIG.get_or_init(|| ProgressHintsConfig {
            include_budget: env_flag("ARW_DL_PROGRESS_INCLUDE_BUDGET"),
            include_disk: env_flag("ARW_DL_PROGRESS_INCLUDE_DISK"),
        })
    }
}

#[derive(Clone, Copy)]
struct DownloadBudgetLimits {
    soft_ms: Option<u64>,
    hard_ms: Option<u64>,
    soft_degrade_pct: u64,
}

impl DownloadBudgetLimits {
    fn global() -> &'static Self {
        static LIMITS: OnceCell<DownloadBudgetLimits> = OnceCell::new();
        LIMITS.get_or_init(|| DownloadBudgetLimits {
            soft_ms: env_u64("ARW_BUDGET_DOWNLOAD_SOFT_MS"),
            hard_ms: env_u64("ARW_BUDGET_DOWNLOAD_HARD_MS"),
            soft_degrade_pct: std::env::var("ARW_BUDGET_SOFT_DEGRADE_PCT")
                .ok()
                .and_then(|s| s.parse::<u64>().ok())
                .map(|pct| pct.min(100))
                .unwrap_or(80),
        })
    }

    fn snapshot(&self, elapsed: Option<Duration>) -> Option<Value> {
        if self.soft_ms.is_none() && self.hard_ms.is_none() && elapsed.is_none() {
            return None;
        }

        let elapsed_ms = elapsed.map(duration_to_millis);
        let mut map = Map::new();

        match self.soft_ms {
            Some(ms) => {
                map.insert("soft_ms".into(), Value::from(ms));
            }
            None => {
                map.insert("soft_ms".into(), Value::Null);
            }
        }

        match self.hard_ms {
            Some(ms) => {
                map.insert("hard_ms".into(), Value::from(ms));
            }
            None => {
                map.insert("hard_ms".into(), Value::Null);
            }
        }

        match elapsed_ms {
            Some(ms) => {
                map.insert("elapsed_ms".into(), Value::from(ms));
            }
            None => {
                map.insert("elapsed_ms".into(), Value::Null);
            }
        }

        if let (Some(soft), Some(elapsed_ms)) = (self.soft_ms, elapsed_ms) {
            let remaining = soft as i128 - elapsed_ms as i128;
            map.insert(
                "soft_remaining_ms".into(),
                Value::from(clamp_i128_to_i64(remaining)),
            );
        } else {
            map.insert("soft_remaining_ms".into(), Value::Null);
        }

        if let (Some(hard), Some(elapsed_ms)) = (self.hard_ms, elapsed_ms) {
            let remaining = hard as i128 - elapsed_ms as i128;
            map.insert(
                "hard_remaining_ms".into(),
                Value::from(clamp_i128_to_i64(remaining)),
            );
        } else {
            map.insert("hard_remaining_ms".into(), Value::Null);
        }

        let state = match elapsed_ms {
            Some(ms) if self.hard_ms.is_some_and(|hard| ms >= hard) => "hard_exceeded",
            Some(ms) if self.soft_ms.is_some_and(|soft| ms >= soft) => "soft_exceeded",
            Some(_) => "ok",
            None => "unknown",
        };
        map.insert("state".into(), Value::from(state));

        Some(Value::Object(map))
    }

    fn degrade_threshold_ms(&self) -> Option<u64> {
        let soft = self.soft_ms?;
        let pct = self.soft_degrade_pct;
        if pct == 0 {
            return None;
        }
        let threshold = ((soft as u128) * pct as u128) / 100;
        let threshold = threshold.min(soft as u128);
        let threshold = threshold.max(1);
        Some(threshold as u64)
    }
}

struct BudgetNotifier {
    limits: &'static DownloadBudgetLimits,
    degraded_sent: bool,
}

impl BudgetNotifier {
    fn new(limits: &'static DownloadBudgetLimits) -> Self {
        let degraded_sent = limits.degrade_threshold_ms().is_none();
        Self {
            limits,
            degraded_sent,
        }
    }

    fn maybe_emit(
        &mut self,
        store: &ModelStore,
        model_id: &str,
        elapsed: Duration,
        downloaded: Option<u64>,
        total: Option<u64>,
        corr_id: &str,
    ) {
        if self.degraded_sent {
            return;
        }
        let Some(trigger_ms) = self.limits.degrade_threshold_ms() else {
            self.degraded_sent = true;
            return;
        };
        if duration_to_millis(elapsed) < trigger_ms {
            return;
        }
        let extra = store.progress_extra_with_hints(None, Some(elapsed), downloaded, total);
        store.publish_progress(
            model_id,
            Some("degraded"),
            Some("soft-budget"),
            extra,
            None,
            Some(corr_id.to_string()),
        );
        self.degraded_sent = true;
    }
}

fn clamp_i128_to_i64(value: i128) -> i64 {
    value.max(i128::from(i64::MIN)).min(i128::from(i64::MAX)) as i64
}

fn duration_to_millis(duration: Duration) -> u64 {
    let millis = duration.as_millis();
    if millis > u128::from(u64::MAX) {
        u64::MAX
    } else {
        millis as u64
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support;
    use tempfile::tempdir;
    use tokio::sync::broadcast::error::TryRecvError;

    fn dummy_download_handle(label: &str) -> DownloadHandle {
        DownloadHandle {
            cancel: CancellationToken::new(),
            task: None,
            job_id: format!("job-{label}"),
            url_display: format!("https://example.com/{label}"),
            corr_id: format!("corr-{label}"),
            dest: DestInfo {
                host: "example.com".into(),
                port: 443,
                protocol: "https".into(),
            },
            started_at: Instant::now(),
        }
    }

    #[tokio::test]
    async fn hash_guard_coalesces_followers() {
        let bus = arw_events::Bus::new_with_replay(16, 16);
        let store = Arc::new(ModelStore::new(bus, None));

        let role_primary = store.register_hash_role("model-a", "hash-one");
        assert!(matches!(role_primary, HashGuardRole::Primary));

        let role_follower = store.register_hash_role("model-b", "hash-one");
        match role_follower {
            HashGuardRole::Coalesced { primary } => assert_eq!(primary, "model-a"),
            _ => panic!("expected follower to coalesce"),
        }

        let targets = store.progress_targets("model-a");
        assert!(targets.contains(&"model-a".to_string()));
        assert!(targets.contains(&"model-b".to_string()));

        let followers = store.followers_for("model-a");
        assert_eq!(followers, vec!["model-b".to_string()]);

        let metrics = store.metrics_value().await;
        assert_eq!(metrics.inflight.len(), 1);
        let entry = &metrics.inflight[0];
        assert_eq!(entry.sha256, "hash-one");
        assert_eq!(entry.primary, "model-a");
        assert!(entry.followers.contains(&"model-b".to_string()));

        let released = store.release_primary_hash("model-a");
        assert_eq!(released, vec!["model-b".to_string()]);
        let targets_after = store.progress_targets("model-a");
        assert_eq!(targets_after, vec!["model-a".to_string()]);
    }

    #[tokio::test]
    async fn metrics_snapshot_stable_shape() {
        std::env::remove_var("ARW_MODELS_MAX_CONC");
        std::env::remove_var("ARW_MODELS_MAX_CONC_HARD");

        let bus = arw_events::Bus::new_with_replay(16, 16);
        let store = Arc::new(ModelStore::new(bus, None));

        let metrics = store.metrics_value().await;
        let value = serde_json::to_value(&metrics).expect("serialize metrics");

        let expected = json!({
            "started": 0,
            "queued": 0,
            "admitted": 0,
            "resumed": 0,
            "canceled": 0,
            "completed": 0,
            "completed_cached": 0,
            "errors": 0,
            "bytes_total": 0,
            "ewma_mbps": null,
            "preflight_ok": 0,
            "preflight_denied": 0,
            "preflight_skipped": 0,
            "coalesced": 0,
            "inflight": [],
            "concurrency": {
                "configured_max": 2,
                "available_permits": 2,
                "held_permits": 0,
                "hard_cap": null,
                "pending_shrink": null,
            },
            "jobs": [],
            "runtime": {
                "idle_timeout_secs": 300,
                "send_retries": 2,
                "stream_retries": 2,
                "retry_backoff_ms": 500,
                "preflight_enabled": true
            },
        });

        assert_eq!(value, expected);
    }

    #[tokio::test]
    async fn hashes_page_groups_and_filters_providers() {
        let bus = arw_events::Bus::new_with_replay(8, 8);
        let store = ModelStore::new(bus, None);
        let items = vec![
            json!({
                "id": "m-primary",
                "sha256": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                "bytes": 10,
                "provider": "alpha",
                "path": "/models/alpha.bin"
            }),
            json!({
                "id": "m-follower",
                "sha256": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                "provider": "beta"
            }),
            json!({
                "id": "m-two",
                "sha256": "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
                "bytes": 7,
                "provider": "alpha",
                "path": "/models/alpha-two.bin"
            }),
        ];
        store.replace_items(items).await;

        let page = store.hashes_page(10, 0, None, None, None, None).await;
        assert_eq!(page.total, 2);
        assert_eq!(page.count, 2);
        assert_eq!(page.limit, 10);
        assert_eq!(page.offset, 0);
        assert_eq!(page.page, 1);
        assert_eq!(page.pages, 1);
        assert!(page.prev_offset.is_none());
        assert!(page.next_offset.is_none());
        assert_eq!(page.last_offset, 0);

        let first = &page.items[0];
        assert_eq!(
            first.sha256,
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
        );
        assert_eq!(first.bytes, 10);
        assert_eq!(first.path, "/models/alpha.bin");
        assert_eq!(first.providers, vec!["alpha", "beta"]);
        assert_eq!(first.models, vec!["m-follower", "m-primary"]);

        let filtered = store
            .hashes_page(10, 0, Some("beta".into()), None, None, None)
            .await;
        assert_eq!(filtered.total, 1);
        assert_eq!(filtered.count, 1);
        assert_eq!(filtered.page, 1);
        assert_eq!(filtered.pages, 1);
        assert!(filtered.prev_offset.is_none());
        assert!(filtered.next_offset.is_none());
        assert_eq!(filtered.last_offset, 0);
        assert_eq!(filtered.items[0].sha256, first.sha256);
    }

    #[tokio::test]
    async fn hashes_page_filters_by_model_id() {
        let bus = arw_events::Bus::new_with_replay(8, 8);
        let store = ModelStore::new(bus, None);
        let items = vec![
            json!({
                "id": "first-model",
                "sha256": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                "bytes": 12,
                "provider": "alpha",
                "path": "/models/a.bin"
            }),
            json!({
                "id": "second-model",
                "sha256": "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
                "bytes": 9,
                "provider": "alpha",
                "path": "/models/b.bin"
            }),
            json!({
                "id": "follower",
                "sha256": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                "provider": "beta"
            }),
        ];
        store.replace_items(items).await;

        let filtered = store
            .hashes_page(10, 0, None, Some("follower".into()), None, None)
            .await;
        assert_eq!(filtered.total, 1);
        assert_eq!(filtered.items.len(), 1);
        assert_eq!(filtered.page, 1);
        assert_eq!(filtered.pages, 1);
        assert!(filtered.prev_offset.is_none());
        assert!(filtered.next_offset.is_none());
        assert_eq!(filtered.last_offset, 0);
        let entry = &filtered.items[0];
        assert_eq!(entry.models, vec!["first-model", "follower"]);
    }

    #[tokio::test]
    async fn hashes_page_reports_offsets() {
        let bus = arw_events::Bus::new_with_replay(8, 8);
        let store = ModelStore::new(bus, None);
        let mut items = Vec::new();
        for i in 0..103 {
            let sha = format!("{:064x}", i + 1);
            items.push(json!({
                "id": format!("model-{i}"),
                "sha256": sha,
                "bytes": 1 + i as u64,
                "provider": "local",
                "path": format!("/models/model-{i}.bin"),
            }));
        }
        store.replace_items(items).await;

        let page1 = store
            .hashes_page(25, 0, None, None, Some("sha256".into()), Some("asc".into()))
            .await;
        assert_eq!(page1.count, 25);
        assert_eq!(page1.page, 1);
        assert_eq!(page1.pages, 5);
        assert_eq!(page1.prev_offset, None);
        assert_eq!(page1.next_offset, Some(25));
        assert_eq!(page1.last_offset, 100);

        let page2 = store
            .hashes_page(
                25,
                page1.next_offset.expect("next offset"),
                None,
                None,
                Some("sha256".into()),
                Some("asc".into()),
            )
            .await;
        assert_eq!(page2.offset, 25);
        assert_eq!(page2.page, 2);
        assert_eq!(page2.prev_offset, Some(0));
        assert_eq!(page2.next_offset, Some(50));

        let page_last = store
            .hashes_page(
                25,
                9999,
                None,
                None,
                Some("sha256".into()),
                Some("asc".into()),
            )
            .await;
        assert_eq!(page_last.offset, 100);
        assert_eq!(page_last.count, 3);
        assert_eq!(page_last.page, 5);
        assert_eq!(page_last.pages, 5);
        assert_eq!(page_last.prev_offset, Some(75));
        assert_eq!(page_last.next_offset, None);
        assert_eq!(page_last.last_offset, 100);
    }

    #[tokio::test]
    async fn manifest_hash_index_invalidates_on_mutation() {
        let bus = arw_events::Bus::new_with_replay(8, 8);
        let store = ModelStore::new(bus, None);
        store
            .replace_items(vec![
                json!({
                    "id": "keep",
                    "sha256": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                    "bytes": 4,
                    "provider": "alpha",
                    "path": "/models/keep.bin"
                }),
                json!({
                    "id": "drop",
                    "sha256": "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
                    "bytes": 8,
                    "provider": "beta",
                    "path": "/models/drop.bin"
                }),
            ])
            .await;

        let first = store.manifest_hash_index().await;
        assert_eq!(first.len(), 2);
        drop(first);

        let removed = store.remove_model("drop").await;
        assert!(removed, "expected model removal to succeed");

        let second = store.manifest_hash_index().await;
        assert_eq!(second.len(), 1);
        assert!(
            second.contains_key("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa")
        );
        assert!(!second
            .contains_key("bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"));
    }

    #[tokio::test]
    async fn cas_gc_verbose_reports_deleted_entries() {
        let tmp = tempdir().expect("tempdir");
        let _ctx = test_support::begin_state_env(tmp.path());

        let bus = arw_events::Bus::new_with_replay(8, 8);
        let store = ModelStore::new(bus, None);

        let keep_hash = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
        let stale_hash = "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";

        store
            .replace_items(vec![json!({
                "id": "keep-model",
                "sha256": keep_hash,
                "bytes": 4,
                "provider": "alpha",
                "path": format!("/models/{keep_hash}"),
            })])
            .await;

        let keep_path = store.cas_blob_path(keep_hash);
        let stale_path = store.cas_blob_path(stale_hash);
        if let Some(parent) = keep_path.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .expect("create cas dir");
        }

        tokio::fs::write(&keep_path, b"keep")
            .await
            .expect("write keep blob");
        tokio::fs::write(&stale_path, b"stale-bytes")
            .await
            .expect("write stale blob");

        let payload = store
            .cas_gc(CasGcRequest {
                ttl_hours: Some(0),
                verbose: Some(true),
            })
            .await
            .expect("gc response");

        assert_eq!(payload.get("scanned").and_then(Value::as_u64), Some(2));
        assert_eq!(payload.get("deleted").and_then(Value::as_u64), Some(1));
        assert_eq!(payload.get("kept").and_then(Value::as_u64), Some(1));

        let deleted_items = payload
            .get("deleted_items")
            .and_then(Value::as_array)
            .expect("deleted items array");
        assert_eq!(deleted_items.len(), 1);
        let first = &deleted_items[0];
        assert_eq!(
            first.get("sha256").and_then(Value::as_str),
            Some(stale_hash)
        );
        assert_eq!(
            first.get("bytes").and_then(Value::as_u64),
            Some(b"stale-bytes".len() as u64)
        );

        tokio::fs::metadata(&keep_path)
            .await
            .expect("keep blob still present");
        assert!(tokio::fs::metadata(&stale_path).await.is_err());
    }

    #[tokio::test]
    async fn concurrency_set_blocking_shrink_waits_for_active_jobs() {
        let bus = arw_events::Bus::new_with_replay(8, 8);
        let store = Arc::new(ModelStore::new(bus, None));

        store
            .downloads
            .insert_job("model-a", dummy_download_handle("a"))
            .await
            .expect("insert job a");
        store
            .downloads
            .insert_job("model-b", dummy_download_handle("b"))
            .await
            .expect("insert job b");

        let store_clone = Arc::clone(&store);
        let join =
            tokio::spawn(
                async move { store_clone.concurrency_set(Some(1), None, Some(true)).await },
            );

        tokio::time::sleep(Duration::from_millis(25)).await;
        assert!(!join.is_finished(), "blocking shrink returned too early");

        let removed = store.downloads.remove_job("model-a").await;
        assert!(removed.is_some(), "expected job removal to succeed");

        let snapshot = join.await.expect("join blocking shrink");
        assert_eq!(snapshot.configured_max, 1);
        assert_eq!(snapshot.pending_shrink, None);
        assert_eq!(snapshot.held_permits, 1);

        let _ = store.downloads.remove_job("model-b").await;
    }

    #[tokio::test]
    async fn concurrency_set_non_blocking_reports_pending_shrink() {
        let bus = arw_events::Bus::new_with_replay(8, 8);
        let store = Arc::new(ModelStore::new(bus, None));

        store
            .downloads
            .insert_job("model-a", dummy_download_handle("a"))
            .await
            .expect("insert job a");
        store
            .downloads
            .insert_job("model-b", dummy_download_handle("b"))
            .await
            .expect("insert job b");

        let snapshot = tokio::time::timeout(
            Duration::from_millis(100),
            store.concurrency_set(Some(1), None, Some(false)),
        )
        .await
        .expect("non-blocking shrink should resolve");

        assert_eq!(snapshot.configured_max, 1);
        assert_eq!(snapshot.pending_shrink, Some(1));
        assert_eq!(snapshot.held_permits, 1);

        let active = store.downloads.active_count().await as u64;
        assert_eq!(active, 2);

        let _ = store.downloads.remove_job("model-a").await;
        let _ = store.downloads.remove_job("model-b").await;
    }

    #[tokio::test]
    async fn concurrency_pending_shrink_clears_after_job_completion() {
        let bus = arw_events::Bus::new_with_replay(8, 8);
        let store = Arc::new(ModelStore::new(bus, None));

        store
            .downloads
            .insert_job("model-a", dummy_download_handle("a"))
            .await
            .expect("insert job a");
        store
            .downloads
            .insert_job("model-b", dummy_download_handle("b"))
            .await
            .expect("insert job b");

        let snapshot = store.concurrency_set(Some(1), None, Some(false)).await;
        assert_eq!(snapshot.pending_shrink, Some(1));
        assert_eq!(snapshot.configured_max, 1);

        let removed = store.downloads.remove_job("model-b").await;
        assert!(removed.is_some(), "expected job removal");

        let follow_up = store.concurrency_get().await;
        assert_eq!(follow_up.pending_shrink, None);
        assert_eq!(follow_up.configured_max, 1);
        assert_eq!(follow_up.held_permits, 1);
        assert_eq!(follow_up.available_permits, 0);

        let _ = store.downloads.remove_job("model-a").await;
    }

    #[tokio::test]
    async fn budget_notifier_emits_degraded_once() {
        let bus = arw_events::Bus::new_with_replay(8, 8);
        let store = Arc::new(ModelStore::new(bus.clone(), None));
        let mut rx = bus.subscribe();

        let limits = Box::leak(Box::new(DownloadBudgetLimits {
            soft_ms: Some(100),
            hard_ms: Some(250),
            soft_degrade_pct: 50,
        }));
        let mut notifier = BudgetNotifier {
            limits,
            degraded_sent: false,
        };

        notifier.maybe_emit(
            store.as_ref(),
            "model-budget-test",
            Duration::from_millis(40),
            Some(5),
            Some(10),
            "corr-budget",
        );
        tokio::task::yield_now().await;
        assert!(matches!(rx.try_recv(), Err(TryRecvError::Empty)));

        notifier.maybe_emit(
            store.as_ref(),
            "model-budget-test",
            Duration::from_millis(60),
            Some(6),
            Some(12),
            "corr-budget",
        );
        let env = rx.recv().await.expect("budget event");
        assert_eq!(env.kind, topics::TOPIC_PROGRESS);
        let payload = env.payload;
        assert_eq!(
            payload.get("status").and_then(Value::as_str),
            Some("degraded")
        );
        assert_eq!(
            payload.get("code").and_then(Value::as_str),
            Some("soft-budget")
        );

        notifier.maybe_emit(
            store.as_ref(),
            "model-budget-test",
            Duration::from_millis(90),
            Some(9),
            Some(18),
            "corr-budget",
        );
        tokio::task::yield_now().await;
        assert!(matches!(rx.try_recv(), Err(TryRecvError::Empty)));
    }
}

fn env_flag(name: &str) -> bool {
    matches!(
        std::env::var(name)
            .ok()
            .map(|v| v.trim().to_ascii_lowercase()),
        Some(val) if matches!(val.as_str(), "1" | "true" | "yes" | "on")
    )
}

fn env_u64(name: &str) -> Option<u64> {
    std::env::var(name)
        .ok()
        .and_then(|s| s.parse::<u64>().ok())
        .filter(|value| *value > 0)
}

#[derive(Clone)]
struct CompletedDownload {
    sha256: String,
    bytes: u64,
    provider: Option<String>,
    url: Option<String>,
    elapsed: Duration,
    cached: bool,
}

enum DownloadOutcome {
    Completed {
        info: CompletedDownload,
        dest: DestInfo,
        corr_id: String,
    },
    Cached {
        info: CompletedDownload,
        dest: DestInfo,
        corr_id: String,
    },
    Canceled {
        elapsed: Duration,
        dest: DestInfo,
        corr_id: String,
    },
    Failed {
        code: String,
        message: String,
        elapsed: Duration,
        dest: DestInfo,
        corr_id: String,
        bytes_in: u64,
    },
}

#[derive(Clone, Deserialize, ToSchema)]
pub struct DownloadRequest {
    pub id: String,
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub provider: Option<String>,
    pub sha256: String,
}

#[derive(Clone, Deserialize, ToSchema)]
pub struct CasGcRequest {
    #[serde(default)]
    pub ttl_hours: Option<u64>,
    #[serde(default)]
    pub verbose: Option<bool>,
}

#[derive(Clone, Serialize, ToSchema)]
pub struct HashItem {
    pub sha256: String,
    pub bytes: u64,
    pub path: String,
    pub providers: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub models: Vec<String>,
}

#[derive(Clone, Serialize, ToSchema)]
pub struct HashPage {
    pub items: Vec<HashItem>,
    pub total: usize,
    pub count: usize,
    pub limit: usize,
    pub offset: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prev_offset: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_offset: Option<usize>,
    pub page: usize,
    pub pages: usize,
    pub last_offset: usize,
}

#[derive(Clone, Serialize)]
struct CasGcDeletedItem {
    sha256: String,
    path: String,
    bytes: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    last_modified: Option<String>,
}

type ManifestHashIndex = HashMap<String, ManifestHashRefs>;

#[derive(Clone, Default)]
struct ManifestHashRefs {
    bytes: u64,
    path: Option<String>,
    providers: HashSet<String>,
    models: HashSet<String>,
}

impl ManifestHashRefs {
    fn ingest_manifest(&mut self, entry: &Value) {
        if self.bytes == 0 {
            if let Some(bytes) = entry.get("bytes").and_then(|v| v.as_u64()) {
                if bytes > 0 {
                    self.bytes = bytes;
                }
            }
        }
        if self.path.as_ref().map(|p| p.is_empty()).unwrap_or(true) {
            if let Some(path) = entry.get("path").and_then(|v| v.as_str()) {
                if !path.is_empty() {
                    self.path = Some(path.to_string());
                }
            }
        }
        let provider = entry
            .get("provider")
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .unwrap_or("unknown");
        self.providers.insert(provider.to_string());
        if let Some(id) = entry.get("id").and_then(|v| v.as_str()) {
            self.models.insert(id.to_string());
        }
    }

    fn to_hash_item(&self, sha256: &str) -> HashItem {
        let mut providers: Vec<_> = self.providers.iter().cloned().collect();
        providers.sort();
        let mut models: Vec<_> = self.models.iter().cloned().collect();
        models.sort();
        HashItem {
            sha256: sha256.to_string(),
            bytes: self.bytes,
            path: self.path.clone().unwrap_or_default(),
            providers,
            models,
        }
    }
}
