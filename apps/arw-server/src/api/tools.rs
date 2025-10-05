use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::Deserialize;
use serde_json::{json, Value};
use utoipa::ToSchema;

use crate::{admin_ok, metrics::cache_stats_snapshot, tools, AppState};

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
                tools::ToolError::Interrupted { reason, detail } => {
                    let message = if let Some(detail) = detail {
                        format!("tool interrupted: {} ({})", reason, detail)
                    } else {
                        format!("tool interrupted: {}", reason)
                    };
                    problem(
                        StatusCode::SERVICE_UNAVAILABLE,
                        "Tool interrupted",
                        Some(message.as_str()),
                    )
                }
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
    Json(cache_stats_snapshot(&stats)).into_response()
}

#[cfg(test)]
mod tests {
    use super::*;
    use arw_core::gating;
    use arw_policy::PolicyEngine;
    use axum::body::to_bytes;
    use axum::http::{header, HeaderMap, HeaderValue, StatusCode};
    use axum::extract::State;
    use serde_json::json;
    use std::path::Path;
    use std::sync::Arc;
    use tempfile::tempdir;

    struct GatingReset {
        path: String,
    }

    impl GatingReset {
        fn new<P: Into<String>>(path: P) -> Self {
            let path = path.into();
            gating::reload_from_config(&path);
            Self { path }
        }
    }

    impl Drop for GatingReset {
        fn drop(&mut self) {
            gating::reload_from_config(&self.path);
        }
    }

    async fn build_state(
        path: &Path,
        env_guard: &mut crate::test_support::env::EnvGuard,
    ) -> AppState {
        crate::util::reset_state_dir_for_tests();
        env_guard.set("ARW_STATE_DIR", path.display().to_string());
        env_guard.remove("ARW_DEBUG");
        let bus = arw_events::Bus::new_with_replay(16, 16);
        let kernel = arw_kernel::Kernel::open(path).expect("init kernel");
        let policy = PolicyEngine::load_from_env();
        let policy_handle = crate::policy::PolicyHandle::new(policy, bus.clone());
        let host: Arc<dyn arw_wasi::ToolHost> = Arc::new(arw_wasi::NoopHost);
        AppState::builder(bus, kernel, policy_handle, host, true)
            .with_sse_capacity(16)
            .build()
            .await
    }

    #[tokio::test]
    async fn tools_run_denies_when_ocr_capability_missing() {
        let temp = tempdir().expect("tempdir");
        let gating_path = temp.path().join("gating.toml");
        let _reset = GatingReset::new(gating_path.to_string_lossy().to_string());
        let mut ctx = crate::test_support::begin_state_env(temp.path());
        ctx.env.set("ARW_ADMIN_TOKEN", "secret-token");
        gating::deny_user(vec!["io:ocr".to_string()]);

        let state = build_state(temp.path(), &mut ctx.env).await;

        let mut headers = HeaderMap::new();
        headers.insert(
            header::AUTHORIZATION,
            HeaderValue::from_static("Bearer secret-token"),
        );
        let request = ToolRunRequest {
            id: "ui.screenshot.ocr".to_string(),
            input: json!({"path": "/tmp/example.png"}),
        };

        let response = tools_run(headers, State(state), Json(request))
            .await
            .into_response();
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
        let (parts, body) = response.into_parts();
        assert_eq!(parts.status, StatusCode::FORBIDDEN);
        let bytes =
            to_bytes(body, usize::MAX).await.expect("forbidden response body");
        let value: serde_json::Value =
            serde_json::from_slice(&bytes).expect("forbidden body json");
        assert_eq!(value["status"].as_u64(), Some(403));
        let detail = value["detail"].as_str().unwrap_or_default();
        assert!(detail.contains("io:ocr"), "detail missing io:ocr: {detail}");
    }

    #[tokio::test]
    async fn tools_run_reports_not_found_for_missing_ocr_tool() {
        let temp = tempdir().expect("tempdir");
        let gating_path = temp.path().join("gating.toml");
        let _reset = GatingReset::new(gating_path.to_string_lossy().to_string());
        let mut ctx = crate::test_support::begin_state_env(temp.path());
        ctx.env.set("ARW_ADMIN_TOKEN", "secret-token");

        let state = build_state(temp.path(), &mut ctx.env).await;

        let mut headers = HeaderMap::new();
        headers.insert(
            header::AUTHORIZATION,
            HeaderValue::from_static("Bearer secret-token"),
        );
        let request = ToolRunRequest {
            id: "ui.screenshot.ocr".to_string(),
            input: json!({"path": "/tmp/example.png"}),
        };

        let response = tools_run(headers, State(state), Json(request))
            .await
            .into_response();
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        let (parts, body) = response.into_parts();
        assert_eq!(parts.status, StatusCode::NOT_FOUND);
        let bytes = to_bytes(body, usize::MAX).await.expect("not found body");
        let value: serde_json::Value =
            serde_json::from_slice(&bytes).expect("not found body json");
        assert_eq!(value["status"].as_u64(), Some(404));
        let detail = value["detail"].as_str().unwrap_or_default();
        assert!(
            detail.contains("ui.screenshot.ocr"),
            "detail missing tool id: {detail}"
        );
    }
}
