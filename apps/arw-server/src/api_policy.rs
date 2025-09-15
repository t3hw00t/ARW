use axum::http::HeaderMap;
use axum::response::IntoResponse;
use axum::{extract::State, Json};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::AppState;
use arw_policy::{AbacRequest, Entity, PolicyEngine};

pub async fn state_policy(State(state): State<AppState>) -> impl axum::response::IntoResponse {
    Json(state.policy.lock().await.snapshot())
}

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
    let newp = PolicyEngine::load_from_env();
    {
        let mut pol = state.policy.lock().await;
        *pol = newp.clone();
    }
    state
        .bus
        .publish("policy.reloaded", &json!(newp.snapshot()));
    (
        axum::http::StatusCode::OK,
        Json(json!({"ok": true, "policy": newp.snapshot()})),
    )
        .into_response()
}

#[derive(Deserialize)]
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
    let d = state.policy.lock().await.evaluate_abac(&AbacRequest {
        action,
        subject: subj,
        resource: res,
    });
    Json(d)
}
