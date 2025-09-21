use axum::response::IntoResponse;
use axum::{extract::State, Json};
use serde::Deserialize;
use serde_json::{json, Value};
use utoipa::ToSchema;

use crate::egress_policy::{self, capability_candidates, lease_grant, reason_code, DenyReason};
use crate::AppState;
use arw_topics as topics;
use tracing::warn;

#[derive(Deserialize, ToSchema)]
pub(crate) struct EgressPreviewReq {
    pub url: String,
    #[serde(default)]
    pub method: Option<String>,
}

/// Dry‑run egress decision for a URL/method.
#[utoipa::path(
    post,
    path = "/egress/preview",
    tag = "Egress",
    request_body = EgressPreviewReq,
    responses(
        (status = 200, body = serde_json::Value),
        (status = 501, description = "Kernel disabled", body = serde_json::Value)
    )
)]
pub async fn egress_preview(
    State(state): State<AppState>,
    Json(req): Json<EgressPreviewReq>,
) -> impl IntoResponse {
    if !state.kernel_enabled() {
        return crate::responses::kernel_disabled();
    }
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
                .into_response();
        }
    };
    let host = url.host_str().map(|s| s.to_string());
    let port = url.port_or_known_default();
    let scheme = url.scheme().to_string();

    let policy = egress_policy::resolve_policy(&state).await;
    let posture_decision = egress_policy::evaluate(&policy, host.as_deref(), port, &scheme);
    let capability_candidates = capability_candidates(host.as_deref(), port, &scheme);
    let mut lease = lease_grant(&state, &capability_candidates).await;

    let mut meta = serde_json::Map::new();
    meta.insert("capabilities".into(), json!(capability_candidates));
    meta.insert("policy_posture".into(), json!(policy.posture.as_str()));
    meta.insert("policy_allow".into(), json!(posture_decision.allow));
    if let Some(reason) = posture_decision.reason {
        meta.insert("policy_reason".into(), json!(reason_code(reason)));
    }
    if let Some(ref lease_val) = lease {
        meta.insert("lease".into(), lease_val.clone());
        meta.insert("allowed_via".into(), json!("lease"));
    }

    if !posture_decision.allow && lease.is_none() {
        let reason = posture_decision
            .reason
            .unwrap_or(DenyReason::HostNotAllowed);
        meta.insert("deny_stage".into(), json!("posture"));
        meta.insert("deny_reason".into(), json!(reason_code(reason)));
        let meta_val = Value::Object(meta);
        if let Err(err) = maybe_log_egress(
            &state,
            "deny",
            Some(reason_code(reason)),
            host.as_deref(),
            port,
            Some(&scheme),
            None,
            None,
            None,
            None,
            Some(&meta_val),
        )
        .await
        {
            warn!(?err, "failed to record denied egress preview");
        }
        return (
            axum::http::StatusCode::OK,
            Json(json!({
                "allow": false,
                "reason": reason_code(reason),
                "host": host,
                "port": port,
                "protocol": scheme,
                "meta": meta_val
            })),
        )
            .into_response();
    }

    let policy_decision = state.policy().lock().await.evaluate_action(&kind);
    if !policy_decision.allow {
        if let Some(cap) = policy_decision.require_capability.as_deref() {
            let lease_vec = vec![cap.to_string()];
            if let Some(lease_val) = lease_grant(&state, &lease_vec).await {
                lease = Some(lease_val.clone());
                meta.insert("lease".into(), lease_val);
                meta.insert("allowed_via".into(), json!("lease"));
                meta.insert("policy_required_capability".into(), json!(cap));
            } else {
                meta.insert("deny_stage".into(), json!("policy"));
                meta.insert("deny_reason".into(), json!("lease_required"));
                meta.insert("policy_required_capability".into(), json!(cap));
                let meta_val = Value::Object(meta);
                if let Err(err) = maybe_log_egress(
                    &state,
                    "deny",
                    Some("lease_required"),
                    host.as_deref(),
                    port,
                    Some(&scheme),
                    None,
                    None,
                    None,
                    None,
                    Some(&meta_val),
                )
                .await
                {
                    warn!(?err, "failed to record lease-required preview");
                }
                return (
                    axum::http::StatusCode::OK,
                    Json(json!({
                        "allow": false,
                        "reason": "lease_required",
                        "host": host,
                        "port": port,
                        "protocol": scheme,
                        "require_capability": cap,
                        "meta": meta_val
                    })),
                )
                    .into_response();
            }
        } else {
            meta.insert("deny_stage".into(), json!("policy"));
            meta.insert("deny_reason".into(), json!("policy"));
            let meta_val = Value::Object(meta);
            if let Err(err) = maybe_log_egress(
                &state,
                "deny",
                Some("policy"),
                host.as_deref(),
                port,
                Some(&scheme),
                None,
                None,
                None,
                None,
                Some(&meta_val),
            )
            .await
            {
                warn!(?err, "failed to record policy-denied preview");
            }
            return (
                axum::http::StatusCode::OK,
                Json(json!({
                    "allow": false,
                    "reason": "policy",
                    "host": host,
                    "port": port,
                    "protocol": scheme,
                    "meta": meta_val
                })),
            )
                .into_response();
        }
    }

    if !meta.contains_key("allowed_via") {
        meta.insert("allowed_via".into(), json!("policy"));
    }
    if let Some(ref lease_val) = lease {
        meta.insert("lease".into(), lease_val.clone());
    }
    let meta_val = Value::Object(meta.clone());

    let log_reason = meta
        .get("allowed_via")
        .and_then(|v| v.as_str())
        .unwrap_or("policy");
    if let Err(err) = maybe_log_egress(
        &state,
        "allow",
        Some(log_reason),
        host.as_deref(),
        port,
        Some(&scheme),
        None,
        None,
        None,
        None,
        Some(&meta_val),
    )
    .await
    {
        warn!(?err, "failed to record egress preview decision");
    }
    (
        axum::http::StatusCode::OK,
        Json(json!({
            "allow": true,
            "host": host,
            "port": port,
            "protocol": scheme,
            "meta": meta_val
        })),
    )
        .into_response()
}

fn net_posture() -> String {
    crate::util::effective_posture()
}

#[allow(clippy::too_many_arguments)]
async fn maybe_log_egress(
    state: &AppState,
    decision: &str,
    reason: Option<&str>,
    host: Option<&str>,
    port: Option<u16>,
    proto: Option<&str>,
    bytes_in: Option<i64>,
    bytes_out: Option<i64>,
    corr_id: Option<&str>,
    proj: Option<&str>,
    meta: Option<&Value>,
) -> anyhow::Result<i64> {
    let mut row_id: i64 = 0;
    if std::env::var("ARW_EGRESS_LEDGER_ENABLE").ok().as_deref() == Some("1")
        && state.kernel_enabled()
    {
        if let Some(kernel) = state.kernel_if_enabled() {
            row_id = kernel
                .append_egress_async(
                    decision.to_string(),
                    reason.map(|s| s.to_string()),
                    host.map(|s| s.to_string()),
                    port.map(|p| p as i64),
                    proto.map(|s| s.to_string()),
                    bytes_in,
                    bytes_out,
                    corr_id.map(|s| s.to_string()),
                    proj.map(|s| s.to_string()),
                    Some(net_posture()),
                    meta.cloned(),
                )
                .await?;
        }
    }
    state.bus().publish(
        topics::TOPIC_EGRESS_LEDGER_APPENDED,
        &json!({
            "id": if row_id > 0 { Value::from(row_id) } else { Value::Null },
            "decision": decision,
            "reason": reason,
            "dest_host": host,
            "dest_port": port,
            "protocol": proto,
            "bytes_in": bytes_in,
            "bytes_out": bytes_out,
            "corr_id": corr_id,
            "proj": proj,
            "posture": net_posture(),
            "meta": meta.cloned().unwrap_or(Value::Null)
        }),
    );
    Ok(row_id)
}
