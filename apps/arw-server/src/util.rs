use anyhow::{anyhow, Result};
pub use arw_core::util::{env_bool, parse_bool_flag};
use once_cell::sync::{Lazy, OnceCell};
#[cfg(test)]
use parking_lot::{ReentrantMutex, ReentrantMutexGuard};
use serde_json::{json, Value};
use std::collections::HashMap;
#[cfg(test)]
use std::path::Path;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::Instant;
use tokio::sync::broadcast;

static STATE_DIR: Lazy<Mutex<OnceCell<PathBuf>>> = Lazy::new(|| Mutex::new(OnceCell::new()));

/// Load a connector manifest from disk.
pub async fn load_connector_manifest(id: &str) -> Result<Value> {
    let path = state_dir().join("connectors").join(format!("{}.json", id));
    let bytes = tokio::fs::read(&path)
        .await
        .map_err(|err| anyhow!("read connector manifest: {err}"))?;
    let value: Value =
        serde_json::from_slice(&bytes).map_err(|err| anyhow!("parse connector manifest: {err}"))?;
    Ok(value)
}

pub fn default_models() -> Vec<Value> {
    vec![
        json!({"id":"llama-3.1-8b-instruct","provider":"local","status":"available"}),
        json!({"id":"qwen2.5-coder-7b","provider":"local","status":"available"}),
    ]
}

pub fn effective_posture() -> String {
    if let Ok(p) = std::env::var("ARW_NET_POSTURE") {
        return p;
    }
    if let Ok(p) = std::env::var("ARW_SECURITY_POSTURE") {
        return p;
    }
    "standard".into()
}

pub fn state_dir() -> PathBuf {
    let cell = STATE_DIR
        .lock()
        .unwrap_or_else(|poison| poison.into_inner());
    if let Some(existing) = cell.get() {
        return existing.clone();
    }

    let resolved = if let Some(paths) = crate::config::effective_paths() {
        PathBuf::from(paths.state_dir.clone())
    } else {
        PathBuf::from(arw_core::effective_paths().state_dir)
    };

    // Value cannot be set by another thread while we hold the lock, but ignore the
    // Result to avoid double-panicking should it ever happen.
    let _ = cell.set(resolved.clone());
    resolved
}

#[cfg(test)]
pub(crate) fn reset_state_dir_for_tests() {
    let mut cell = STATE_DIR
        .lock()
        .unwrap_or_else(|poison| poison.into_inner());
    cell.take();
}

#[cfg(test)]
static STATE_DIR_TEST_LOCK: Lazy<ReentrantMutex<()>> = Lazy::new(|| ReentrantMutex::new(()));

#[cfg(test)]
pub(crate) struct StateDirTestGuard {
    prev: Option<String>,
    _lock: ReentrantMutexGuard<'static, ()>,
}

#[cfg(test)]
pub(crate) fn scoped_state_dir_for_tests(
    path: &Path,
    env_guard: &mut crate::test_support::env::EnvGuard,
) -> StateDirTestGuard {
    // ReentrantMutex allows callers to nest state guards in the same thread without deadlock.
    let lock = STATE_DIR_TEST_LOCK.lock();
    let prev = std::env::var("ARW_STATE_DIR").ok();
    reset_state_dir_for_tests();
    env_guard.set("ARW_STATE_DIR", path.display().to_string());
    StateDirTestGuard { prev, _lock: lock }
}

#[cfg(test)]
impl Drop for StateDirTestGuard {
    fn drop(&mut self) {
        if let Some(prev) = &self.prev {
            std::env::set_var("ARW_STATE_DIR", prev);
        } else {
            std::env::remove_var("ARW_STATE_DIR");
        }
        reset_state_dir_for_tests();
    }
}

pub fn attach_memory_ptr(value: &mut Value) {
    let Some(obj) = value.as_object_mut() else {
        return;
    };
    if obj.contains_key("ptr") {
        return;
    }
    if let Some(id) = obj.get("id").and_then(|v| v.as_str()) {
        obj.insert("ptr".into(), json!({"kind": "memory", "id": id}));
    }
}

pub fn smoke_profile_enabled() -> bool {
    static ENABLED: OnceCell<bool> = OnceCell::new();
    *ENABLED.get_or_init(|| {
        std::env::var("ARW_SMOKE_MODE")
            .ok()
            .map(|raw| raw.trim().to_ascii_lowercase())
            .map(|value| matches!(value.as_str(), "1" | "true" | "yes" | "smoke" | "vision"))
            .unwrap_or(false)
    })
}

/// Receive the next bus event, handling lag/backpressure transparently.
/// Returns `None` when the channel is closed.
pub async fn next_bus_event(
    rx: &mut broadcast::Receiver<arw_events::Envelope>,
    bus: &arw_events::Bus,
    task: &str,
) -> Option<arw_events::Envelope> {
    const LAG_LOG_INTERVAL_MS: u128 = 2_000;
    const LAG_LOG_THRESHOLD: u64 = 500;
    loop {
        match rx.recv().await {
            Ok(env) => return Some(env),
            Err(broadcast::error::RecvError::Lagged(skipped)) => {
                bus.note_lag(skipped);
                let mut tracker = LAG_TRACKER.lock().unwrap_or_else(|p| p.into_inner());
                let state = tracker.entry(task.to_string()).or_default();
                state.accum = state.accum.saturating_add(skipped);
                if state.last.elapsed().as_millis() >= LAG_LOG_INTERVAL_MS
                    || state.accum >= LAG_LOG_THRESHOLD
                {
                    tracing::warn!(
                        target: "arw::bus",
                        %task,
                        skipped = state.accum,
                        "bus receiver lagged; skipped events (aggregated)"
                    );
                    state.total = state.total.saturating_add(state.accum);
                    state.accum = 0;
                    state.last = Instant::now();
                }
                continue;
            }
            Err(broadcast::error::RecvError::Closed) => {
                tracing::warn!(target: "arw::bus", %task, "bus closed; stopping listener");
                return None;
            }
        }
    }
}

struct LagState {
    last: Instant,
    accum: u64,
    total: u64,
}

impl Default for LagState {
    fn default() -> Self {
        Self {
            last: Instant::now(),
            accum: 0,
            total: 0,
        }
    }
}

static LAG_TRACKER: Lazy<Mutex<HashMap<String, LagState>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));

/// Snapshot of bus lag per task for diagnostics.
pub fn bus_lag_stats() -> Vec<(String, u64)> {
    let tracker = LAG_TRACKER.lock().unwrap_or_else(|p| p.into_inner());
    tracker
        .iter()
        .map(|(k, v)| (k.clone(), v.total + v.accum))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_bool_flag_recognizes_common_values() {
        assert_eq!(parse_bool_flag("true"), Some(true));
        assert_eq!(parse_bool_flag("YES"), Some(true));
        assert_eq!(parse_bool_flag("0"), Some(false));
        assert_eq!(parse_bool_flag("off"), Some(false));
        assert_eq!(parse_bool_flag("maybe"), None);
        assert_eq!(parse_bool_flag(""), None);
    }

    #[test]
    fn env_bool_uses_parse_and_restores_state() {
        let mut guard = crate::test_support::env::guard();
        guard.set("ARW_BOOL_TEST", "on");
        assert_eq!(env_bool("ARW_BOOL_TEST"), Some(true));
        guard.set("ARW_BOOL_TEST", "No");
        assert_eq!(env_bool("ARW_BOOL_TEST"), Some(false));
        guard.remove("ARW_BOOL_TEST");
        assert_eq!(env_bool("ARW_BOOL_TEST"), None);
    }
}
