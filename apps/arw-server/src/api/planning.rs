use axum::{
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use tracing::error;

use arw_contracts::PlanRequest;
use serde::Serialize;
use utoipa::ToSchema;

use crate::{planning_executor::PlanExecutor, responses, AppState};

#[allow(dead_code)]
#[derive(Serialize, ToSchema)]
struct PlanDocRequest {
    #[schema(value_type = serde_json::Value)]
    policy: serde_json::Value,
    #[schema(value_type = Option<serde_json::Value>)]
    #[serde(skip_serializing_if = "Option::is_none")]
    memory: Option<serde_json::Value>,
}

#[allow(dead_code)]
#[derive(Serialize, ToSchema)]
struct PlanDocResponse {
    #[schema(value_type = serde_json::Value)]
    plan: serde_json::Value,
    #[schema(value_type = serde_json::Value)]
    policy: serde_json::Value,
    #[schema(value_type = Option<serde_json::Value>)]
    #[serde(skip_serializing_if = "Option::is_none")]
    memory: Option<serde_json::Value>,
}

#[utoipa::path(
    post,
    path = "/v1/plan",
    tag = "Planning",
    request_body = PlanDocRequest,
    responses(
        (status = 200, description = "Planner output", body = PlanDocResponse),
        (status = 400, description = "Invalid plan request", body = arw_protocol::ProblemDetails)
    )
)]
pub async fn plan(State(state): State<AppState>, Json(request): Json<PlanRequest>) -> Response {
    match state.planner().plan(request) {
        Ok(plan_response) => {
            PlanExecutor::record_plan_metrics(&state, &plan_response);
            Json(plan_response).into_response()
        }
        Err(err) => {
            error!(
                target: "arw::planning",
                error = %err,
                "plan request rejected"
            );
            let detail = err.to_string();
            responses::problem_response(
                StatusCode::BAD_REQUEST,
                "Invalid Plan Request",
                Some(detail.as_str()),
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{begin_state_env, build_state, contracts};
    use arw_contracts::CompressionMode;
    use axum::{
        body::{to_bytes, Body},
        http::{Request, StatusCode},
        routing::post,
        Router,
    };
    use tempfile::tempdir;
    use tower::ServiceExt;

    #[tokio::test]
    async fn plan_endpoint_returns_plan_and_records_metrics() {
        crate::test_support::init_tracing();
        let temp = tempdir().expect("temp dir");
        let mut ctx = begin_state_env(temp.path());
        let state = build_state(temp.path(), &mut ctx.env).await;

        let response = plan(
            State(state.clone()),
            Json(contracts::sample_plan_request(false)),
        )
        .await;

        assert_eq!(response.status(), StatusCode::OK);
        let body_bytes = to_bytes(response.into_body(), 1024 * 1024)
            .await
            .expect("body");
        let plan_response: crate::planning::PlanResponse =
            serde_json::from_slice(&body_bytes).expect("deserialize plan response");

        assert_eq!(plan_response.plan.target_tokens, 1024);
        assert!(
            plan_response
                .plan
                .applied_modes
                .contains(&CompressionMode::Transclude),
            "expected transclude mode"
        );
        assert_eq!(
            plan_response.policy.persona.id, "persona:test-fixture",
            "fixture persona should flow through plan response"
        );

        let summary = state.metrics().snapshot();
        assert_eq!(summary.plan.total, 1);
        assert_eq!(summary.plan.last_engine.as_deref(), Some("llama.cpp"));
        assert_eq!(
            summary.plan.mode_counts.get("transclude"),
            Some(&1),
            "mode counts should include transclude"
        );
    }

    #[tokio::test]
    async fn plan_endpoint_rejects_invalid_request() {
        let temp = tempdir().expect("temp dir");
        let mut ctx = begin_state_env(temp.path());
        let state = build_state(temp.path(), &mut ctx.env).await;

        let mut request = contracts::sample_plan_request(false);
        request.policy.compression.target_tokens = 0;

        let response = plan(State(state), Json(request)).await;
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn plan_route_via_router_handles_request() {
        crate::test_support::init_tracing();
        let temp = tempdir().expect("temp dir");
        let mut ctx = begin_state_env(temp.path());
        let state = build_state(temp.path(), &mut ctx.env).await;

        let router = Router::new()
            .route("/v1/plan", post(super::plan))
            .with_state(state.clone());

        let request = Request::post("/v1/plan")
            .header(axum::http::header::CONTENT_TYPE, "application/json")
            .body(Body::from(
                serde_json::to_vec(&contracts::sample_plan_request(false))
                    .expect("serialize plan request"),
            ))
            .expect("request build");

        let response = router.oneshot(request).await.expect("router response");
        assert_eq!(response.status(), StatusCode::OK);

        let body_bytes = to_bytes(response.into_body(), 1024 * 1024)
            .await
            .expect("body bytes");
        let plan_response: crate::planning::PlanResponse =
            serde_json::from_slice(&body_bytes).expect("deserialize plan response");
        assert_eq!(plan_response.policy.persona.id, "persona:test-fixture");

        let metrics = state.metrics().snapshot();
        assert_eq!(metrics.plan.total, 1);
    }
}
