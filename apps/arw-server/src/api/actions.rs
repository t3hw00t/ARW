use anyhow::Error;
use axum::response::IntoResponse;
use axum::{
    extract::{Path, State},
    Json,
};
use chrono::SecondsFormat;
use serde::Deserialize;
use serde_json::{json, Value};
use tracing::warn;
use utoipa::ToSchema;

use crate::{capsule_guard, staging, AppState};
use arw_topics as topics;

#[derive(Deserialize, ToSchema)]
pub(crate) struct ActionReq {
    pub kind: String,
    #[serde(default)]
    pub input: Value,
    #[serde(default)]
    pub idem_key: Option<String>,
}

#[derive(Debug, Clone)]
pub(crate) struct ActionSubmitOutcome {
    pub id: String,
    pub staged: bool,
    pub stage_mode: Option<String>,
    pub reused: bool,
}

#[derive(Debug)]
pub(crate) enum SubmitActionError {
    KernelDisabled,
    PolicyDenied {
        require_capability: Option<String>,
        explain: serde_json::Value,
    },
    QueueFull {
        limit: i64,
        queued: i64,
    },
    Internal(Error),
}

pub(crate) async fn submit_action(
    state: &AppState,
    req: ActionReq,
) -> Result<ActionSubmitOutcome, SubmitActionError> {
    capsule_guard::refresh_capsules(state).await;
    if !state.kernel_enabled() {
        return Err(SubmitActionError::KernelDisabled);
    }

    let decision = state.policy().lock().await.evaluate_action(&req.kind);
    if !decision.allow {
        if let Some(cap) = decision.require_capability.as_deref() {
            let lease = state
                .kernel()
                .find_valid_lease_async("local", cap)
                .await
                .map_err(SubmitActionError::Internal)?;
            if lease.is_none() {
                state.bus().publish(
                    topics::TOPIC_POLICY_DECISION,
                    &json!({
                        "action": req.kind,
                        "allow": false,
                        "require_capability": cap,
                        "explain": decision.explain,
                    }),
                );
                return Err(SubmitActionError::PolicyDenied {
                    require_capability: Some(cap.to_string()),
                    explain: decision.explain,
                });
            }
        }
    }

    let mut reuse_id: Option<String> = None;
    if let Some(ref idem) = req.idem_key {
        match state.kernel().find_action_by_idem_async(idem).await {
            Ok(Some(existing)) => reuse_id = Some(existing),
            Ok(None) => {}
            Err(err) => {
                warn!(target: "actions", "find_action_by_idem failed: {err:?}");
            }
        }
    }

    if reuse_id.is_none() {
        match staging::maybe_stage_action(state, &req.kind, &req.input).await {
            Ok(Some(staging_id)) => {
                return Ok(ActionSubmitOutcome {
                    id: staging_id,
                    staged: true,
                    stage_mode: Some(staging::mode_label().to_string()),
                    reused: false,
                });
            }
            Ok(None) => {}
            Err(err) => {
                warn!(target: "staging", "failed to stage action: {err:?}");
            }
        }
        let max_q: i64 = std::env::var("ARW_ACTIONS_QUEUE_MAX")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(1024);
        if max_q > 0 {
            if let Ok(nq) = state.kernel().count_actions_by_state_async("queued").await {
                if nq >= max_q {
                    return Err(SubmitActionError::QueueFull {
                        limit: max_q,
                        queued: nq,
                    });
                }
            }
        }
    }

    let (id, reused) = if let Some(ref idem) = req.idem_key {
        if let Some(existing) = reuse_id.clone() {
            (existing, true)
        } else {
            let id = uuid::Uuid::new_v4().to_string();
            if let Err(err) = state
                .kernel()
                .insert_action_async(
                    &id,
                    &req.kind,
                    &req.input,
                    None,
                    Some(idem.as_str()),
                    "queued",
                )
                .await
            {
                return Err(SubmitActionError::Internal(err));
            }
            (id, false)
        }
    } else {
        let id = uuid::Uuid::new_v4().to_string();
        if let Err(err) = state
            .kernel()
            .insert_action_async(&id, &req.kind, &req.input, None, None, "queued")
            .await
        {
            return Err(SubmitActionError::Internal(err));
        }
        (id, false)
    };

    let payload = json!({"id": id, "kind": req.kind, "status": "queued"});
    let now = chrono::Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true);
    let env = arw_events::Envelope {
        time: now,
        kind: topics::TOPIC_ACTIONS_SUBMITTED.into(),
        payload,
        policy: None,
        ce: None,
    };
    state.bus().publish(&env.kind, &env.payload);
    if let Err(err) = state
        .kernel()
        .append_contribution_async("local", "task.submit", 1.0, "task", None, None, None)
        .await
    {
        warn!(target: "actions", "append_contribution failed: {err:?}");
    }

    Ok(ActionSubmitOutcome {
        id,
        staged: false,
        stage_mode: None,
        reused,
    })
}

/// Submit an action to the triad queue.
#[utoipa::path(
    post,
    path = "/actions",
    tag = "Actions",
    request_body = ActionReq,
    responses(
        (status = 202, description = "Accepted", body = serde_json::Value),
        (status = 501, description = "Kernel disabled", body = serde_json::Value)
    )
)]
pub async fn actions_submit(
    State(state): State<AppState>,
    Json(req): Json<ActionReq>,
) -> axum::response::Response {
    match submit_action(&state, req).await {
        Ok(outcome) if outcome.staged => (
            axum::http::StatusCode::ACCEPTED,
            Json(json!({
                "staged": true,
                "id": outcome.id,
                "mode": outcome.stage_mode.unwrap_or_else(|| staging::mode_label().to_string())
            })),
        )
            .into_response(),
        Ok(outcome) => (
            axum::http::StatusCode::ACCEPTED,
            Json(json!({
                "id": outcome.id,
                "ok": true,
                "staged": false,
                "reused": outcome.reused
            })),
        )
            .into_response(),
        Err(SubmitActionError::KernelDisabled) => crate::responses::kernel_disabled(),
        Err(SubmitActionError::PolicyDenied {
            require_capability,
            explain,
        }) => (
            axum::http::StatusCode::FORBIDDEN,
            Json(json!({
                "type":"about:blank","title":"Forbidden","status":403,
                "detail":"Denied (lease required)",
                "explain": explain,
                "require_capability": require_capability
            })),
        )
            .into_response(),
        Err(SubmitActionError::QueueFull { limit, queued }) => (
            axum::http::StatusCode::TOO_MANY_REQUESTS,
            Json(json!({
                "type":"about:blank","title":"Too Many Requests","status":429,
                "detail":"queue is full","limit": limit, "queued": queued
            })),
        )
            .into_response(),
        Err(SubmitActionError::Internal(err)) => (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({
                "type":"about:blank","title":"Error","status":500,
                "detail": err.to_string()
            })),
        )
            .into_response(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use arw_policy::PolicyEngine;
    use axum::{body::to_bytes, http::StatusCode};
    use serde_json::Value;
    use std::sync::Arc;
    use tempfile::tempdir;
    use tokio::sync::Mutex;

    async fn build_state(path: &std::path::Path) -> AppState {
        std::env::set_var("ARW_STATE_DIR", path.display().to_string());
        let bus = arw_events::Bus::new_with_replay(16, 16);
        let kernel = arw_kernel::Kernel::open(path).expect("init kernel");
        let policy = PolicyEngine::load_from_env();
        let policy_arc = Arc::new(Mutex::new(policy));
        let host: Arc<dyn arw_wasi::ToolHost> = Arc::new(arw_wasi::NoopHost);
        AppState::builder(bus, kernel, policy_arc, host, true)
            .with_sse_capacity(16)
            .build()
            .await
    }

    #[tokio::test]
    async fn actions_get_exposes_guard_and_posture() {
        let temp = tempdir().expect("tempdir");
        let state = build_state(temp.path()).await;

        let action_id = uuid::Uuid::new_v4().to_string();
        state
            .kernel()
            .insert_action_async(
                &action_id,
                "net.http.get",
                &json!({"url": "https://example.com"}),
                None,
                None,
                "completed",
            )
            .await
            .expect("insert action");

        let stored_output = json!({
            "value": {"status": "ok"},
            "posture": "secure",
            "guard": {
                "allowed": true,
                "policy_allow": false,
                "required_capabilities": ["net:http", "io:egress"],
                "lease": {
                    "id": "lease-1",
                    "subject": Some("local"),
                    "capability": "net:http",
                    "scope": None::<String>,
                    "ttl_until": "2099-01-01T00:00:00Z"
                }
            }
        });

        state
            .kernel()
            .update_action_result_async(action_id.clone(), Some(stored_output.clone()), None)
            .await
            .expect("store output");

        let response = actions_get(State(state.clone()), Path(action_id.clone())).await;
        let (parts, body) = response.into_parts();
        assert_eq!(parts.status, StatusCode::OK);
        let bytes = to_bytes(body, usize::MAX).await.expect("body bytes");
        let value: Value = serde_json::from_slice(&bytes).expect("json");
        assert_eq!(value["id"].as_str(), Some(action_id.as_str()));
        assert_eq!(value["output"].get("value"), stored_output.get("value"),);
        assert_eq!(value["output"]["posture"].as_str(), Some("secure"));
        let expected_guard = json!({
            "allowed": true,
            "policy_allow": false,
            "required_capabilities": ["net:http", "io:egress"],
            "lease": {
                "capability": "net:http",
                "ttl_until": "2099-01-01T00:00:00Z"
            }
        });
        assert_eq!(value["output"]["guard"], expected_guard);
        assert_eq!(value["guard"], expected_guard);
        assert_eq!(value["posture"].as_str(), Some("secure"));
    }
}

/// Get action details by id.
#[utoipa::path(
    get,
    path = "/actions/{id}",
    tag = "Actions",
    params(("id" = String, Path, description = "Action id")),
    responses(
        (status = 200, description = "Action", body = serde_json::Value),
        (status = 404, description = "Not found"),
        (status = 501, description = "Kernel disabled", body = serde_json::Value)
    )
)]
pub async fn actions_get(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> axum::response::Response {
    if !state.kernel_enabled() {
        return crate::responses::kernel_disabled();
    }
    match state.kernel().get_action_async(&id).await {
        Ok(Some(a)) => {
            let mut body = serde_json::Map::new();
            body.insert("id".into(), json!(a.id));
            body.insert("kind".into(), json!(a.kind));
            body.insert("state".into(), json!(a.state));
            body.insert("input".into(), a.input);
            let sanitized_output = a
                .output
                .as_ref()
                .map(sanitize_output_value)
                .unwrap_or(Value::Null);
            body.insert("output".into(), sanitized_output.clone());
            if let Some(err) = a.error {
                body.insert("error".into(), json!(err));
            }
            if let Value::Object(ref map) = sanitized_output {
                if let Some(posture) = map.get("posture") {
                    body.entry("posture".to_string()).or_insert(posture.clone());
                }
                if let Some(guard) = map.get("guard") {
                    body.entry("guard".to_string()).or_insert(guard.clone());
                }
            }
            body.insert("created".into(), json!(a.created));
            body.insert("updated".into(), json!(a.updated));
            (axum::http::StatusCode::OK, Json(Value::Object(body))).into_response()
        }
        Ok(None) => (
            axum::http::StatusCode::NOT_FOUND,
            Json(json!({"type":"about:blank","title":"Not Found","status":404})),
        )
            .into_response(),
        Err(e) => (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            Json(
                json!({"type":"about:blank","title":"Error","status":500, "detail": e.to_string()}),
            ),
        )
            .into_response(),
    }
}

fn sanitize_output_value(value: &Value) -> Value {
    match value {
        Value::Object(map) => {
            let mut sanitized = map.clone();
            if let Some(guard) = map.get("guard") {
                sanitized.insert("guard".into(), sanitize_guard_value(guard));
            }
            Value::Object(sanitized)
        }
        other => other.clone(),
    }
}

fn sanitize_guard_value(value: &Value) -> Value {
    if let Value::Object(map) = value {
        let mut sanitized = serde_json::Map::new();
        if let Some(v) = map.get("allowed") {
            sanitized.insert("allowed".into(), v.clone());
        }
        if let Some(v) = map.get("policy_allow") {
            sanitized.insert("policy_allow".into(), v.clone());
        }
        if let Some(v) = map.get("required_capabilities") {
            sanitized.insert("required_capabilities".into(), v.clone());
        }
        if let Some(lease) = map.get("lease") {
            if let Value::Object(lease_map) = lease {
                let mut redacted = serde_json::Map::new();
                if let Some(cap) = lease_map.get("capability") {
                    redacted.insert("capability".into(), cap.clone());
                }
                if let Some(ttl) = lease_map.get("ttl_until") {
                    redacted.insert("ttl_until".into(), ttl.clone());
                }
                if let Some(scope) = lease_map.get("scope") {
                    if !scope.is_null() {
                        redacted.insert("scope".into(), scope.clone());
                    }
                }
                if !redacted.is_empty() {
                    sanitized.insert("lease".into(), Value::Object(redacted));
                }
            }
        }
        Value::Object(sanitized)
    } else {
        value.clone()
    }
}

#[derive(Deserialize, ToSchema)]
pub(crate) struct ActionStateReq {
    pub state: String,
    #[serde(default)]
    pub error: Option<String>,
}
/// Update lifecycle state of an action.
#[utoipa::path(
    post,
    path = "/actions/{id}/state",
    tag = "Actions",
    params(("id" = String, Path, description = "Action id")),
    request_body = ActionStateReq,
    responses(
        (status = 200, description = "Updated", body = serde_json::Value),
        (status = 404, description = "Not found"),
        (status = 400, description = "Invalid state"),
        (status = 501, description = "Kernel disabled", body = serde_json::Value)
    )
)]
pub async fn actions_state_set(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(req): Json<ActionStateReq>,
) -> axum::response::Response {
    if !state.kernel_enabled() {
        return crate::responses::kernel_disabled();
    }
    let allowed = ["queued", "running", "completed", "failed"];
    if !allowed.contains(&req.state.as_str()) {
        return (
            axum::http::StatusCode::BAD_REQUEST,
            Json(
                json!({"type":"about:blank","title":"Bad Request","status":400, "detail":"invalid state"}),
            ),
        )
            .into_response();
    }
    match state.kernel().set_action_state_async(&id, &req.state).await {
        Ok(true) => {
            // Publish a transition event
            let kind = match req.state.as_str() {
                "running" => topics::TOPIC_ACTIONS_RUNNING,
                "completed" => topics::TOPIC_ACTIONS_COMPLETED,
                "failed" => topics::TOPIC_ACTIONS_FAILED,
                _ => topics::TOPIC_ACTIONS_UPDATED,
            };
            let payload = json!({"id": id, "state": req.state, "error": req.error});
            let now = chrono::Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true);
            let env = arw_events::Envelope {
                time: now,
                kind: kind.into(),
                payload: payload.clone(),
                policy: None,
                ce: None,
            };
            state.bus().publish(&env.kind, &env.payload);
            (axum::http::StatusCode::OK, Json(json!({"ok": true}))).into_response()
        }
        Ok(false) => (
            axum::http::StatusCode::NOT_FOUND,
            Json(json!({"type":"about:blank","title":"Not Found","status":404})),
        )
            .into_response(),
        Err(e) => (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            Json(
                json!({"type":"about:blank","title":"Error","status":500, "detail": e.to_string()}),
            ),
        )
            .into_response(),
    }
}
