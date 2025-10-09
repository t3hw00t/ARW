use axum::{extract::State, http::HeaderMap, response::IntoResponse, Json};
use serde_json::json;

use crate::{api::http_utils, AppState};

/// Background tasks status snapshot.
#[utoipa::path(
    get,
    path = "/state/tasks",
    tag = "State",
    responses((status = 200, description = "Background tasks", body = serde_json::Value))
)]
pub async fn state_tasks(headers: HeaderMap, State(state): State<AppState>) -> impl IntoResponse {
    let (version, tasks) = state.metrics().tasks_snapshot_with_version();
    if let Some(resp) = http_utils::state_version_not_modified(&headers, "tasks", version) {
        return resp;
    }
    let mut response = Json(json!({ "version": version, "tasks": tasks })).into_response();
    http_utils::apply_state_version_headers(response.headers_mut(), "tasks", version);
    response
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{api::state::tests::build_state, test_support::begin_state_env};
    use axum::http::header;
    use axum::http::HeaderMap;
    use tempfile::tempdir;

    #[tokio::test]
    async fn state_tasks_honors_if_none_match() {
        let temp = tempdir().expect("tempdir");
        let mut ctx = begin_state_env(temp.path());
        let state = build_state(temp.path(), &mut ctx.env).await;

        let metrics = state.metrics();
        metrics.task_started("demo");
        metrics.task_completed("demo");

        let first = state_tasks(HeaderMap::new(), State(state.clone()))
            .await
            .into_response();
        let etag = first
            .headers()
            .get(header::ETAG)
            .cloned()
            .expect("tasks etag");

        let mut headers = HeaderMap::new();
        headers.insert(header::IF_NONE_MATCH, etag);
        let response = state_tasks(headers, State(state)).await.into_response();
        assert_eq!(response.status(), StatusCode::NOT_MODIFIED);
    }
}
