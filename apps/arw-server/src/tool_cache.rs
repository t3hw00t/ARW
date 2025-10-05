use moka::future::Cache;
use once_cell::sync::Lazy;
#[cfg(test)]
use serde_json::json;
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use std::io::ErrorKind;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};
use tokio::fs;

use crate::singleflight::{FlightGuard, Singleflight};
use crate::util;

static DEFAULT_DENY_LIST: Lazy<HashSet<&'static str>> =
    Lazy::new(|| HashSet::from(["http.fetch", "fs.patch", "app.vscode.open"]));

static DEFAULT_DENY_PREFIXES: &[&str] = &[
    "http.", "net.", "fs.", "app.", "ui.", "proc.", "exec.", "project.",
];

fn parse_env_set(name: &str) -> Option<HashSet<String>> {
    let raw = std::env::var(name).ok()?.trim().to_string();
    if raw.is_empty() {
        return None;
    }
    let set = raw
        .split(',')
        .map(|item| item.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect::<HashSet<_>>();
    if set.is_empty() {
        None
    } else {
        Some(set)
    }
}

fn cache_capacity() -> u64 {
    std::env::var("ARW_TOOLS_CACHE_CAP")
        .ok()
        .and_then(|s| s.parse::<i64>().ok())
        .map(|n| n.max(0) as u64)
        .unwrap_or(2048)
}

fn cache_ttl() -> Duration {
    let secs = std::env::var("ARW_TOOLS_CACHE_TTL_SECS")
        .ok()
        .and_then(|s| s.parse::<i64>().ok())
        .map(|n| n.max(0) as u64)
        .unwrap_or(600);
    Duration::from_secs(secs.max(1))
}

const DEFAULT_MAX_PAYLOAD_BYTES: u64 = 4 * 1024 * 1024; // 4 MiB

fn parse_payload_limit(raw: &str) -> Result<Option<u64>, ()> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Ok(Some(DEFAULT_MAX_PAYLOAD_BYTES));
    }

    let lowered = trimmed.to_ascii_lowercase();
    if matches!(lowered.as_str(), "off" | "disable" | "disabled" | "none") {
        return Ok(None);
    }

    let mut split = 0usize;
    let mut has_digit = false;
    for (idx, ch) in trimmed.char_indices() {
        if ch.is_ascii_digit() || ch == '_' {
            has_digit = has_digit || ch.is_ascii_digit();
            split = idx + ch.len_utf8();
            continue;
        }
        split = idx;
        break;
    }

    if !has_digit {
        return Err(());
    }

    let (number_part, rest) = trimmed.split_at(split);
    let digits: String = number_part.chars().filter(|c| *c != '_').collect();
    if digits.is_empty() {
        return Err(());
    }

    let base = digits.parse::<u64>().map_err(|_| ())?;
    if base == 0 {
        return Ok(None);
    }

    let suffix = rest.trim().to_ascii_lowercase();
    let multiplier: u64 = match suffix.as_str() {
        "" | "b" | "byte" | "bytes" => 1,
        "k" | "kb" | "kib" => 1024,
        "m" | "mb" | "mib" => 1024u64.pow(2),
        "g" | "gb" | "gib" => 1024u64.pow(3),
        "t" | "tb" | "tib" => 1024u64.pow(4),
        _ => return Err(()),
    };

    match base.checked_mul(multiplier) {
        Some(bytes) if bytes > 0 => Ok(Some(bytes)),
        _ => Err(()),
    }
}

fn cache_max_payload_bytes() -> Option<u64> {
    match std::env::var("ARW_TOOLS_CACHE_MAX_PAYLOAD_BYTES") {
        Ok(raw) => match parse_payload_limit(&raw) {
            Ok(limit) => limit,
            Err(_) => {
                tracing::warn!(
                    "invalid ARW_TOOLS_CACHE_MAX_PAYLOAD_BYTES '{}'; using default {}",
                    raw,
                    DEFAULT_MAX_PAYLOAD_BYTES
                );
                Some(DEFAULT_MAX_PAYLOAD_BYTES)
            }
        },
        Err(_) => Some(DEFAULT_MAX_PAYLOAD_BYTES),
    }
}

fn canonicalize_json(value: &Value) -> Value {
    match value {
        Value::Object(map) => {
            let mut pairs: Vec<(&String, &Value)> = map.iter().collect();
            pairs.sort_by(|a, b| a.0.cmp(b.0));
            let mut out = Map::new();
            for (key, val) in pairs {
                out.insert(key.clone(), canonicalize_json(val));
            }
            Value::Object(out)
        }
        Value::Array(arr) => {
            let items = arr.iter().map(canonicalize_json).collect::<Vec<_>>();
            Value::Array(items)
        }
        _ => value.clone(),
    }
}

fn tool_version(tool_id: &str) -> &'static str {
    for info in arw_core::introspect_tools() {
        if info.id == tool_id {
            return info.version;
        }
    }
    "0.0.0"
}

fn env_signature() -> String {
    let mut pairs: Vec<(String, String)> = Vec::new();
    let capture = |key: &str, acc: &mut Vec<(String, String)>| {
        if let Ok(val) = std::env::var(key) {
            let trimmed = val.trim();
            if !trimmed.is_empty() {
                acc.push((key.to_string(), trimmed.to_string()));
            }
        }
    };

    for key in [
        "ARW_POLICY_VERSION",
        "ARW_POLICY_VER",
        "ARW_SECRETS_VERSION",
        "ARW_SECRETS_VER",
        "ARW_PROJECT_ID",
        "ARW_NET_POSTURE",
        "ARW_TOOLS_CACHE_SALT",
    ] {
        capture(key, &mut pairs);
    }

    let gating = arw_core::gating::snapshot();
    if let Ok(bytes) = serde_json::to_vec(&gating) {
        let mut hasher = Sha256::new();
        hasher.update(&bytes);
        pairs.push(("GATING".into(), format!("{:x}", hasher.finalize())));
    }

    pairs.sort_by(|a, b| a.0.cmp(&b.0));
    let mut out = String::new();
    for (key, value) in pairs {
        out.push_str(&key);
        out.push('=');
        out.push_str(&value);
        out.push(';');
    }
    out
}

fn compute_digest(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}

#[derive(Debug, Clone, Copy)]
struct EntryMetrics {
    payload_bytes: u64,
    miss_elapsed_ms: u64,
}

pub struct ToolCache {
    cache: Option<Cache<String, String>>,
    cas_dir: PathBuf,
    allow_list: Option<HashSet<String>>,
    deny_list: HashSet<String>,
    capacity: u64,
    ttl: Duration,
    max_payload_bytes: Option<u64>,
    stats: CacheCounters,
    flights: Singleflight,
    entry_metrics: Arc<Mutex<HashMap<String, EntryMetrics>>>,
    digest_ref_counts: Arc<Mutex<HashMap<String, u64>>>,
}

impl ToolCache {
    pub fn new() -> Self {
        let capacity = cache_capacity();
        let ttl = cache_ttl();
        let max_payload_bytes = cache_max_payload_bytes();
        let allow_list = parse_env_set("ARW_TOOLS_CACHE_ALLOW");
        let mut deny_list = parse_env_set("ARW_TOOLS_CACHE_DENY").unwrap_or_default();
        for entry in DEFAULT_DENY_LIST.iter() {
            deny_list.insert((*entry).to_string());
        }
        let cas_dir = util::state_dir().join("tools").join("by-digest");
        let cas_dir_shared = Arc::new(cas_dir.clone());
        let entry_metrics = Arc::new(Mutex::new(HashMap::new()));
        let digest_ref_counts = Arc::new(Mutex::new(HashMap::new()));
        let cache = if capacity == 0 {
            None
        } else {
            let eviction_metrics = Arc::clone(&entry_metrics);
            let eviction_counts = Arc::clone(&digest_ref_counts);
            let cas_dir_eviction = Arc::clone(&cas_dir_shared);
            Some(
                Cache::builder()
                    .max_capacity(capacity)
                    .time_to_live(ttl)
                    .async_eviction_listener(move |key: Arc<String>, digest: String, cause| {
                        let metrics = Arc::clone(&eviction_metrics);
                        let counts = Arc::clone(&eviction_counts);
                        let cas_dir = Arc::clone(&cas_dir_eviction);
                        Box::pin(async move {
                            if let Ok(mut map) = metrics.lock() {
                                map.remove(key.as_ref());
                            }

                            let mut remove_file = false;
                            let mut refs_remaining = 0u64;
                            if let Ok(mut counts) = counts.lock() {
                                if let Some(count) = counts.get_mut(&digest) {
                                    if *count > 0 {
                                        *count -= 1;
                                        refs_remaining = *count;
                                        if *count == 0 {
                                            counts.remove(&digest);
                                            remove_file = true;
                                        }
                                    }
                                } else {
                                    tracing::trace!(digest = %digest, "tool_cache eviction missing digest ref");
                                }
                            }

                            if remove_file {
                                let path = cas_dir.join(format!("{}.json", digest));
                                if let Err(err) = fs::remove_file(&path).await {
                                    if err.kind() != ErrorKind::NotFound {
                                        tracing::debug!(
                                            "tool_cache::evict failed to remove {}: {}",
                                            path.display(),
                                            err
                                        );
                                    }
                                }
                            } else {
                                tracing::trace!(
                                    digest = %digest,
                                    ?cause,
                                    refs_remaining,
                                    "tool_cache eviction retained digest"
                                );
                            }
                        })
                    })
                    .build(),
            )
        };
        Self {
            cache,
            cas_dir,
            allow_list,
            deny_list,
            capacity,
            ttl,
            max_payload_bytes,
            stats: CacheCounters::new(),
            flights: Singleflight::default(),
            entry_metrics,
            digest_ref_counts,
        }
    }

    pub fn enabled(&self) -> bool {
        self.cache.is_some()
    }

    pub fn stats(&self) -> ToolCacheStats {
        let hit = self.stats.hits.load(Ordering::Relaxed);
        let miss = self.stats.miss.load(Ordering::Relaxed);
        let coalesced = self.stats.coalesced.load(Ordering::Relaxed);
        let errors = self.stats.errors.load(Ordering::Relaxed);
        let bypass = self.stats.bypass.load(Ordering::Relaxed);
        let payload_too_large = self.stats.payload_too_large.load(Ordering::Relaxed);
        let entries = self.cache.as_ref().map(|c| c.entry_count()).unwrap_or(0);

        let latency_saved_ms_total = self.stats.latency_saved_ms.load(Ordering::Relaxed);
        let latency_saved_samples = self.stats.latency_saved_samples.load(Ordering::Relaxed);
        let avg_latency_saved_ms = if latency_saved_samples > 0 {
            latency_saved_ms_total as f64 / latency_saved_samples as f64
        } else {
            0.0
        };
        let last_latency_saved_ms = if latency_saved_samples > 0 {
            Some(self.stats.last_latency_saved_ms.load(Ordering::Relaxed))
        } else {
            None
        };

        let payload_bytes_saved_total = self.stats.payload_bytes_saved.load(Ordering::Relaxed);
        let payload_saved_samples = self.stats.payload_saved_samples.load(Ordering::Relaxed);
        let avg_payload_bytes_saved = if payload_saved_samples > 0 {
            payload_bytes_saved_total as f64 / payload_saved_samples as f64
        } else {
            0.0
        };
        let last_payload_bytes = if payload_saved_samples > 0 {
            Some(self.stats.last_payload_bytes.load(Ordering::Relaxed))
        } else {
            None
        };

        let hit_age_samples = self.stats.hit_age_samples.load(Ordering::Relaxed);
        let avg_hit_age_secs = if hit_age_samples > 0 {
            self.stats.hit_age_total.load(Ordering::Relaxed) as f64 / hit_age_samples as f64
        } else {
            0.0
        };
        let last_hit_age_secs = if hit_age_samples > 0 {
            Some(self.stats.last_hit_age_secs.load(Ordering::Relaxed))
        } else {
            None
        };
        let max_hit_age_secs = if hit_age_samples > 0 {
            Some(self.stats.max_hit_age_secs.load(Ordering::Relaxed))
        } else {
            None
        };

        let total_guarded = miss.saturating_add(coalesced);
        let stampede_suppression_rate = if total_guarded > 0 {
            coalesced as f64 / total_guarded as f64
        } else {
            0.0
        };

        ToolCacheStats {
            hit,
            miss,
            coalesced,
            errors,
            bypass,
            payload_too_large,
            capacity: self.capacity,
            ttl_secs: self.ttl.as_secs(),
            entries,
            max_payload_bytes: self.max_payload_bytes,
            latency_saved_ms_total,
            latency_saved_samples,
            avg_latency_saved_ms,
            payload_bytes_saved_total,
            payload_saved_samples,
            avg_payload_bytes_saved,
            avg_hit_age_secs,
            hit_age_samples,
            last_hit_age_secs,
            max_hit_age_secs,
            stampede_suppression_rate,
            last_latency_saved_ms,
            last_payload_bytes,
        }
    }

    pub fn is_cacheable(&self, tool_id: &str) -> bool {
        if let Some(allow) = &self.allow_list {
            return allow.contains(tool_id);
        }
        if self.deny_list.contains(tool_id) {
            return false;
        }
        DEFAULT_DENY_PREFIXES
            .iter()
            .all(|prefix| !tool_id.starts_with(prefix))
    }

    pub fn action_key(&self, tool_id: &str, input: &Value) -> String {
        let version = tool_version(tool_id);
        let mut hasher = Sha256::new();
        hasher.update(tool_id.as_bytes());
        hasher.update(b"@\0");
        hasher.update(version.as_bytes());
        hasher.update(b"\0");
        let env_sig = env_signature();
        hasher.update(b"env:\0");
        hasher.update(env_sig.as_bytes());
        hasher.update(b"\0");
        let canon = canonicalize_json(input);
        if let Ok(bytes) = serde_json::to_vec(&canon) {
            hasher.update(&bytes);
        }
        format!("{:x}", hasher.finalize())
    }

    fn cas_path(&self, digest: &str) -> PathBuf {
        self.cas_dir.join(format!("{}.json", digest))
    }

    fn update_entry_metrics(&self, key: &str, payload_bytes: u64, miss_elapsed_ms: u64) {
        if let Ok(mut map) = self.entry_metrics.lock() {
            map.insert(
                key.to_string(),
                EntryMetrics {
                    payload_bytes,
                    miss_elapsed_ms,
                },
            );
        }
    }

    fn clear_entry_metrics(&self, key: &str) {
        if let Ok(mut map) = self.entry_metrics.lock() {
            map.remove(key);
        }
    }

    fn increment_digest_ref(&self, digest: &str) {
        if let Ok(mut counts) = self.digest_ref_counts.lock() {
            let counter = counts.entry(digest.to_string()).or_insert(0);
            *counter = counter.saturating_add(1);
        }
    }

    fn entry_metrics(&self, key: &str) -> Option<EntryMetrics> {
        self.entry_metrics
            .lock()
            .ok()
            .and_then(|map| map.get(key).copied())
    }

    pub async fn lookup(&self, key: &str) -> Option<ToolCacheHit> {
        let cache = self.cache.as_ref()?;
        let digest = cache.get(key).await?;
        let path = self.cas_path(&digest);
        match fs::read(&path).await {
            Ok(bytes) => match serde_json::from_slice::<Value>(&bytes) {
                Ok(value) => {
                    self.stats.hits.fetch_add(1, Ordering::Relaxed);
                    let age_secs = fs::metadata(&path)
                        .await
                        .ok()
                        .and_then(|meta| meta.modified().ok())
                        .and_then(|ts| SystemTime::now().duration_since(ts).ok())
                        .map(|dur| dur.as_secs());
                    let payload_bytes = Some(bytes.len() as u64);
                    Some(ToolCacheHit {
                        value,
                        digest,
                        age_secs,
                        payload_bytes,
                    })
                }
                Err(err) => {
                    tracing::warn!("tool_cache::lookup deserialize error: {}", err);
                    self.stats.errors.fetch_add(1, Ordering::Relaxed);
                    cache.invalidate(key).await;
                    self.clear_entry_metrics(key);
                    None
                }
            },
            Err(err) => {
                tracing::debug!("tool_cache::lookup miss on disk: {}", err);
                self.stats.errors.fetch_add(1, Ordering::Relaxed);
                cache.invalidate(key).await;
                self.clear_entry_metrics(key);
                None
            }
        }
    }

    pub async fn store(&self, key: &str, value: &Value, miss_elapsed_ms: u64) -> StoreOutcome {
        let bytes = match serde_json::to_vec(value) {
            Ok(bytes) => bytes,
            Err(err) => {
                tracing::warn!("tool_cache::store failed to serialize value: {}", err);
                self.stats.errors.fetch_add(1, Ordering::Relaxed);
                return StoreOutcome::Failed {
                    digest: None,
                    payload_bytes: 0,
                    miss_elapsed_ms,
                    reason: StoreError::Serialize,
                };
            }
        };

        let payload_bytes = bytes.len() as u64;
        if let Some(limit) = self.max_payload_bytes {
            if payload_bytes > limit {
                tracing::debug!(
                    "tool_cache::store skipping key {} (payload {} exceeds limit {})",
                    key,
                    payload_bytes,
                    limit
                );
                self.record_payload_too_large();
                return StoreOutcome::Skipped {
                    reason: StoreSkipReason::PayloadTooLarge,
                    payload_bytes,
                    miss_elapsed_ms,
                };
            }
        }

        let digest = compute_digest(&bytes);
        if let Some(cache) = &self.cache {
            let path = self.cas_path(&digest);
            if fs::metadata(&path).await.is_err() {
                if let Some(parent) = path.parent() {
                    if let Err(err) = fs::create_dir_all(parent).await {
                        tracing::warn!(
                            "tool_cache::store failed to create dir {}: {}",
                            parent.display(),
                            err
                        );
                        self.stats.errors.fetch_add(1, Ordering::Relaxed);
                        return StoreOutcome::Failed {
                            digest: Some(digest),
                            payload_bytes,
                            miss_elapsed_ms,
                            reason: StoreError::CreateDir,
                        };
                    }
                }
                if let Err(err) = fs::write(&path, &bytes).await {
                    tracing::warn!(
                        "tool_cache::store failed to write digest {}: {}",
                        digest,
                        err
                    );
                    self.stats.errors.fetch_add(1, Ordering::Relaxed);
                    return StoreOutcome::Failed {
                        digest: Some(digest),
                        payload_bytes,
                        miss_elapsed_ms,
                        reason: StoreError::Write,
                    };
                }
            }
            cache.insert(key.to_string(), digest.clone()).await;
            self.increment_digest_ref(&digest);
            self.update_entry_metrics(key, payload_bytes, miss_elapsed_ms);
            self.stats.miss.fetch_add(1, Ordering::Relaxed);
            StoreOutcome::Cached {
                digest,
                payload_bytes,
                miss_elapsed_ms,
            }
        } else {
            tracing::debug!("tool_cache::store invoked while cache disabled; skipping");
            self.record_bypass();
            StoreOutcome::Skipped {
                reason: StoreSkipReason::CacheDisabled,
                payload_bytes,
                miss_elapsed_ms,
            }
        }
    }

    pub fn record_bypass(&self) {
        self.stats.bypass.fetch_add(1, Ordering::Relaxed);
    }

    pub fn record_payload_too_large(&self) {
        self.stats.payload_too_large.fetch_add(1, Ordering::Relaxed);
        self.record_bypass();
    }

    pub(crate) fn record_hit_metrics(
        &self,
        key: &str,
        hit: &ToolCacheHit,
        hit_elapsed_ms: u64,
    ) -> Option<u64> {
        if let Some(age) = hit.age_secs {
            self.stats.hit_age_total.fetch_add(age, Ordering::Relaxed);
            self.stats.hit_age_samples.fetch_add(1, Ordering::Relaxed);
            self.stats.last_hit_age_secs.store(age, Ordering::Relaxed);
            self.stats
                .max_hit_age_secs
                .fetch_max(age, Ordering::Relaxed);
        }

        if let Some(bytes) = hit.payload_bytes {
            self.stats
                .last_payload_bytes
                .store(bytes, Ordering::Relaxed);
        }

        let entry = self.entry_metrics(key);
        if let Some(entry) = entry {
            let saved = entry.miss_elapsed_ms.saturating_sub(hit_elapsed_ms);
            self.stats
                .latency_saved_ms
                .fetch_add(saved, Ordering::Relaxed);
            self.stats
                .latency_saved_samples
                .fetch_add(1, Ordering::Relaxed);
            self.stats
                .last_latency_saved_ms
                .store(saved, Ordering::Relaxed);
            self.stats
                .payload_bytes_saved
                .fetch_add(entry.payload_bytes, Ordering::Relaxed);
            self.stats
                .payload_saved_samples
                .fetch_add(1, Ordering::Relaxed);
            self.stats
                .last_payload_bytes
                .store(entry.payload_bytes, Ordering::Relaxed);
            Some(saved)
        } else {
            None
        }
    }

    pub(crate) fn begin_singleflight(&self, key: &str) -> FlightGuard<'_> {
        self.flights.begin(key)
    }

    pub(crate) fn record_coalesced_wait(&self) {
        self.stats.coalesced.fetch_add(1, Ordering::Relaxed);
    }

    pub fn max_payload_bytes(&self) -> Option<u64> {
        self.max_payload_bytes
    }

    #[cfg(test)]
    async fn run_pending_tasks(&self) {
        if let Some(cache) = &self.cache {
            cache.run_pending_tasks().await;
        }
    }

    #[cfg(test)]
    fn digest_ref_count(&self, digest: &str) -> u64 {
        self.digest_ref_counts
            .lock()
            .ok()
            .and_then(|map| map.get(digest).copied())
            .unwrap_or(0)
    }

    #[cfg(test)]
    async fn invalidate_key(&self, key: &str) {
        if let Some(cache) = &self.cache {
            cache.invalidate(key).await;
        }
    }
}
pub struct ToolCacheHit {
    pub value: Value,
    pub digest: String,
    pub age_secs: Option<u64>,
    pub payload_bytes: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StoreOutcome {
    Cached {
        digest: String,
        payload_bytes: u64,
        miss_elapsed_ms: u64,
    },
    Skipped {
        reason: StoreSkipReason,
        payload_bytes: u64,
        miss_elapsed_ms: u64,
    },
    Failed {
        digest: Option<String>,
        payload_bytes: u64,
        miss_elapsed_ms: u64,
        reason: StoreError,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StoreSkipReason {
    PayloadTooLarge,
    CacheDisabled,
}

impl StoreSkipReason {
    pub(crate) fn as_str(&self) -> &'static str {
        match self {
            StoreSkipReason::PayloadTooLarge => "payload_too_large",
            StoreSkipReason::CacheDisabled => "cache_disabled",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StoreError {
    Serialize,
    CreateDir,
    Write,
}

impl StoreError {
    pub(crate) fn as_str(&self) -> &'static str {
        match self {
            StoreError::Serialize => "serialize_failed",
            StoreError::CreateDir => "create_dir_failed",
            StoreError::Write => "store_failed",
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct ToolCacheStats {
    pub hit: u64,
    pub miss: u64,
    pub coalesced: u64,
    pub errors: u64,
    pub bypass: u64,
    pub payload_too_large: u64,
    pub capacity: u64,
    pub ttl_secs: u64,
    pub entries: u64,
    pub max_payload_bytes: Option<u64>,
    pub latency_saved_ms_total: u64,
    pub latency_saved_samples: u64,
    pub avg_latency_saved_ms: f64,
    pub payload_bytes_saved_total: u64,
    pub payload_saved_samples: u64,
    pub avg_payload_bytes_saved: f64,
    pub avg_hit_age_secs: f64,
    pub hit_age_samples: u64,
    pub last_hit_age_secs: Option<u64>,
    pub max_hit_age_secs: Option<u64>,
    pub stampede_suppression_rate: f64,
    pub last_latency_saved_ms: Option<u64>,
    pub last_payload_bytes: Option<u64>,
}

struct CacheCounters {
    hits: AtomicU64,
    miss: AtomicU64,
    coalesced: AtomicU64,
    errors: AtomicU64,
    bypass: AtomicU64,
    payload_too_large: AtomicU64,
    latency_saved_ms: AtomicU64,
    latency_saved_samples: AtomicU64,
    last_latency_saved_ms: AtomicU64,
    payload_bytes_saved: AtomicU64,
    payload_saved_samples: AtomicU64,
    last_payload_bytes: AtomicU64,
    hit_age_total: AtomicU64,
    hit_age_samples: AtomicU64,
    last_hit_age_secs: AtomicU64,
    max_hit_age_secs: AtomicU64,
}

impl CacheCounters {
    fn new() -> Self {
        Self {
            hits: AtomicU64::new(0),
            miss: AtomicU64::new(0),
            coalesced: AtomicU64::new(0),
            errors: AtomicU64::new(0),
            bypass: AtomicU64::new(0),
            payload_too_large: AtomicU64::new(0),
            latency_saved_ms: AtomicU64::new(0),
            latency_saved_samples: AtomicU64::new(0),
            last_latency_saved_ms: AtomicU64::new(0),
            payload_bytes_saved: AtomicU64::new(0),
            payload_saved_samples: AtomicU64::new(0),
            last_payload_bytes: AtomicU64::new(0),
            hit_age_total: AtomicU64::new(0),
            hit_age_samples: AtomicU64::new(0),
            last_hit_age_secs: AtomicU64::new(0),
            max_hit_age_secs: AtomicU64::new(0),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[tokio::test]
    async fn cache_roundtrip() {
        let tmp = tempdir().unwrap();
        let mut ctx = crate::test_support::begin_state_env(tmp.path());
        ctx.env.set("ARW_TOOLS_CACHE_CAP", "16");
        ctx.env.set("ARW_TOOLS_CACHE_TTL_SECS", "60");
        let cache = ToolCache::new();
        assert!(cache.enabled());
        assert!(cache.is_cacheable("demo.echo"));
        let payload = json!({"hello": "world"});
        let key = cache.action_key("demo.echo", &payload);
        assert!(cache.lookup(&key).await.is_none());
        let (digest, payload_bytes) = match cache.store(&key, &payload, 42).await {
            StoreOutcome::Cached {
                digest,
                payload_bytes,
                miss_elapsed_ms,
            } => {
                assert_eq!(
                    payload_bytes,
                    serde_json::to_vec(&payload).unwrap().len() as u64
                );
                assert_eq!(miss_elapsed_ms, 42);
                (digest, payload_bytes)
            }
            other => panic!("unexpected store outcome: {:?}", other),
        };
        let hit = cache.lookup(&key).await.expect("cache hit");
        assert_eq!(hit.value, payload);
        assert_eq!(hit.digest, digest);
        assert_eq!(hit.payload_bytes, Some(payload_bytes));
        assert!(cache.cas_path(&hit.digest).exists());
        let saved = cache
            .record_hit_metrics(&key, &hit, 10)
            .expect("latency saved");
        assert_eq!(saved, 32);
        let stats = cache.stats();
        assert_eq!(stats.hit, 1);
        assert_eq!(stats.miss, 1);
        assert_eq!(stats.latency_saved_ms_total, 32);
        assert_eq!(stats.latency_saved_samples, 1);
        assert!(stats.avg_hit_age_secs >= 0.0);
        // ctx holds state/env guards until end of scope
    }

    #[tokio::test]
    async fn allow_list_overrides_defaults() {
        let tmp = tempdir().unwrap();
        let mut ctx = crate::test_support::begin_state_env(tmp.path());
        ctx.env.set("ARW_TOOLS_CACHE_CAP", "16");
        ctx.env.set("ARW_TOOLS_CACHE_TTL_SECS", "60");
        ctx.env.set("ARW_TOOLS_CACHE_ALLOW", "demo.echo");
        let cache = ToolCache::new();
        assert!(cache.is_cacheable("demo.echo"));
        assert!(!cache.is_cacheable("guardrails.check"));
        // ctx holds state/env guards until end of scope
    }

    #[tokio::test]
    async fn store_skips_payloads_over_limit() {
        let tmp = tempdir().unwrap();
        let mut ctx = crate::test_support::begin_state_env(tmp.path());
        ctx.env.set("ARW_TOOLS_CACHE_CAP", "16");
        ctx.env.set("ARW_TOOLS_CACHE_TTL_SECS", "60");
        ctx.env.set("ARW_TOOLS_CACHE_MAX_PAYLOAD_BYTES", "64");
        let cache = ToolCache::new();
        assert!(cache.enabled());
        let payload = json!({ "blob": "a".repeat(256) });
        let key = cache.action_key("demo.echo", &payload);
        match cache.store(&key, &payload, 99).await {
            StoreOutcome::Skipped {
                reason: StoreSkipReason::PayloadTooLarge,
                payload_bytes,
                ..
            } => {
                assert!(payload_bytes > 64);
            }
            other => panic!("expected payload-too-large skip, got {:?}", other),
        }
        assert!(cache.lookup(&key).await.is_none());
        let stats = cache.stats();
        assert_eq!(stats.payload_too_large, 1);
        assert_eq!(stats.bypass, 1);
        assert_eq!(stats.entries, 0);
    }

    #[tokio::test]
    async fn eviction_removes_cas_files() {
        let tmp = tempdir().unwrap();
        let mut ctx = crate::test_support::begin_state_env(tmp.path());
        ctx.env.set("ARW_TOOLS_CACHE_CAP", "16");
        ctx.env.set("ARW_TOOLS_CACHE_TTL_SECS", "60");
        let cache = ToolCache::new();
        assert!(cache.enabled());

        let payload = json!({ "run": 1 });
        let key = cache.action_key("demo.echo", &payload);
        let digest = match cache.store(&key, &payload, 50).await {
            StoreOutcome::Cached { digest, .. } => digest,
            other => panic!("expected cached outcome, got {:?}", other),
        };

        let path = cache.cas_path(&digest);
        assert!(path.exists(), "digest should be materialised");
        assert_eq!(cache.digest_ref_count(&digest), 1);

        cache.invalidate_key(&key).await;
        cache.run_pending_tasks().await;

        for _ in 0..10 {
            if !path.exists() && cache.digest_ref_count(&digest) == 0 {
                break;
            }
            cache.run_pending_tasks().await;
            tokio::time::sleep(Duration::from_millis(20)).await;
        }

        assert!(
            !path.exists(),
            "invalidating digest {} should remove CAS file (ref_count={})",
            digest,
            cache.digest_ref_count(&digest)
        );
        assert_eq!(cache.digest_ref_count(&digest), 0);
    }

    #[tokio::test]
    async fn payload_limit_parses_units_and_disables() {
        let tmp = tempdir().unwrap();
        let mut ctx = crate::test_support::begin_state_env(tmp.path());
        ctx.env.set("ARW_TOOLS_CACHE_CAP", "16");
        ctx.env.set("ARW_TOOLS_CACHE_TTL_SECS", "60");

        ctx.env.set("ARW_TOOLS_CACHE_MAX_PAYLOAD_BYTES", "512kb");
        let cache_kb = ToolCache::new();
        assert_eq!(cache_kb.stats().max_payload_bytes, Some(512 * 1024));

        ctx.env.set("ARW_TOOLS_CACHE_MAX_PAYLOAD_BYTES", "off");
        let cache_off = ToolCache::new();
        assert_eq!(cache_off.stats().max_payload_bytes, None);

        ctx.env
            .set("ARW_TOOLS_CACHE_MAX_PAYLOAD_BYTES", "junk-value");
        let cache_default = ToolCache::new();
        assert_eq!(
            cache_default.stats().max_payload_bytes,
            Some(DEFAULT_MAX_PAYLOAD_BYTES)
        );

        ctx.env.set("ARW_TOOLS_CACHE_MAX_PAYLOAD_BYTES", "2MB");
        let cache_mb = ToolCache::new();
        assert_eq!(cache_mb.stats().max_payload_bytes, Some(2 * 1024 * 1024));

        ctx.env.set("ARW_TOOLS_CACHE_MAX_PAYLOAD_BYTES", "0");
        let cache_zero = ToolCache::new();
        assert_eq!(cache_zero.stats().max_payload_bytes, None);
    }
}
