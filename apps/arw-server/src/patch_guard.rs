use serde_json::{json, Value};

/// Scan proposed patch operations for obvious safety risks (permissions expansion,
/// prompt injection bait, SSRF markers, secret markers, etc.). Returns a list of structured
/// findings; an empty list indicates no issues detected.
pub(crate) fn check_patches_for_risks(patches: &[Value]) -> Vec<Value> {
    let mut issues: Vec<Value> = Vec::new();
    for (idx, patch) in patches.iter().enumerate() {
        let target = patch.get("target").and_then(|v| v.as_str()).unwrap_or("");
        let op = patch.get("op").and_then(|v| v.as_str()).unwrap_or("");
        if !matches!(op, "merge" | "set") {
            // Only inspect merge/set operations in this pass.
            continue;
        }
        let value_ref = patch.get("value").unwrap_or(&Value::Null);
        if let Some(obj) = value_ref.as_object() {
            if obj.contains_key("permissions") || obj.contains_key("leases") {
                issues.push(json!({
                    "idx": idx,
                    "target": target,
                    "code": "permissions_change",
                    "detail": "patch touches permissions/leases; require lease/approval"
                }));
            }
        }
        let body = value_ref.to_string();
        for needle in [
            "169.254.169.254", // AWS IMDS
            "\"file://",       // local file scheme inside string literal
            "127.0.0.1:",
            "localhost:",
        ] {
            if body.contains(needle) {
                issues.push(json!({
                    "idx": idx,
                    "target": target,
                    "code": "ssrf_pattern",
                    "detail": format!("found pattern '{}'; deny unless allowlisted", needle)
                }));
            }
        }
        if body.contains("ignore previous") || body.contains("disregard instructions") {
            issues.push(json!({
                "idx": idx,
                "target": target,
                "code": "prompt_injection",
                "detail": "prompt contains injection-style phrasing"
            }));
        }
        for key in ["API_KEY", "SECRET", "TOKEN", "PASSWORD"] {
            if body.contains(key) {
                issues.push(json!({
                    "idx": idx,
                    "target": target,
                    "code": "secret_marker",
                    "detail": format!("value references '{}'; ensure redaction", key)
                }));
            }
        }
    }
    issues
}

/// Whether to enforce the red-team findings as hard failures (`ARW_PATCH_SAFETY=1/true/enforce`).
pub(crate) fn safety_enforced() -> bool {
    std::env::var("ARW_PATCH_SAFETY")
        .ok()
        .map(|s| matches!(s.trim(), "1" | "true" | "TRUE" | "enforce" | "ENFORCE"))
        .unwrap_or(false)
}
