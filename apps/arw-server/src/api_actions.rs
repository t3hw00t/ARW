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

use crate::{staging, AppState};
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
    let _ = state.capsules().replay_all().await;
    if !state.kernel_enabled() {
        return Err(SubmitActionError::KernelDisabled);
    }

    let decision = state.policy.lock().await.evaluate_action(&req.kind);
    if !decision.allow {
        if let Some(cap) = decision.require_capability.as_deref() {
            let lease = state
                .kernel()
                .find_valid_lease_async("local", cap)
                .await
                .map_err(|e| SubmitActionError::Internal(e.into()))?;
            if lease.is_none() {
                state.bus.publish(
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
        match staging::maybe_stage_action(&state, &req.kind, &req.input).await {
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
                return Err(SubmitActionError::Internal(err.into()));
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
            return Err(SubmitActionError::Internal(err.into()));
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
    state.bus.publish(&env.kind, &env.payload);
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
                "staged": false
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
        Ok(Some(a)) => (
            axum::http::StatusCode::OK,
            Json(json!({
                "id": a.id,
                "kind": a.kind,
                "state": a.state,
                "input": a.input,
                "output": a.output,
                "error": a.error,
                "created": a.created,
                "updated": a.updated
            })),
        )
            .into_response(),
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
            state.bus.publish(&env.kind, &env.payload);
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
