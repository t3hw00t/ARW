use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::Deserialize;
use serde_json::{json, Value};
use utoipa::ToSchema;

use crate::{admin_ok, tools, AppState};

fn unauthorized() -> Response {
    (
        StatusCode::UNAUTHORIZED,
        Json(json!({"type":"about:blank","title":"Unauthorized","status":401})),
    )
        .into_response()
}

fn problem(status: StatusCode, title: &str, detail: Option<&str>) -> Response {
    let mut body = json!({"type":"about:blank","title": title,"status": status.as_u16()});
    if let Some(d) = detail {
        body["detail"] = json!(d);
    }
    (status, Json(body)).into_response()
}

fn gating_detail(key: &str, fallback: &str) -> String {
    if let Some(meta) = arw_core::gating_keys::find(key) {
        format!(
            "{} [{}] {} ({}) - {}",
            fallback, key, meta.title, meta.stability, meta.summary
        )
    } else {
        format!("{} [{}]", fallback, key)
    }
}

#[utoipa::path(
    get,
    path = "/admin/tools",
    tag = "Admin/Tools",
    responses(
        (status = 200, description = "Registered tools", body = serde_json::Value),
        (status = 401, description = "Unauthorized"),
    )
)]
pub async fn tools_list(headers: HeaderMap) -> impl IntoResponse {
    if !admin_ok(&headers) {
        return unauthorized();
    }
    let mut items = Vec::new();
    for info in arw_core::introspect_tools() {
        items.push(json!({
            "id": info.id,
            "version": info.version,
            "summary": info.summary,
            "stability": info.stability,
            "capabilities": info.capabilities,
        }));
    }
    Json(json!({"items": items})).into_response()
}

#[derive(Deserialize, ToSchema)]
pub struct ToolRunRequest {
    pub id: String,
    #[serde(default)]
    pub input: Value,
}

#[utoipa::path(
    post,
    path = "/admin/tools/run",
    tag = "Admin/Tools",
    request_body = ToolRunRequest,
    responses(
        (status = 200, description = "Tool output", body = serde_json::Value),
        (status = 400, description = "Invalid request"),
        (status = 401, description = "Unauthorized"),
        (status = 403, description = "Forbidden"),
        (status = 404, description = "Unknown tool"),
        (status = 500, description = "Tool runtime error")
    )
)]
pub async fn tools_run(
    headers: HeaderMap,
    State(state): State<AppState>,
    Json(req): Json<ToolRunRequest>,
) -> impl IntoResponse {
    if !admin_ok(&headers) {
        return unauthorized();
    }

    let ingress_key = format!("io:ingress:tools.{}", req.id);
    if !arw_core::gating::allowed(&ingress_key) {
        let detail = gating_detail(&ingress_key, "gated:ingress");
        return problem(StatusCode::FORBIDDEN, "Forbidden", Some(detail.as_str()));
    }

    if req.id == "ui.screenshot.capture" && !arw_core::gating::allowed("io:screenshot") {
        let detail = gating_detail("io:screenshot", "gated:screenshot");
        return problem(StatusCode::FORBIDDEN, "Forbidden", Some(detail.as_str()));
    }
    if req.id == "ui.screenshot.ocr" && !arw_core::gating::allowed("io:ocr") {
        let detail = gating_detail("io:ocr", "gated:ocr");
        return problem(StatusCode::FORBIDDEN, "Forbidden", Some(detail.as_str()));
    }

    let tool_id = req.id.clone();
    let run_result = tools::run_tool(&state, &req.id, req.input).await;

    let output = match run_result {
        Ok(value) => value,
        Err(err) => {
            return match err {
                tools::ToolError::Unsupported(id) => problem(
                    StatusCode::NOT_FOUND,
                    "Not Found",
                    Some(&format!("unknown tool id: {}", id)),
                ),
                tools::ToolError::Invalid(msg) => {
                    problem(StatusCode::BAD_REQUEST, "Bad Request", Some(&msg))
                }
                tools::ToolError::Runtime(msg) => problem(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "Tool runtime error",
                    Some(&msg),
                ),
                tools::ToolError::Denied {
                    reason,
                    dest_host,
                    dest_port,
                    protocol,
                } => {
                    let mut detail = format!("denied: {}", reason);
                    if let Some(host) = dest_host {
                        detail.push_str(&format!(" host={}", host));
                    }
                    if let Some(port) = dest_port {
                        detail.push_str(&format!(" port={}", port));
                    }
                    if let Some(proto) = protocol {
                        detail.push_str(&format!(" proto={}", proto));
                    }
                    problem(StatusCode::FORBIDDEN, "Forbidden", Some(&detail))
                }
            };
        }
    };

    let egress_key = format!("io:egress:tools.{}", tool_id);
    if !arw_core::gating::allowed(&egress_key) {
        let detail = gating_detail(&egress_key, "gated:egress");
        return problem(StatusCode::FORBIDDEN, "Forbidden", Some(detail.as_str()));
    }

    Json(output).into_response()
}

#[utoipa::path(
    get,
    path = "/admin/tools/cache_stats",
    tag = "Admin/Tools",
    responses(
        (status = 200, description = "Cache statistics", body = serde_json::Value),
        (status = 401, description = "Unauthorized"),
    )
)]
pub async fn tools_cache_stats(
    headers: HeaderMap,
    State(state): State<AppState>,
) -> impl IntoResponse {
    if !admin_ok(&headers) {
        return unauthorized();
    }
    let stats = state.tool_cache().stats();
    Json(json!({
        "hit": stats.hit,
        "miss": stats.miss,
        "coalesced": stats.coalesced,
        "errors": stats.errors,
        "bypass": stats.bypass,
        "capacity": stats.capacity,
        "ttl_secs": stats.ttl_secs,
        "entries": stats.entries,
    }))
    .into_response()
}
