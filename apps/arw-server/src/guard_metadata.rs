use serde_json::{Map, Value};

/// Apply posture and guard metadata to a JSON map, respecting overwrite semantics.
pub(crate) fn apply_posture_and_guard(
    map: &mut Map<String, Value>,
    posture: Option<&str>,
    guard: Option<Value>,
    overwrite: bool,
) {
    if let Some(posture_value) = posture {
        if overwrite || !map.contains_key("posture") {
            map.insert("posture".into(), Value::String(posture_value.to_string()));
        }
    }

    if let Some(guard_value) = guard {
        if overwrite || !map.contains_key("guard") {
            map.insert("guard".into(), guard_value);
        }
    }
}

/// Redact internal guard metadata to the public representation.
pub(crate) fn sanitize_guard_value(value: &Value) -> Value {
    if let Value::Object(map) = value {
        let mut sanitized = Map::new();
        if let Some(v) = map.get("allowed") {
            sanitized.insert("allowed".into(), v.clone());
        }
        if let Some(v) = map.get("policy_allow") {
            sanitized.insert("policy_allow".into(), v.clone());
        }
        if let Some(v) = map.get("required_capabilities") {
            sanitized.insert("required_capabilities".into(), v.clone());
        }
        if let Some(lease) = map.get("lease") {
            if let Value::Object(lease_map) = lease {
                let mut redacted = Map::new();
                if let Some(cap) = lease_map.get("capability") {
                    redacted.insert("capability".into(), cap.clone());
                }
                if let Some(ttl) = lease_map.get("ttl_until") {
                    redacted.insert("ttl_until".into(), ttl.clone());
                }
                if let Some(scope) = lease_map.get("scope") {
                    if !scope.is_null() {
                        redacted.insert("scope".into(), scope.clone());
                    }
                }
                if !redacted.is_empty() {
                    sanitized.insert("lease".into(), Value::Object(redacted));
                }
            }
        }
        Value::Object(sanitized)
    } else {
        value.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn apply_posture_and_guard_respects_overwrite() {
        let mut map = Map::new();
        map.insert("posture".into(), Value::String("existing".into()));
        apply_posture_and_guard(&mut map, Some("new"), None, false);
        assert_eq!(map.get("posture").and_then(Value::as_str), Some("existing"));

        apply_posture_and_guard(&mut map, Some("forced"), None, true);
        assert_eq!(map.get("posture").and_then(Value::as_str), Some("forced"));

        apply_posture_and_guard(&mut map, None, Some(Value::Bool(true)), false);
        assert_eq!(map.get("guard"), Some(&Value::Bool(true)));
    }

    #[test]
    fn sanitize_guard_value_drops_private_fields() {
        let raw = json!({
            "allowed": true,
            "policy_allow": false,
            "required_capabilities": ["fs"],
            "lease": {
                "id": "lease-1",
                "capability": "fs",
                "scope": null,
                "ttl_until": "2099-01-01T00:00:00Z",
                "subject": "local",
            },
        });

        let sanitized = sanitize_guard_value(&raw);
        assert_eq!(sanitized["allowed"], Value::Bool(true));
        assert!(sanitized.get("policy_allow").is_some());
        assert_eq!(sanitized["lease"]["capability"], Value::String("fs".into()));
        assert!(sanitized["lease"].get("id").is_none());
    }
}
