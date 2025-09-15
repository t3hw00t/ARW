use axum::response::IntoResponse;
use axum::{extract::State, Json};
use serde::Deserialize;
use serde_json::json;

use crate::AppState;

#[derive(Deserialize)]
pub(crate) struct EgressPreviewReq {
    pub url: String,
    #[serde(default)]
    pub method: Option<String>,
}

pub async fn egress_preview(
    State(state): State<AppState>,
    Json(req): Json<EgressPreviewReq>,
) -> impl IntoResponse {
    // Parse URL and derive action kind (method-specific)
    let method = req
        .method
        .unwrap_or_else(|| "GET".into())
        .to_ascii_uppercase();
    let kind = format!("net.http.{}", method.to_ascii_lowercase());
    let url = match url::Url::parse(&req.url) {
        Ok(u) => u,
        Err(e) => {
            return (
                axum::http::StatusCode::BAD_REQUEST,
                Json(
                    json!({"type":"about:blank","title":"Bad Request","status":400, "detail": e.to_string()}),
                ),
            )
        }
    };
    let host = url.host_str().map(|s| s.to_string());
    let port = url.port();
    let scheme = url.scheme().to_string();

    // IP-literal check
    if std::env::var("ARW_EGRESS_BLOCK_IP_LITERALS")
        .ok()
        .as_deref()
        == Some("1")
    {
        if let Some(h) = &host {
            if h.parse::<std::net::IpAddr>().is_ok() {
                let _ = maybe_log_egress(
                    &state,
                    "deny",
                    Some("ip_literal"),
                    host.as_deref(),
                    port,
                    Some(&scheme),
                    None,
                    None,
                );
                return (
                    axum::http::StatusCode::OK,
                    Json(
                        json!({"allow": false, "reason": "ip_literal", "host": host, "port": port, "protocol": scheme}),
                    ),
                );
            }
        }
    }

    // Allowlist (env-based) quick check
    if let Ok(list) = std::env::var("ARW_NET_ALLOWLIST") {
        if !list.trim().is_empty() {
            let hosts: Vec<String> = list
                .split(',')
                .map(|s| s.trim().to_ascii_lowercase())
                .filter(|s| !s.is_empty())
                .map(|s| s.to_string())
                .collect();
            if let Some(h) = &host {
                let hlow = h.to_ascii_lowercase();
                let ok = hosts.iter().any(|p| {
                    let plow = p.to_ascii_lowercase();
                    hlow == plow || hlow.ends_with(&format!(".{plow}"))
                });
                if !ok {
                    let _ = maybe_log_egress(
                        &state,
                        "deny",
                        Some("allowlist"),
                        host.as_deref(),
                        port,
                        Some(&scheme),
                        None,
                        None,
                    );
                    return (
                        axum::http::StatusCode::OK,
                        Json(
                            json!({"allow": false, "reason": "allowlist", "host": host, "port": port, "protocol": scheme}),
                        ),
                    );
                }
            }
        }
    }

    // Policy evaluation (ABAC facade)
    let decision = state.policy.lock().await.evaluate_action(&kind);
    if !decision.allow {
        if let Some(cap) = decision.require_capability.as_deref() {
            if state
                .kernel
                .find_valid_lease("local", cap)
                .ok()
                .flatten()
                .is_none()
            {
                let _ = maybe_log_egress(
                    &state,
                    "deny",
                    Some("lease_required"),
                    host.as_deref(),
                    port,
                    Some(&scheme),
                    None,
                    None,
                );
                return (
                    axum::http::StatusCode::OK,
                    Json(
                        json!({"allow": false, "reason": "lease_required", "require_capability": cap, "host": host, "port": port, "protocol": scheme}),
                    ),
                );
            }
        }
    }
    let _ = maybe_log_egress(
        &state,
        "allow",
        Some("preview"),
        host.as_deref(),
        port,
        Some(&scheme),
        None,
        None,
    );
    (
        axum::http::StatusCode::OK,
        Json(json!({"allow": true, "host": host, "port": port, "protocol": scheme})),
    )
}

fn net_posture() -> String {
    crate::util::effective_posture()
}

fn maybe_log_egress(
    state: &AppState,
    decision: &str,
    reason: Option<&str>,
    host: Option<&str>,
    port: Option<u16>,
    proto: Option<&str>,
    bytes_in: Option<i64>,
    bytes_out: Option<i64>,
) -> anyhow::Result<i64> {
    if std::env::var("ARW_EGRESS_LEDGER_ENABLE").ok().as_deref() == Some("1") {
        return state.kernel.append_egress(
            decision,
            reason,
            host,
            port.map(|p| p as i64).as_ref().copied(),
            proto,
            bytes_in,
            bytes_out,
            None,
            None,
            Some(&net_posture()),
        );
    }
    Ok(0)
}
