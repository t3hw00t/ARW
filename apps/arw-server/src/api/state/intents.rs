use axum::http::{HeaderMap, StatusCode};
use axum::response::IntoResponse;
use axum::Json;
use serde_json::json;

use crate::{admin_ok, api::http_utils, state_observer};

/// Recent intents stream (rolling window) with a monotonic version counter.
#[utoipa::path(
    get,
    path = "/state/intents",
    tag = "State",
    operation_id = "state_intents_doc",
    description = "Recent intents stream (rolling window) with a monotonic version counter.",
    responses(
        (status = 200, description = "Recent intents", body = serde_json::Value),
        (status = 401, description = "Unauthorized", body = serde_json::Value)
    )
)]
pub async fn state_intents(headers: HeaderMap) -> impl IntoResponse {
    if !admin_ok(&headers).await {
        return (
            StatusCode::UNAUTHORIZED,
            Json(json!({"type":"about:blank","title":"Unauthorized","status":401})),
        )
            .into_response();
    }
    let (version, items) = state_observer::intents_snapshot().await;
    if let Some(resp) = http_utils::state_version_not_modified(&headers, "intents", version) {
        return resp;
    }
    let mut response = Json(json!({"version": version, "items": items})).into_response();
    http_utils::apply_state_version_headers(response.headers_mut(), "intents", version);
    response
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::begin_state_env;
    use axum::body::Body;
    use axum::http::{header, HeaderMap};
    use chrono::{SecondsFormat, Utc};
    use http_body_util::BodyExt;
    use serde_json::{json, Value};
    async fn collect_body(body: Body) -> bytes::Bytes {
        BodyExt::collect(body).await.expect("body bytes").to_bytes()
    }

    use tempfile::tempdir;

    use crate::api::state::tests::build_state;

    #[tokio::test]
    async fn state_intents_includes_version() {
        let temp = tempdir().expect("tempdir");
        let mut ctx = begin_state_env(temp.path());
        crate::state_observer::reset_for_tests().await;

        let _state = build_state(temp.path(), &mut ctx.env).await;

        let env = arw_events::Envelope {
            time: Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true),
            kind: "intents.proposed".to_string(),
            payload: json!({"corr_id": "demo", "goal": "test"}),
            policy: None,
            ce: None,
        };

        crate::state_observer::ingest_for_tests(&env).await;

        let response = state_intents(HeaderMap::new()).await.into_response();
        let (parts, body) = response.into_parts();
        assert_eq!(parts.status, StatusCode::OK);
        assert_eq!(
            parts
                .headers
                .get(header::ETAG)
                .and_then(|v| v.to_str().ok()),
            Some("\"state-intents-v1\"")
        );
        let bytes = collect_body(body).await;
        let value: Value = serde_json::from_slice(&bytes).expect("json");
        assert_eq!(value["version"].as_u64(), Some(1));
        let items = value["items"].as_array().expect("items array");
        assert_eq!(items.len(), 1);
        assert_eq!(items[0]["kind"].as_str(), Some("intents.proposed"));

        let env2 = arw_events::Envelope {
            time: Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true),
            kind: "intents.accepted".to_string(),
            payload: json!({"corr_id": "demo", "goal": "test"}),
            policy: None,
            ce: None,
        };
        crate::state_observer::ingest_for_tests(&env2).await;

        let response = state_intents(HeaderMap::new()).await.into_response();
        let (parts, body) = response.into_parts();
        assert_eq!(parts.status, StatusCode::OK);
        assert_eq!(
            parts
                .headers
                .get(header::ETAG)
                .and_then(|v| v.to_str().ok()),
            Some("\"state-intents-v2\"")
        );
        let bytes = collect_body(body).await;
        let value: Value = serde_json::from_slice(&bytes).expect("json 2");
        assert_eq!(value["version"].as_u64(), Some(2));
        let items = value["items"].as_array().expect("items array 2");
        assert_eq!(items.len(), 2);

        crate::state_observer::reset_for_tests().await;
    }

    #[tokio::test]
    async fn state_intents_honors_if_none_match() {
        let temp = tempdir().expect("tempdir");
        let mut ctx = begin_state_env(temp.path());
        crate::state_observer::reset_for_tests().await;

        let _state = build_state(temp.path(), &mut ctx.env).await;
        let env = arw_events::Envelope {
            time: Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true),
            kind: "intents.proposed".to_string(),
            payload: json!({"corr_id": "demo", "goal": "test"}),
            policy: None,
            ce: None,
        };
        crate::state_observer::ingest_for_tests(&env).await;

        let first = state_intents(HeaderMap::new()).await.into_response();
        let etag = first.headers().get(header::ETAG).cloned().expect("etag");

        let mut headers = HeaderMap::new();
        headers.insert(header::IF_NONE_MATCH, etag.clone());
        let response = state_intents(headers).await.into_response();
        assert_eq!(response.status(), StatusCode::NOT_MODIFIED);
        crate::state_observer::reset_for_tests().await;
    }
}
