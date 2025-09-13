use serde_json::{json, Value};

// Simple red-team checks for proposed patches/configs.
// Returns a list of issues; empty list means pass.

pub fn check_patches_for_risks(patches: &[Value]) -> Vec<Value> {
    let mut issues: Vec<Value> = Vec::new();
    for (i, p) in patches.iter().enumerate() {
        let target = p.get("target").and_then(|s| s.as_str()).unwrap_or("");
        let op = p.get("op").and_then(|s| s.as_str()).unwrap_or("");
        let val = p.get("value").cloned().unwrap_or(Value::Null);
        // Only inspect merge objects
        if op != "merge" {
            continue;
        }

        // Check for widening permissions/leases
        if let Some(obj) = val.as_object() {
            if obj.get("permissions").is_some() || obj.get("leases").is_some() {
                issues.push(json!({
                    "idx": i,
                    "target": target,
                    "code": "permissions_change",
                    "detail": "patch touches permissions/leases; require lease/approval"
                }));
            }
        }
        // Check for SSRF-like patterns in URLs
        let body = val.to_string();
        for needle in [
            "169.254.169.254", // AWS IMDS
            "\"file://",       // local fs scheme in strings
            "127.0.0.1:",
            "localhost:",
        ] {
            if body.contains(needle) {
                issues.push(json!({
                    "idx": i,
                    "target": target,
                    "code": "ssrf_pattern",
                    "detail": format!("found pattern '{}'; deny unless allowlisted", needle)
                }));
            }
        }
        // Prompt injection bait words in prompt-like fields
        if body.contains("ignore previous") || body.contains("disregard instructions") {
            issues.push(json!({
                "idx": i,
                "target": target,
                "code": "prompt_injection",
                "detail": "prompt contains injection-y phrasing"
            }));
        }
        // Secrets in logs: naive check for common env var names
        for key in ["API_KEY", "SECRET", "TOKEN", "PASSWORD"] {
            if body.contains(key) {
                issues.push(json!({
                    "idx": i,
                    "target": target,
                    "code": "secret_marker",
                    "detail": format!("value references '{}'; ensure redaction", key)
                }));
            }
        }
    }
    issues
}
