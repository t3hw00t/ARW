use arw_policy::PolicyEngine;
use arw_wasi::ToolHost;
use axum::http::HeaderMap;
use chrono::Utc;
use serde_json::json;
use std::net::SocketAddr;
use tower::limit::ConcurrencyLimitLayer;
use tower_http::{compression::CompressionLayer, trace::TraceLayer};
// jsonschema moved to modules
use sha2::Digest as _;
use tokio::sync::Mutex;
use utoipa::OpenApi;

mod access_log;
mod api;
mod app_state;
mod capsule_guard;
mod chat;
mod cluster;
pub mod config;
mod context_loop;
mod coverage;
mod distill;
mod egress_policy;
mod egress_proxy;
mod experiments;
mod feedback;
mod goldens;
mod governor;
#[cfg(feature = "grpc")]
mod grpc;
mod http_timeout;
mod metrics;
mod models;
mod openapi;
mod patch_guard;
mod read_models;
mod research_watcher;
mod responses;
mod review;
mod runtime_matrix;
mod security;
mod self_model;
mod sse_cache;
mod staging;
mod state_observer;
mod tool_cache;
mod tools;
mod training;
mod util;
mod worker;
mod working_set;
mod world;

mod router;

pub(crate) use app_state::AppState;
pub(crate) use router::build_router;

#[tokio::main]
async fn main() {
    // OpenAPI/spec export mode for CI/docs sync (no server startup).
    if let Ok(path) = std::env::var("OPENAPI_OUT") {
        if let Some(parent) = std::path::Path::new(&path).parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let yaml = crate::openapi::ApiDoc::openapi()
            .to_yaml()
            .unwrap_or_else(|_| "openapi: 3.0.3".into());
        if let Err(e) = std::fs::write(&path, yaml) {
            eprintln!(
                "error: failed to write generated OPENAPI_OUT ({}): {}",
                path, e
            );
            std::process::exit(2);
        }
        // Emit selected schemas used in docs (gating contract & capsule)
        {
            use schemars::schema_for;
            let dir = std::path::Path::new("spec/schemas");
            let _ = std::fs::create_dir_all(dir);
            let contract_schema = schema_for!(arw_core::gating::ContractCfg);
            let capsule_schema = schema_for!(arw_protocol::GatingCapsule);
            let _ = std::fs::write(
                dir.join("gating_contract.json"),
                serde_json::to_string_pretty(&contract_schema).unwrap(),
            );
            let _ = std::fs::write(
                dir.join("gating_capsule.json"),
                serde_json::to_string_pretty(&capsule_schema).unwrap(),
            );
        }
        // Gating keys index for docs convenience
        {
            let keys_path = std::path::Path::new("docs/GATING_KEYS.md");
            let generated_at = Utc::now().format("%Y-%m-%d %H:%M UTC").to_string();
            let mut out = format!(
                "---\ntitle: Gating Keys\n---\n\n# Gating Keys\nGenerated: {}\nType: Reference\n\nGenerated from code.\n\n",
                generated_at
            );
            for k in arw_core::gating_keys::list() {
                out.push_str(&format!("- `{}`\n", k));
            }
            if !out.ends_with('\n') {
                out.push('\n');
            }
            let _ = std::fs::write(keys_path, out);
        }
        return;
    }

    arw_otel::init();
    // Apply performance presets early so env-based tunables pick up sensible defaults.
    // Explicit env vars still take precedence over these seeded values.
    let _tier = arw_core::perf::apply_performance_preset();
    http_timeout::init_from_env();
    let bus = arw_events::Bus::new_with_replay(256, 256);
    let kernel = arw_kernel::Kernel::open(&crate::util::state_dir()).expect("init kernel");
    let kernel_enabled = config::kernel_enabled_from_env();
    // dual-write bus events to kernel and track DB ids for SSE when enabled
    let sse_id_map = std::sync::Arc::new(Mutex::new(sse_cache::SseIdCache::with_capacity(2048)));
    let metrics = std::sync::Arc::new(metrics::Metrics::default());
    if kernel_enabled {
        let mut rx = bus.subscribe();
        let k2 = kernel.clone();
        let sse_ids = sse_id_map.clone();
        let metrics_clone = metrics.clone();
        tokio::spawn(async move {
            while let Ok(env) = rx.recv().await {
                metrics_clone.record_event(&env.kind);
                if let Ok(row_id) = k2.append_event_async(&env).await {
                    let mut hasher = sha2::Sha256::new();
                    hasher.update(env.time.as_bytes());
                    hasher.update(env.kind.as_bytes());
                    if let Ok(pbytes) = serde_json::to_vec(&env.payload) {
                        hasher.update(&pbytes);
                    }
                    let digest = hasher.finalize();
                    let key = u64::from_le_bytes([
                        digest[0], digest[1], digest[2], digest[3], digest[4], digest[5],
                        digest[6], digest[7],
                    ]);
                    let mut cache = sse_ids.lock().await;
                    cache.insert(key, row_id);
                }
            }
        });
    } else {
        let mut rx = bus.subscribe();
        let metrics_clone = metrics.clone();
        tokio::spawn(async move {
            while let Ok(env) = rx.recv().await {
                metrics_clone.record_event(&env.kind);
            }
        });
    }
    let policy = PolicyEngine::load_from_env();
    let policy_arc = std::sync::Arc::new(Mutex::new(policy));
    // Initialize simple WASI host with http.fetch support
    let host: std::sync::Arc<dyn ToolHost> = {
        match arw_wasi::LocalHost::new() {
            Ok(h) => std::sync::Arc::new(h),
            Err(_) => std::sync::Arc::new(arw_wasi::NoopHost),
        }
    };
    // Curated endpoints list recorded as routes are added (avoid drift)
    let (app, endpoints_acc, endpoints_meta_acc) = build_router();
    let state = AppState::builder(bus, kernel, policy_arc.clone(), host, kernel_enabled)
        .with_config_state(std::sync::Arc::new(Mutex::new(json!({}))))
        .with_config_history(std::sync::Arc::new(Mutex::new(Vec::new())))
        .with_metrics(metrics.clone())
        .with_sse_cache(sse_id_map)
        .with_endpoints(std::sync::Arc::new(endpoints_acc))
        .with_endpoints_meta(std::sync::Arc::new(endpoints_meta_acc))
        .build()
        .await;
    read_models::publish_read_model_patch(
        &state.bus(),
        "policy_capsules",
        &json!({"items": [], "count": 0}),
    );
    world::load_persisted().await;
    // Start a simple local action worker (demo)
    if state.kernel_enabled() {
        worker::start_local_worker(state.clone());
    }
    // Start read-model publishers (logic units, orchestrator jobs)
    read_models::start_read_models(state.clone());
    cluster::start(state.clone());
    runtime_matrix::start(state.clone());
    state_observer::start(state.clone());
    world::start(state.clone());
    distill::start(state.clone());
    self_model::start_aggregators(state.clone());
    research_watcher::start(state.clone());
    // Start/stop egress proxy based on current settings
    egress_proxy::apply_current(state.clone()).await;
    // Watch trust store file and publish rpu.trust.changed on reloads
    {
        let bus = state.bus();
        tokio::spawn(async move {
            use std::time::Duration;
            let path = std::env::var("ARW_TRUST_CAPSULES")
                .ok()
                .unwrap_or_else(|| "configs/trust_capsules.json".to_string());
            let mut last_mtime: Option<std::time::SystemTime> = None;
            loop {
                let mut changed = false;
                if let Ok(md) = std::fs::metadata(&path) {
                    if let Ok(mt) = md.modified() {
                        if last_mtime.map(|t| t < mt).unwrap_or(true) {
                            last_mtime = Some(mt);
                            changed = true;
                        }
                    }
                }
                if changed {
                    arw_core::rpu::reload_trust();
                    let count = arw_core::rpu::trust_snapshot().len();
                    let payload = serde_json::json!({
                        "count": count,
                        "path": path,
                        "ts_ms": arw_core::rpu::trust_last_reload_ms()
                    });
                    bus.publish(arw_topics::TOPIC_RPU_TRUST_CHANGED, &payload);
                }
                tokio::time::sleep(Duration::from_secs(5)).await;
            }
        });
    }
    #[cfg(feature = "grpc")]
    let _grpc_task = crate::grpc::spawn(state.clone());
    let capsule_mw_state = state.clone();
    let app = app.with_state(state);
    let app = app.layer(axum::middleware::from_fn(move |req, next| {
        let st = capsule_mw_state.clone();
        async move { capsule_guard::capsule_mw(st, req, next).await }
    }));
    let metrics_layer = metrics.clone();
    let app = app.layer(axum::middleware::from_fn(move |req, next| {
        let metrics = metrics_layer.clone();
        async move { metrics::track_http(metrics, req, next).await }
    }));
    // HTTP layers: compression, tracing, and concurrency limit
    let conc: usize = std::env::var("ARW_HTTP_MAX_CONC")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(1024);
    let app = app
        .layer(CompressionLayer::new())
        .layer(TraceLayer::new_for_http())
        .layer(ConcurrencyLimitLayer::new(conc));
    // Bind address/port (env overrides)
    let bind = std::env::var("ARW_BIND").unwrap_or_else(|_| "127.0.0.1".into());
    let port: u16 = std::env::var("ARW_PORT")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(8091);
    // Security: refuse public bind without an admin token
    let token_set = std::env::var("ARW_ADMIN_TOKEN")
        .ok()
        .is_some_and(|v| !v.is_empty())
        || std::env::var("ARW_ADMIN_TOKEN_SHA256")
            .ok()
            .is_some_and(|v| !v.is_empty());
    let is_loopback = {
        let b = bind.trim().to_ascii_lowercase();
        b == "127.0.0.1" || b == "::1" || b == "[::1]" || b == "localhost"
    };
    if !is_loopback && !token_set {
        eprintln!(
            "error: ARW_BIND={} is public and ARW_ADMIN_TOKEN/ARW_ADMIN_TOKEN_SHA256 not set; refusing to start",
            bind
        );
        std::process::exit(2);
    }
    let addr: SocketAddr = format!("{}:{}", bind, port).parse().unwrap();
    // Global middleware: security headers, optional access log, then app
    let app = app
        .layer(axum::middleware::from_fn(security::headers_mw))
        .layer(axum::middleware::from_fn(access_log::access_log_mw));
    axum::serve(
        tokio::net::TcpListener::bind(addr).await.unwrap(),
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .await
    .unwrap();
}

#[cfg(test)]
#[allow(clippy::items_after_test_module)]
mod http_tests {
    use super::*;
    use crate::router::paths;
    use arw_core::rpu;
    use arw_protocol::GatingCapsule;
    use arw_topics::{self as topics, TOPIC_POLICY_CAPSULE_APPLIED, TOPIC_READMODEL_PATCH};
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
    use tokio::time::timeout;
    use tower::util::ServiceExt;

    async fn build_state(dir: &Path) -> AppState {
        std::env::set_var("ARW_STATE_DIR", dir.display().to_string());
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
        let state_dir = temp.path().to_path_buf();

        let state = build_state(&state_dir).await;
        worker::start_local_worker(state.clone());
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
    async fn capsule_middleware_applies_and_publishes_read_model() {
        let temp = tempdir().expect("tempdir");
        let trust_path = temp.path().join("trust_capsules.json");
        let signing = SigningKey::from_bytes(&[7u8; 32]);
        let issuer = "test-issuer";
        write_trust_store(&trust_path, issuer, &signing);
        std::env::set_var("ARW_TRUST_CAPSULES", trust_path.display().to_string());
        rpu::reload_trust();

        let state_dir = temp.path().to_path_buf();
        let state = build_state(&state_dir).await;
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

        std::env::remove_var("ARW_TRUST_CAPSULES");
    }

    #[tokio::test]
    async fn lease_creation_emits_event_and_updates_read_model() {
        let temp = tempdir().expect("tempdir");
        let state = build_state(temp.path()).await;
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

pub(crate) fn admin_ok(headers: &HeaderMap) -> bool {
    // When ARW_ADMIN_TOKEN or ARW_ADMIN_TOKEN_SHA256 is set, require it in Authorization: Bearer or X-ARW-Admin
    let token_plain = std::env::var("ARW_ADMIN_TOKEN")
        .ok()
        .filter(|t| !t.is_empty());
    let token_hash = std::env::var("ARW_ADMIN_TOKEN_SHA256")
        .ok()
        .filter(|t| !t.is_empty());
    if token_plain.is_none() && token_hash.is_none() {
        return true;
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
