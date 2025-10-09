use axum::{
    extract::{Query, State},
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    Json,
};
use serde::Deserialize;
use serde_json::json;
use utoipa::{IntoParams, ToSchema};

use super::numeric_version_from_field;
use crate::{admin_ok, api::http_utils, identity::IdentitySnapshot, tools::guardrails_metrics_value, world, AppState};

/// Guardrails circuit-breaker metrics snapshot.
#[utoipa::path(
    get,
    path = "/state/guardrails_metrics",
    tag = "State",
    responses(
        (status = 200, description = "Guardrails metrics", body = serde_json::Value),
        (status = 401, description = "Unauthorized", body = serde_json::Value)
    )
)]
pub async fn state_guardrails_metrics(headers: HeaderMap) -> impl IntoResponse {
    if !admin_ok(&headers).await {
        return (
            StatusCode::UNAUTHORIZED,
            Json(json!({"type":"about:blank","title":"Unauthorized","status":401})),
        )
            .into_response();
    }
    Json(guardrails_metrics_value()).into_response()
}

/// Active policy capsules snapshot.
#[utoipa::path(
    get,
    path = "/state/policy/capsules",
    tag = "Policy",
    responses((status = 200, description = "Active capsules", body = serde_json::Value))
)]
pub async fn state_policy_capsules(State(state): State<AppState>) -> impl IntoResponse {
    Json(state.capsules().snapshot().await)
}

/// Identity registry snapshot.
#[utoipa::path(
    get,
    path = "/state/identity",
    tag = "State",
    operation_id = "state_identity",
    responses(
        (status = 200, description = "Identity registry snapshot", body = IdentitySnapshot),
        (status = 401, description = "Unauthorized", body = arw_protocol::ProblemDetails)
    )
)]
pub async fn state_identity(
    headers: HeaderMap,
    State(state): State<AppState>,
) -> impl IntoResponse {
    if !admin_ok(&headers).await {
        return crate::responses::unauthorized(None);
    }
    let snapshot = state.identity().snapshot().await;
    Json(snapshot).into_response()
}

#[derive(Debug, Default, Deserialize, ToSchema)]
#[serde(default)]
pub struct WorldQuery {
    pub proj: Option<String>,
}

/// Project world model snapshot (belief graph view).
#[utoipa::path(
    get,
    path = "/state/world",
    tag = "State",
    operation_id = "state_world_doc",
    description = "Project world model snapshot (belief graph view).",
    params(("proj" = Option<String>, Query, description = "Project id")),
    responses(
        (status = 200, description = "World model", body = serde_json::Value),
        (status = 401, description = "Unauthorized", body = serde_json::Value)
    )
)]
pub async fn state_world(headers: HeaderMap, Query(q): Query<WorldQuery>) -> impl IntoResponse {
    if !admin_ok(&headers).await {
        return (
            StatusCode::UNAUTHORIZED,
            Json(json!({"type":"about:blank","title":"Unauthorized","status":401})),
        )
            .into_response();
    }
    let map = world::snapshot_project_map(q.proj.as_deref()).await;
    let version = map.version;
    if let Some(resp) = http_utils::state_version_not_modified(&headers, "world", version) {
        return resp;
    }
    let body = serde_json::to_value(map).unwrap_or_else(|_| json!({}));
    let mut response = Json(body).into_response();
    http_utils::apply_state_version_headers(response.headers_mut(), "world", version);
    response
}

#[derive(Debug, Default, Deserialize, IntoParams, ToSchema)]
#[serde(default)]
pub struct WorldSelectQuery {
    pub proj: Option<String>,
    pub q: Option<String>,
    pub k: Option<usize>,
    pub lambda: Option<f64>,
}

/// Select top-k claims for a query.
#[utoipa::path(
    get,
    path = "/state/world/select",
    tag = "State",
    operation_id = "state_world_select_doc",
    description = "Select top-k claims for a query.",
    params(
        ("proj" = Option<String>, Query, description = "Project id"),
        ("q" = Option<String>, Query, description = "Query string"),
        ("k" = Option<usize>, Query, description = "Top K"),
        ("lambda" = Option<f64>, Query, description = "Diversity weight (0-1)")
    ),
    responses(
        (status = 200, description = "Selected claims", body = serde_json::Value),
        (status = 401, description = "Unauthorized", body = serde_json::Value)
    )
)]
pub async fn state_world_select(
    headers: HeaderMap,
    Query(q): Query<WorldSelectQuery>,
) -> impl IntoResponse {
    if !admin_ok(&headers).await {
        return (
            StatusCode::UNAUTHORIZED,
            Json(json!({"type":"about:blank","title":"Unauthorized","status":401})),
        )
            .into_response();
    }
    let query = q.q.unwrap_or_default();
    let k = q.k.unwrap_or(8);
    let lambda = q.lambda.unwrap_or(0.5);
    let items = world::select_top_claims_diverse(q.proj.as_deref(), &query, k, lambda).await;
    Json(json!({"items": items})).into_response()
}

/// Kernel contributions snapshot.
#[utoipa::path(
    get,
    path = "/state/contributions",
    tag = "State",
    responses(
        (status = 200, description = "Contributions list", body = serde_json::Value),
        (status = 501, description = "Kernel disabled", body = serde_json::Value)
    )
)]
pub async fn state_contributions(
    headers: HeaderMap,
    State(state): State<AppState>,
) -> impl IntoResponse {
    if !admin_ok(&headers).await {
        return (
            StatusCode::UNAUTHORIZED,
            Json(json!({"type":"about:blank","title":"Unauthorized","status":401})),
        )
            .into_response();
    }
    if !state.kernel_enabled() {
        return crate::responses::kernel_disabled();
    }
    let items = state
        .kernel()
        .list_contributions_async(200)
        .await
        .unwrap_or_default();
    let version = numeric_version_from_field(&items, "id");
    if let Some(resp) =
        http_utils::state_version_not_modified(&headers, "contributions", version)
    {
        return resp;
    }
    let mut response = Json(json!({"version": version, "items": items})).into_response();
    http_utils::apply_state_version_headers(response.headers_mut(), "contributions", version);
    response
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        api::state::tests::build_state,
        capsule_guard::CapsuleSnapshot,
        test_support::{begin_state_env, env::guard},
    };
    use arw_events;
    use arw_topics;
    use axum::extract::Query;
    use axum::http::{header, HeaderMap};
    use chrono::{SecondsFormat, Utc};
    use serde_json::json;
    use tempfile::tempdir;

    #[tokio::test]
    async fn state_guardrails_metrics_requires_admin() {
        let response = state_guardrails_metrics(HeaderMap::new()).await.into_response();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn state_policy_capsules_returns_snapshot() {
        let temp = tempdir().expect("tempdir");
        let mut ctx = begin_state_env(temp.path());
        let state = build_state(temp.path(), &mut ctx.env).await;
        state.capsules().install_for_tests(vec![CapsuleSnapshot {
            id: "demo".into(),
            status: "active".into(),
            summary: Some("Demo capsule".into()),
            ..Default::default()
        }]);

        let response = state_policy_capsules(State(state)).await.into_response();
        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn state_identity_requires_admin() {
        let temp = tempdir().expect("tempdir");
        let mut ctx = begin_state_env(temp.path());
        let state = build_state(temp.path(), &mut ctx.env).await;
        let response = state_identity(HeaderMap::new(), State(state))
            .await
            .into_response();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn state_world_honors_if_none_match() {
        let mut env_guard = guard();
        env_guard.set("ARW_DEBUG", "1");
        crate::world::reset_for_tests().await;

        let env = arw_events::Envelope {
            time: Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true),
            kind: arw_topics::TOPIC_PROJECTS_CREATED.to_string(),
            payload: json!({"name": "demo"}),
            policy: None,
            ce: None,
        };
        crate::world::ingest_for_tests(&env).await;

        let first = state_world(HeaderMap::new(), Query(WorldQuery { proj: None }))
            .await
            .into_response();
        let etag = first.headers().get(header::ETAG).cloned().expect("world etag");

        let mut headers = HeaderMap::new();
        headers.insert(header::IF_NONE_MATCH, etag);
        let response = state_world(headers, Query(WorldQuery { proj: None }))
            .await
            .into_response();
        assert_eq!(response.status(), StatusCode::NOT_MODIFIED);

        crate::world::reset_for_tests().await;
    }

    #[tokio::test]
    async fn state_world_select_filters_claims() {
        let mut env_guard = guard();
        env_guard.set("ARW_DEBUG", "1");
        let query_params = WorldSelectQuery {
            proj: None,
            q: Some("demo query".into()),
            k: Some(3),
            lambda: Some(0.4),
        };
        let response = state_world_select(HeaderMap::new(), Query(query_params))
            .await
            .into_response();
        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn state_contributions_honors_if_none_match() {
        let temp = tempdir().expect("tempdir");
        let mut ctx = begin_state_env(temp.path());
        let state = build_state(temp.path(), &mut ctx.env).await;

        let response = state_contributions(HeaderMap::new(), State(state.clone()))
            .await
            .into_response();
        let etag = response.headers().get(header::ETAG).cloned().expect("etag");

        let mut headers = HeaderMap::new();
        headers.insert(header::IF_NONE_MATCH, etag);
        let response = state_contributions(headers, State(state)).await.into_response();
        assert_eq!(response.status(), StatusCode::NOT_MODIFIED);
    }
}
