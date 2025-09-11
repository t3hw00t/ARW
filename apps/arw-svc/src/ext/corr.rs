use uuid::Uuid;
use serde_json::Value;

pub fn new_corr_id() -> String {
    Uuid::new_v4().to_string()
}

pub fn ensure_corr(v: &mut Value) -> String {
    if let Some(obj) = v.as_object_mut() {
        if let Some(s) = obj.get("corr_id").and_then(|x| x.as_str()) {
            return s.to_string();
        }
        let id = new_corr_id();
        obj.insert("corr_id".into(), Value::String(id.clone()));
        id
    } else {
        let id = new_corr_id();
        *v = serde_json::json!({"corr_id": id, "value": v.clone()});
        id
    }
}

