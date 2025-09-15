use axum::{extract::{State, Path}, Json};
use axum::response::IntoResponse;
use chrono::SecondsFormat;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::AppState;

#[derive(Deserialize)]
pub(crate) struct ActionReq {
    pub kind: String,
    #[serde(default)]
    pub input: Value,
    #[serde(default)]
    pub idem_key: Option<String>,
}

pub async fn actions_submit(State(state): State<AppState>, Json(req): Json<ActionReq>) -> impl IntoResponse {
    // Backpressure: deny if too many queued
    let max_q: i64 = std::env::var("ARW_ACTIONS_QUEUE_MAX").ok().and_then(|s| s.parse().ok()).unwrap_or(1024);
    if max_q > 0 {
        if let Ok(nq) = state.kernel.count_actions_by_state("queued") {
            if nq >= max_q {
                return (
                    axum::http::StatusCode::TOO_MANY_REQUESTS,
                    Json(json!({
                        "type":"about:blank","title":"Too Many Requests","status":429,
                        "detail":"queue is full","limit": max_q, "queued": nq
                    })),
                );
            }
        }
    }
    // Policy check: enforce lease rules when allow_all=false
    let decision = state.policy.lock().await.evaluate_action(&req.kind);
    if !decision.allow {
        if let Some(cap) = decision.require_capability.as_deref() {
            if state.kernel.find_valid_lease("local", cap).ok().flatten().is_none() {
                // emit policy.decision event (denied)
                state.bus.publish(
                    "policy.decision",
                    &json!({
                        "action": req.kind,
                        "allow": false,
                        "require_capability": cap,
                        "explain": decision.explain,
                    }),
                );
                return (
                    axum::http::StatusCode::FORBIDDEN,
                    Json(json!({
                        "type":"about:blank","title":"Forbidden","status":403,
                        "detail":"Denied (lease required)",
                        "explain": decision.explain
                    })),
                );
            }
        }
    }
    let id = if let Some(idem) = &req.idem_key {
        if let Ok(Some(existing)) = state.kernel.find_action_by_idem(idem) {
            existing
        } else {
            let id = uuid::Uuid::new_v4().to_string();
            let _ = state
                .kernel
                .insert_action(&id, &req.kind, &req.input, None, Some(idem.as_str()), "queued");
            id
        }
    } else {
        let id = uuid::Uuid::new_v4().to_string();
        let _ = state
            .kernel
            .insert_action(&id, &req.kind, &req.input, None, None, "queued");
        id
    };
    // Publish submitted event
    let payload = json!({"id": id, "kind": req.kind, "status": "queued"});
    let now = chrono::Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true);
    let env = arw_events::Envelope { time: now, kind: "actions.submitted".into(), payload, policy: None, ce: None };
    state.bus.publish(&env.kind, &env.payload);
    // Contribution scaffold: record a task submit (qty=1 task)
    let _ = state
        .kernel
        .append_contribution("local", "task.submit", 1.0, "task", None, None, None);
    (
        axum::http::StatusCode::ACCEPTED,
        Json(json!({"id": env.payload["id"], "ok": true})),
    )
}

pub async fn actions_get(State(state): State<AppState>, Path(id): Path<String>) -> impl IntoResponse {
    match state.kernel.get_action(&id) {
        Ok(Some(a)) => {
            (
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
        }
        Ok(None) => (
            axum::http::StatusCode::NOT_FOUND,
            Json(json!({"type":"about:blank","title":"Not Found","status":404})),
        ),
        Err(e) => (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"type":"about:blank","title":"Error","status":500, "detail": e.to_string()})),
        ),
    }
}

#[derive(Deserialize)]
pub(crate) struct ActionStateReq { pub state: String, #[serde(default)] pub error: Option<String> }
pub async fn actions_state_set(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(req): Json<ActionStateReq>,
) -> impl IntoResponse {
    let allowed = ["queued", "running", "completed", "failed"];
    if !allowed.contains(&req.state.as_str()) {
        return (
            axum::http::StatusCode::BAD_REQUEST,
            Json(json!({"type":"about:blank","title":"Bad Request","status":400, "detail":"invalid state"})),
        );
    }
    match state.kernel.set_action_state(&id, &req.state) {
        Ok(true) => {
            // Publish a transition event
            let kind = match req.state.as_str() {
                "running" => "actions.running",
                "completed" => "actions.completed",
                "failed" => "actions.failed",
                _ => "actions.updated",
            };
            let payload = json!({"id": id, "state": req.state, "error": req.error});
            let now = chrono::Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true);
            let env = arw_events::Envelope { time: now, kind: kind.into(), payload: payload.clone(), policy: None, ce: None };
            state.bus.publish(&env.kind, &env.payload);
            (
                axum::http::StatusCode::OK,
                Json(json!({"ok": true})),
            )
        }
        Ok(false) => (
            axum::http::StatusCode::NOT_FOUND,
            Json(json!({"type":"about:blank","title":"Not Found","status":404})),
        ),
        Err(e) => (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"type":"about:blank","title":"Error","status":500, "detail": e.to_string()})),
        ),
    }
}

