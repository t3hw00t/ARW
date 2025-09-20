use arw_events::Bus;
use arw_topics as topics;
use chrono::{DateTime, Utc};
use fs2::available_space;
use futures_util::StreamExt;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Number, Value};
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::fs;
use tokio::io::AsyncWriteExt;
use tokio::sync::{Mutex, Notify, RwLock};
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;
use tracing::{error, warn};

use crate::{http_timeout, read_models, util};
use once_cell::sync::OnceCell;
use utoipa::ToSchema;

const DEFAULT_CONCURRENCY: u64 = 2;
const DOWNLOAD_EVENT_KIND: &str = "models.download.progress";
const PROGRESS_EMIT_BYTES: u64 = 5 * 1024 * 1024; // 5 MiB
const PROGRESS_EMIT_INTERVAL: Duration = Duration::from_millis(750);

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
}

impl MetricsState {
    fn snapshot(&self) -> Value {
        json!({
            "started": self.started,
            "queued": self.queued,
            "admitted": self.admitted,
            "resumed": self.resumed,
            "canceled": self.canceled,
            "completed": self.completed,
            "completed_cached": self.completed_cached,
            "errors": self.errors,
            "bytes_total": self.bytes_total,
            "ewma_mbps": self.ewma_mbps,
        })
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
}

struct DownloadHandle {
    cancel: CancellationToken,
    task: Option<JoinHandle<()>>,
    job_id: String,
    url: Option<String>,
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

    async fn cancel_job(&self, model_id: &str) -> Option<()> {
        let handle = {
            let mut jobs = self.jobs.lock().await;
            jobs.remove(model_id)
        };
        if let Some(handle) = handle {
            handle.cancel.cancel();
            self.notify.notify_waiters();
            if let Some(task) = handle.task {
                tokio::spawn(async move {
                    if let Err(err) = task.await {
                        warn!("cancelled download join err: {err}");
                    }
                });
            }
            Some(())
        } else {
            None
        }
    }

    async fn job_snapshot(&self) -> Vec<Value> {
        let jobs = self.jobs.lock().await;
        jobs.iter()
            .map(|(model_id, handle)| {
                json!({
                    "model_id": model_id,
                    "job_id": handle.job_id,
                    "url": handle.url,
                    "started_at": handle.started_at.elapsed().as_secs(),
                })
            })
            .collect()
    }
}

pub struct ModelStore {
    items: RwLock<Vec<Value>>,
    default_id: RwLock<String>,
    concurrency: RwLock<ConcurrencyState>,
    metrics: RwLock<MetricsState>,
    downloads: DownloadsState,
    http_client: Client,
    bus: Bus,
}

impl ModelStore {
    pub fn new(bus: Bus) -> Self {
        Self {
            items: RwLock::new(Vec::new()),
            default_id: RwLock::new(String::new()),
            concurrency: RwLock::new(ConcurrencyState::new()),
            metrics: RwLock::new(MetricsState::default()),
            downloads: DownloadsState::new(),
            http_client: Client::new(),
            bus,
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
        let concurrency = self.concurrency_snapshot().await;
        json!({
            "items": items,
            "default": default,
            "concurrency": concurrency,
            "metrics": metrics,
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

    pub async fn concurrency_get(&self) -> Value {
        self.concurrency_snapshot().await
    }

    pub async fn concurrency_set(
        &self,
        configured_max: Option<u64>,
        hard_cap: Option<u64>,
    ) -> Value {
        {
            let mut state = self.concurrency.write().await;
            if let Some(max) = configured_max {
                state.configured_max = max.max(1);
            }
            state.hard_cap = hard_cap.filter(|v| *v > 0);
        }
        self.downloads.notify.notify_waiters();
        self.concurrency_snapshot().await
    }

    pub async fn metrics_value(&self) -> Value {
        self.metrics.read().await.clone().snapshot()
    }

    pub async fn jobs_snapshot(&self) -> Value {
        let active = self.downloads.job_snapshot().await;
        let concurrency = self.concurrency_snapshot().await;
        json!({
            "active": active,
            "inflight_hashes": [],
            "concurrency": concurrency,
        })
    }

    pub async fn hashes_page(
        &self,
        limit: usize,
        offset: usize,
        provider: Option<String>,
        sort: Option<String>,
        order: Option<String>,
    ) -> Value {
        let items = self.items.read().await.clone();
        let mut map: HashMap<String, (u64, String, HashSet<String>)> = HashMap::new();
        for entry in items.iter() {
            let Some(hash) = entry.get("sha256").and_then(|v| v.as_str()) else {
                continue;
            };
            if hash.len() != 64 {
                continue;
            }
            let bytes = entry.get("bytes").and_then(|v| v.as_u64()).unwrap_or(0);
            let path = entry
                .get("path")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let provider_val = entry
                .get("provider")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown")
                .to_string();
            let bucket =
                map.entry(hash.to_string())
                    .or_insert((bytes, path.clone(), HashSet::new()));
            if bucket.0 == 0 && bytes > 0 {
                bucket.0 = bytes;
            }
            if bucket.1.is_empty() && !path.is_empty() {
                bucket.1 = path;
            }
            bucket.2.insert(provider_val);
        }
        let mut rows: Vec<HashItem> = map
            .into_iter()
            .map(|(sha256, (bytes, path, providers))| HashItem {
                sha256,
                bytes,
                path,
                providers: providers.into_iter().collect(),
            })
            .collect();
        if let Some(filter) = provider.as_ref() {
            rows.retain(|row| row.providers.iter().any(|p| p == filter));
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
        let offset = offset.min(total);
        let limit = limit.clamp(1, 10_000);
        let end = offset.saturating_add(limit).min(total);
        let slice = rows[offset..end].to_vec();
        json!({
            "items": slice,
            "total": total,
            "count": end.saturating_sub(offset),
            "limit": limit,
            "offset": offset,
        })
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

        let started_at = Instant::now();
        self.with_metrics(|m| m.record_started()).await;
        let start_extra = self.progress_extra_with_hints(
            Some(json!({"url": url})),
            Some(Duration::from_secs(0)),
            None,
            None,
        );
        self.publish_progress(id, Some("started"), None, start_extra, None);
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

        let handle = DownloadHandle {
            cancel,
            task: None,
            job_id: job_id.clone(),
            url: req.url.clone(),
            started_at,
        };

        self.downloads
            .insert_job(id, handle)
            .await
            .map_err(|_| "download already in progress".to_string())?;

        let job_handle = tokio::spawn(async move {
            let outcome = runner
                .run_download_job(
                    model_id.clone(),
                    url_clone,
                    provider_clone,
                    sha_clone,
                    cancel_clone,
                    started_at,
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
        if self.downloads.cancel_job(id).await.is_some() {
            let extra = self.progress_extra_with_hints(None, None, None, None);
            self.publish_progress(id, Some("canceled"), None, extra, None);
            self.with_metrics(|m| m.record_canceled()).await;
            self.upsert_model_status(id, "canceled", None, None).await;
            Ok(())
        } else {
            let extra = self.progress_extra_with_hints(None, None, None, None);
            self.publish_progress(id, Some("no-active-job"), None, extra, None);
            Err("no active download".into())
        }
    }

    pub async fn cas_gc(&self, req: CasGcRequest) -> Result<Value, String> {
        let ttl_hours = req.ttl_hours.unwrap_or(24);
        let cutoff = chrono::Utc::now() - chrono::Duration::hours(ttl_hours as i64);
        let referenced = self.referenced_hashes().await;
        let cas_dir = self.cas_dir();
        let mut scanned = 0u64;
        let mut kept = 0u64;
        let mut deleted = 0u64;
        let mut deleted_bytes = 0u64;

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
                if referenced.contains(&fname) {
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
                let modified: DateTime<Utc> = meta
                    .modified()
                    .ok()
                    .map(DateTime::<Utc>::from)
                    .unwrap_or_else(Utc::now);
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
            }
        }

        let payload = json!({
            "scanned": scanned,
            "kept": kept,
            "deleted": deleted,
            "deleted_bytes": deleted_bytes,
            "ttl_hours": ttl_hours,
        });
        self.bus.publish(topics::TOPIC_MODELS_CAS_GC, &payload);
        Ok(payload)
    }

    async fn run_download_job(
        self: Arc<Self>,
        model_id: String,
        url: String,
        provider: Option<String>,
        sha_hint: String,
        cancel: CancellationToken,
        started_at: Instant,
    ) -> DownloadOutcome {
        self.with_metrics(|m| m.record_admitted()).await;
        let initial_extra =
            self.progress_extra_with_hints(None, Some(started_at.elapsed()), None, None);
        self.publish_progress(&model_id, Some("downloading"), None, initial_extra, None);

        let start = started_at;
        let limits = DownloadBudgetLimits::global();
        let mut budget_notifier = BudgetNotifier::new(limits);
        let tmp_dir = self.models_dir().join("tmp");
        if let Err(err) = fs::create_dir_all(&tmp_dir).await {
            error!("models tmp dir create failed: {err}");
            return DownloadOutcome::Failed {
                code: "io".into(),
                message: err.to_string(),
                elapsed: start.elapsed(),
            };
        }
        let tmp_path = tmp_dir.join(format!("{}.part", uuid::Uuid::new_v4()));

        let response = match self
            .http_client
            .get(&url)
            .timeout(http_timeout::get_duration())
            .send()
            .await
            .and_then(|r| r.error_for_status())
        {
            Ok(resp) => resp,
            Err(err) => {
                error!("model download http error: {err}");
                let code = if err.is_timeout() {
                    "request-timeout"
                } else {
                    "http"
                };
                return DownloadOutcome::Failed {
                    code: code.into(),
                    message: err.to_string(),
                    elapsed: start.elapsed(),
                };
            }
        };

        let total = response.content_length();
        let mut stream = response.bytes_stream();
        let mut file = match fs::File::create(&tmp_path).await {
            Ok(f) => f,
            Err(err) => {
                error!("model download file create failed: {err}");
                return DownloadOutcome::Failed {
                    code: "io".into(),
                    message: err.to_string(),
                    elapsed: start.elapsed(),
                };
            }
        };

        let mut hasher = Sha256::new();
        let mut downloaded: u64 = 0;
        let mut last_emit_bytes = 0u64;
        let mut last_emit_at = Instant::now();
        let mut last_disk_check = Instant::now();

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
                    return DownloadOutcome::Failed {
                        code: "size_limit".into(),
                        message: format!(
                            "download size {total_bytes} exceeds max {max} (ARW_MODELS_MAX_MB)"
                        ),
                        elapsed: start.elapsed(),
                    };
                }
            }
        }
        if let Some(quota) = quota_bytes {
            if let Some(total_bytes) = total {
                if cas_usage_bytes.saturating_add(total_bytes) > quota {
                    return DownloadOutcome::Failed {
                        code: "quota_exceeded".into(),
                        message: format!(
                            "quota {quota} bytes would be exceeded by download ({total_bytes} bytes)"
                        ),
                        elapsed: start.elapsed(),
                    };
                }
            }
        }
        if reserve_bytes > 0 {
            if let Ok(avail) = Self::available_space(self.state_dir()) {
                if avail <= reserve_bytes {
                    return DownloadOutcome::Failed {
                        code: "disk_insufficient".into(),
                        message: format!(
                            "available disk {avail} <= reserve {reserve_bytes} (ARW_MODELS_DISK_RESERVE_MB)"
                        ),
                        elapsed: start.elapsed(),
                    };
                }
                if let Some(total_bytes) = total {
                    if avail.saturating_sub(reserve_bytes) < total_bytes {
                        return DownloadOutcome::Failed {
                            code: "disk_insufficient".into(),
                            message: format!(
                                "not enough free space for download: need {total_bytes} + reserve {reserve_bytes}, only {avail} available"
                            ),
                            elapsed: start.elapsed(),
                        };
                    }
                }
            }
        }

        loop {
            let next = tokio::select! {
                chunk = stream.next() => chunk,
                _ = cancel.cancelled() => {
                    let _ = fs::remove_file(&tmp_path).await;
                    return DownloadOutcome::Canceled {
                        elapsed: start.elapsed(),
                    };
                }
            };
            let Some(next) = next else {
                break;
            };
            let chunk = match next {
                Ok(c) => c,
                Err(err) => {
                    error!("model download chunk error: {err}");
                    let _ = fs::remove_file(&tmp_path).await;
                    return DownloadOutcome::Failed {
                        code: "http".into(),
                        message: err.to_string(),
                        elapsed: start.elapsed(),
                    };
                }
            };
            if let Err(err) = file.write_all(&chunk).await {
                error!("model download write error: {err}");
                let _ = fs::remove_file(&tmp_path).await;
                return DownloadOutcome::Failed {
                    code: "io".into(),
                    message: err.to_string(),
                    elapsed: start.elapsed(),
                };
            }
            hasher.update(&chunk);
            downloaded = downloaded.saturating_add(chunk.len() as u64);

            if let Some(max) = max_bytes {
                if downloaded > max {
                    let _ = fs::remove_file(&tmp_path).await;
                    return DownloadOutcome::Failed {
                        code: "size_limit".into(),
                        message: format!("download exceeded max size {max} (ARW_MODELS_MAX_MB)"),
                        elapsed: start.elapsed(),
                    };
                }
            }
            if let Some(quota) = quota_bytes {
                if cas_usage_bytes.saturating_add(downloaded) > quota {
                    let _ = fs::remove_file(&tmp_path).await;
                    return DownloadOutcome::Failed {
                        code: "quota_exceeded".into(),
                        message: format!(
                            "quota {quota} bytes exceeded (downloaded {downloaded}, existing {cas_usage_bytes})"
                        ),
                        elapsed: start.elapsed(),
                    };
                }
            }
            if reserve_bytes > 0 && last_disk_check.elapsed() >= Duration::from_secs(1) {
                if let Ok(avail) = Self::available_space(self.state_dir()) {
                    if avail <= reserve_bytes {
                        let _ = fs::remove_file(&tmp_path).await;
                        return DownloadOutcome::Failed {
                            code: "disk_insufficient".into(),
                            message: format!(
                                "download aborted: free space {avail} <= reserve {reserve_bytes}"
                            ),
                            elapsed: start.elapsed(),
                        };
                    }
                }
                last_disk_check = Instant::now();
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
                self.publish_progress(&model_id, Some("downloading"), None, extra, None);
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
            );

            if let Some(hard_ms) = limits.hard_ms {
                if duration_to_millis(elapsed_now) >= hard_ms {
                    let _ = fs::remove_file(&tmp_path).await;
                    return DownloadOutcome::Failed {
                        code: "hard-budget".into(),
                        message: format!(
                            "download exceeded hard budget {hard_ms} ms (ARW_BUDGET_DOWNLOAD_HARD_MS)"
                        ),
                        elapsed: elapsed_now,
                    };
                }
            }
        }

        if let Err(err) = file.sync_all().await {
            error!("model download sync error: {err}");
            let _ = fs::remove_file(&tmp_path).await;
            return DownloadOutcome::Failed {
                code: "io".into(),
                message: err.to_string(),
                elapsed: start.elapsed(),
            };
        }

        let sha256 = format!("{:x}", hasher.finalize());
        if !sha_hint.eq_ignore_ascii_case(&sha256) {
            let _ = fs::remove_file(&tmp_path).await;
            return DownloadOutcome::Failed {
                code: "sha256_mismatch".into(),
                message: format!("expected {sha_hint}, got {sha256}"),
                elapsed: start.elapsed(),
            };
        }

        let cas_path = self.cas_dir().join(&sha256);
        let cas_exists = cas_path.exists();
        if !cas_exists {
            if let Some(parent) = cas_path.parent() {
                if let Err(err) = fs::create_dir_all(parent).await {
                    error!("cas dir create failed: {err}");
                    let _ = fs::remove_file(&tmp_path).await;
                    return DownloadOutcome::Failed {
                        code: "io".into(),
                        message: err.to_string(),
                        elapsed: start.elapsed(),
                    };
                }
            }
            if let Err(err) = fs::rename(&tmp_path, &cas_path).await {
                error!("cas rename failed: {err}");
                let _ = fs::remove_file(&tmp_path).await;
                return DownloadOutcome::Failed {
                    code: "io".into(),
                    message: err.to_string(),
                    elapsed: start.elapsed(),
                };
            }
        } else if let Err(err) = fs::remove_file(&tmp_path).await {
            warn!("tmp cleanup failed: {err}");
        }

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
            DownloadOutcome::Cached(success)
        } else {
            DownloadOutcome::Completed(success)
        }
    }

    async fn finish_download(&self, model_id: String, outcome: DownloadOutcome) {
        match outcome {
            DownloadOutcome::Completed(info) => {
                self.handle_success(&model_id, info, false).await;
            }
            DownloadOutcome::Cached(info) => {
                self.handle_success(&model_id, info, true).await;
            }
            DownloadOutcome::Canceled { elapsed } => {
                self.with_metrics(|m| m.record_canceled()).await;
                let extra = self.progress_extra_with_hints(None, Some(elapsed), None, None);
                self.publish_progress(&model_id, Some("canceled"), None, extra, None);
                self.upsert_model_status(&model_id, "canceled", None, None)
                    .await;
            }
            DownloadOutcome::Failed {
                code,
                message,
                elapsed,
            } => {
                self.with_metrics(|m| m.record_error()).await;
                let extra = self.progress_extra_with_hints(
                    Some(json!({"error": message})),
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
                );
                self.mark_error(&model_id, &code, &message).await;
            }
        }
        self.downloads.remove_job(&model_id).await;
        self.emit_patch().await;
    }

    async fn handle_success(&self, model_id: &str, info: CompletedDownload, cached: bool) {
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
        self.publish_progress(model_id, Some("complete"), None, extra, None);
        if let Err(err) = self.record_success(model_id, &info).await {
            warn!("record success failed: {err}");
        }
    }

    async fn record_success(&self, model_id: &str, info: &CompletedDownload) -> Result<(), String> {
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
        self.bus
            .publish(topics::TOPIC_MODELS_MANIFEST_WRITTEN, &manifest);
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
        if let Err(err) = self.persist_items_snapshot().await {
            warn!("persist models after error failed: {err}");
        }
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
        if let Err(err) = self.persist_items_snapshot().await {
            warn!("persist models after status update failed: {err}");
        }
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
        read_models::publish_read_model_patch(&self.bus, "models_metrics", &snapshot);
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
        read_models::publish_read_model_patch(&self.bus, "models_metrics", &snapshot);
    }

    fn publish_progress(
        &self,
        id: &str,
        status: Option<&str>,
        code: Option<&str>,
        extra: Option<Value>,
        error_code: Option<String>,
    ) {
        let mut obj = Map::new();
        obj.insert("id".into(), Value::String(id.to_string()));
        if let Some(status) = status {
            obj.insert("status".into(), Value::String(status.into()));
        }
        if let Some(code) = code {
            obj.insert("code".into(), Value::String(code.into()));
        }
        if let Some(Value::Object(map)) = extra {
            for (k, v) in map {
                obj.insert(k, v);
            }
        }
        if let Some(code) = error_code {
            obj.insert("error_code".into(), Value::String(code));
        }
        self.bus.publish(DOWNLOAD_EVENT_KIND, &Value::Object(obj));
    }

    async fn concurrency_snapshot(&self) -> Value {
        let state = self.concurrency.read().await.clone();
        let configured = state.configured();
        let active = self.downloads.active_count().await as u64;
        let available = configured.saturating_sub(active);
        json!({
            "configured_max": configured,
            "available_permits": available,
            "held_permits": active.min(configured),
            "hard_cap": state.hard_cap,
            "pending_shrink": null,
        })
    }

    async fn replace_items(&self, items: Vec<Value>) {
        {
            let mut guard = self.items.write().await;
            *guard = items;
        }
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

    async fn referenced_hashes(&self) -> HashSet<String> {
        let mut set = HashSet::new();
        let items = self.items.read().await;
        for entry in items.iter() {
            if let Some(hash) = entry.get("sha256").and_then(|v| v.as_str()) {
                set.insert(hash.to_string());
            }
        }
        set
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
        store.publish_progress(model_id, Some("degraded"), Some("soft-budget"), extra, None);
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

struct CompletedDownload {
    sha256: String,
    bytes: u64,
    provider: Option<String>,
    url: Option<String>,
    elapsed: Duration,
    cached: bool,
}

enum DownloadOutcome {
    Completed(CompletedDownload),
    Cached(CompletedDownload),
    Canceled {
        elapsed: Duration,
    },
    Failed {
        code: String,
        message: String,
        elapsed: Duration,
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
}

#[derive(Clone, Serialize, ToSchema)]
pub struct HashItem {
    pub sha256: String,
    pub bytes: u64,
    pub path: String,
    pub providers: Vec<String>,
}
