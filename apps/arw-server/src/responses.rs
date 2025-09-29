use arw_protocol::ProblemDetails;
use axum::http::{HeaderMap, StatusCode};
use axum::response::IntoResponse;
use axum::Json;
use serde_json::Value;
use uuid::Uuid;

pub fn problem_details(status: StatusCode, title: &str, detail: Option<&str>) -> ProblemDetails {
    ProblemDetails {
        r#type: "about:blank".to_string(),
        title: title.to_string(),
        status: status.as_u16(),
        detail: detail.map(|v| v.to_string()),
        instance: None,
        trace_id: None,
        code: None,
    }
}

pub fn problem_response(
    status: StatusCode,
    title: &str,
    detail: Option<&str>,
) -> axum::response::Response {
    (status, Json(problem_details(status, title, detail))).into_response()
}

pub fn kernel_disabled() -> axum::response::Response {
    problem_response(
        StatusCode::NOT_IMPLEMENTED,
        "Kernel Disabled",
        Some("Operation requires ARW_KERNEL_ENABLE=1"),
    )
}

pub fn unauthorized(detail: Option<&str>) -> axum::response::Response {
    problem_response(StatusCode::UNAUTHORIZED, "Unauthorized", detail)
}

pub fn attach_corr(payload: &mut Value) {
    if let Value::Object(map) = payload {
        if !map.contains_key("corr_id") {
            map.insert("corr_id".into(), Value::String(Uuid::new_v4().to_string()));
        }
    }
}

pub fn require_admin(headers: &HeaderMap) -> Result<(), Box<axum::response::Response>> {
    if crate::admin_ok(headers) {
        Ok(())
    } else {
        Err(Box::new(unauthorized(None)))
    }
}
