use moka::future::Cache;
use once_cell::sync::Lazy;
#[cfg(test)]
use serde_json::json;
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime};
use tokio::fs;

use crate::util;

static DEFAULT_DENY_LIST: Lazy<HashSet<&'static str>> =
    Lazy::new(|| HashSet::from(["http.fetch", "fs.patch", "app.vscode.open"]));

static DEFAULT_DENY_PREFIXES: &[&str] = &["http.", "net.", "fs.", "app.", "ui.", "proc.", "exec."];

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

pub struct ToolCache {
    cache: Option<Cache<String, String>>,
    cas_dir: PathBuf,
    allow_list: Option<HashSet<String>>,
    deny_list: HashSet<String>,
    capacity: u64,
    ttl: Duration,
    stats: CacheCounters,
}

impl ToolCache {
    pub fn new() -> Self {
        let capacity = cache_capacity();
        let ttl = cache_ttl();
        let allow_list = parse_env_set("ARW_TOOLS_CACHE_ALLOW");
        let mut deny_list = parse_env_set("ARW_TOOLS_CACHE_DENY").unwrap_or_default();
        for entry in DEFAULT_DENY_LIST.iter() {
            deny_list.insert((*entry).to_string());
        }
        let cache = if capacity == 0 {
            None
        } else {
            Some(
                Cache::builder()
                    .max_capacity(capacity)
                    .time_to_live(ttl)
                    .build(),
            )
        };
        Self {
            cache,
            cas_dir: util::state_dir().join("tools").join("by-digest"),
            allow_list,
            deny_list,
            capacity,
            ttl,
            stats: CacheCounters::new(),
        }
    }

    pub fn enabled(&self) -> bool {
        self.cache.is_some()
    }

    pub fn stats(&self) -> ToolCacheStats {
        ToolCacheStats {
            hit: self.stats.hits.load(Ordering::Relaxed),
            miss: self.stats.miss.load(Ordering::Relaxed),
            coalesced: self.stats.coalesced.load(Ordering::Relaxed),
            errors: self.stats.errors.load(Ordering::Relaxed),
            bypass: self.stats.bypass.load(Ordering::Relaxed),
            capacity: self.capacity,
            ttl_secs: self.ttl.as_secs(),
            entries: self.cache.as_ref().map(|c| c.entry_count()).unwrap_or(0),
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
                    Some(ToolCacheHit {
                        value,
                        digest,
                        age_secs,
                    })
                }
                Err(err) => {
                    tracing::warn!("tool_cache::lookup deserialize error: {}", err);
                    self.stats.errors.fetch_add(1, Ordering::Relaxed);
                    cache.invalidate(key).await;
                    None
                }
            },
            Err(err) => {
                tracing::debug!("tool_cache::lookup miss on disk: {}", err);
                self.stats.errors.fetch_add(1, Ordering::Relaxed);
                cache.invalidate(key).await;
                None
            }
        }
    }

    pub async fn store(&self, key: &str, value: &Value) -> Option<StoreOutcome> {
        let bytes = serde_json::to_vec(value).ok()?;
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
                        return Some(StoreOutcome {
                            digest,
                            cached: false,
                        });
                    }
                }
                if let Err(err) = fs::write(&path, &bytes).await {
                    tracing::warn!(
                        "tool_cache::store failed to write digest {}: {}",
                        digest,
                        err
                    );
                    self.stats.errors.fetch_add(1, Ordering::Relaxed);
                    return Some(StoreOutcome {
                        digest,
                        cached: false,
                    });
                }
            }
            cache.insert(key.to_string(), digest.clone()).await;
            self.stats.miss.fetch_add(1, Ordering::Relaxed);
            Some(StoreOutcome {
                digest,
                cached: true,
            })
        } else {
            Some(StoreOutcome {
                digest,
                cached: false,
            })
        }
    }

    pub fn record_bypass(&self) {
        self.stats.bypass.fetch_add(1, Ordering::Relaxed);
    }
}

pub struct ToolCacheHit {
    pub value: Value,
    pub digest: String,
    pub age_secs: Option<u64>,
}

pub struct StoreOutcome {
    pub digest: String,
    pub cached: bool,
}

#[derive(Debug, Clone, Copy)]
pub struct ToolCacheStats {
    pub hit: u64,
    pub miss: u64,
    pub coalesced: u64,
    pub errors: u64,
    pub bypass: u64,
    pub capacity: u64,
    pub ttl_secs: u64,
    pub entries: u64,
}

struct CacheCounters {
    hits: AtomicU64,
    miss: AtomicU64,
    coalesced: AtomicU64,
    errors: AtomicU64,
    bypass: AtomicU64,
}

impl CacheCounters {
    fn new() -> Self {
        Self {
            hits: AtomicU64::new(0),
            miss: AtomicU64::new(0),
            coalesced: AtomicU64::new(0),
            errors: AtomicU64::new(0),
            bypass: AtomicU64::new(0),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use once_cell::sync::Lazy;
    use std::sync::Mutex;
    use tempfile::tempdir;

    static ENV_LOCK: Lazy<Mutex<()>> = Lazy::new(|| Mutex::new(()));

    fn setup_env(dir: &std::path::Path) -> crate::util::StateDirTestGuard {
        let guard = crate::util::scoped_state_dir_for_tests(dir);
        std::env::set_var("ARW_TOOLS_CACHE_CAP", "16");
        std::env::set_var("ARW_TOOLS_CACHE_TTL_SECS", "60");
        guard
    }

    #[tokio::test]
    async fn cache_roundtrip() {
        let tmp = tempdir().unwrap();
        let state_guard;
        {
            let _guard = ENV_LOCK.lock().unwrap();
            state_guard = setup_env(tmp.path());
        }
        let cache = ToolCache::new();
        assert!(cache.enabled());
        assert!(cache.is_cacheable("demo.echo"));
        let payload = json!({"hello": "world"});
        let key = cache.action_key("demo.echo", &payload);
        assert!(cache.lookup(&key).await.is_none());
        let outcome = cache.store(&key, &payload).await.expect("store outcome");
        assert!(outcome.cached);
        let hit = cache.lookup(&key).await.expect("cache hit");
        assert_eq!(hit.value, payload);
        assert_eq!(hit.digest, outcome.digest);
        assert!(cache.cas_path(&hit.digest).exists());
        std::env::remove_var("ARW_TOOLS_CACHE_CAP");
        std::env::remove_var("ARW_TOOLS_CACHE_TTL_SECS");
        drop(state_guard);
    }

    #[tokio::test]
    async fn allow_list_overrides_defaults() {
        let tmp = tempdir().unwrap();
        let state_guard;
        {
            let _guard = ENV_LOCK.lock().unwrap();
            state_guard = setup_env(tmp.path());
            std::env::set_var("ARW_TOOLS_CACHE_ALLOW", "demo.echo");
        }
        let cache = ToolCache::new();
        assert!(cache.is_cacheable("demo.echo"));
        assert!(!cache.is_cacheable("guardrails.check"));
        std::env::remove_var("ARW_TOOLS_CACHE_ALLOW");
        std::env::remove_var("ARW_TOOLS_CACHE_CAP");
        std::env::remove_var("ARW_TOOLS_CACHE_TTL_SECS");
        drop(state_guard);
    }
}
