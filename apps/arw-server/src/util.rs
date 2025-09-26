use anyhow::{anyhow, Result};
use once_cell::sync::{Lazy, OnceCell};
use serde_json::{json, Value};
use std::path::PathBuf;
use std::sync::Mutex;
#[cfg(test)]
use std::{path::Path, sync::MutexGuard};

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
    let cell = STATE_DIR.lock().expect("state dir cache lock");
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
    let mut cell = STATE_DIR.lock().expect("state dir cache lock");
    cell.take();
}

#[cfg(test)]
static STATE_DIR_TEST_LOCK: Lazy<Mutex<()>> = Lazy::new(|| Mutex::new(()));

#[cfg(test)]
pub(crate) struct StateDirTestGuard {
    prev: Option<String>,
    _lock: MutexGuard<'static, ()>,
}

#[cfg(test)]
pub(crate) fn scoped_state_dir_for_tests(path: &Path) -> StateDirTestGuard {
    let lock = STATE_DIR_TEST_LOCK.lock().expect("state dir test lock");
    let prev = std::env::var("ARW_STATE_DIR").ok();
    reset_state_dir_for_tests();
    std::env::set_var("ARW_STATE_DIR", path.display().to_string());
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
