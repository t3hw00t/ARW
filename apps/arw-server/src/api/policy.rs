use axum::http::HeaderMap;
use axum::response::IntoResponse;
use axum::{extract::State, Json};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::path::Path;
use tokio::fs as afs;
use utoipa::ToSchema;

use crate::config;
use crate::{tools::guardrails, AppState};
use arw_policy::{AbacRequest, Entity};
use arw_topics as topics;
use tracing::warn;

/// Current ABAC policy snapshot.
#[utoipa::path(
    get,
    path = "/state/policy",
    tag = "Policy",
    responses((status = 200, description = "Policy snapshot", body = serde_json::Value))
)]
pub async fn state_policy(State(state): State<AppState>) -> impl axum::response::IntoResponse {
    Json(state.policy().snapshot().await)
}

/// Reload policy from env/config (admin token required).
#[utoipa::path(
    post,
    path = "/policy/reload",
    tag = "Policy",
    responses(
        (status = 200, description = "Reloaded", body = serde_json::Value),
        (status = 401, description = "Unauthorized")
    )
)]
pub async fn policy_reload(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> impl axum::response::IntoResponse {
    if !crate::admin_ok(&headers) {
        return (
            axum::http::StatusCode::UNAUTHORIZED,
            Json(json!({"type":"about:blank","title":"Unauthorized","status":401})),
        )
            .into_response();
    }
    let newp = state.policy().reload_from_env().await;
    (
        axum::http::StatusCode::OK,
        Json(json!({"ok": true, "policy": newp.snapshot()})),
    )
        .into_response()
}

#[derive(Deserialize, ToSchema)]
pub(crate) struct PolicySimReq {
    #[serde(default)]
    kind: Option<String>,
    #[serde(default)]
    action: Option<String>,
    #[serde(default)]
    subject: Option<Value>,
    #[serde(default)]
    resource: Option<Value>,
}

/// Evaluate a candidate ABAC request payload.
#[utoipa::path(
    post,
    path = "/policy/simulate",
    tag = "Policy",
    request_body = PolicySimReq,
    responses((status = 200, description = "Decision", body = serde_json::Value))
)]
pub async fn policy_simulate(
    State(state): State<AppState>,
    Json(req): Json<PolicySimReq>,
) -> impl axum::response::IntoResponse {
    let action = req.action.or(req.kind).unwrap_or_default();
    let subj = req.subject.map(|v| Entity {
        kind: v
            .get("kind")
            .and_then(|x| x.as_str())
            .unwrap_or("node")
            .to_string(),
        id: v
            .get("id")
            .and_then(|x| x.as_str())
            .unwrap_or("local")
            .to_string(),
        attrs: v.get("attrs").cloned().unwrap_or(serde_json::json!({})),
    });
    let res = req.resource.map(|v| Entity {
        kind: v
            .get("kind")
            .and_then(|x| x.as_str())
            .unwrap_or("action")
            .to_string(),
        id: v
            .get("id")
            .and_then(|x| x.as_str())
            .unwrap_or(&action)
            .to_string(),
        attrs: v.get("attrs").cloned().unwrap_or(serde_json::json!({})),
    });
    let d = state
        .policy()
        .evaluate_abac(&AbacRequest {
            action,
            subject: subj,
            resource: res,
        })
        .await;
    Json(d)
}

#[derive(Deserialize, ToSchema)]
pub struct GuardrailApplyRequest {
    pub preset: String,
    #[serde(default)]
    pub dry_run: bool,
}

#[derive(Serialize, ToSchema)]
pub struct GuardrailApplyResponse {
    pub ok: bool,
    pub preset: String,
    pub path: String,
    pub digest: String,
    pub dry_run: bool,
}

#[utoipa::path(
    post,
    path = "/policy/guardrails/apply",
    tag = "Policy",
    request_body = GuardrailApplyRequest,
    responses(
        (status = 200, description = "Guardrail preset applied", body = GuardrailApplyResponse),
        (status = 400, description = "Invalid request", body = arw_protocol::ProblemDetails),
        (status = 401, description = "Unauthorized", body = arw_protocol::ProblemDetails),
        (status = 404, description = "Preset not found", body = arw_protocol::ProblemDetails),
        (status = 500, description = "Error", body = arw_protocol::ProblemDetails)
    )
)]
pub async fn policy_guardrails_apply(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<GuardrailApplyRequest>,
) -> impl IntoResponse {
    if let Err(resp) = crate::responses::require_admin(&headers) {
        return *resp;
    }

    let preset = req.preset.trim();
    if preset.is_empty() {
        return crate::responses::problem_response(
            axum::http::StatusCode::BAD_REQUEST,
            "Bad Request",
            Some("preset must be provided"),
        );
    }

    let Some(preset_path) = config::guardrail_preset_path(preset) else {
        return crate::responses::problem_response(
            axum::http::StatusCode::NOT_FOUND,
            "Preset not found",
            Some("guardrail preset missing"),
        );
    };

    let bytes = match afs::read(&preset_path).await {
        Ok(b) => b,
        Err(err) => {
            return crate::responses::problem_response(
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                "Error",
                Some(&format!("failed to read preset: {err}")),
            );
        }
    };

    let digest = digest_hex(&bytes);
    let dest = config::gating_config_path()
        .unwrap_or_else(|| crate::util::state_dir().join("configs").join("gating.toml"));
    let dest_display = dest.display().to_string();

    if !req.dry_run {
        if let Some(parent) = dest.parent() {
            if let Err(err) = afs::create_dir_all(parent).await {
                return crate::responses::problem_response(
                    axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                    "Error",
                    Some(&format!("failed to create config directory: {err}")),
                );
            }
        }
        if let Err(err) = write_atomic(&dest, &bytes).await {
            return crate::responses::problem_response(
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                "Error",
                Some(&format!("failed to write gating config: {err}")),
            );
        }
        arw_core::gating::reload_from_config(&dest_display);
        state.bus().publish(
            topics::TOPIC_POLICY_GUARDRAILS_APPLIED,
            &json!({
                "preset": preset,
                "path": dest_display,
                "digest": digest,
            }),
        );
        if let Err(err) = guardrails::record_applied(preset, &digest, &dest_display).await {
            warn!(%preset, %dest_display, error = %err, "failed to persist guardrail metadata");
        }
    }

    Json(GuardrailApplyResponse {
        ok: true,
        preset: preset.to_string(),
        path: dest_display,
        digest,
        dry_run: req.dry_run,
    })
    .into_response()
}

fn digest_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}

async fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        afs::create_dir_all(parent).await?;
    }
    let tmp = path.with_extension("tmp");
    afs::write(&tmp, bytes).await?;
    match afs::rename(&tmp, path).await {
        Ok(_) => Ok(()),
        Err(_) => {
            let _ = afs::remove_file(path).await;
            let result = afs::rename(&tmp, path).await;
            if result.is_err() {
                let _ = afs::remove_file(&tmp).await;
            }
            result
        }
    }
}
