use std::sync::Arc;

use arw_events::Bus;
use arw_kernel::Kernel;
use arw_policy::PolicyEngine;
use arw_wasi::ToolHost;
use serde_json::json;
use tokio::sync::Mutex;
use tracing::info;
#[cfg(not(test))]
use utoipa::OpenApi;

use crate::{
    access_log,
    app_state::AppState,
    capsule_guard, config, egress_proxy, metrics, read_models,
    router::build_router,
    security,
    sse_cache::SseIdCache,
    tasks::{TaskHandle, TaskManager},
    worker, world,
};

#[cfg(feature = "grpc")]
use crate::grpc;

pub(crate) struct BootstrapOutput {
    pub router: axum::Router<AppState>,
    pub state: AppState,
    pub metrics: Arc<metrics::Metrics>,
    pub background_tasks: TaskManager,
}

pub(crate) async fn build() -> BootstrapOutput {
    config::apply_effective_paths();
    let initial_config = config::load_initial_config_state();
    config::init_gating_from_configs();
    config::init_cache_policy_from_manifest();

    let bus = Bus::new_with_replay(256, 256);
    let kernel = Kernel::open(&crate::util::state_dir()).expect("init kernel");
    let kernel_enabled = config::kernel_enabled_from_env();
    let metrics = Arc::new(metrics::Metrics::default());
    let sse_id_map = Arc::new(Mutex::new(SseIdCache::with_capacity(2048)));

    let mut background_tasks = TaskManager::with_metrics(metrics.clone());

    background_tasks.extend(spawn_bus_forwarders(
        bus.clone(),
        kernel.clone(),
        kernel_enabled,
        metrics.clone(),
        sse_id_map.clone(),
    ));

    let policy_arc = Arc::new(Mutex::new(PolicyEngine::load_from_env()));
    let host: Arc<dyn ToolHost> = match arw_wasi::LocalHost::new() {
        Ok(host) => Arc::new(host),
        Err(_) => Arc::new(arw_wasi::NoopHost),
    };

    let (router, endpoints, endpoints_meta) = build_router();

    let config::InitialConfigState {
        value: initial_config_value,
        history: initial_history,
        source: initial_source,
    } = initial_config;
    if let Some(src) = initial_source {
        info!(config_source = %src, "runtime config source detected");
    }
    let config_state = Arc::new(Mutex::new(initial_config_value));
    let config_history = Arc::new(Mutex::new(initial_history));

    let state = AppState::builder(bus, kernel, policy_arc, host, kernel_enabled)
        .with_config_state(config_state)
        .with_config_history(config_history)
        .with_metrics(metrics.clone())
        .with_sse_cache(sse_id_map)
        .with_endpoints(Arc::new(endpoints))
        .with_endpoints_meta(Arc::new(endpoints_meta))
        .build()
        .await;

    background_tasks.merge(initialise_state(&state, kernel_enabled).await);

    BootstrapOutput {
        router,
        state,
        metrics,
        background_tasks,
    }
}

pub(crate) fn attach_stateful_layers(
    router: axum::Router<AppState>,
    state: AppState,
    metrics: Arc<metrics::Metrics>,
) -> axum::Router<()> {
    let router = router.with_state::<()>(state.clone());
    let capsule_state = state.clone();
    let router = router.layer(axum::middleware::from_fn(move |req, next| {
        let state = capsule_state.clone();
        async move { capsule_guard::capsule_mw(state, req, next).await }
    }));
    let metrics_layer = metrics.clone();
    router.layer(axum::middleware::from_fn(move |req, next| {
        let metrics = metrics_layer.clone();
        async move { metrics::track_http(metrics, req, next).await }
    }))
}

pub(crate) fn attach_http_layers(
    router: axum::Router<()>,
    concurrency_limit: usize,
) -> axum::Router<()> {
    use tower::limit::ConcurrencyLimitLayer;
    use tower_http::{compression::CompressionLayer, trace::TraceLayer};

    router
        .layer(CompressionLayer::new())
        .layer(TraceLayer::new_for_http())
        .layer(ConcurrencyLimitLayer::new(concurrency_limit))
}

pub(crate) fn attach_global_layers(router: axum::Router<()>) -> axum::Router<()> {
    router
        .layer(axum::middleware::from_fn(security::headers_mw))
        .layer(axum::middleware::from_fn(access_log::access_log_mw))
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum HttpConfigError {
    #[error("invalid ARW_HTTP_MAX_CONC: {0}")]
    InvalidConcurrency(String),
    #[error("invalid ARW_PORT: {0}")]
    InvalidPort(String),
    #[error("invalid ARW_BIND: {0}")]
    InvalidBind(String),
    #[error(
        "ARW_BIND={bind} is public and ARW_ADMIN_TOKEN/ARW_ADMIN_TOKEN_SHA256 not set; refusing to start"
    )]
    MissingAdminToken { bind: String },
}

pub(crate) struct HttpConfig {
    pub addr: std::net::SocketAddr,
    pub concurrency_limit: usize,
}

pub(crate) fn http_config_from_env() -> Result<HttpConfig, HttpConfigError> {
    let concurrency_limit = std::env::var("ARW_HTTP_MAX_CONC")
        .ok()
        .map(|raw| {
            raw.parse()
                .map_err(|_| HttpConfigError::InvalidConcurrency(raw))
        })
        .transpose()? // Option<Result> -> Result<Option>
        .unwrap_or(1024);

    let bind = std::env::var("ARW_BIND").unwrap_or_else(|_| "127.0.0.1".into());
    let port_raw = std::env::var("ARW_PORT").unwrap_or_else(|_| "8091".into());
    let port: u16 = port_raw
        .parse()
        .map_err(|_| HttpConfigError::InvalidPort(port_raw))?;

    enforce_admin_token_guard(&bind)?;

    let addr = format!("{}:{}", bind, port)
        .parse()
        .map_err(|_| HttpConfigError::InvalidBind(bind.clone()))?;

    Ok(HttpConfig {
        addr,
        concurrency_limit,
    })
}

pub(crate) fn ensure_openapi_export() -> Result<Option<String>, std::io::Error> {
    if let Ok(path) = std::env::var("OPENAPI_OUT") {
        export_openapi(&path)?;
        export_gating_schemas()?;
        export_gating_keys()?;
        return Ok(Some(path));
    }
    Ok(None)
}

fn enforce_admin_token_guard(bind: &str) -> Result<(), HttpConfigError> {
    let token_set = std::env::var("ARW_ADMIN_TOKEN")
        .ok()
        .is_some_and(|v| !v.is_empty())
        || std::env::var("ARW_ADMIN_TOKEN_SHA256")
            .ok()
            .is_some_and(|v| !v.is_empty());

    let bind_lower = bind.trim().to_ascii_lowercase();
    let is_loopback = matches!(
        bind_lower.as_str(),
        "127.0.0.1" | "::1" | "[::1]" | "localhost"
    );

    if !is_loopback && !token_set {
        return Err(HttpConfigError::MissingAdminToken {
            bind: bind.to_string(),
        });
    }
    Ok(())
}

fn export_openapi(path: &str) -> Result<(), std::io::Error> {
    if let Some(parent) = std::path::Path::new(path).parent() {
        std::fs::create_dir_all(parent)?;
    }
    let yaml = crate::openapi::ApiDoc::openapi()
        .to_yaml()
        .unwrap_or_else(|_| "openapi: 3.0.3".into());
    std::fs::write(path, yaml)
}

fn export_gating_schemas() -> Result<(), std::io::Error> {
    use schemars::schema_for;

    let dir = std::path::Path::new("spec/schemas");
    std::fs::create_dir_all(dir)?;
    let contract_schema = schema_for!(arw_core::gating::ContractCfg);
    let capsule_schema = schema_for!(arw_protocol::GatingCapsule);
    std::fs::write(
        dir.join("gating_contract.json"),
        serde_json::to_string_pretty(&contract_schema).unwrap(),
    )?;
    std::fs::write(
        dir.join("gating_capsule.json"),
        serde_json::to_string_pretty(&capsule_schema).unwrap(),
    )
}

fn export_gating_keys() -> Result<(), std::io::Error> {
    use chrono::Utc;

    let keys_path = std::path::Path::new("docs/GATING_KEYS.md");
    std::fs::create_dir_all(
        keys_path
            .parent()
            .unwrap_or_else(|| std::path::Path::new(".")),
    )?;
    let generated_at = Utc::now().format("%Y-%m-%d %H:%M UTC").to_string();
    let markdown = arw_core::gating_keys::render_markdown(&generated_at);
    std::fs::write(keys_path, markdown)?;

    let json_path = keys_path.with_extension("json");
    let json_payload = arw_core::gating_keys::render_json(Some(&generated_at));
    std::fs::write(
        json_path,
        serde_json::to_string_pretty(&json_payload).unwrap_or_else(|_| "{}".into()),
    )
}

fn spawn_bus_forwarders(
    bus: Bus,
    kernel: Kernel,
    kernel_enabled: bool,
    metrics: Arc<metrics::Metrics>,
    sse_id_map: Arc<Mutex<SseIdCache>>,
) -> Vec<TaskHandle> {
    let mut handles = Vec::new();
    if kernel_enabled {
        let mut rx = bus.subscribe();
        let kernel = kernel.clone();
        let handle = tokio::spawn(async move {
            use sha2::Digest as _;

            while let Ok(env) = rx.recv().await {
                metrics.record_event(&env.kind);
                if let Ok(row_id) = kernel.append_event_async(&env).await {
                    let mut hasher = sha2::Sha256::new();
                    hasher.update(env.time.as_bytes());
                    hasher.update(env.kind.as_bytes());
                    if let Ok(payload_bytes) = serde_json::to_vec(&env.payload) {
                        hasher.update(&payload_bytes);
                    }
                    let digest = hasher.finalize();
                    let key = u64::from_le_bytes([
                        digest[0], digest[1], digest[2], digest[3], digest[4], digest[5],
                        digest[6], digest[7],
                    ]);
                    let mut cache = sse_id_map.lock().await;
                    cache.insert(key, row_id);
                }
            }
        });
        handles.push(TaskHandle::new("bus.forward.kernel", handle));
    } else {
        let mut rx = bus.subscribe();
        let handle = tokio::spawn(async move {
            while let Ok(env) = rx.recv().await {
                metrics.record_event(&env.kind);
            }
        });
        handles.push(TaskHandle::new("bus.forward.metrics", handle));
    }
    handles
}

async fn initialise_state(state: &AppState, kernel_enabled: bool) -> TaskManager {
    let mut tasks = TaskManager::with_metrics(state.metrics());
    read_models::publish_read_model_patch(
        &state.bus(),
        "policy_capsules",
        &json!({ "items": [], "count": 0 }),
    );

    world::load_persisted().await;

    if kernel_enabled {
        tasks.push(worker::start_local_worker(state.clone()));
    }

    tasks.extend(read_models::start_read_models(state.clone()));
    tasks.extend(crate::cluster::start(state.clone()));
    tasks.extend(crate::runtime_matrix::start(state.clone()));
    tasks.extend(crate::state_observer::start(state.clone()));
    tasks.extend(crate::world::start(state.clone()));
    tasks.push(crate::distill::start(state.clone()));
    tasks.extend(crate::self_model::start_aggregators(state.clone()));
    tasks.extend(crate::research_watcher::start(state.clone()));
    tasks.push(crate::capsule_guard::start_refresh_task(state.clone()));

    egress_proxy::apply_current(state.clone()).await;
    tasks.push(spawn_trust_store_watcher(state.clone()));

    #[cfg(feature = "grpc")]
    {
        tasks.push(TaskHandle::new("grpc.server", grpc::spawn(state.clone())));
    }
    tasks
}

fn spawn_trust_store_watcher(state: AppState) -> TaskHandle {
    let bus = state.bus();
    TaskHandle::new(
        "trust.watcher",
        tokio::spawn(async move {
            use std::io::ErrorKind;
            use std::time::{Duration, SystemTime};

            let path = std::env::var("ARW_TRUST_CAPSULES")
                .ok()
                .unwrap_or_else(|| "configs/trust_capsules.json".to_string());
            let mut last_mtime: Option<SystemTime> = None;
            let mut last_present: Option<bool> = None;

            loop {
                let mut changed = false;
                match tokio::fs::metadata(&path).await {
                    Ok(metadata) => {
                        let modified = metadata.modified().ok();
                        let saw_file_before = matches!(last_present, Some(true));
                        if !saw_file_before {
                            changed = true;
                        } else if let (Some(prev), Some(current)) = (last_mtime, modified) {
                            if current > prev {
                                changed = true;
                            }
                        } else if modified.is_some() && last_mtime.is_none() {
                            changed = true;
                        }
                        last_present = Some(true);
                        last_mtime = modified;
                    }
                    Err(err) => {
                        if err.kind() == ErrorKind::NotFound {
                            if last_present != Some(false) {
                                changed = true;
                                last_present = Some(false);
                                last_mtime = None;
                            }
                        } else {
                            tracing::warn!(
                                target: "arw::policy",
                                path = %path,
                                error = %err,
                                "trust watcher metadata probe failed",
                            );
                        }
                    }
                }

                if changed {
                    arw_core::rpu::reload_trust();
                    let count = arw_core::rpu::trust_snapshot().len();
                    let payload = serde_json::json!({
                        "count": count,
                        "path": path,
                        "ts_ms": arw_core::rpu::trust_last_reload_ms(),
                        "exists": matches!(last_present, Some(true)),
                    });
                    bus.publish(arw_topics::TOPIC_RPU_TRUST_CHANGED, &payload);
                }

                tokio::time::sleep(Duration::from_secs(5)).await;
            }
        }),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use once_cell::sync::Lazy;
    use std::{env, sync::Mutex};

    static ENV_LOCK: Lazy<Mutex<()>> = Lazy::new(|| Mutex::new(()));

    #[test]
    fn enforce_admin_token_loopback_allowed_without_token() {
        let _guard = ENV_LOCK.lock().unwrap();
        env::remove_var("ARW_ADMIN_TOKEN");
        env::remove_var("ARW_ADMIN_TOKEN_SHA256");
        assert!(enforce_admin_token_guard("127.0.0.1").is_ok());
        assert!(enforce_admin_token_guard("localhost").is_ok());
    }

    #[test]
    fn enforce_admin_token_requires_token_for_public_bind() {
        let _guard = ENV_LOCK.lock().unwrap();
        env::remove_var("ARW_ADMIN_TOKEN");
        env::remove_var("ARW_ADMIN_TOKEN_SHA256");
        let err = enforce_admin_token_guard("0.0.0.0").unwrap_err();
        assert!(matches!(err, HttpConfigError::MissingAdminToken { .. }));
    }

    #[test]
    fn enforce_admin_token_allows_public_bind_with_token() {
        let _guard = ENV_LOCK.lock().unwrap();
        env::set_var("ARW_ADMIN_TOKEN", "token");
        assert!(enforce_admin_token_guard("0.0.0.0").is_ok());
        env::remove_var("ARW_ADMIN_TOKEN");
    }
}
