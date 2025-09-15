use serde_json::Value;
use serde_json::json;

pub fn default_models() -> Vec<Value> {
    vec![
        json!({"id":"llama-3.1-8b-instruct","provider":"local","status":"available"}),
        json!({"id":"qwen2.5-coder-7b","provider":"local","status":"available"}),
    ]
}

pub fn effective_posture() -> String {
    if let Ok(p) = std::env::var("ARW_NET_POSTURE") { return p; }
    if let Ok(p) = std::env::var("ARW_SECURITY_POSTURE") { return p; }
    "standard".into()
}
