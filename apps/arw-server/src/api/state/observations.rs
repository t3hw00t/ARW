use axum::{
    extract::Query,
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    Json,
};
use chrono::{DateTime, Utc};
use serde::Deserialize;
use serde_json::json;
use utoipa::{IntoParams, ToSchema};

use crate::{admin_ok, api::http_utils, state_observer};

#[derive(Debug, Default, Deserialize, ToSchema, IntoParams)]
#[serde(default)]
pub struct StateObservationsQuery {
    /// Limit the number of items returned (most recent first); defaults to all retained observations.
    pub limit: Option<usize>,
    /// Restrict results to event kinds matching this prefix (e.g. `actions.`).
    pub kind_prefix: Option<String>,
    /// Only include observations emitted after this RFC3339 timestamp.
    pub since: Option<String>,
}

/// Recent observations from the event bus.
#[utoipa::path(
    get,
    path = "/state/observations",
    tag = "State",
    operation_id = "state_observations_doc",
    description = "Recent observations from the event bus.",
    params(StateObservationsQuery),
    responses(
        (status = 200, description = "Recent observations", body = serde_json::Value),
        (status = 401, description = "Unauthorized", body = serde_json::Value)
    )
)]
pub async fn state_observations(
    headers: HeaderMap,
    Query(params): Query<StateObservationsQuery>,
) -> impl IntoResponse {
    if !admin_ok(&headers).await {
        return (
            StatusCode::UNAUTHORIZED,
            Json(json!({"type":"about:blank","title":"Unauthorized","status":401})),
        )
            .into_response();
    }
    let StateObservationsQuery {
        limit,
        kind_prefix,
        since,
    } = params;
    let kind_prefix_ref = kind_prefix.as_deref();
    let since_filter: Option<DateTime<Utc>> = match since {
        Some(raw) => match DateTime::parse_from_rfc3339(raw.trim()) {
            Ok(dt) => Some(dt.with_timezone(&Utc)),
            Err(_) => {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(json!({
                        "type": "about:blank",
                        "title": "Invalid `since` value",
                        "detail": "`since` must be an RFC3339 timestamp (e.g., 2025-10-02T17:15:00Z)",
                        "status": 400
                    })),
                )
                    .into_response();
            }
        },
        None => None,
    };
    let (version, items) =
        state_observer::observations_snapshot(limit, kind_prefix_ref, since_filter).await;
    if let Some(resp) = http_utils::state_version_not_modified(&headers, "observations", version) {
        return resp;
    }
    let mut response = Json(json!({"version": version, "items": items})).into_response();
    http_utils::apply_state_version_headers(response.headers_mut(), "observations", version);
    response
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support;
    use axum::body::Body;
    use axum::extract::Query;
    use axum::http::{header, HeaderMap, StatusCode};
    use chrono::{Duration, SecondsFormat, Utc};
    use http_body_util::BodyExt;
    use serde_json::{json, Value};

    async fn collect_body(body: Body) -> bytes::Bytes {
        BodyExt::collect(body).await.expect("body bytes").to_bytes()
    }

    #[tokio::test]
    async fn state_observations_honors_if_none_match() {
        let mut env_guard = test_support::env::guard();
        env_guard.set("ARW_DEBUG", "1");
        crate::state_observer::reset_for_tests().await;

        let envelope = arw_events::Envelope {
            time: Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true),
            kind: "obs.debug".to_string(),
            payload: json!({"message": "hello"}),
            policy: None,
            ce: None,
        };
        crate::state_observer::ingest_for_tests(&envelope).await;

        let first = state_observations(HeaderMap::new(), Query(StateObservationsQuery::default()))
            .await
            .into_response();
        let etag = first
            .headers()
            .get(header::ETAG)
            .cloned()
            .expect("observations etag");

        let mut headers = HeaderMap::new();
        headers.insert(header::IF_NONE_MATCH, etag);
        let response = state_observations(headers, Query(StateObservationsQuery::default()))
            .await
            .into_response();
        assert_eq!(response.status(), StatusCode::NOT_MODIFIED);
        crate::state_observer::reset_for_tests().await;
    }

    #[tokio::test]
    async fn state_observations_supports_limit_and_prefix() {
        let mut env_guard = test_support::env::guard();
        env_guard.set("ARW_DEBUG", "1");
        crate::state_observer::reset_for_tests().await;

        let envs = [
            arw_events::Envelope {
                time: Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true),
                kind: "obs.one".to_string(),
                payload: json!({"seq": 1}),
                policy: None,
                ce: None,
            },
            arw_events::Envelope {
                time: Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true),
                kind: "intents.proposed".to_string(),
                payload: json!({"seq": 2}),
                policy: None,
                ce: None,
            },
            arw_events::Envelope {
                time: Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true),
                kind: "obs.two".to_string(),
                payload: json!({"seq": 3}),
                policy: None,
                ce: None,
            },
        ];

        for env in &envs {
            crate::state_observer::ingest_for_tests(env).await;
        }

        let params = StateObservationsQuery {
            limit: Some(1),
            kind_prefix: None,
            since: None,
        };
        let response = state_observations(HeaderMap::new(), Query(params))
            .await
            .into_response();
        let (parts, body) = response.into_parts();
        assert_eq!(parts.status, StatusCode::OK);
        let bytes = collect_body(body).await;
        let value: Value = serde_json::from_slice(&bytes).expect("json");
        assert_eq!(value["items"].as_array().map(|a| a.len()), Some(1));
        assert_eq!(value["items"][0]["payload"]["seq"].as_i64(), Some(3));

        let params = StateObservationsQuery {
            limit: None,
            kind_prefix: Some("obs.".to_string()),
            since: None,
        };
        let response = state_observations(HeaderMap::new(), Query(params))
            .await
            .into_response();
        let (parts, body) = response.into_parts();
        assert_eq!(parts.status, StatusCode::OK);
        let bytes = collect_body(body).await;
        let value: Value = serde_json::from_slice(&bytes).expect("json");
        assert_eq!(value["items"].as_array().map(|a| a.len()), Some(2));
        assert_eq!(value["items"][0]["payload"]["seq"].as_i64(), Some(1));
        assert_eq!(value["items"][1]["payload"]["seq"].as_i64(), Some(3));

        crate::state_observer::reset_for_tests().await;
    }

    #[tokio::test]
    async fn state_observations_supports_since_filter() {
        let mut env_guard = test_support::env::guard();
        env_guard.set("ARW_DEBUG", "1");
        crate::state_observer::reset_for_tests().await;

        let older = Utc::now() - Duration::seconds(60);
        let newer = Utc::now();
        let envs = [
            arw_events::Envelope {
                time: older.to_rfc3339_opts(SecondsFormat::Millis, true),
                kind: "obs.old".to_string(),
                payload: json!({"seq": 1}),
                policy: None,
                ce: None,
            },
            arw_events::Envelope {
                time: newer.to_rfc3339_opts(SecondsFormat::Millis, true),
                kind: "obs.new".to_string(),
                payload: json!({"seq": 2}),
                policy: None,
                ce: None,
            },
        ];

        for env in &envs {
            crate::state_observer::ingest_for_tests(env).await;
        }

        let threshold = older + Duration::seconds(1);
        let params = StateObservationsQuery {
            limit: None,
            kind_prefix: None,
            since: Some(threshold.to_rfc3339_opts(SecondsFormat::Millis, true)),
        };
        let response = state_observations(HeaderMap::new(), Query(params))
            .await
            .into_response();
        let (parts, body) = response.into_parts();
        assert_eq!(parts.status, StatusCode::OK);
        let bytes = collect_body(body).await;
        let value: Value = serde_json::from_slice(&bytes).expect("json");
        let items = value["items"].as_array().cloned().unwrap_or_default();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0]["kind"].as_str(), Some("obs.new"));

        crate::state_observer::reset_for_tests().await;
    }

    #[tokio::test]
    async fn state_observations_rejects_invalid_since() {
        let mut env_guard = test_support::env::guard();
        env_guard.set("ARW_DEBUG", "1");
        crate::state_observer::reset_for_tests().await;

        let params = StateObservationsQuery {
            limit: None,
            kind_prefix: None,
            since: Some("not-a-timestamp".to_string()),
        };
        let response = state_observations(HeaderMap::new(), Query(params))
            .await
            .into_response();
        let (parts, body) = response.into_parts();
        assert_eq!(parts.status, StatusCode::BAD_REQUEST);
        let bytes = collect_body(body).await;
        let value: Value = serde_json::from_slice(&bytes).expect("json");
        assert_eq!(value["title"].as_str(), Some("Invalid `since` value"));

        crate::state_observer::reset_for_tests().await;
    }
}
