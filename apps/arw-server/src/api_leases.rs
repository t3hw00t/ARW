use axum::http::HeaderMap;
use axum::response::IntoResponse;
use axum::{extract::State, Json};
use serde::Deserialize;
use serde_json::json;
use utoipa::ToSchema;

use crate::{admin_ok, AppState};

#[derive(Deserialize, ToSchema)]
pub(crate) struct LeaseReq {
    pub capability: String,
    #[serde(default)]
    pub scope: Option<String>,
    #[serde(default)]
    pub ttl_secs: Option<u64>,
    #[serde(default)]
    pub budget: Option<f64>,
}

/// Allocate a capability lease (admin-only when `ARW_ADMIN_TOKEN` is configured).
#[utoipa::path(
    post,
    path = "/leases",
    tag = "Leases",
    request_body = LeaseReq,
    responses(
        (status = 201, description = "Created", body = serde_json::Value),
        (status = 401, description = "Unauthorized")
    )
)]
pub async fn leases_create(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<LeaseReq>,
) -> impl IntoResponse {
    if !admin_ok(&headers) {
        return (
            axum::http::StatusCode::UNAUTHORIZED,
            Json(json!({
                "type": "about:blank",
                "title": "Unauthorized",
                "status": 401
            })),
        );
    }
    let ttl = req.ttl_secs.unwrap_or(3600);
    let until = chrono::Utc::now() + chrono::Duration::seconds(ttl as i64);
    let ttl_until = until.to_rfc3339();
    let id = uuid::Uuid::new_v4().to_string();
    if let Err(e) = state.kernel.insert_lease(
        &id,
        "local",
        &req.capability,
        req.scope.as_deref(),
        &ttl_until,
        req.budget,
        None,
    ) {
        return (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            Json(
                json!({"type":"about:blank","title":"Error","status":500, "detail": e.to_string()}),
            ),
        );
    }
    (
        axum::http::StatusCode::CREATED,
        Json(json!({"id": id, "ttl_until": ttl_until})),
    )
}

/// Snapshot of active leases.
#[utoipa::path(get, path = "/state/leases", tag = "Leases", responses((status = 200, body = serde_json::Value)))]
pub async fn state_leases(State(state): State<AppState>) -> impl IntoResponse {
    let items = state.kernel.list_leases(200).unwrap_or_default();
    Json(json!({"items": items}))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::{HeaderMap, HeaderValue, StatusCode};
    use axum::response::IntoResponse;
    use http_body_util::BodyExt;
    use once_cell::sync::Lazy;
    use serde_json::Value;
    use std::collections::VecDeque;
    use std::sync::{Arc, Mutex as StdMutex, MutexGuard};
    use tokio::sync::Mutex;
    use uuid::Uuid;

    static ENV_LOCK: Lazy<StdMutex<()>> = Lazy::new(|| StdMutex::new(()));

    struct AdminTokenGuard {
        _guard: MutexGuard<'static, ()>,
    }

    impl AdminTokenGuard {
        fn with_token(token: &str) -> Self {
            let guard = ENV_LOCK.lock().unwrap();
            std::env::set_var("ARW_ADMIN_TOKEN", token);
            Self { _guard: guard }
        }
    }

    impl Drop for AdminTokenGuard {
        fn drop(&mut self) {
            std::env::remove_var("ARW_ADMIN_TOKEN");
        }
    }

    fn test_state() -> AppState {
        let tmp = std::env::temp_dir().join(format!("arw-server-test-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&tmp).unwrap();
        let kernel = arw_kernel::Kernel::open(&tmp).unwrap();
        let host: Arc<dyn arw_wasi::ToolHost> = Arc::new(arw_wasi::NoopHost::default());
        AppState {
            bus: arw_events::Bus::new(16),
            kernel,
            policy: Arc::new(Mutex::new(arw_policy::PolicyEngine::load_from_env())),
            host,
            config_state: Arc::new(Mutex::new(serde_json::json!({}))),
            config_history: Arc::new(Mutex::new(Vec::new())),
            sse_id_map: Arc::new(Mutex::new(VecDeque::new())),
            endpoints: Arc::new(Vec::new()),
            endpoints_meta: Arc::new(Vec::new()),
            metrics: Arc::new(crate::metrics::Metrics::new()),
        }
    }

    #[tokio::test]
    async fn rejects_missing_admin_token() {
        let _guard = AdminTokenGuard::with_token("sekret");
        let state = test_state();
        let response = leases_create(
            State(state.clone()),
            HeaderMap::new(),
            Json(LeaseReq {
                capability: "net:http".into(),
                scope: None,
                ttl_secs: Some(60),
                budget: None,
            }),
        )
        .await
        .into_response();

        let status = response.status();
        let body = response.into_body().collect().await.unwrap().to_bytes();
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        let json: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["title"], "Unauthorized");

        let leases = state.kernel.list_leases(10).unwrap();
        assert!(leases.is_empty());
    }

    #[tokio::test]
    async fn allows_authorized_admin_token() {
        let _guard = AdminTokenGuard::with_token("sekret");
        let state = test_state();
        let mut headers = HeaderMap::new();
        headers.insert(
            axum::http::header::AUTHORIZATION,
            HeaderValue::from_static("Bearer sekret"),
        );
        let response = leases_create(
            State(state.clone()),
            headers,
            Json(LeaseReq {
                capability: "net:http".into(),
                scope: Some("scope1".into()),
                ttl_secs: Some(120),
                budget: Some(5.0),
            }),
        )
        .await
        .into_response();

        let status = response.status();
        let body = response.into_body().collect().await.unwrap().to_bytes();
        assert_eq!(status, StatusCode::CREATED);
        let json: Value = serde_json::from_slice(&body).unwrap();
        assert!(json["id"].as_str().is_some());
        assert!(json["ttl_until"].as_str().is_some());

        let leases = state.kernel.list_leases(10).unwrap();
        assert_eq!(leases.len(), 1);
        assert_eq!(leases[0]["capability"], "net:http");
        assert_eq!(leases[0]["scope"], serde_json::json!("scope1"));
    }
}
