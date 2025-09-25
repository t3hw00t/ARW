use anyhow::{anyhow, Result};
use serde_json::{json, Value};
use std::path::PathBuf;

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
    if let Some(paths) = crate::config::effective_paths() {
        PathBuf::from(paths.state_dir)
    } else {
        PathBuf::from(arw_core::effective_paths().state_dir)
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
