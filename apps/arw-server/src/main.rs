use axum::http::HeaderMap;
use sha2::Digest as _;
use std::{net::SocketAddr, time::Duration};
use tracing::{error, info};

mod access_log;
mod api;
mod app_state;
mod bootstrap;
mod capsule_guard;
mod chat;
mod cluster;
pub mod config;
mod context_loop;
mod coverage;
mod distill;
mod egress_log;
mod egress_policy;
mod egress_proxy;
mod experiments;
mod ext;
mod feedback;
mod goldens;
mod governor;
#[cfg(feature = "grpc")]
mod grpc;
mod guard_metadata;
mod http_timeout;
mod memory_hygiene;
mod memory_service;
mod metrics;
mod models;
mod openapi;
mod patch_guard;
mod read_models;
mod research_watcher;
mod responses;
mod review;
mod runtime;
mod runtime_matrix;
mod security;
mod self_model;
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

    let http_cfg = match bootstrap::http_config_from_env() {
        Ok(cfg) => cfg,
        Err(err) => {
            eprintln!("error: {err}");
            std::process::exit(2);
        }
    };

    let app = bootstrap::attach_global_layers(bootstrap::attach_http_layers(
        bootstrap::attach_stateful_layers(router, state, metrics),
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
        let policy_arc = Arc::new(Mutex::new(policy));
        let host: Arc<dyn ToolHost> = Arc::new(arw_wasi::NoopHost);
        AppState::builder(bus, kernel, policy_arc, host, true)
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
        let _state_guard = crate::util::scoped_state_dir_for_tests(temp.path());
        let state_dir = temp.path().to_path_buf();
        let mut env_guard = env::guard();
        let state = build_state(&state_dir, &mut env_guard).await;
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
        let _state_guard = crate::util::scoped_state_dir_for_tests(temp.path());
        let state_dir = temp.path().to_path_buf();
        let mut env_guard = env::guard();
        let state = build_state(&state_dir, &mut env_guard).await;

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
                    .body(Body::empty())
                    .expect("admin debug request"),
            )
            .await
            .expect("admin debug response");
        assert_eq!(admin_resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn capsule_middleware_applies_and_publishes_read_model() {
        let temp = tempdir().expect("tempdir");
        let _state_guard = crate::util::scoped_state_dir_for_tests(temp.path());
        let trust_path = temp.path().join("trust_capsules.json");
        let signing = SigningKey::from_bytes(&[7u8; 32]);
        let issuer = "test-issuer";
        write_trust_store(&trust_path, issuer, &signing);
        let mut env_guard = env::guard();
        env_guard.set("ARW_TRUST_CAPSULES", trust_path.display().to_string());
        rpu::reload_trust();

        let state_dir = temp.path().to_path_buf();
        let state = build_state(&state_dir, &mut env_guard).await;
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
        env_guard.remove("ARW_TRUST_CAPSULES");
    }

    #[tokio::test]
    async fn lease_creation_emits_event_and_updates_read_model() {
        let temp = tempdir().expect("tempdir");
        let mut env_guard = env::guard();
        let state = build_state(temp.path(), &mut env_guard).await;
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
    // Debug mode opens admin surfaces for local development convenience.
    if env_truthy("ARW_DEBUG") {
        return true;
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
        let got_hex = {
            let mut hasher = sha2::Sha256::new();
            hasher.update(ptok.as_bytes());
            let digest = hasher.finalize();
            hex::encode(digest)
        };
        return ct_eq(want.as_bytes(), got_hex.as_bytes())
            || token_plain
                .as_ref()
                .map(|p| ct_eq(p.as_bytes(), ptok.as_bytes()))
                .unwrap_or(false);
    }
    if let Some(ref p) = token_plain {
        return ct_eq(p.as_bytes(), ptok.as_bytes());
    }
    false
}

// ---------- Config Plane (moved to api_config) ----------
// moved to api_memory
// moved to api_config
