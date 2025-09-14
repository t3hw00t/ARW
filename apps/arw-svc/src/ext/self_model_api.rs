use crate::AppState;
use axum::{extract::State, response::IntoResponse, Json};
use serde::Deserialize;
use serde_json::{json, Value};

#[utoipa::path(get, path = "/state/self", tag = "Public/State", responses(
    (status=200, description="Self-models list")
))]
pub async fn self_state_list() -> impl IntoResponse {
    let items: Vec<Value> = crate::ext::self_model::list()
        .await
        .into_iter()
        .map(|(id, v)| json!({"agent": id, "model": v}))
        .collect();
    super::ok(json!({"items": items}))
}

#[utoipa::path(get, path = "/state/self/{agent}", tag = "Public/State", params(("agent" = String, Path, description = "Agent id")), responses(
    (status=200, description="Self-model for agent")
))]
pub async fn self_state_get(
    axum::extract::Path(agent): axum::extract::Path<String>,
) -> impl IntoResponse {
    let v = crate::ext::self_model::load(&agent)
        .await
        .unwrap_or_else(|| json!({}));
    super::ok(v)
}

#[derive(Deserialize, utoipa::ToSchema)]
pub struct ProposeReq {
    pub agent: String,
    #[serde(default)]
    pub patch: Value,
    #[serde(default)]
    pub rationale: Option<String>,
}

#[arw_macros::arw_admin(
    method = "POST",
    path = "/admin/self_model/propose",
    summary = "Propose a self-model update"
)]
pub async fn self_model_propose(
    State(state): State<AppState>,
    Json(req): Json<ProposeReq>,
) -> impl IntoResponse {
    let Ok(env) = crate::ext::self_model::propose_update(
        &req.agent,
        req.patch.clone(),
        req.rationale.clone(),
    )
    .await
    else {
        return super::ApiError::bad_request("invalid proposal").into_response();
    };
    let mut payload = json!({
        "agent": req.agent,
        "proposal_id": env.get("id").and_then(|s| s.as_str()).unwrap_or_default(),
        "touches_policies": false,
        "widens_scope": false,
        "rationale": req.rationale,
    });
    super::corr::ensure_corr(&mut payload);
    state
        .bus
        .publish(crate::ext::topics::TOPIC_SELFMODEL_PROPOSED, &payload);
    super::ok(env).into_response()
}

#[derive(Deserialize, utoipa::ToSchema)]
pub struct ApplyReq {
    pub proposal_id: String,
}

#[arw_macros::arw_admin(
    method = "POST",
    path = "/admin/self_model/apply",
    summary = "Apply a self-model proposal"
)]
pub async fn self_model_apply(
    State(state): State<AppState>,
    Json(req): Json<ApplyReq>,
) -> impl IntoResponse {
    match crate::ext::self_model::apply_proposal(&req.proposal_id).await {
        Ok(res) => {
            let mut payload = json!({
                "proposal_id": req.proposal_id,
                "agent": res.get("agent").and_then(|s| s.as_str()).unwrap_or_default(),
            });
            super::corr::ensure_corr(&mut payload);
            state
                .bus
                .publish(crate::ext::topics::TOPIC_SELFMODEL_UPDATED, &payload);
            super::ok(res).into_response()
        }
        Err(e) => super::ApiError::bad_request(&e).into_response(),
    }
}
