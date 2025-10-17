use axum::http::HeaderMap;
use axum::{
    extract::{Path, Query, State},
    response::IntoResponse,
    Json,
};
use serde::Deserialize;
use serde_json::{json, Value};
use utoipa::{IntoParams, ToSchema};

use crate::{api::state::persona::persona_disabled_response, responses, AppState};

#[derive(Debug, Default, Deserialize, ToSchema)]
#[serde(default)]
pub struct PersonaProposalBody {
    pub submitted_by: Option<String>,
    pub diff: Value,
    pub rationale: Option<String>,
    pub telemetry_scope: Option<Value>,
    pub leases_required: Option<Value>,
}

impl PersonaProposalBody {
    fn into_create(self, persona_id: String) -> arw_kernel::PersonaProposalCreate {
        arw_kernel::PersonaProposalCreate {
            persona_id,
            submitted_by: self.submitted_by.unwrap_or_else(|| "admin".to_string()),
            diff: if self.diff.is_null() {
                json!([])
            } else {
                self.diff
            },
            rationale: self.rationale,
            telemetry_scope: self.telemetry_scope.unwrap_or_else(|| json!({})),
            leases_required: self.leases_required.unwrap_or_else(|| json!([])),
        }
    }
}

#[derive(Debug, Default, Deserialize, ToSchema)]
#[serde(default)]
pub struct PersonaProposalDecisionBody {
    pub applied_by: Option<String>,
    pub diff: Option<Value>,
}

#[derive(Debug, Default, Deserialize, ToSchema)]
#[serde(default)]
pub struct PersonaFeedbackBody {
    pub signal: Option<String>,
    pub strength: Option<f32>,
    pub note: Option<String>,
    pub metadata: Option<Value>,
}

#[derive(Debug, Default, Deserialize, IntoParams)]
#[serde(default)]
pub struct PersonaFeedbackQuery {
    pub kind: Option<String>,
}

#[utoipa::path(
    post,
    path = "/admin/persona/{id}/proposals",
    tag = "Persona",
    params(("id" = String, Path, description = "Persona identifier")),
    request_body = PersonaProposalBody,
    responses(
        (status = 200, description = "Proposal created", body = serde_json::Value),
        (status = 401, description = "Unauthorized"),
        (status = 404, description = "Persona not found"),
        (status = 501, description = "Persona subsystem disabled")
    )
)]
pub async fn persona_proposal_create(
    headers: HeaderMap,
    State(state): State<AppState>,
    Path(persona_id): Path<String>,
    Json(body): Json<PersonaProposalBody>,
) -> impl IntoResponse {
    if !state.persona_enabled() {
        return persona_disabled_response();
    }

    if let Err(resp) = responses::require_admin(&headers).await {
        return *resp;
    }

    let service = match state.persona() {
        Some(service) => service,
        None => return persona_disabled_response(),
    };

    if let Err(resp) = ensure_persona_manage(&state).await {
        return resp;
    }

    match service.get_entry(persona_id.clone()).await {
        Ok(Some(_)) => {}
        Ok(None) => {
            return responses::problem_response(
                axum::http::StatusCode::NOT_FOUND,
                "Persona Not Found",
                Some("Persona id not found"),
            )
        }
        Err(err) => {
            return responses::problem_response(
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                "Failed to load persona",
                Some(&err.to_string()),
            )
        }
    }

    match service
        .create_proposal(body.into_create(persona_id.clone()))
        .await
    {
        Ok(proposal_id) => Json(json!({ "proposal_id": proposal_id })).into_response(),
        Err(err) => responses::problem_response(
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            "Failed to create persona proposal",
            Some(&err.to_string()),
        ),
    }
}

#[utoipa::path(
    post,
    path = "/admin/persona/proposals/{id}/approve",
    tag = "Persona",
    params(("id" = String, Path, description = "Proposal identifier")),
    request_body = PersonaProposalDecisionBody,
    responses(
        (status = 200, description = "Proposal approved", body = serde_json::Value),
        (status = 401, description = "Unauthorized"),
        (status = 404, description = "Proposal not found"),
        (status = 501, description = "Persona subsystem disabled")
    )
)]
pub async fn persona_proposal_approve(
    headers: HeaderMap,
    State(state): State<AppState>,
    Path(proposal_id): Path<String>,
    Json(body): Json<PersonaProposalDecisionBody>,
) -> impl IntoResponse {
    update_proposal_status(headers, state, proposal_id, "approved", Some(body)).await
}

#[utoipa::path(
    post,
    path = "/admin/persona/proposals/{id}/reject",
    tag = "Persona",
    params(("id" = String, Path, description = "Proposal identifier")),
    responses(
        (status = 200, description = "Proposal rejected", body = serde_json::Value),
        (status = 401, description = "Unauthorized"),
        (status = 404, description = "Proposal not found"),
        (status = 501, description = "Persona subsystem disabled")
    )
)]
pub async fn persona_proposal_reject(
    headers: HeaderMap,
    State(state): State<AppState>,
    Path(proposal_id): Path<String>,
) -> impl IntoResponse {
    update_proposal_status(headers, state, proposal_id, "rejected", None).await
}

async fn update_proposal_status(
    headers: HeaderMap,
    state: AppState,
    proposal_id: String,
    status: &str,
    body: Option<PersonaProposalDecisionBody>,
) -> axum::response::Response {
    if !state.persona_enabled() {
        return persona_disabled_response();
    }

    if let Err(resp) = responses::require_admin(&headers).await {
        return *resp;
    }

    if let Err(resp) = ensure_persona_manage(&state).await {
        return resp;
    }

    let service = match state.persona() {
        Some(service) => service,
        None => return persona_disabled_response(),
    };

    let existing = match service.get_proposal(proposal_id.clone()).await {
        Ok(Some(proposal)) => proposal,
        Ok(None) => {
            return responses::problem_response(
                axum::http::StatusCode::NOT_FOUND,
                "Proposal Not Found",
                Some("Proposal id not found"),
            )
        }
        Err(err) => {
            return responses::problem_response(
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                "Failed to load persona proposal",
                Some(&err.to_string()),
            )
        }
    };

    let status_update = arw_kernel::PersonaProposalStatusUpdate {
        status: status.to_string(),
    };

    let applied_by = body.as_ref().and_then(|b| b.applied_by.clone());
    let diff_override = body
        .as_ref()
        .and_then(|b| b.diff.clone())
        .filter(|diff| !diff.is_null());

    match service
        .update_proposal_status(proposal_id.clone(), status_update)
        .await
    {
        Ok(true) => {
            let applied_diff = diff_override.unwrap_or_else(|| existing.diff.clone());
            if status == "approved" {
                if let Err(err) = service
                    .apply_diff(existing.persona_id.clone(), applied_diff.clone())
                    .await
                {
                    return responses::problem_response(
                        axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                        "Failed to apply persona diff",
                        Some(&err.to_string()),
                    );
                }
            }

            if let Err(err) = service
                .append_history(arw_kernel::PersonaHistoryAppend {
                    persona_id: existing.persona_id.clone(),
                    proposal_id: Some(proposal_id.clone()),
                    diff: applied_diff.clone(),
                    applied_by,
                })
                .await
            {
                return responses::problem_response(
                    axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                    "Failed to record persona history",
                    Some(&err.to_string()),
                );
            }

            Json(json!({
                "proposal_id": proposal_id,
                "status": status,
            }))
            .into_response()
        }
        Ok(false) => responses::problem_response(
            axum::http::StatusCode::CONFLICT,
            "Proposal Update Failed",
            Some("Proposal status was not updated"),
        ),
        Err(err) => responses::problem_response(
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            "Failed to update proposal",
            Some(&err.to_string()),
        ),
    }
}

#[utoipa::path(
    post,
    path = "/persona/{id}/feedback",
    tag = "Persona",
    params(
        ("id" = String, Path, description = "Persona identifier"),
        PersonaFeedbackQuery
    ),
    request_body = PersonaFeedbackBody,
    responses(
        (status = 202, description = "Feedback accepted", body = serde_json::Value),
        (status = 404, description = "Persona not found"),
        (status = 501, description = "Persona subsystem disabled")
    )
)]
pub async fn persona_feedback_submit(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(query): Query<PersonaFeedbackQuery>,
    Json(body): Json<PersonaFeedbackBody>,
) -> impl IntoResponse {
    if !state.persona_enabled() {
        return persona_disabled_response();
    }

    let service = match state.persona() {
        Some(service) => service,
        None => return persona_disabled_response(),
    };

    match service.get_entry(id.clone()).await {
        Ok(Some(_)) => {}
        Ok(None) => {
            return responses::problem_response(
                axum::http::StatusCode::NOT_FOUND,
                "Persona Not Found",
                Some("Persona id not found"),
            )
        }
        Err(err) => {
            return responses::problem_response(
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                "Failed to load persona",
                Some(&err.to_string()),
            )
        }
    }

    let persona_id_for_payload = id.clone();
    let payload = json!({
        "persona_id": persona_id_for_payload,
        "kind": query.kind,
        "signal": body.signal,
        "strength": body.strength,
        "note": body.note,
        "metadata": body.metadata,
    });

    if let Err(err) = service.publish_feedback(state.bus(), id, payload).await {
        return responses::problem_response(
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            "Failed to publish feedback",
            Some(&err.to_string()),
        );
    }

    (
        axum::http::StatusCode::ACCEPTED,
        Json(json!({ "status": "accepted" })),
    )
        .into_response()
}

async fn ensure_persona_manage(state: &AppState) -> Result<(), axum::response::Response> {
    let snapshot = state.policy().snapshot().await;
    let allow_all = snapshot
        .get("allow_all")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let has_policy_allowances = snapshot
        .get("lease_rules")
        .and_then(|v| v.as_array())
        .map(|rules| !rules.is_empty())
        .unwrap_or(false)
        || snapshot.get("cedar").map(|v| !v.is_null()).unwrap_or(false);

    let decision = state.policy().evaluate_action("persona.manage").await;
    if decision.allow && (allow_all || has_policy_allowances) {
        return Ok(());
    }

    match state
        .kernel()
        .find_valid_lease_async("local", "persona:manage")
        .await
    {
        Ok(Some(_)) => Ok(()),
        Ok(None) => Err(responses::problem_response(
            axum::http::StatusCode::FORBIDDEN,
            "Persona Manage Forbidden",
            Some("persona:manage lease required"),
        )),
        Err(err) => Err(responses::problem_response(
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            "Persona lease lookup failed",
            Some(&err.to_string()),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        api::state::persona::{
            state_persona_get, state_persona_history, state_persona_list, PersonaHistoryQuery,
            PersonaListQuery,
        },
        test_support::begin_state_env,
    };
    use axum::{
        extract::{Query, State},
        http::{HeaderMap, StatusCode},
    };
    use chrono::{Duration, SecondsFormat, Utc};
    use http_body_util::BodyExt as _;
    use serde_json::json;
    use std::{fs, sync::Arc};
    use tempfile::tempdir;

    async fn seed_persona(state: &AppState) {
        let service = state.persona().expect("persona service");
        service
            .upsert_entry(arw_kernel::PersonaEntryUpsert {
                id: "persona-1".into(),
                owner_kind: "workspace".into(),
                owner_ref: "ws".into(),
                name: Some("Companion".into()),
                archetype: Some("ally".into()),
                traits: json!({ "tone": "neutral" }),
                preferences: json!({ "cite": true }),
                worldview: json!({ "mission": "assist" }),
                vibe_profile: json!({ "sentiment": 0.5 }),
                calibration: json!({ "confidence": 0.6 }),
            })
            .await
            .expect("seed persona");
    }

    #[tokio::test]
    async fn persona_proposal_flow_and_history() {
        crate::test_support::init_tracing();
        let temp = tempdir().expect("tempdir");
        let mut ctx = begin_state_env(temp.path());
        ctx.env.set("ARW_PERSONA_ENABLE", "1");
        ctx.env.set("ARW_DEBUG", "1");
        crate::util::reset_state_dir_for_tests();
        ctx.env
            .set("ARW_STATE_DIR", temp.path().display().to_string());

        let bus = arw_events::Bus::new_with_replay(64, 64);
        let kernel = arw_kernel::Kernel::open(temp.path()).expect("init kernel");
        let policy = arw_policy::PolicyEngine::load_from_env();
        let policy_handle = crate::policy::PolicyHandle::new(policy, bus.clone());
        let host: Arc<dyn arw_wasi::ToolHost> = Arc::new(arw_wasi::NoopHost);
        let state = crate::AppState::builder(bus, kernel, policy_handle, host, true)
            .with_sse_capacity(64)
            .with_persona_enabled(true)
            .build()
            .await;
        seed_persona(&state).await;

        // baseline list
        let list_resp =
            state_persona_list(State(state.clone()), Query(PersonaListQuery::default()))
                .await
                .into_response();
        assert_eq!(list_resp.status(), StatusCode::OK);

        // create proposal
        let proposal_resp = persona_proposal_create(
            HeaderMap::new(),
            State(state.clone()),
            Path("persona-1".to_string()),
            Json(PersonaProposalBody {
                diff: json!([
                    {"op": "replace", "path": "/name", "value": "Guide"},
                    {"op": "add", "path": "/traits/tone", "value": "warm"}
                ]),
                ..Default::default()
            }),
        )
        .await
        .into_response();
        assert_eq!(proposal_resp.status(), StatusCode::OK);
        let proposal_body = proposal_resp.into_body().collect().await.unwrap();
        let proposal_id = serde_json::from_slice::<serde_json::Value>(&proposal_body.to_bytes())
            .unwrap()["proposal_id"]
            .as_str()
            .unwrap()
            .to_string();

        // approve
        let approve_resp = persona_proposal_approve(
            HeaderMap::new(),
            State(state.clone()),
            Path(proposal_id.clone()),
            Json(PersonaProposalDecisionBody::default()),
        )
        .await
        .into_response();
        assert_eq!(approve_resp.status(), StatusCode::OK);

        // entry updated
        let entry_resp = state_persona_get(State(state.clone()), Path("persona-1".to_string()))
            .await
            .into_response();
        assert_eq!(entry_resp.status(), StatusCode::OK);
        let entry_body = entry_resp.into_body().collect().await.unwrap();
        let entry_json: serde_json::Value = serde_json::from_slice(&entry_body.to_bytes()).unwrap();
        assert_eq!(entry_json["name"], json!("Guide"));
        assert_eq!(entry_json["traits"]["tone"], json!("warm"));

        // history present
        let history_resp = state_persona_history(
            State(state.clone()),
            Path("persona-1".to_string()),
            Query(PersonaHistoryQuery::default()),
        )
        .await
        .into_response();
        assert_eq!(history_resp.status(), StatusCode::OK);
        let history_body = history_resp.into_body().collect().await.unwrap();
        let history_json: serde_json::Value =
            serde_json::from_slice(&history_body.to_bytes()).unwrap();
        assert!(history_json["items"]
            .as_array()
            .map(|arr| !arr.is_empty())
            .unwrap_or(false));
    }

    #[tokio::test]
    async fn persona_feedback_emits_event() {
        crate::test_support::init_tracing();
        let temp = tempdir().expect("tempdir");
        let mut ctx = begin_state_env(temp.path());
        ctx.env.set("ARW_PERSONA_ENABLE", "1");
        ctx.env.set("ARW_DEBUG", "1");
        crate::util::reset_state_dir_for_tests();
        ctx.env
            .set("ARW_STATE_DIR", temp.path().display().to_string());

        let bus = arw_events::Bus::new_with_replay(64, 64);
        let kernel = arw_kernel::Kernel::open(temp.path()).expect("init kernel");
        let policy = arw_policy::PolicyEngine::load_from_env();
        let policy_handle = crate::policy::PolicyHandle::new(policy, bus.clone());
        let host: Arc<dyn arw_wasi::ToolHost> = Arc::new(arw_wasi::NoopHost);
        let state = crate::AppState::builder(bus, kernel, policy_handle, host, true)
            .with_sse_capacity(64)
            .with_persona_enabled(true)
            .build()
            .await;
        seed_persona(&state).await;

        let resp = persona_feedback_submit(
            State(state.clone()),
            Path("persona-1".to_string()),
            Query(PersonaFeedbackQuery {
                kind: Some("vibe".into()),
            }),
            Json(PersonaFeedbackBody {
                signal: Some("warmer".into()),
                strength: Some(0.7),
                note: Some("felt distant".into()),
                metadata: None,
            }),
        )
        .await
        .into_response();

        assert_eq!(resp.status(), StatusCode::ACCEPTED);
    }

    #[tokio::test]
    async fn persona_proposal_requires_policy_or_lease() {
        crate::test_support::init_tracing();
        let temp = tempdir().expect("tempdir");
        let mut ctx = begin_state_env(temp.path());
        ctx.env.set("ARW_PERSONA_ENABLE", "1");
        ctx.env.set("ARW_DEBUG", "1");

        let policy_path = temp.path().join("policy.json");
        fs::write(&policy_path, r#"{ "allow_all": false }"#).expect("write policy");
        ctx.env
            .set("ARW_POLICY_FILE", policy_path.to_string_lossy().as_ref());

        crate::util::reset_state_dir_for_tests();
        ctx.env
            .set("ARW_STATE_DIR", temp.path().display().to_string());

        let bus = arw_events::Bus::new_with_replay(64, 64);
        let kernel = arw_kernel::Kernel::open(temp.path()).expect("init kernel");
        let policy = arw_policy::PolicyEngine::load_from_env();
        let policy_handle = crate::policy::PolicyHandle::new(policy, bus.clone());
        let host: Arc<dyn arw_wasi::ToolHost> = Arc::new(arw_wasi::NoopHost);
        let state = crate::AppState::builder(bus, kernel, policy_handle, host, true)
            .with_sse_capacity(64)
            .with_persona_enabled(true)
            .build()
            .await;
        seed_persona(&state).await;

        let forbidden = persona_proposal_create(
            HeaderMap::new(),
            State(state.clone()),
            Path("persona-1".to_string()),
            Json(PersonaProposalBody {
                diff: json!([{ "op": "replace", "path": "/name", "value": "Guide" }]),
                ..Default::default()
            }),
        )
        .await
        .into_response();
        assert_eq!(forbidden.status(), StatusCode::FORBIDDEN);

        let ttl = (Utc::now() + Duration::hours(1)).to_rfc3339_opts(SecondsFormat::Millis, true);
        state
            .kernel()
            .insert_lease(
                "lease-persona",
                "local",
                "persona:manage",
                None,
                &ttl,
                None,
                None,
            )
            .expect("insert lease");

        let allowed = persona_proposal_create(
            HeaderMap::new(),
            State(state.clone()),
            Path("persona-1".to_string()),
            Json(PersonaProposalBody {
                diff: json!([{ "op": "replace", "path": "/name", "value": "Guide" }]),
                ..Default::default()
            }),
        )
        .await
        .into_response();
        assert_eq!(allowed.status(), StatusCode::OK);
    }
}
