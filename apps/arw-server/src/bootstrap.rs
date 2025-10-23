use std::sync::Arc;
use std::time::Duration;

use arw_events::Bus;
use arw_kernel::Kernel;
use arw_policy::PolicyEngine;
use arw_wasi::ToolHost;
use serde_json::json;
use tokio::sync::Mutex;
use tracing::{debug, error, info, warn};
#[cfg(not(test))]
use utoipa::OpenApi;

fn smoke_mode_enabled() -> bool {
    matches!(
        std::env::var("ARW_SMOKE_MODE")
            .ok()
            .map(|v| v.trim().to_ascii_lowercase()),
        Some(ref v)
            if v == "1"
                || v == "true"
                || v == "yes"
                || v == "smoke"
                || v == "vision"
    )
}

use crate::{
    access_log,
    app_state::AppState,
    capsule_guard, config, config_watcher, egress_proxy, identity, metrics, queue, read_models,
    responses,
    router::build_router,
    runtime_bundle_resolver, security,
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
    let smoke_mode = smoke_mode_enabled();

    let bus = Bus::new_with_replay(256, 256);
    let kernel = Kernel::open(&crate::util::state_dir()).expect("init kernel");
    let kernel_enabled = config::kernel_enabled_from_env();
    let persona_enabled = kernel_enabled && config::persona_enabled_from_env();
    let metrics = Arc::new(metrics::Metrics::default());
    let queue_signals = Arc::new(queue::QueueSignals::default());
    let sse_id_map = Arc::new(Mutex::new(SseIdCache::with_capacity(2048)));

    let mut background_tasks = TaskManager::with_metrics(metrics.clone());

    background_tasks.extend(spawn_bus_forwarders(
        bus.clone(),
        kernel.clone(),
        kernel_enabled,
        metrics.clone(),
        sse_id_map.clone(),
    ));

    let policy_handle =
        crate::policy::PolicyHandle::new(PolicyEngine::load_from_env(), bus.clone());
    let host: Arc<dyn ToolHost> = match arw_wasi::LocalHost::new() {
        Ok(host) => Arc::new(host),
        Err(err) => {
            error!(
                target: "arw::tools",
                error = %err,
                "failed to initialise WASI host; falling back to NoopHost"
            );
            let ts_ms = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64;
            let payload = json!({
                "status": "degraded",
                "component": "tools.host",
                "reason": "wasi_init_failed",
                "error": err.to_string(),
                "ts_ms": ts_ms,
            });
            bus.publish(arw_topics::TOPIC_SERVICE_HEALTH, &payload);
            Arc::new(arw_wasi::NoopHost)
        }
    };

    let identity_registry = identity::IdentityRegistry::new(bus.clone()).await;
    identity::set_global_registry(identity_registry.clone());

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

    let state = AppState::builder(bus, kernel, policy_handle, host, kernel_enabled)
        .with_persona_enabled(persona_enabled)
        .with_config_state(config_state)
        .with_config_history(config_history)
        .with_metrics(metrics.clone())
        .with_sse_cache(sse_id_map)
        .with_queue_signals(queue_signals.clone())
        .with_endpoints(Arc::new(endpoints))
        .with_endpoints_meta(Arc::new(endpoints_meta))
        .with_identity(identity_registry.clone())
        .build()
        .await;

    if kernel_enabled {
        if let Ok(depth) = state.kernel().count_actions_by_state_async("queued").await {
            state.metrics().queue_reset(depth.max(0) as u64);
        }
    } else {
        state.metrics().queue_reset(0);
    }

    if let Err(err) = state.runtime_supervisor().install_builtin_adapters().await {
        warn!(
            target: "arw::runtime",
            error = %err,
            "failed to register built-in runtime adapters"
        );
    }
    if let Err(err) = state.runtime_supervisor().load_manifests_from_disk().await {
        warn!(
            target: "arw::runtime",
            error = %err,
            "failed to load managed runtime manifests"
        );
    }
    if let Err(err) = state.runtime_bundles().reload().await {
        warn!(
            target = "arw::runtime",
            error = %err,
            "failed to refresh runtime bundle catalogs"
        );
    }
    if let Err(err) =
        runtime_bundle_resolver::reconcile(state.runtime_supervisor(), state.runtime_bundles())
            .await
    {
        warn!(
            target = "arw::runtime",
            error = %err,
            "failed to register bundle runtimes"
        );
    }

    let initial_env_cfg = state.config_state().lock().await.clone();
    config::apply_env_overrides_from(&initial_env_cfg);
    crate::logic_units_builtin::seed(&state).await;

    background_tasks.merge(initialise_state(&state, kernel_enabled, smoke_mode).await);
    if kernel_enabled {
        if let Some(handle) = spawn_embed_backfill_task(
            state.clone(),
            embed_backfill_batch_from_env(),
            embed_backfill_idle_from_env(),
        ) {
            background_tasks.push(handle);
        }
    }
    if !smoke_mode {
        background_tasks.extend(config_watcher::start(state.clone()));
        if let Some(handle) = identity_registry.watch() {
            background_tasks.push(handle);
        }
    }

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
    let router = router.layer(axum::middleware::from_fn(
        crate::request_ctx::correlation_mw,
    ));
    let metrics_layer = metrics.clone();
    let router = router.layer(axum::middleware::from_fn(move |req, next| {
        let metrics = metrics_layer.clone();
        async move { metrics::track_http(metrics, req, next).await }
    }));
    router.layer(axum::middleware::from_fn(responses::envelope_mw))
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
        .layer(axum::middleware::from_fn(access_log::access_log_mw))
        .layer(axum::middleware::from_fn(security::headers_mw))
        .layer(axum::middleware::from_fn(security::client_addr_mw))
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
    let contract_bytes =
        serde_json::to_vec_pretty(&contract_schema).map_err(std::io::Error::other)?;
    let capsule_bytes =
        serde_json::to_vec_pretty(&capsule_schema).map_err(std::io::Error::other)?;
    std::fs::write(dir.join("gating_contract.json"), contract_bytes)?;
    std::fs::write(dir.join("gating_capsule.json"), capsule_bytes)
}

fn export_gating_keys() -> Result<(), std::io::Error> {
    use chrono::Utc;

    fn ensure_updated_from_generated(markdown: &str) -> String {
        let mut lines: Vec<String> = markdown
            .replace("\r\n", "\n")
            .split('\n')
            .map(str::to_string)
            .collect();
        if let Some((generated_idx, generated_value)) = lines
            .iter()
            .enumerate()
            .find_map(|(idx, line)| line.strip_prefix("Generated: ").map(|rest| (idx, rest)))
        {
            if let Some(date_part) = generated_value.split(' ').next().filter(|s| !s.is_empty()) {
                let updated_line = format!("Updated: {}", date_part);
                if let Some(existing_idx) =
                    lines.iter().position(|line| line.starts_with("Updated: "))
                {
                    lines[existing_idx] = updated_line;
                } else {
                    lines.insert(generated_idx, updated_line);
                }
            }
        }
        let mut output = lines.join("\n");
        if markdown.ends_with('\n') && !output.ends_with('\n') {
            output.push('\n');
        }
        output
    }

    fn normalize_markdown_for_compare(content: &str) -> String {
        let normalized_input = content.replace("\r\n", "\n");
        let lines: Vec<String> = normalized_input
            .lines()
            .map(|line| {
                if line.starts_with("Generated: ") {
                    "Generated: <timestamp>".to_string()
                } else if line.starts_with("Updated: ") {
                    "Updated: <timestamp>".to_string()
                } else {
                    line.to_string()
                }
            })
            .collect();
        let mut joined = lines.join("\n");
        if content.ends_with('\n') {
            joined.push('\n');
        }
        joined
    }

    fn normalize_json_for_compare(content: &str) -> Option<serde_json::Value> {
        let mut value: serde_json::Value = serde_json::from_str(content).ok()?;
        if let serde_json::Value::Object(ref mut map) = value {
            map.insert(
                "generated".into(),
                serde_json::Value::String("<timestamp>".into()),
            );
        }
        Some(value)
    }

    let keys_path = std::path::Path::new("docs/GATING_KEYS.md");
    std::fs::create_dir_all(
        keys_path
            .parent()
            .unwrap_or_else(|| std::path::Path::new(".")),
    )?;
    let generated_at = Utc::now().format("%Y-%m-%d %H:%M UTC").to_string();
    let markdown =
        ensure_updated_from_generated(&arw_core::gating_keys::render_markdown(&generated_at));

    let should_update_markdown = match std::fs::read_to_string(keys_path) {
        Ok(existing) => {
            normalize_markdown_for_compare(&existing) != normalize_markdown_for_compare(&markdown)
        }
        Err(_) => true,
    };
    if should_update_markdown {
        std::fs::write(keys_path, &markdown)?;
    }

    let json_path = keys_path.with_extension("json");
    let json_payload = arw_core::gating_keys::render_json(Some(&generated_at));
    let json_text = serde_json::to_string_pretty(&json_payload).map_err(std::io::Error::other)?;

    let should_update_json = match std::fs::read_to_string(&json_path) {
        Ok(existing) => {
            normalize_json_for_compare(&existing) != normalize_json_for_compare(&json_text)
        }
        Err(_) => true,
    };

    if should_update_json {
        std::fs::write(json_path, json_text)?;
    }

    Ok(())
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
        let kernel = kernel.clone();
        let bus_clone = bus.clone();
        let name = "bus.forward.kernel";
        let handle = crate::tasks::spawn_supervised_with(
            name,
            move || {
                let metrics = metrics.clone();
                let sse_id_map = sse_id_map.clone();
                let kernel = kernel.clone();
                let mut rx = bus_clone.subscribe();
                let bus_for_task = bus_clone.clone();
                let task_name = name.to_string();
                async move {
                    use sha2::Digest as _;
                    while let Some(env) =
                        crate::util::next_bus_event(&mut rx, &bus_for_task, &task_name).await
                    {
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
                }
            },
            Some({
                let bus = bus.clone();
                let name = name.to_string();
                move |restarts| {
                    if restarts >= 5 {
                        let payload = serde_json::json!({
                            "status": "degraded",
                            "component": name,
                            "reason": "task_thrashing",
                            "restarts_window": restarts,
                            "window_secs": 30,
                        });
                        bus.publish(arw_topics::TOPIC_SERVICE_HEALTH, &payload);
                    }
                }
            }),
        );
        handles.push(handle);
    } else {
        let bus_clone = bus.clone();
        let name = "bus.forward.metrics";
        let handle = crate::tasks::spawn_supervised_with(
            name,
            move || {
                let metrics = metrics.clone();
                let mut rx = bus_clone.subscribe();
                let bus_for_task = bus_clone.clone();
                let task_name = name.to_string();
                async move {
                    while let Some(env) =
                        crate::util::next_bus_event(&mut rx, &bus_for_task, &task_name).await
                    {
                        metrics.record_event(&env.kind);
                    }
                }
            },
            Some({
                let bus = bus.clone();
                let name = name.to_string();
                move |restarts| {
                    if restarts >= 5 {
                        let payload = serde_json::json!({
                            "status": "degraded",
                            "component": name,
                            "reason": "task_thrashing",
                            "restarts_window": restarts,
                            "window_secs": 30,
                        });
                        bus.publish(arw_topics::TOPIC_SERVICE_HEALTH, &payload);
                    }
                }
            }),
        );
        handles.push(handle);
    }
    handles
}

async fn initialise_state(state: &AppState, kernel_enabled: bool, smoke_mode: bool) -> TaskManager {
    let mut tasks = TaskManager::with_metrics(state.metrics());
    // Announce and clear any crash markers from previous runs.
    crate::crashguard::sweep_on_start(state).await;
    // If configured and recent crashes detected, set an initial safe-mode delay that supervised tasks respect.
    crate::crashguard::maybe_enter_safe_mode(state);
    // If in safe mode, schedule a transition notice when the delay elapses.
    let until = crate::crashguard::safe_mode_until_ms();
    if until > 0 {
        let bus = state.bus();
        tasks.push(TaskHandle::new(
            "safe_mode.announce_exit",
            tokio::spawn(async move {
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_millis() as u64;
                if until > now {
                    tokio::time::sleep(std::time::Duration::from_millis(until - now + 250)).await;
                }
                let payload = serde_json::json!({
                    "status": "recovered",
                    "component": "safe_mode",
                    "reason": "delay_elapsed",
                });
                bus.publish(arw_topics::TOPIC_SERVICE_HEALTH, &payload);
            }),
        ));
    }
    read_models::publish_read_model_patch(
        &state.bus(),
        "policy_capsules",
        &json!({ "items": [], "count": 0 }),
    );

    if !smoke_mode {
        world::load_persisted().await;
    }

    if kernel_enabled && !smoke_mode {
        let worker_count = worker::desired_worker_count();
        info!(
            target: "arw::worker",
            workers = worker_count,
            "starting local worker pool"
        );
        state.metrics().set_worker_configured(worker_count as u64);
        for slot in 0..worker_count {
            tasks.push(worker::start_local_worker(state.clone(), slot));
        }
    }

    tasks.extend(read_models::start_read_models(state.clone()));
    if !smoke_mode {
        tasks.extend(crate::cluster::start(state.clone()));
    }
    tasks.extend(crate::runtime::start(state.clone()));
    tasks.extend(crate::runtime_matrix::start(state.clone()));
    tasks.extend(crate::state_observer::start(state.clone()));
    if !smoke_mode {
        tasks.extend(crate::world::start(state.clone()));
        tasks.push(crate::distill::start(state.clone()));
        tasks.push(crate::context_cascade::start(state.clone()));
        tasks.push(crate::training::start_logic_history_recorder(state.clone()));
        tasks.push(crate::memory_hygiene::start(state.clone()));
        tasks.extend(crate::self_model::start_aggregators(state.clone()));
        tasks.extend(crate::research_watcher::start(state.clone()));
        tasks.push(crate::capsule_guard::start_refresh_task(state.clone()));

        egress_proxy::apply_current(state.clone()).await;
        tasks.push(spawn_trust_store_watcher(state.clone()));
    }

    #[cfg(feature = "grpc")]
    {
        if !smoke_mode {
            tasks.push(TaskHandle::new("grpc.server", grpc::spawn(state.clone())));
        }
    }
    tasks
}

fn spawn_trust_store_watcher(state: AppState) -> TaskHandle {
    let bus = state.bus();
    crate::tasks::spawn_supervised("trust.watcher", move || {
        let bus = bus.clone();
        async move {
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
        }
    })
}

fn embed_backfill_batch_from_env() -> usize {
    std::env::var("ARW_MEMORY_EMBED_BACKFILL_BATCH")
        .ok()
        .and_then(|s| s.parse::<usize>().ok())
        .unwrap_or(256)
}

fn embed_backfill_idle_from_env() -> Duration {
    std::env::var("ARW_MEMORY_EMBED_BACKFILL_IDLE_SEC")
        .ok()
        .and_then(|s| s.parse::<u64>().ok())
        .filter(|&secs| secs > 0)
        .map(Duration::from_secs)
        .unwrap_or_else(|| Duration::from_secs(300))
}

fn spawn_embed_backfill_task(state: AppState, batch: usize, idle: Duration) -> Option<TaskHandle> {
    if batch == 0 {
        return None;
    }
    let metrics = state.metrics();
    Some(crate::tasks::spawn_supervised(
        "memory.embed_backfill",
        move || {
            let kernel = state.kernel().clone();
            let metrics = metrics.clone();
            async move {
                loop {
                    match kernel.backfill_embed_blobs_async(batch).await {
                        Ok(updated) => {
                            let pending = match kernel.pending_embed_backfill_async().await {
                                Ok(value) => Some(value),
                                Err(err) => {
                                    warn!(
                                        target: "arw::memory",
                                        error = %err,
                                        "memory embed backfill pending count failed"
                                    );
                                    None
                                }
                            };
                            metrics.record_embed_backfill(updated as u64, pending);
                            if updated == 0 {
                                tokio::time::sleep(idle).await;
                            } else {
                                debug!(
                                    target: "arw::memory",
                                    %updated,
                                    batch,
                                    "backfilled memory embedding blobs"
                                );
                                tokio::time::sleep(Duration::from_millis(50)).await;
                            }
                        }
                        Err(err) => {
                            let err_msg = err.to_string();
                            metrics.record_embed_backfill_error(&err_msg);
                            warn!(
                                target: "arw::memory",
                                error = %err_msg,
                                "memory embed backfill failed"
                            );
                            tokio::time::sleep(idle).await;
                        }
                    }
                }
            }
        },
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::env as test_env;

    #[test]
    fn enforce_admin_token_loopback_allowed_without_token() {
        let mut guard = test_env::guard();
        guard.remove("ARW_ADMIN_TOKEN");
        guard.remove("ARW_ADMIN_TOKEN_SHA256");
        assert!(enforce_admin_token_guard("127.0.0.1").is_ok());
        assert!(enforce_admin_token_guard("localhost").is_ok());
    }

    #[test]
    fn enforce_admin_token_requires_token_for_public_bind() {
        let mut guard = test_env::guard();
        guard.remove("ARW_ADMIN_TOKEN");
        guard.remove("ARW_ADMIN_TOKEN_SHA256");
        let err = enforce_admin_token_guard("0.0.0.0").unwrap_err();
        assert!(matches!(err, HttpConfigError::MissingAdminToken { .. }));
    }

    #[test]
    fn enforce_admin_token_allows_public_bind_with_token() {
        let mut guard = test_env::guard();
        guard.set("ARW_ADMIN_TOKEN", "token");
        assert!(enforce_admin_token_guard("0.0.0.0").is_ok());
    }
}
