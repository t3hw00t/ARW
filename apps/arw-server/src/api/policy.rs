use axum::http::HeaderMap;
use axum::response::IntoResponse;
use axum::{
    extract::{Query, State},
    Json,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::HashSet;
use std::path::Path;
use tokio::fs as afs;
use utoipa::ToSchema;

use super::events::EventsJournalResponse;
use crate::config;
use crate::{
    capsule_guard::{self, CapsuleAdoptError},
    request_ctx,
    tools::guardrails,
    AppState,
};
use arw_core::capsule_presets::{self, CapsulePresetError};
use arw_events::Envelope;
use arw_policy::{AbacRequest, Entity};
use arw_protocol::GatingCapsule;
use arw_topics as topics;
use tracing::{info, warn};

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
    if !crate::admin_ok(&headers).await {
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub corr_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request_id: Option<String>,
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
    if let Err(resp) = crate::responses::require_admin(&headers).await {
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

    let corr = request_ctx::current();

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

        let mut event = json!({
            "preset": preset,
            "path": dest_display,
            "digest": digest,
        });
        if let Some(ref corr) = corr {
            if let Some(obj) = event.as_object_mut() {
                obj.entry("corr_id".to_string())
                    .or_insert_with(|| Value::String(corr.corr_id().to_string()));
                obj.entry("request_id".to_string())
                    .or_insert_with(|| Value::String(corr.request_id().to_string()));
            }
        }

        state
            .bus()
            .publish(topics::TOPIC_POLICY_GUARDRAILS_APPLIED, &event);
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
        corr_id: corr.as_ref().map(|c| c.corr_id().to_string()),
        request_id: corr.as_ref().map(|c| c.request_id().to_string()),
    })
    .into_response()
}

#[derive(Serialize, ToSchema)]
pub struct CapsulePresetSummaryResponse {
    pub id: String,
    pub file_name: String,
    pub path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub issuer: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hop_ttl: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lease_duration_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub renew_within_ms: Option<u64>,
    pub denies: usize,
    pub contracts: usize,
    pub signature_present: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sha256: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub modified_ms: Option<u64>,
}

impl From<capsule_presets::CapsulePresetSummary> for CapsulePresetSummaryResponse {
    fn from(summary: capsule_presets::CapsulePresetSummary) -> Self {
        Self {
            id: summary.id,
            file_name: summary.file_name,
            path: summary.path,
            version: summary.version,
            issuer: summary.issuer,
            hop_ttl: summary.hop_ttl,
            lease_duration_ms: summary.lease_duration_ms,
            renew_within_ms: summary.renew_within_ms,
            denies: summary.denies,
            contracts: summary.contracts,
            signature_present: summary.signature_present,
            sha256: summary.sha256,
            modified_ms: summary.modified_ms,
        }
    }
}

#[derive(Serialize, ToSchema)]
pub struct CapsulePresetListResponse {
    pub presets: Vec<CapsulePresetSummaryResponse>,
    pub count: usize,
}

/// List capsule presets packaged with the install.
#[utoipa::path(
    get,
    path = "/admin/policy/capsules/presets",
    tag = "Policy",
    responses(
        (status = 200, description = "Available capsule presets", body = CapsulePresetListResponse),
        (status = 401, description = "Unauthorized", body = arw_protocol::ProblemDetails),
        (status = 500, description = "Preset load failed", body = arw_protocol::ProblemDetails)
    )
)]
pub async fn policy_capsules_presets(headers: HeaderMap) -> impl IntoResponse {
    if let Err(resp) = crate::responses::require_admin(&headers).await {
        return *resp;
    }
    match capsule_presets::list_capsule_presets() {
        Ok(presets) => {
            let presets: Vec<CapsulePresetSummaryResponse> = presets
                .into_iter()
                .map(|preset| CapsulePresetSummaryResponse::from(preset.summary))
                .collect();
            let response = CapsulePresetListResponse {
                count: presets.len(),
                presets,
            };
            Json(response).into_response()
        }
        Err(err) => {
            tracing::error!(
                target: "arw::policy",
                error = %err,
                "failed to list capsule presets"
            );
            crate::responses::problem_response(
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                "Preset Load Failed",
                Some(&err.to_string()),
            )
        }
    }
}

#[derive(Deserialize, ToSchema)]
pub struct CapsuleAdoptRequest {
    #[serde(default)]
    pub preset_id: Option<String>,
    #[serde(default)]
    pub capsule: Option<Value>,
    #[serde(default)]
    pub reason: Option<String>,
}

#[derive(Serialize, ToSchema)]
pub struct CapsuleAdoptResponse {
    pub ok: bool,
    pub notify: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub preset_id: Option<String>,
    pub capsule_id: String,
    pub snapshot: Value,
}

/// Adopt a capsule preset or inline capsule payload.
#[utoipa::path(
    post,
    path = "/admin/policy/capsules/adopt",
    tag = "Policy",
    request_body = CapsuleAdoptRequest,
    responses(
        (status = 200, description = "Capsule adopted", body = CapsuleAdoptResponse),
        (status = 400, description = "Invalid request", body = arw_protocol::ProblemDetails),
        (status = 401, description = "Unauthorized", body = arw_protocol::ProblemDetails),
        (status = 403, description = "Verification failed", body = arw_protocol::ProblemDetails),
        (status = 404, description = "Preset not found", body = arw_protocol::ProblemDetails),
        (status = 500, description = "Preset load failed", body = arw_protocol::ProblemDetails)
    )
)]
pub async fn policy_capsules_adopt(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<CapsuleAdoptRequest>,
) -> impl IntoResponse {
    if let Err(resp) = crate::responses::require_admin(&headers).await {
        return *resp;
    }

    let preset_opt = req.preset_id.as_ref().and_then(|raw| {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed)
        }
    });
    let has_capsule_payload = req.capsule.is_some();

    if preset_opt.is_some() && has_capsule_payload {
        return crate::responses::problem_response(
            axum::http::StatusCode::BAD_REQUEST,
            "Bad Request",
            Some("provide either `preset_id` or `capsule`, not both"),
        );
    }
    if preset_opt.is_none() && !has_capsule_payload {
        return crate::responses::problem_response(
            axum::http::StatusCode::BAD_REQUEST,
            "Bad Request",
            Some("provide `preset_id` or an inline `capsule` payload"),
        );
    }

    let (capsule, preset_id) = match preset_opt {
        Some(id) => match capsule_presets::load_capsule_preset(id) {
            Ok(preset) => (preset.capsule, Some(id.to_string())),
            Err(CapsulePresetError::NotFound(_)) => {
                return crate::responses::problem_response(
                    axum::http::StatusCode::NOT_FOUND,
                    "Preset Not Found",
                    Some("capsule preset not found"),
                );
            }
            Err(err) => {
                tracing::error!(
                    target: "arw::policy",
                    preset_id = id,
                    error = %err,
                    "failed to load capsule preset"
                );
                return crate::responses::problem_response(
                    axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                    "Preset Load Failed",
                    Some(&err.to_string()),
                );
            }
        },
        None => {
            let value = req.capsule.clone().unwrap();
            match serde_json::from_value::<GatingCapsule>(value) {
                Ok(cap) => (cap, None),
                Err(err) => {
                    return crate::responses::problem_response(
                        axum::http::StatusCode::BAD_REQUEST,
                        "Invalid Capsule",
                        Some(&err.to_string()),
                    );
                }
            }
        }
    };

    let corr = request_ctx::current();
    match capsule_guard::adopt_capsule_direct(&state, &capsule, corr.as_ref()).await {
        Ok(outcome) => {
            if let Some(reason) = req.reason.as_deref() {
                info!(
                    target: "arw::policy",
                    capsule_id = %outcome.snapshot.id,
                    preset = preset_id.as_deref().unwrap_or("inline"),
                    reason,
                    "policy capsule adopted via admin endpoint"
                );
            } else {
                info!(
                    target: "arw::policy",
                    capsule_id = %outcome.snapshot.id,
                    preset = preset_id.as_deref().unwrap_or("inline"),
                    "policy capsule adopted via admin endpoint"
                );
            }
            let snapshot = state.capsules().snapshot().await;
            Json(CapsuleAdoptResponse {
                ok: true,
                notify: outcome.notify,
                preset_id,
                capsule_id: outcome.snapshot.id,
                snapshot,
            })
            .into_response()
        }
        Err(CapsuleAdoptError::VerificationFailed) => crate::responses::problem_response(
            axum::http::StatusCode::FORBIDDEN,
            "Capsule Verification Failed",
            Some("capsule verification failed"),
        ),
    }
}

#[derive(Deserialize, ToSchema)]
pub struct CapsuleAuditQuery {
    #[serde(default)]
    pub limit: Option<usize>,
    #[serde(default)]
    pub prefix: Option<String>,
}

/// Return recent capsule audit events (policy.capsule.*).
#[utoipa::path(
    get,
    path = "/admin/policy/capsules/audit",
    tag = "Policy",
    params(
        ("limit" = Option<usize>, Query, description = "Max entries to return (default 50, max 500)"),
        ("prefix" = Option<String>, Query, description = "CSV of additional prefixes to include")
    ),
    responses(
        (status = 200, description = "Capsule audit entries", body = EventsJournalResponse),
        (status = 401, description = "Unauthorized", body = arw_protocol::ProblemDetails),
        (status = 404, description = "Journal disabled", body = arw_protocol::ProblemDetails),
        (status = 500, description = "Journal read failed", body = arw_protocol::ProblemDetails)
    )
)]
pub async fn policy_capsules_audit(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<CapsuleAuditQuery>,
) -> impl IntoResponse {
    if let Err(resp) = crate::responses::require_admin(&headers).await {
        return *resp;
    }
    if !state.kernel_enabled() {
        return crate::responses::problem_response(
            axum::http::StatusCode::NOT_FOUND,
            "Journal Disabled",
            Some("Capsule audit requires the kernel event journal"),
        );
    }
    let limit = query.limit.unwrap_or(50).min(500);
    let mut prefixes: Vec<String> = vec!["policy.capsule.".to_string()];
    if let Some(extra) = &query.prefix {
        for raw in extra.split(',') {
            let trimmed = raw.trim();
            if trimmed.is_empty() {
                continue;
            }
            let lower = trimmed.to_ascii_lowercase();
            if !prefixes.iter().any(|p| p == &lower) {
                prefixes.push(lower);
            }
        }
    }
    let query_prefixes = prefixes.clone();
    match state
        .kernel()
        .tail_events_async(limit as i64, query_prefixes.clone())
        .await
    {
        Ok((rows, total)) => {
            let entries: Vec<Envelope> = rows
                .into_iter()
                .map(|row| Envelope {
                    time: row.time,
                    kind: row.kind,
                    payload: row.payload,
                    policy: None,
                    ce: None,
                })
                .collect();
            let total_i64 = total.max(0);
            let truncated = total_i64 > entries.len() as i64;
            let response = EventsJournalResponse {
                prefixes: Some(query_prefixes),
                limit,
                total_matched: total_i64 as usize,
                truncated,
                skipped_lines: 0,
                source_files: vec!["sqlite:events".to_string()],
                entries,
            };
            (axum::http::StatusCode::OK, Json(response)).into_response()
        }
        Err(err) => crate::responses::problem_response(
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            "Journal Read Failed",
            Some(&err.to_string()),
        ),
    }
}

#[derive(Deserialize, ToSchema)]
pub struct CapsuleTeardownRequest {
    #[serde(default)]
    pub ids: Vec<String>,
    #[serde(default)]
    pub all: bool,
    #[serde(default)]
    pub reason: Option<String>,
    #[serde(default)]
    pub dry_run: bool,
}

#[derive(Serialize, ToSchema)]
pub struct CapsuleTeardownResponse {
    pub ok: bool,
    pub removed: Vec<Value>,
    pub not_found: Vec<String>,
    pub remaining: usize,
    pub dry_run: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

/// Emergency teardown for active policy capsules.
#[utoipa::path(
    post,
    path = "/admin/policy/capsules/teardown",
    tag = "Policy",
    request_body = CapsuleTeardownRequest,
    responses(
        (status = 200, description = "Teardown result", body = CapsuleTeardownResponse),
        (status = 400, description = "Invalid request", body = arw_protocol::ProblemDetails),
        (status = 401, description = "Unauthorized", body = arw_protocol::ProblemDetails)
    )
)]
pub async fn policy_capsules_teardown(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<CapsuleTeardownRequest>,
) -> impl IntoResponse {
    if let Err(resp) = crate::responses::require_admin(&headers).await {
        return *resp;
    }

    let mut ids_clean: Vec<String> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    for raw in req.ids {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            continue;
        }
        if seen.insert(trimmed.to_string()) {
            ids_clean.push(trimmed.to_string());
        }
    }

    let selection = if req.all {
        capsule_guard::CapsuleTeardownSelection::All
    } else {
        if ids_clean.is_empty() {
            return crate::responses::problem_response(
                axum::http::StatusCode::BAD_REQUEST,
                "Bad Request",
                Some("provide `all`: true or at least one capsule id"),
            );
        }
        capsule_guard::CapsuleTeardownSelection::Ids(&ids_clean)
    };

    let spec = capsule_guard::CapsuleTeardownSpec {
        selection,
        reason: req.reason.as_deref(),
        dry_run: req.dry_run,
    };

    let capsule_guard::CapsuleTeardownOutcome {
        removed,
        not_found,
        remaining,
        dry_run,
        reason,
    } = capsule_guard::emergency_teardown(&state, &spec).await;

    if dry_run {
        info!(
            target: "arw::policy",
            planned = removed.len(),
            missing = not_found.len(),
            reason = reason.as_deref().unwrap_or(""),
            "policy capsule teardown dry-run"
        );
    } else {
        warn!(
            target: "arw::policy",
            removed = removed.len(),
            missing = not_found.len(),
            reason = reason.as_deref().unwrap_or(""),
            "policy capsule emergency teardown executed"
        );
    }

    Json(CapsuleTeardownResponse {
        ok: true,
        removed,
        not_found,
        remaining,
        dry_run,
        reason,
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{self, env as test_env};
    use arw_events::Bus;
    use arw_policy::PolicyEngine;
    use arw_topics::TOPIC_POLICY_GUARDRAILS_APPLIED;
    use arw_wasi::NoopHost;
    use axum::body::{to_bytes, Body};
    use axum::extract::connect_info::ConnectInfo;
    use axum::http::{Request, StatusCode};
    use axum::{middleware, routing::post, Router};
    use std::net::SocketAddr;
    use std::sync::Arc;
    use tempfile::tempdir;
    use tokio::sync::Mutex;
    use tokio::time::{timeout, Duration};
    use tower::ServiceExt;

    async fn build_state(dir: &Path, env_guard: &mut test_env::EnvGuard) -> AppState {
        test_support::init_tracing();
        env_guard.set("ARW_STATE_DIR", dir.display().to_string());
        env_guard.set("ARW_DEBUG", "0");

        let bus = Bus::new_with_replay(64, 64);
        let kernel = arw_kernel::Kernel::open(dir).expect("init kernel for tests");
        let policy = PolicyEngine::load_from_env();
        let policy_handle = crate::policy::PolicyHandle::new(policy, bus.clone());
        let host: Arc<dyn arw_wasi::ToolHost> = Arc::new(NoopHost);

        AppState::builder(bus, kernel, policy_handle, host, true)
            .with_config_state(Arc::new(Mutex::new(json!({ "mode": "test" }))))
            .with_config_history(Arc::new(Mutex::new(Vec::new())))
            .with_sse_capacity(64)
            .build()
            .await
    }

    async fn policy_simulate_denies_action(action: &str) {
        let temp = tempdir().expect("tempdir");
        let mut ctx = test_support::begin_state_env(temp.path());
        ctx.env.remove("ARW_POLICY_FILE");
        ctx.env.set("ARW_SECURITY_POSTURE", "standard");

        let state = build_state(temp.path(), &mut ctx.env).await;
        let app = Router::new()
            .route("/policy/simulate", post(policy_simulate))
            .with_state(state);

        let request = Request::builder()
            .method("POST")
            .uri("/policy/simulate")
            .header("content-type", "application/json")
            .body(Body::from(format!(r#"{{"action":"{action}"}}"#)))
            .expect("request");

        let response = app.oneshot(request).await.expect("router response");
        assert_eq!(response.status(), StatusCode::OK);
        let bytes = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body bytes");
        let decision: Value = serde_json::from_slice(&bytes).expect("decision json");
        assert_eq!(decision.get("allow"), Some(&Value::Bool(false)));
        assert_eq!(
            decision.get("require_capability").and_then(Value::as_str),
            Some("runtime:manage")
        );
        let reason = decision
            .get("explain")
            .and_then(|v| v.get("reason"))
            .and_then(Value::as_str)
            .unwrap_or_default();
        assert_eq!(reason, "lease_required");
    }

    #[tokio::test]
    async fn policy_simulate_flags_runtime_restore_capability_requirement() {
        policy_simulate_denies_action("runtime.supervisor.restore").await;
    }

    #[tokio::test]
    async fn policy_simulate_flags_runtime_shutdown_capability_requirement() {
        policy_simulate_denies_action("runtime.supervisor.shutdown").await;
    }

    #[tokio::test]
    async fn guardrail_apply_event_carries_correlation_metadata() {
        let temp = tempdir().expect("tempdir");
        let mut ctx = test_support::begin_state_env(temp.path());
        ctx.env.set("ARW_ADMIN_TOKEN", "secret-token");
        ctx.env
            .set("ARW_CONFIG_DIR", temp.path().display().to_string());
        crate::tools::guardrails::reset_last_applied_for_tests();

        let preset_dir = temp.path().join("configs").join("guardrails");
        std::fs::create_dir_all(&preset_dir).expect("preset dir");
        std::fs::write(preset_dir.join("demo.toml"), b"# guardrail preset\n")
            .expect("write preset");

        let state = build_state(temp.path(), &mut ctx.env).await;
        let bus = state.bus();
        let mut rx =
            bus.subscribe_filtered(vec![TOPIC_POLICY_GUARDRAILS_APPLIED.to_string()], Some(4));

        let app = Router::new()
            .route("/policy/guardrails/apply", post(policy_guardrails_apply))
            .with_state(state)
            .layer(middleware::from_fn(crate::security::client_addr_mw))
            .layer(middleware::from_fn(crate::request_ctx::correlation_mw));

        let body = Body::from(r#"{"preset":"demo","dry_run":false}"#);
        let mut request = Request::builder()
            .method("POST")
            .uri("/policy/guardrails/apply")
            .header("content-type", "application/json")
            .header("x-arw-admin", "secret-token")
            .header("x-request-id", "req-guardrails")
            .header("x-arw-corr", "corr-guardrails")
            .body(body)
            .expect("request");
        request
            .extensions_mut()
            .insert(ConnectInfo(SocketAddr::from(([127, 0, 0, 1], 9999))));

        let response = app.oneshot(request).await.expect("response from router");
        assert_eq!(response.status(), StatusCode::OK);
        let bytes = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("response bytes");
        let value: Value = serde_json::from_slice(&bytes).expect("response json");
        assert_eq!(value.get("preset").and_then(Value::as_str), Some("demo"));
        assert_eq!(
            value.get("corr_id").and_then(Value::as_str),
            Some("corr-guardrails")
        );
        assert_eq!(
            value.get("request_id").and_then(Value::as_str),
            Some("req-guardrails")
        );

        let event = timeout(Duration::from_secs(1), rx.recv())
            .await
            .expect("event available")
            .expect("bus open");
        assert_eq!(event.kind, TOPIC_POLICY_GUARDRAILS_APPLIED);
        assert_eq!(event.payload["preset"].as_str(), Some("demo"));
        assert_eq!(event.payload["corr_id"].as_str(), Some("corr-guardrails"));
        assert_eq!(event.payload["request_id"].as_str(), Some("req-guardrails"));
        assert_eq!(event.payload["digest"].as_str(), value["digest"].as_str());
    }
}
