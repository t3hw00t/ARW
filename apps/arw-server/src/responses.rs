use axum::{http::StatusCode, response::IntoResponse, Json};
use serde_json::{json, Value};
use uuid::Uuid;

pub fn kernel_disabled() -> axum::response::Response {
    (
        StatusCode::NOT_IMPLEMENTED,
        Json(json!({
            "type": "about:blank",
            "title": "Kernel Disabled",
            "status": 501,
            "detail": "Operation requires ARW_KERNEL_ENABLE=1"
        })),
    )
        .into_response()
}

pub fn attach_corr(payload: &mut Value) {
    if let Value::Object(map) = payload {
        if !map.contains_key("corr_id") {
            map.insert("corr_id".into(), Value::String(Uuid::new_v4().to_string()));
        }
    }
}
