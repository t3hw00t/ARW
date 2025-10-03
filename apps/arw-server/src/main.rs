use axum::http::HeaderMap;
use sha2::Digest as _;
use std::{net::SocketAddr, time::Duration};
use tracing::{error, info, warn};

mod access_log;
mod api;
mod app_state;
mod autonomy;
mod bootstrap;
mod capsule_guard;
mod chat;
mod cluster;
pub mod config;
mod config_watcher;
mod context_loop;
mod context_metrics;
mod coverage;
mod crashguard;
mod distill;
mod egress_log;
mod egress_policy;
mod egress_proxy;
mod experiments;
mod feedback;
mod goldens;
mod governor;
#[cfg(feature = "grpc")]
mod grpc;
mod guard_metadata;
mod http_client;
mod http_timeout;
mod memory_hygiene;
mod memory_service;
mod metrics;
mod models;
mod openapi;
mod patch_guard;
mod policy;
mod project_snapshots;
mod queue;
mod read_models;
mod research_watcher;
mod responses;
mod review;
mod runtime;
mod runtime_matrix;
mod security;
mod self_model;
mod singleflight;
mod sse_cache;
mod staging;
mod state_observer;
mod tasks;
#[cfg(test)]
mod test_support;
mod tool_cache;
mod tools;
mod training;
mod util;
mod worker;
mod working_set;
mod world;

mod router;

pub(crate) use app_state::AppState;

#[tokio::main]
async fn main() {
    // Crash guard: capture panics and write markers for recovery.
    crashguard::install();
    match bootstrap::ensure_openapi_export() {
        Ok(Some(_)) => return,
        Ok(None) => {}
        Err(err) => {
            eprintln!("error: failed to write generated OPENAPI_OUT: {err}");
            std::process::exit(2);
        }
    }

    arw_otel::init();
    // Apply performance presets early so env-based tunables pick up sensible defaults.
    // Explicit env vars still take precedence over these seeded values.
    let _tier = arw_core::perf::apply_performance_preset();
    http_timeout::init_from_env();
    let bootstrap::BootstrapOutput {
        router,
        state,
        metrics,
        background_tasks,
    } = bootstrap::build().await;

    // Announce service start for observability.
    state.bus().publish(
        arw_topics::TOPIC_SERVICE_START,
        &serde_json::json!({
            "ts_ms": arw_core::rpu::trust_last_reload_ms(),
            "version": env!("CARGO_PKG_VERSION"),
        }),
    );

    let http_cfg = match bootstrap::http_config_from_env() {
        Ok(cfg) => cfg,
        Err(err) => {
            eprintln!("error: {err}");
            std::process::exit(2);
        }
    };

    let app = bootstrap::attach_global_layers(bootstrap::attach_http_layers(
        bootstrap::attach_stateful_layers(router, state.clone(), metrics),
        http_cfg.concurrency_limit,
    ));

    let listener = tokio::net::TcpListener::bind(http_cfg.addr)
        .await
        .expect("bind server socket");

    let server = axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .with_graceful_shutdown(shutdown_signal());

    if let Err(err) = server.await {
        error!("http server exited with error: {err}");
    }

    info!("shutting down background tasks");
    // Announce service stop for observability.
    state.bus().publish(
        arw_topics::TOPIC_SERVICE_STOP,
        &serde_json::json!({
            "ts_ms": arw_core::rpu::trust_last_reload_ms(),
            "reason": "shutdown",
        }),
    );
    background_tasks
        .shutdown_with_grace(Duration::from_secs(5))
        .await;
}

async fn shutdown_signal() {
    info!("shutdown signal listener active");
    #[cfg(unix)]
    {
        use tokio::signal::unix::{signal, SignalKind};

        let mut term = signal(SignalKind::terminate()).expect("install SIGTERM handler");
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {},
            _ = term.recv() => {},
        }
    }

    #[cfg(not(unix))]
    {
        let _ = tokio::signal::ctrl_c().await;
    }

    info!("shutdown signal received");
}

#[cfg(test)]
#[allow(clippy::items_after_test_module)]
mod http_tests {
    use super::*;
    use crate::{
        router::{self, paths},
        test_support::env,
    };
    use arw_core::rpu;
    use arw_policy::PolicyEngine;
    use arw_protocol::GatingCapsule;
    use arw_topics::{self as topics, TOPIC_POLICY_CAPSULE_APPLIED, TOPIC_READMODEL_PATCH};
    use arw_wasi::ToolHost;
    use axum::{
        body::Body,
        http::{Request, StatusCode},
        middleware,
        routing::{get, post},
        Router,
    };
    use base64::{engine::general_purpose::STANDARD as BASE64_STD, Engine};
    use ed25519_dalek::{Signer, SigningKey};
    use http_body_util::BodyExt;
    use serde_json::{json, Value};
    use std::{collections::HashMap, fs, path::Path, sync::Arc, time::Duration};
    use tempfile::tempdir;
    use tokio::{sync::Mutex, time::timeout};
    use tower::util::ServiceExt;

    async fn build_state(dir: &Path, env_guard: &mut env::EnvGuard) -> AppState {
        env_guard.set("ARW_DEBUG", "1");
        crate::util::reset_state_dir_for_tests();
        env_guard.set("ARW_STATE_DIR", dir.display().to_string());
        let bus = arw_events::Bus::new_with_replay(64, 64);
        let kernel = arw_kernel::Kernel::open(dir).expect("init kernel for tests");
        let policy = PolicyEngine::load_from_env();
        let policy_handle = crate::policy::PolicyHandle::new(policy, bus.clone());
        let host: Arc<dyn ToolHost> = Arc::new(arw_wasi::NoopHost);
        AppState::builder(bus, kernel, policy_handle, host, true)
            .with_config_state(Arc::new(Mutex::new(json!({"mode": "test"}))))
            .with_config_history(Arc::new(Mutex::new(Vec::new())))
            .with_sse_capacity(64)
            .build()
            .await
    }

    fn router_with_actions(state: AppState) -> Router {
        Router::new()
            .route(paths::ACTIONS, post(api::actions::actions_submit))
            .route(paths::ACTIONS_ID, get(api::actions::actions_get))
            .with_state(state)
    }

    fn router_with_capsule(state: AppState) -> Router {
        let capsule_state = state.clone();
        Router::new()
            .route("/admin/ping", get(|| async { StatusCode::OK }))
            .layer(middleware::from_fn(move |req, next| {
                let st = capsule_state.clone();
                async move { capsule_guard::capsule_mw(st, req, next).await }
            }))
            .with_state(state)
    }

    fn write_trust_store(path: &Path, issuer: &str, signing: &SigningKey) {
        let trust = json!({
            "issuers": [
                {
                    "id": issuer,
                    "alg": "ed25519",
                    "key_b64": BASE64_STD.encode(signing.verifying_key().to_bytes()),
                }
            ]
        });
        fs::write(path, trust.to_string()).expect("write trust store");
    }

    fn signed_capsule(signing: &SigningKey, issuer: &str, id: &str) -> GatingCapsule {
        let issued_at_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;
        let mut capsule = GatingCapsule {
            id: id.to_string(),
            version: "1".into(),
            issued_at_ms,
            issuer: Some(issuer.to_string()),
            hop_ttl: Some(3),
            propagate: Some("none".into()),
            denies: vec![],
            contracts: vec![],
            lease_duration_ms: Some(10_000),
            renew_within_ms: Some(5_000),
            signature: None,
        };
        let mut unsigned = capsule.clone();
        unsigned.signature = None;
        let bytes = serde_json::to_vec(&unsigned).expect("serialize capsule");
        let sig = signing.sign(&bytes);
        capsule.signature = Some(BASE64_STD.encode(sig.to_bytes()));
        capsule
    }

    #[tokio::test]
    async fn http_action_roundtrip_completes() {
        let temp = tempdir().expect("tempdir");
        let mut ctx = crate::test_support::begin_state_env(temp.path());
        let state_dir = temp.path().to_path_buf();
        let state = build_state(&state_dir, &mut ctx.env).await;
        let _worker = worker::start_local_worker(state.clone());
        let app = router_with_actions(state);

        let submit_body = json!({
            "kind": "demo.echo",
            "input": { "msg": "hello-roundtrip" }
        });
        let submit_req = Request::builder()
            .method("POST")
            .uri(paths::ACTIONS)
            .header(axum::http::header::CONTENT_TYPE, "application/json")
            .body(Body::from(submit_body.to_string()))
            .expect("submit request");
        let submit_resp = app
            .clone()
            .oneshot(submit_req)
            .await
            .expect("submit response");
        assert_eq!(submit_resp.status(), StatusCode::ACCEPTED);
        let submit_bytes = submit_resp
            .into_body()
            .collect()
            .await
            .expect("submit body collect")
            .to_bytes();
        let submit_json: Value = serde_json::from_slice(&submit_bytes).expect("submit body json");
        let action_id = submit_json["id"].as_str().expect("action id").to_string();

        let mut completed: Option<Value> = None;
        for _ in 0..30 {
            let get_req = Request::builder()
                .method("GET")
                .uri(format!("{}/{}", paths::ACTIONS, action_id))
                .body(Body::empty())
                .expect("get request");
            let get_resp = app.clone().oneshot(get_req).await.expect("get response");
            assert_eq!(get_resp.status(), StatusCode::OK);
            let body_bytes = get_resp
                .into_body()
                .collect()
                .await
                .expect("get body collect")
                .to_bytes();
            let payload: Value = serde_json::from_slice(&body_bytes).expect("get body json");
            if payload["state"] == "completed" {
                completed = Some(payload);
                break;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }

        let payload = completed.expect("action completed");
        assert_eq!(payload["state"], "completed");
        assert_eq!(payload["output"]["echo"]["msg"], json!("hello-roundtrip"));
    }

    #[tokio::test]
    async fn debug_alias_returns_not_found() {
        let temp = tempdir().expect("tempdir");
        let mut ctx = crate::test_support::begin_state_env(temp.path());
        crate::security::reset_admin_rate_limiter_for_tests();
        ctx.env.set("ARW_ADMIN_TOKEN", "secret123");
        ctx.env.set("ARW_DEBUG", "1");
        let state_dir = temp.path().to_path_buf();
        let state = build_state(&state_dir, &mut ctx.env).await;

        let (router, _, _) = router::build_router();
        let app = router.with_state(state);

        let legacy_resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/debug")
                    .body(Body::empty())
                    .expect("legacy request"),
            )
            .await
            .expect("legacy response");
        assert_eq!(legacy_resp.status(), StatusCode::NOT_FOUND);

        let admin_resp = app
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri(paths::ADMIN_DEBUG)
                    .header("X-ARW-Admin", "secret123")
                    .body(Body::empty())
                    .expect("admin debug request"),
            )
            .await
            .expect("admin debug response");
        let status = admin_resp.status();
        if status != StatusCode::OK {
            let body = admin_resp
                .into_body()
                .collect()
                .await
                .expect("admin debug body")
                .to_bytes();
            panic!(
                "expected 200 OK, got {} with body {}",
                status,
                String::from_utf8_lossy(&body)
            );
        }
    }

    #[tokio::test]
    async fn healthz_envelope_wraps_when_enabled() {
        let temp = tempdir().expect("tempdir");
        let mut ctx = crate::test_support::begin_state_env(temp.path());
        ctx.env.set("ARW_API_ENVELOPE", "1");

        let state_dir = temp.path().to_path_buf();
        let state = build_state(&state_dir, &mut ctx.env).await;
        let (router, _, _) = router::build_router();
        let metrics = Arc::new(crate::metrics::Metrics::default());
        let app = crate::bootstrap::attach_stateful_layers(router, state, metrics);

        let response = app
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri(paths::HEALTHZ)
                    .body(Body::empty())
                    .expect("healthz request"),
            )
            .await
            .expect("healthz response");

        assert_eq!(response.status(), StatusCode::OK);
        let body_bytes = response
            .into_body()
            .collect()
            .await
            .expect("collect body")
            .to_bytes();
        let value: Value = serde_json::from_slice(&body_bytes).expect("json envelope");
        assert_eq!(value.get("ok"), Some(&Value::Bool(true)));
        let data = value
            .get("data")
            .and_then(|d| d.as_object())
            .expect("data object");
        assert_eq!(data.get("ok"), Some(&Value::Bool(true)));
    }

    #[tokio::test]
    async fn healthz_envelope_skips_when_requested() {
        use crate::responses::{HEADER_ENVELOPE_APPLIED, HEADER_ENVELOPE_BYPASS};

        let temp = tempdir().expect("tempdir");
        let mut ctx = crate::test_support::begin_state_env(temp.path());
        ctx.env.set("ARW_API_ENVELOPE", "1");

        let state_dir = temp.path().to_path_buf();
        let state = build_state(&state_dir, &mut ctx.env).await;
        let (router, _, _) = router::build_router();
        let metrics = Arc::new(crate::metrics::Metrics::default());
        let app = crate::bootstrap::attach_stateful_layers(router, state, metrics);

        let response = app
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri(paths::HEALTHZ)
                    .header(crate::responses::HEADER_ENVELOPE_REQUEST, "0")
                    .body(Body::empty())
                    .expect("healthz request"),
            )
            .await
            .expect("healthz response");

        assert_eq!(response.status(), StatusCode::OK);
        assert!(response.headers().get(HEADER_ENVELOPE_APPLIED).is_none());
        assert_eq!(
            response
                .headers()
                .get(HEADER_ENVELOPE_BYPASS)
                .and_then(|v| v.to_str().ok()),
            Some("1")
        );
        let body_bytes = response
            .into_body()
            .collect()
            .await
            .expect("collect body")
            .to_bytes();
        let value: Value = serde_json::from_slice(&body_bytes).expect("json raw");
        assert_eq!(value.get("ok"), Some(&Value::Bool(true)));
        assert!(value.get("data").is_none());
    }

    #[tokio::test]
    async fn healthz_envelope_forced_when_requested() {
        use crate::responses::HEADER_ENVELOPE_APPLIED;

        let temp = tempdir().expect("tempdir");
        let mut ctx = crate::test_support::begin_state_env(temp.path());
        ctx.env.remove("ARW_API_ENVELOPE");

        let state_dir = temp.path().to_path_buf();
        let state = build_state(&state_dir, &mut ctx.env).await;
        let (router, _, _) = router::build_router();
        let metrics = Arc::new(crate::metrics::Metrics::default());
        let app = crate::bootstrap::attach_stateful_layers(router, state, metrics);

        let response = app
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri(format!("{}?arw-envelope=1", paths::HEALTHZ))
                    .body(Body::empty())
                    .expect("healthz request"),
            )
            .await
            .expect("healthz response");

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response
                .headers()
                .get(HEADER_ENVELOPE_APPLIED)
                .and_then(|v| v.to_str().ok()),
            Some("1")
        );
        let body_bytes = response
            .into_body()
            .collect()
            .await
            .expect("collect body")
            .to_bytes();
        let value: Value = serde_json::from_slice(&body_bytes).expect("json envelope");
        assert_eq!(value.get("ok"), Some(&Value::Bool(true)));
        assert!(value.get("data").is_some());
    }

    #[tokio::test]
    async fn capsule_middleware_applies_and_publishes_read_model() {
        let temp = tempdir().expect("tempdir");
        // Initialize tracing for easier debugging when running this test solo.
        #[cfg(test)]
        crate::test_support::init_tracing();
        let trust_path = temp.path().join("trust_capsules.json");
        let signing = SigningKey::from_bytes(&[7u8; 32]);
        let issuer = "test-issuer";
        write_trust_store(&trust_path, issuer, &signing);
        let mut ctx = crate::test_support::begin_state_env(temp.path());
        ctx.env
            .set("ARW_TRUST_CAPSULES", trust_path.display().to_string());
        rpu::reload_trust();

        let state_dir = temp.path().to_path_buf();
        let state = build_state(&state_dir, &mut ctx.env).await;
        let bus = state.bus();
        let mut rx = bus.subscribe_filtered(
            vec![
                TOPIC_POLICY_CAPSULE_APPLIED.to_string(),
                TOPIC_READMODEL_PATCH.to_string(),
            ],
            Some(16),
        );

        let router = router_with_capsule(state.clone());
        let capsule = signed_capsule(&signing, issuer, "capsule-http");
        let capsule_json = serde_json::to_string(&capsule).expect("capsule json");

        let response = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/admin/ping")
                    .header("X-ARW-Capsule", capsule_json)
                    .body(Body::empty())
                    .expect("capsule request"),
            )
            .await
            .expect("capsule response");
        assert_eq!(response.status(), StatusCode::OK);

        let mut events: HashMap<String, serde_json::Value> = HashMap::new();
        while events.len() < 2 {
            let env = timeout(Duration::from_secs(1), rx.recv())
                .await
                .expect("bus event")
                .expect("bus not closed");
            events.insert(env.kind.clone(), env.payload);
        }

        let applied = events
            .remove(TOPIC_POLICY_CAPSULE_APPLIED)
            .expect("applied event");
        assert_eq!(applied["id"].as_str(), Some("capsule-http"));
        assert_eq!(applied["issuer"].as_str(), Some(issuer));

        let patch = events
            .remove(TOPIC_READMODEL_PATCH)
            .expect("read model patch");
        assert_eq!(patch["id"].as_str(), Some("policy_capsules"));
        let patch_items = patch["patch"].as_array().expect("patch array");
        assert!(!patch_items.is_empty(), "patch should include diff");

        let snapshot = state.capsules().snapshot().await;
        let items = snapshot["items"].as_array().expect("capsule items");
        assert_eq!(items.len(), 1);
        assert_eq!(items[0]["id"].as_str(), Some("capsule-http"));
        ctx.env.remove("ARW_TRUST_CAPSULES");
    }

    #[tokio::test]
    async fn lease_creation_emits_event_and_updates_read_model() {
        let temp = tempdir().expect("tempdir");
        let mut ctx = crate::test_support::begin_state_env(temp.path());
        let state = build_state(temp.path(), &mut ctx.env).await;
        let bus = state.bus();
        let mut rx = bus.subscribe_filtered(
            vec![
                topics::TOPIC_LEASES_CREATED.to_string(),
                topics::TOPIC_READMODEL_PATCH.to_string(),
            ],
            Some(16),
        );
        let app = Router::new()
            .route(paths::LEASES, post(api::leases::leases_create))
            .route(paths::STATE_LEASES, get(api::leases::state_leases))
            .with_state(state.clone());

        let req_body = json!({"capability": "net:http", "ttl_secs": 15});
        let request = Request::builder()
            .method("POST")
            .uri(paths::LEASES)
            .header(axum::http::header::CONTENT_TYPE, "application/json")
            .body(Body::from(req_body.to_string()))
            .expect("lease request");
        let response = app.clone().oneshot(request).await.expect("lease resp");
        assert_eq!(response.status(), StatusCode::CREATED);
        let body_bytes = response
            .into_body()
            .collect()
            .await
            .expect("lease body")
            .to_bytes();
        let payload: Value = serde_json::from_slice(&body_bytes).expect("lease json");
        let lease_id = payload["id"].as_str().expect("lease id");

        let mut saw_created = false;
        let mut saw_patch = false;
        for _ in 0..2 {
            let env = timeout(Duration::from_secs(1), rx.recv())
                .await
                .expect("bus timeout")
                .expect("bus closed");
            match env.kind.as_str() {
                topics::TOPIC_LEASES_CREATED => {
                    saw_created = true;
                    assert_eq!(env.payload["id"].as_str(), Some(lease_id));
                    assert_eq!(env.payload["capability"].as_str(), Some("net:http"));
                }
                topics::TOPIC_READMODEL_PATCH => {
                    if env.payload["id"].as_str() == Some("policy_leases") {
                        saw_patch = true;
                    }
                }
                _ => {}
            }
        }
        assert!(saw_created, "expected leases.created event");
        assert!(saw_patch, "expected policy_leases patch");

        let state_resp = app
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri(paths::STATE_LEASES)
                    .body(Body::empty())
                    .expect("leases state request"),
            )
            .await
            .expect("state leases resp");
        assert_eq!(state_resp.status(), StatusCode::OK);
        let state_body = state_resp
            .into_body()
            .collect()
            .await
            .expect("state body")
            .to_bytes();
        let state_json: Value = serde_json::from_slice(&state_body).expect("state json");
        assert_eq!(state_json["count"].as_u64(), Some(1));
        assert_eq!(state_json["items"].as_array().map(|v| v.len()), Some(1));
    }

    #[tokio::test]
    async fn admin_debug_denies_remote_even_in_debug_mode() {
        use axum::extract::ConnectInfo;
        use axum::http::{Request, StatusCode};
        use axum::{routing::get, Router};
        use std::net::SocketAddr;
        use tower::util::ServiceExt;

        // Build a minimal router with client-addr middleware so admin_ok sees the caller IP.
        let app = Router::new()
            .route(paths::ADMIN_DEBUG, get(crate::api::ui::debug_ui))
            .layer(axum::middleware::from_fn(crate::security::client_addr_mw));

        // Enable debug but simulate a remote caller via X-Forwarded-For.
        let mut env = crate::test_support::env::guard();
        env.set("ARW_DEBUG", "1");
        let mut req = Request::builder()
            .method("GET")
            .uri(paths::ADMIN_DEBUG)
            .header("x-forwarded-for", "8.8.8.8")
            .body(Body::empty())
            .expect("request");
        req.extensions_mut()
            .insert(ConnectInfo(SocketAddr::from(([203, 0, 113, 10], 4242))));
        let resp = app.clone().oneshot(req).await.expect("response");
        // Expect Unauthorized without an admin token when not loopback.
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);

        // Cleanup
        env.remove("ARW_DEBUG");
    }

    #[tokio::test]
    async fn admin_debug_allows_loopback_in_debug_mode() {
        use axum::extract::ConnectInfo;
        use axum::http::{Request, StatusCode};
        use axum::{routing::get, Router};
        use std::net::SocketAddr;
        use tower::util::ServiceExt;

        let app = Router::new()
            .route(paths::ADMIN_DEBUG, get(crate::api::ui::debug_ui))
            .layer(axum::middleware::from_fn(crate::security::client_addr_mw));

        let mut env = crate::test_support::env::guard();
        env.set("ARW_DEBUG", "1");
        let mut req = Request::builder()
            .method("GET")
            .uri(paths::ADMIN_DEBUG)
            .header("x-forwarded-for", "127.0.0.1")
            .body(Body::empty())
            .expect("request");
        req.extensions_mut()
            .insert(ConnectInfo(SocketAddr::from(([127, 0, 0, 1], 9000))));
        let resp = app.clone().oneshot(req).await.expect("response");
        assert_eq!(resp.status(), StatusCode::OK);
        env.remove("ARW_DEBUG");
    }

    #[tokio::test]
    async fn admin_debug_denies_forwarded_remote_when_trusted() {
        use axum::extract::ConnectInfo;
        use axum::http::{Request, StatusCode};
        use axum::{routing::get, Router};
        use std::net::SocketAddr;
        use tower::util::ServiceExt;

        let app = Router::new()
            .route(paths::ADMIN_DEBUG, get(crate::api::ui::debug_ui))
            .layer(axum::middleware::from_fn(crate::security::client_addr_mw));

        let mut env = crate::test_support::env::guard();
        env.set("ARW_DEBUG", "1");
        env.set("ARW_TRUST_FORWARD_HEADERS", "1");

        let mut req = Request::builder()
            .method("GET")
            .uri(paths::ADMIN_DEBUG)
            .header("x-forwarded-for", "198.51.100.25")
            .body(Body::empty())
            .expect("request");
        // Actual socket is loopback (typical reverse proxy scenario), but forwarded header carries remote IP.
        req.extensions_mut()
            .insert(ConnectInfo(SocketAddr::from(([127, 0, 0, 1], 7000))));

        let resp = app.clone().oneshot(req).await.expect("response");
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);

        env.remove("ARW_TRUST_FORWARD_HEADERS");
        env.remove("ARW_DEBUG");
    }

    #[tokio::test]
    async fn admin_ui_assets_require_auth() {
        use axum::extract::ConnectInfo;
        use axum::http::{Request, StatusCode};
        use axum::{routing::get, Router};
        use std::net::SocketAddr;
        use tower::util::ServiceExt;

        let app = Router::new()
            .route(
                "/admin/ui/assets/models.js",
                get(crate::api::ui::ui_models_js),
            )
            .layer(axum::middleware::from_fn(crate::security::client_addr_mw));

        let mut env = crate::test_support::env::guard();
        env.set("ARW_DEBUG", "0");
        let mut req = Request::builder()
            .method("GET")
            .uri("/admin/ui/assets/models.js")
            .header("x-forwarded-for", "127.0.0.1")
            .body(Body::empty())
            .expect("request");
        req.extensions_mut()
            .insert(ConnectInfo(SocketAddr::from(([127, 0, 0, 1], 9100))));
        let resp = app.clone().oneshot(req).await.expect("response");
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
        env.remove("ARW_DEBUG");
    }
}

fn env_truthy(key: &str) -> bool {
    std::env::var(key)
        .ok()
        .map(|v| {
            let trimmed = v.trim();
            matches!(
                trimmed.to_ascii_lowercase().as_str(),
                "1" | "true" | "yes" | "on" | "debug"
            )
        })
        .unwrap_or(false)
}

pub(crate) fn admin_ok(headers: &HeaderMap) -> bool {
    let addrs = crate::security::client_addrs();
    // Debug mode opens admin surfaces for local development convenience,
    // but only for local callers. In unit tests or routers without the
    // client-addr middleware, we may not have an address; allow in that
    // case to preserve test ergonomics.
    if env_truthy("ARW_DEBUG") {
        tracing::trace!(
            target: "arw::security",
            remote = addrs.remote(),
            forwarded = addrs.forwarded(),
            forwarded_trusted = addrs.forwarded_trusted(),
            "admin_ok debug gate"
        );
        if addrs.remote().is_none() {
            // Middleware not installed (tests or minimal routers). Allow.
            return true;
        }
        if addrs.remote_is_loopback() {
            if !addrs.forwarded_trusted() {
                if !addrs.forwarded().is_some() || addrs.forwarded_is_loopback() {
                    return true;
                }
            } else if addrs.forwarded_is_loopback() {
                return true;
            }
        }
        // Debug mode is on but the request appears to originate from a remote caller.
    }

    // When ARW_ADMIN_TOKEN or ARW_ADMIN_TOKEN_SHA256 is set, require it in Authorization: Bearer or X-ARW-Admin
    let token_plain = std::env::var("ARW_ADMIN_TOKEN")
        .ok()
        .filter(|t| !t.is_empty());
    let token_hash = std::env::var("ARW_ADMIN_TOKEN_SHA256")
        .ok()
        .filter(|t| !t.is_empty());
    if token_plain.is_none() && token_hash.is_none() {
        return false;
    }
    // Extract presented token
    let mut presented: Option<String> = None;
    if let Some(hv) = headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|h| h.to_str().ok())
    {
        if let Some(bearer) = hv.strip_prefix("Bearer ") {
            presented = Some(bearer.to_string());
        }
    }
    if presented.is_none() {
        if let Some(hv) = headers.get("X-ARW-Admin").and_then(|h| h.to_str().ok()) {
            presented = Some(hv.to_string());
        }
    }
    let Some(ptok) = presented else { return false };
    let fingerprint = {
        let mut hasher = sha2::Sha256::new();
        hasher.update(ptok.as_bytes());
        hex::encode(hasher.finalize())
    };
    if !crate::security::admin_rate_limit_allow(&fingerprint, &addrs) {
        warn!(
            target: "arw::security",
            remote = addrs.remote().unwrap_or("unknown"),
            forwarded = addrs.forwarded().unwrap_or("none"),
            "admin auth rate limit exceeded"
        );
        return false;
    }
    // Constant-time eq helper
    fn ct_eq(a: &[u8], b: &[u8]) -> bool {
        if a.len() != b.len() {
            return false;
        }
        let mut diff: u8 = 0;
        for i in 0..a.len() {
            diff |= a[i] ^ b[i];
        }
        diff == 0
    }
    if let Some(ref hpref) = token_hash {
        let want = hpref.trim().to_ascii_lowercase();
        if ct_eq(want.as_bytes(), fingerprint.as_bytes()) {
            return true;
        }
        return token_plain
            .as_ref()
            .map(|p| ct_eq(p.as_bytes(), ptok.as_bytes()))
            .unwrap_or(false);
    }
    if let Some(ref p) = token_plain {
        return ct_eq(p.as_bytes(), ptok.as_bytes());
    }
    false
}

// Shared test guard to serialize env/rate-limiter tests across the binary.
#[cfg(test)]
pub(crate) static ADMIN_ENV_GUARD: once_cell::sync::Lazy<std::sync::Mutex<()>> =
    once_cell::sync::Lazy::new(|| std::sync::Mutex::new(()));

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::{HeaderMap, HeaderValue};
    use sha2::Digest;

    fn auth_headers(token: &str) -> HeaderMap {
        let mut headers = HeaderMap::new();
        let value = format!("Bearer {}", token);
        headers.insert(
            axum::http::header::AUTHORIZATION,
            HeaderValue::from_str(&value).expect("auth header"),
        );
        headers
    }

    #[test]
    fn admin_ok_rate_limits_plain_token() {
        let _lock = super::ADMIN_ENV_GUARD.lock().unwrap();
        crate::security::reset_admin_rate_limiter_for_tests();
        let mut env = crate::test_support::env::guard();
        env.set("ARW_DEBUG", "0");
        env.set("ARW_ADMIN_TOKEN", "secret");
        env.remove("ARW_ADMIN_TOKEN_SHA256");
        env.set("ARW_ADMIN_RATE_LIMIT", "2");
        env.set("ARW_ADMIN_RATE_WINDOW_SECS", "3600");

        let headers = auth_headers("secret");
        assert!(admin_ok(&headers));
        assert!(admin_ok(&headers));
        assert!(!admin_ok(&headers));

        crate::security::reset_admin_rate_limiter_for_tests();
    }

    #[test]
    fn admin_ok_rate_limits_hashed_token() {
        let _lock = super::ADMIN_ENV_GUARD.lock().unwrap();
        crate::security::reset_admin_rate_limiter_for_tests();
        let mut env = crate::test_support::env::guard();
        env.set("ARW_DEBUG", "0");
        env.remove("ARW_ADMIN_TOKEN");
        let plain = "topsecret";
        let digest = {
            let mut h = sha2::Sha256::new();
            h.update(plain.as_bytes());
            hex::encode(h.finalize())
        };
        env.set("ARW_ADMIN_TOKEN_SHA256", digest);
        env.set("ARW_ADMIN_RATE_LIMIT", "1");
        env.set("ARW_ADMIN_RATE_WINDOW_SECS", "300");

        let headers = auth_headers(plain);
        assert!(admin_ok(&headers));
        assert!(!admin_ok(&headers));

        crate::security::reset_admin_rate_limiter_for_tests();
    }
}

#[cfg(test)]
mod prop_tests {
    use super::*;
    use axum::http::{HeaderMap, HeaderValue};
    use proptest::prelude::*;

    fn auth_headers(token: &str) -> HeaderMap {
        let mut headers = HeaderMap::new();
        let value = format!("Bearer {}", token);
        headers.insert(
            axum::http::header::AUTHORIZATION,
            HeaderValue::from_str(&value).expect("auth header"),
        );
        headers
    }

    proptest! {
        #[test]
        fn hashed_token_allows_once_denies_second(ref token in proptest::string::string_regex("[-._~A-Za-z0-9]{1,64}").unwrap()) {
            let _lock = super::ADMIN_ENV_GUARD.lock().unwrap();
            crate::security::reset_admin_rate_limiter_for_tests();
            let mut env = crate::test_support::env::guard();
            env.set("ARW_DEBUG", "0");
            env.remove("ARW_ADMIN_TOKEN");
            // Compute SHA256 of the random token
            let mut h = sha2::Sha256::new();
            h.update(token.as_bytes());
            let digest = hex::encode(h.finalize());
            env.set("ARW_ADMIN_TOKEN_SHA256", &digest);
            env.set("ARW_ADMIN_RATE_LIMIT", "1");
            env.set("ARW_ADMIN_RATE_WINDOW_SECS", "60");

            let headers = auth_headers(token);
            prop_assert!(admin_ok(&headers));
            prop_assert!(!admin_ok(&headers));

            crate::security::reset_admin_rate_limiter_for_tests();
        }
    }
}

// ---------- Config Plane (moved to api_config) ----------
// moved to api_memory
// moved to api_config
