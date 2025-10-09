use axum::http::{HeaderMap, StatusCode};
use axum::response::IntoResponse;
use axum::Json;
use serde_json::json;

use crate::{admin_ok, api::http_utils, state_observer};

/// Current beliefs snapshot derived from events.
#[utoipa::path(
    get,
    path = "/state/beliefs",
    tag = "State",
    operation_id = "state_beliefs_doc",
    description = "Current beliefs snapshot derived from events.",
    responses(
        (status = 200, description = "Beliefs snapshot", body = serde_json::Value),
        (status = 401, description = "Unauthorized", body = serde_json::Value)
    )
)]
pub async fn state_beliefs(headers: HeaderMap) -> impl IntoResponse {
    if !admin_ok(&headers).await {
        return (
            StatusCode::UNAUTHORIZED,
            Json(json!({"type":"about:blank","title":"Unauthorized","status":401})),
        )
            .into_response();
    }
    let (version, items) = state_observer::beliefs_snapshot().await;
    if let Some(resp) = http_utils::state_version_not_modified(&headers, "beliefs", version) {
        return resp;
    }
    let mut response = Json(json!({"version": version, "items": items})).into_response();
    http_utils::apply_state_version_headers(response.headers_mut(), "beliefs", version);
    response
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support;
    use axum::http::{header, HeaderMap};
    use chrono::{SecondsFormat, Utc};

    #[tokio::test]
    async fn state_beliefs_honors_if_none_match() {
        let mut env_guard = test_support::env::guard();
        env_guard.set("ARW_DEBUG", "1");
        crate::state_observer::reset_for_tests().await;

        let envelope = arw_events::Envelope {
            time: Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true),
            kind: "beliefs.updated".to_string(),
            payload: json!({"claim": "alpha"}),
            policy: None,
            ce: None,
        };
        crate::state_observer::ingest_for_tests(&envelope).await;

        let first = state_beliefs(HeaderMap::new()).await.into_response();
        let etag = first
            .headers()
            .get(header::ETAG)
            .cloned()
            .expect("beliefs etag");

        let mut headers = HeaderMap::new();
        headers.insert(header::IF_NONE_MATCH, etag);
        let response = state_beliefs(headers).await.into_response();
        assert_eq!(response.status(), StatusCode::NOT_MODIFIED);
        crate::state_observer::reset_for_tests().await;
    }
}
