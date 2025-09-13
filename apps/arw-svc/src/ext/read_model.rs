use once_cell::sync::OnceCell;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::Mutex;

/// Generic JSON Patch emitter for read‑models over the event bus.
/// Maintains last snapshots keyed by (topic,id) and publishes RFC 6902 patches
/// on change. Intended for low‑cardinality models (metrics, small summaries).
mod imp {
    use super::*;
    static LAST: OnceCell<Mutex<HashMap<String, Value>>> = OnceCell::new();
    fn map() -> &'static Mutex<HashMap<String, Value>> {
        LAST.get_or_init(|| Mutex::new(HashMap::new()))
    }
    pub fn diff_and_publish(bus: &arw_events::Bus, topic: &str, id: &str, cur: &Value) {
        let key = format!("{}:{}", topic, id);
        let mut guard = map().lock().unwrap();
        let prev = guard.get(&key).cloned().unwrap_or_else(|| json!({}));
        let patch = json_patch::diff(&prev, cur);
        let patch_val = serde_json::to_value(&patch).unwrap_or_else(|_| json!([]));
        let non_empty = patch_val.as_array().map(|a| !a.is_empty()).unwrap_or(false);
        if non_empty {
            let mut payload = json!({"id": id, "patch": patch_val});
            crate::ext::corr::ensure_corr(&mut payload);
            bus.publish(topic, &payload);
            guard.insert(key, cur.clone());
        }
    }
}

/// Publish a JSON Patch for read‑model `id` under `topic` when it changes.
pub fn emit_patch(bus: &arw_events::Bus, topic: &str, id: &str, current: &Value) {
    imp::diff_and_publish(bus, topic, id, current);
}

/// Convenience: publish the same model under both a specific and a generic topic.
pub fn emit_patch_dual(
    bus: &arw_events::Bus,
    specific_topic: &str,
    generic_topic: &str,
    id: &str,
    current: &Value,
) {
    // Compute independently to keep code simple; objects are tiny.
    imp::diff_and_publish(bus, specific_topic, id, current);
    imp::diff_and_publish(bus, generic_topic, id, current);
}
