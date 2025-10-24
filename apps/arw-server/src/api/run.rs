use axum::{
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use tracing::error;
use utoipa::ToSchema;

use arw_contracts::PlanRequest;

use crate::{
    planning::PlanResponse,
    planning_executor::{PlanApplicationReport, PlanExecutor},
    responses, AppState,
};

#[derive(serde::Serialize, serde::Deserialize)]
pub struct RunResponse {
    pub plan: PlanResponse,
    pub applied: PlanApplicationReport,
}

#[allow(dead_code)]
#[derive(serde::Serialize, ToSchema)]
struct RunDocApplied {
    #[schema(value_type = Option<String>)]
    kv_policy: Option<String>,
    #[schema(value_type = [String])]
    notes: Vec<String>,
    #[schema(value_type = [String])]
    warnings: Vec<String>,
}

#[allow(dead_code)]
#[derive(serde::Serialize, ToSchema)]
struct RunDocResponse {
    #[schema(value_type = serde_json::Value)]
    plan: serde_json::Value,
    applied: RunDocApplied,
}

#[allow(dead_code)]
#[derive(serde::Serialize, ToSchema)]
struct RunDocRequest {
    #[schema(value_type = serde_json::Value)]
    policy: serde_json::Value,
    #[schema(value_type = Option<serde_json::Value>)]
    #[serde(skip_serializing_if = "Option::is_none")]
    memory: Option<serde_json::Value>,
}

#[utoipa::path(
    post,
    path = "/v1/run",
    tag = "Planning",
    request_body = RunDocRequest,
    responses(
        (status = 200, description = "Planner output applied to runtime", body = RunDocResponse),
        (status = 400, description = "Invalid run request", body = arw_protocol::ProblemDetails)
    )
)]
pub async fn run(State(state): State<AppState>, Json(request): Json<PlanRequest>) -> Response {
    match state.planner().plan(request) {
        Ok(plan_response) => {
            let applied = PlanExecutor::apply(&state, &plan_response).await;
            Json(RunResponse {
                plan: plan_response,
                applied,
            })
            .into_response()
        }
        Err(err) => {
            error!(
                target: "arw::planning",
                error = %err,
                "run request planning failed"
            );
            let detail = err.to_string();
            responses::problem_response(
                StatusCode::BAD_REQUEST,
                "Invalid Run Request",
                Some(detail.as_str()),
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{begin_state_env, build_state, contracts};
    use axum::{
        body::{to_bytes, Body},
        http::{Request, StatusCode},
        routing::post,
        Router,
    };
    use tempfile::tempdir;
    use tower::ServiceExt;

    #[tokio::test]
    async fn run_endpoint_applies_plan_and_records_metrics() {
        crate::test_support::init_tracing();
        let temp = tempdir().expect("temp dir");
        let mut ctx = begin_state_env(temp.path());
        let state = build_state(temp.path(), &mut ctx.env).await;

        let response = run(
            State(state.clone()),
            Json(contracts::sample_plan_request(true)),
        )
        .await;

        assert_eq!(response.status(), StatusCode::OK);
        let body_bytes = to_bytes(response.into_body(), 1024 * 1024)
            .await
            .expect("body");
        let run_response: RunResponse =
            serde_json::from_slice(&body_bytes).expect("deserialize run response");

        assert_eq!(
            run_response.applied.kv_policy.as_deref(),
            Some("snapkv"),
            "executor should apply snapkv policy from fixture"
        );
        assert!(
            run_response
                .applied
                .notes
                .iter()
                .any(|note| note.contains("kv cache policy")),
            "expected note about kv cache policy application"
        );
        assert!(
            run_response.plan.memory.is_some(),
            "expected memory payload to round-trip when provided"
        );

        let metrics = state.metrics().snapshot();
        assert_eq!(metrics.plan.total, 1);
        assert_eq!(
            metrics.plan.guard_failures,
            run_response.plan.plan.guard_failures.unwrap_or(0) as u64
        );
        assert_eq!(
            metrics.plan.kv_policy_counts.get("snapkv"),
            Some(&1),
            "kv policy counts should register snapkv"
        );
    }

    #[tokio::test]
    async fn run_route_via_router_handles_request() {
        crate::test_support::init_tracing();
        let temp = tempdir().expect("temp dir");
        let mut ctx = begin_state_env(temp.path());
        let state = build_state(temp.path(), &mut ctx.env).await;

        let router = Router::new()
            .route("/v1/run", post(super::run))
            .with_state(state.clone());

        let request = Request::post("/v1/run")
            .header(axum::http::header::CONTENT_TYPE, "application/json")
            .body(Body::from(
                serde_json::to_vec(&contracts::sample_plan_request(true))
                    .expect("serialize plan request"),
            ))
            .expect("request");

        let response = router.oneshot(request).await.expect("router response");
        assert_eq!(response.status(), StatusCode::OK);

        let body_bytes = to_bytes(response.into_body(), 1024 * 1024)
            .await
            .expect("body bytes");
        let run_response: RunResponse =
            serde_json::from_slice(&body_bytes).expect("deserialize run response");
        assert_eq!(
            run_response.applied.kv_policy.as_deref(),
            Some("snapkv"),
            "router path should still apply kv policy"
        );

        let summary = state.metrics().snapshot();
        assert_eq!(summary.plan.total, 1);
    }
}
