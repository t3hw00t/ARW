use serde_json::{json, Value};
use std::path::PathBuf;

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
