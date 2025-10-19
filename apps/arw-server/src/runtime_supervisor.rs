use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, Instant},
};

use arw_events::Bus;
use arw_runtime::{
    AdapterError, RuntimeAdapter, RuntimeDescriptor, RuntimeModality, RuntimeRestartBudget,
    RuntimeSeverity, RuntimeState, RuntimeStatus,
};
use async_trait::async_trait;
use once_cell::sync::Lazy;
use serde::Deserialize;
use tokio::sync::RwLock;
use tokio_util::sync::CancellationToken;
use tracing::{debug, error, info, warn};

#[cfg(test)]
use arw_runtime::RuntimeAccelerator;

use crate::runtime::{RuntimeRegistry, RuntimeRestoreError};

static DEFAULT_HEALTH_INTERVAL_MS: Lazy<u64> = Lazy::new(|| {
    std::env::var("ARW_RUNTIME_HEALTH_INTERVAL_MS")
        .ok()
        .and_then(|value| value.trim().parse::<u64>().ok())
        .filter(|value| *value >= 100)
        .unwrap_or(5_000)
});

#[derive(Clone)]
pub struct SupervisorOptions {
    pub health_interval: Duration,
}

impl Default for SupervisorOptions {
    fn default() -> Self {
        Self {
            health_interval: Duration::from_millis(*DEFAULT_HEALTH_INTERVAL_MS),
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum SupervisorError {
    #[error("adapter `{0}` not registered")]
    MissingAdapter(String),
    #[error("runtime `{0}` not known to supervisor")]
    UnknownRuntime(String),
    #[error(transparent)]
    Adapter(#[from] AdapterError),
    #[error("manifest error: {0}")]
    Manifest(String),
    #[error("io error: {0}")]
    Io(String),
}

#[derive(Clone)]
pub(crate) struct ManagedRuntimeDefinition {
    descriptor: RuntimeDescriptor,
    adapter_id: String,
    auto_start: bool,
    preset: Option<String>,
    source: Option<String>,
}

impl ManagedRuntimeDefinition {
    pub(crate) fn new(
        descriptor: RuntimeDescriptor,
        adapter_id: String,
        auto_start: bool,
        preset: Option<String>,
        source: Option<String>,
    ) -> Self {
        Self {
            descriptor,
            adapter_id,
            auto_start,
            preset,
            source,
        }
    }

    pub(crate) fn source(&self) -> Option<&str> {
        self.source.as_deref()
    }
}

struct ActiveRuntime {
    adapter_id: String,
    handle: arw_runtime::RuntimeHandle,
    cancel: CancellationToken,
    task: tokio::task::JoinHandle<()>,
}

fn definition_requires_restart(
    old: &ManagedRuntimeDefinition,
    new: &ManagedRuntimeDefinition,
) -> bool {
    if old.adapter_id != new.adapter_id {
        return true;
    }
    let old_desc = &old.descriptor;
    let new_desc = &new.descriptor;
    old_desc.adapter != new_desc.adapter
        || old_desc.profile != new_desc.profile
        || old_desc.modalities != new_desc.modalities
        || old_desc.accelerator != new_desc.accelerator
        || old_desc.name != new_desc.name
        || old_desc.tags != new_desc.tags
        || old.preset != new.preset
}

pub struct RuntimeSupervisor {
    registry: Arc<RuntimeRegistry>,
    bus: Bus,
    options: SupervisorOptions,
    adapters: RwLock<HashMap<String, Arc<dyn RuntimeAdapter>>>,
    definitions: RwLock<HashMap<String, ManagedRuntimeDefinition>>,
    active: RwLock<HashMap<String, ActiveRuntime>>,
}

impl RuntimeSupervisor {
    pub async fn new(registry: Arc<RuntimeRegistry>, bus: Bus) -> Arc<Self> {
        Self::new_with_options(registry, bus, SupervisorOptions::default()).await
    }

    pub async fn new_with_options(
        registry: Arc<RuntimeRegistry>,
        bus: Bus,
        options: SupervisorOptions,
    ) -> Arc<Self> {
        let supervisor = Arc::new(Self {
            registry: registry.clone(),
            bus,
            options,
            adapters: RwLock::new(HashMap::new()),
            definitions: RwLock::new(HashMap::new()),
            active: RwLock::new(HashMap::new()),
        });
        registry.attach_supervisor(&supervisor).await;
        supervisor
    }

    pub async fn register_adapter(&self, adapter: Arc<dyn RuntimeAdapter>) {
        self.adapters
            .write()
            .await
            .insert(adapter.id().to_string(), adapter);
    }

    pub async fn install_builtin_adapters(&self) -> Result<(), SupervisorError> {
        let adapter = ProcessRuntimeAdapter::new().map_err(SupervisorError::Adapter)?;
        self.register_adapter(adapter).await;
        Ok(())
    }

    pub async fn runtime_ids_with_source_prefix(&self, prefix: &str) -> Vec<String> {
        let guard = self.definitions.read().await;
        guard
            .iter()
            .filter_map(|(id, definition)| {
                definition
                    .source()
                    .filter(|src| src.starts_with(prefix))
                    .map(|_| id.clone())
            })
            .collect()
    }

    pub async fn remove_definition(&self, id: &str) -> Result<(), SupervisorError> {
        let existed = {
            let mut guard = self.definitions.write().await;
            guard.remove(id)
        };
        if existed.is_none() {
            return Ok(());
        }

        self.shutdown_runtime(id).await?;
        self.registry.remove_descriptor(id).await;
        info!(
            target = "arw::runtime",
            runtime = %id,
            "runtime definition removed"
        );
        Ok(())
    }

    pub async fn install_definition(
        &self,
        definition: ManagedRuntimeDefinition,
    ) -> Result<(), SupervisorError> {
        let mut new_definition = definition;
        let runtime_id = new_definition.descriptor.id.clone();
        let adapter_metadata = {
            let guard = self.adapters.read().await;
            guard
                .get(&new_definition.adapter_id)
                .map(|adapter| adapter.metadata())
        };
        if let Some(metadata) = adapter_metadata {
            if new_definition.descriptor.modalities.is_empty() && !metadata.modalities.is_empty() {
                new_definition.descriptor.modalities = metadata.modalities.clone();
            }
            if new_definition.descriptor.accelerator.is_none() {
                new_definition.descriptor.accelerator = metadata.default_accelerator.clone();
            }
            if new_definition.descriptor.profile.is_none() {
                if let Some(default_profile) = metadata.default_profiles.first().cloned() {
                    new_definition.descriptor.profile = Some(default_profile);
                }
            }
            for (key, value) in metadata.tags.iter() {
                new_definition
                    .descriptor
                    .tags
                    .entry(key.clone())
                    .or_insert_with(|| value.clone());
            }
            if metadata.default_profiles.len() > 1
                && !new_definition
                    .descriptor
                    .tags
                    .contains_key("adapter.default_profiles")
            {
                if let Ok(serialized) = serde_json::to_string(&metadata.default_profiles) {
                    new_definition
                        .descriptor
                        .tags
                        .insert("adapter.default_profiles".into(), serialized);
                }
            }
        }
        let auto_start = new_definition.auto_start;
        let preset = new_definition.preset.clone();
        self.registry
            .register_descriptor(new_definition.descriptor.clone())
            .await;
        let previous_definition = {
            let mut guard = self.definitions.write().await;
            guard.insert(runtime_id.clone(), new_definition.clone())
        };
        if self.registry.descriptor(&runtime_id).await.is_some() {
            let mut snapshot = RuntimeStatus::new(runtime_id.clone(), RuntimeState::Offline)
                .with_summary("Runtime registered; awaiting supervisor launch");
            snapshot.detail.push("Managed by RuntimeSupervisor".into());
            snapshot.set_severity(RuntimeSeverity::Info);
            self.registry.apply_status(snapshot).await;
        }
        let needs_restart = previous_definition
            .as_ref()
            .map(|old| definition_requires_restart(old, &new_definition))
            .unwrap_or(false);
        let is_active = {
            let guard = self.active.read().await;
            guard.contains_key(&runtime_id)
        };

        if auto_start {
            if is_active {
                if needs_restart {
                    let registry = self.registry.clone();
                    let runtime_id_clone = runtime_id.clone();
                    let preset_clone = preset.clone();
                    tokio::spawn(async move {
                        match registry
                            .request_restore(&runtime_id_clone, true, preset_clone, None)
                            .await
                        {
                            Ok(_) => info!(
                                target = "arw::runtime",
                                runtime = %runtime_id_clone,
                                "auto-start restart queued"
                            ),
                            Err(RuntimeRestoreError::RestartDenied { .. }) => warn!(
                                target = "arw::runtime",
                                runtime = %runtime_id_clone,
                                "auto-start restart skipped: restart budget exhausted"
                            ),
                            Err(RuntimeRestoreError::RestoreFailed { reason }) => warn!(
                                target = "arw::runtime",
                                runtime = %runtime_id_clone,
                                error = %reason,
                                "auto-start restart failed"
                            ),
                        }
                    });
                } else {
                    info!(
                        target = "arw::runtime",
                        runtime = %runtime_id,
                        "auto-start ensured: runtime already running"
                    );
                }
            } else {
                let registry = self.registry.clone();
                let runtime_id_clone = runtime_id.clone();
                let preset_clone = preset.clone();
                tokio::spawn(async move {
                    match registry
                        .request_restore(&runtime_id_clone, false, preset_clone, None)
                        .await
                    {
                        Ok(_) => info!(
                            target = "arw::runtime",
                            runtime = %runtime_id_clone,
                            "auto-start restore queued"
                        ),
                        Err(RuntimeRestoreError::RestartDenied { .. }) => warn!(
                            target = "arw::runtime",
                            runtime = %runtime_id_clone,
                            "auto-start skipped: restart budget exhausted"
                        ),
                        Err(RuntimeRestoreError::RestoreFailed { reason }) => warn!(
                            target = "arw::runtime",
                            runtime = %runtime_id_clone,
                            error = %reason,
                            "auto-start restore failed"
                        ),
                    }
                });
            }
        } else if is_active {
            if let Err(err) = self.shutdown_runtime(&runtime_id).await {
                warn!(
                    target = "arw::runtime",
                    runtime = %runtime_id,
                    error = %err,
                    "auto-start disabled but runtime shutdown failed"
                );
            } else {
                info!(
                    target = "arw::runtime",
                    runtime = %runtime_id,
                    "auto-start disabled: runtime stopped"
                );
            }
        } else {
            info!(
                target = "arw::runtime",
                runtime = %runtime_id,
                "auto-start disabled: runtime already stopped"
            );
        }
        Ok(())
    }

    pub async fn load_manifests_from_disk(&self) -> Result<(), SupervisorError> {
        let paths = manifest_paths();
        if paths.is_empty() {
            return Ok(());
        }

        for path in paths {
            if let Err(err) = self.load_manifest(&path).await {
                warn!(
                    target: "arw::runtime",
                    path = %path.display(),
                    error = %err,
                    "failed to load runtime manifest"
                );
            }
        }
        Ok(())
    }

    async fn load_manifest(&self, path: &Path) -> Result<(), SupervisorError> {
        let bytes = tokio::fs::read(path)
            .await
            .map_err(|err| SupervisorError::Io(err.to_string()))?;
        let manifest: RuntimeManifest =
            toml::from_slice(&bytes).map_err(|err| SupervisorError::Manifest(err.to_string()))?;
        if let Some(version) = manifest.version {
            if version != 1 {
                warn!(
                    target = "arw::runtime",
                    path = %path.display(),
                    version,
                    "unsupported runtime manifest version"
                );
            }
        }
        let mut installed = 0usize;
        let mut keep_ids: HashSet<String> = HashSet::new();
        for entry in manifest.runtimes {
            let entry_id = entry.id.clone();
            let definition = manifest_entry_to_definition(entry, Some(path.to_path_buf()))?;
            keep_ids.insert(entry_id);
            self.install_definition(definition).await?;
            installed += 1;
        }
        let source = path.display().to_string();
        self.remove_definitions_from_source(&source, &keep_ids)
            .await;
        info!(
            target: "arw::runtime",
            path = %path.display(),
            installed,
            "runtime manifest loaded"
        );
        Ok(())
    }

    async fn remove_definitions_from_source(&self, source: &str, keep: &HashSet<String>) {
        let mut removed: Vec<String> = Vec::new();
        {
            let mut guard = self.definitions.write().await;
            guard.retain(|id, definition| {
                let matches_source = definition
                    .source
                    .as_deref()
                    .map(|s| s == source)
                    .unwrap_or(false);
                if matches_source && !keep.contains(id) {
                    removed.push(id.clone());
                    false
                } else {
                    true
                }
            });
        }

        for runtime_id in removed {
            if let Err(err) = self.shutdown_runtime(&runtime_id).await {
                warn!(
                    target = "arw::runtime",
                    runtime = %runtime_id,
                    error = %err,
                    "failed to shut down runtime removed from manifest"
                );
            }
            self.registry.remove_descriptor(&runtime_id).await;
            info!(
                target = "arw::runtime",
                runtime = %runtime_id,
                source = %source,
                "runtime manifest entry removed"
            );
        }
    }

    pub async fn restore_runtime(
        self: &Arc<Self>,
        id: &str,
        restart: bool,
        budget_hint: Option<RuntimeRestartBudget>,
        restore_job_id: Option<String>,
    ) -> Result<(), SupervisorError> {
        let definition = {
            let guard = self.definitions.read().await;
            guard
                .get(id)
                .cloned()
                .ok_or_else(|| SupervisorError::UnknownRuntime(id.to_string()))?
        };
        let adapter = {
            let guard = self.adapters.read().await;
            guard
                .get(&definition.adapter_id)
                .cloned()
                .ok_or_else(|| SupervisorError::MissingAdapter(definition.adapter_id.clone()))?
        };

        if restart {
            let _ = self.shutdown_runtime(id).await;
        }

        let prepared = adapter
            .prepare(arw_runtime::PrepareContext {
                descriptor: &definition.descriptor,
            })
            .await?;

        let mut status = RuntimeStatus::new(id.to_string(), RuntimeState::Starting)
            .with_summary("Runtime launch requested")
            .touch();
        status
            .detail
            .push(format!("Adapter: {}", definition.adapter_id.as_str()));
        if let Some(source) = definition.source.as_ref() {
            status.detail.push(format!("Source: {}", source));
        }
        if let Some(budget) = budget_hint.as_ref() {
            status.detail.push(format_budget_hint(budget));
        }
        self.registry.apply_status(status).await;

        let handle = adapter.launch(prepared).await?;

        let cancel = CancellationToken::new();
        let task = self.spawn_health_task(
            id.to_string(),
            definition.adapter_id.clone(),
            handle.clone(),
            cancel.clone(),
            restore_job_id.clone(),
        );

        let mut active_guard = self.active.write().await;
        if let Some(existing) = active_guard.remove(id) {
            existing.cancel.cancel();
            existing.task.abort();
        }
        active_guard.insert(
            id.to_string(),
            ActiveRuntime {
                adapter_id: definition.adapter_id,
                handle,
                cancel,
                task,
            },
        );
        Ok(())
    }

    pub async fn shutdown_runtime(&self, id: &str) -> Result<(), SupervisorError> {
        let entry = {
            let mut guard = self.active.write().await;
            guard.remove(id)
        };
        if let Some(active) = entry {
            active.cancel.cancel();
            active.task.abort();
            if let Some(adapter) = self.adapters.read().await.get(&active.adapter_id).cloned() {
                let result = adapter.shutdown(active.handle.clone());
                if let Err(err) = result.await {
                    warn!(
                        target: "arw::runtime",
                        runtime = %id,
                        adapter = %active.adapter_id,
                        error = %err,
                        "runtime shutdown reported error"
                    );
                }
            }
            let mut status = RuntimeStatus::new(id.to_string(), RuntimeState::Offline)
                .with_summary("Runtime stopped")
                .touch();
            status.set_severity(RuntimeSeverity::Info);
            self.registry.apply_status(status).await;
        }
        Ok(())
    }

    fn spawn_health_task(
        self: &Arc<Self>,
        runtime_id: String,
        adapter_id: String,
        handle: arw_runtime::RuntimeHandle,
        cancel: CancellationToken,
        restore_job_id: Option<String>,
    ) -> tokio::task::JoinHandle<()> {
        let supervisor = Arc::clone(self);
        tokio::spawn(async move {
            supervisor
                .run_health_loop(runtime_id, adapter_id, handle, cancel, restore_job_id)
                .await;
        })
    }

    async fn run_health_loop(
        self: Arc<Self>,
        runtime_id: String,
        adapter_id: String,
        handle: arw_runtime::RuntimeHandle,
        cancel: CancellationToken,
        mut restore_job_id: Option<String>,
    ) {
        let Some(adapter) = self.adapters.read().await.get(&adapter_id).cloned() else {
            warn!(
                target: "arw::runtime",
                runtime = %runtime_id,
                adapter = %adapter_id,
                "health loop aborted: adapter missing"
            );
            return;
        };
        let mut ticker = tokio::time::interval(self.options.health_interval);
        let mut announced = false;
        loop {
            tokio::select! {
                _ = cancel.cancelled() => {
                    debug!(
                        target: "arw::runtime",
                        runtime = %runtime_id,
                        "health loop cancelled"
                    );
                    break;
                }
                _ = ticker.tick() => {
                    match adapter.health(&handle).await {
                        Ok(report) => {
                            self.registry.apply_status(report.status).await;
                            if !announced {
                                let mut payload =
                                    serde_json::json!({ "runtime": runtime_id.clone(), "ok": true });
                                if let Some(job_id) = restore_job_id.take() {
                                    if let serde_json::Value::Object(ref mut map) = payload {
                                        map.insert("job_id".into(), serde_json::json!(job_id));
                                    }
                                }
                                self.bus
                                    .publish(arw_topics::TOPIC_RUNTIME_RESTORE_COMPLETED, &payload);
                                announced = true;
                            }
                        }
                        Err(err) => {
                            let mut status = RuntimeStatus::new(runtime_id.clone(), RuntimeState::Error)
                                .with_summary("Runtime failed health check")
                                .touch();
                            status.set_severity(RuntimeSeverity::Error);
                            status.detail.push(err.to_string());
                            self.registry.apply_status(status).await;
                            warn!(
                                target: "arw::runtime",
                                runtime = %runtime_id,
                                error = %err,
                                "runtime reported unhealthy status"
                            );
                            if let Some(job_id) = restore_job_id.take() {
                                self.bus.publish(
                                    arw_topics::TOPIC_RUNTIME_RESTORE_COMPLETED,
                                    &serde_json::json!({
                                        "runtime": runtime_id.clone(),
                                        "ok": false,
                                        "error": err.to_string(),
                                        "job_id": job_id,
                                    }),
                                );
                            }
                            if let Err(shutdown_err) = adapter.shutdown(handle.clone()).await {
                                warn!(
                                    target: "arw::runtime",
                                    runtime = %runtime_id,
                                    adapter = %adapter_id,
                                    error = %shutdown_err,
                                    "runtime shutdown after health failure reported error"
                                );
                            }
                            {
                                let mut guard = self.active.write().await;
                                if let Some(existing) = guard.get(&runtime_id) {
                                    if existing.adapter_id == adapter_id
                                        && existing.handle.id == handle.id
                                    {
                                        guard.remove(&runtime_id);
                                    }
                                }
                            }
                            let restart_plan = {
                                let guard = self.definitions.read().await;
                                guard
                                    .get(&runtime_id)
                                    .map(|definition| (definition.auto_start, definition.preset.clone()))
                            };
                            if let Some((true, preset)) = restart_plan {
                                let registry = self.registry.clone();
                                let runtime_id_clone = runtime_id.clone();
                                tokio::spawn(async move {
                                    match registry
                                        .request_restore(&runtime_id_clone, true, preset.clone(), None)
                                        .await
                                    {
                                        Ok(_) => info!(
                                            target: "arw::runtime",
                                            runtime = %runtime_id_clone,
                                            "auto-restart restore queued"
                                        ),
                                        Err(RuntimeRestoreError::RestartDenied { .. }) => warn!(
                                            target: "arw::runtime",
                                            runtime = %runtime_id_clone,
                                            "auto-restart skipped: restart budget exhausted"
                                        ),
                                        Err(RuntimeRestoreError::RestoreFailed { reason }) => warn!(
                                            target: "arw::runtime",
                                            runtime = %runtime_id_clone,
                                            error = %reason,
                                            "auto-restart restore failed"
                                        ),
                                    }
                                });
                            }
                            break;
                        }
                    }
                }
            }
        }

        if let Some(job_id) = restore_job_id.take() {
            self.bus.publish(
                arw_topics::TOPIC_RUNTIME_RESTORE_COMPLETED,
                &serde_json::json!({
                    "runtime": runtime_id,
                    "ok": false,
                    "error": "Restore monitoring ended",
                    "job_id": job_id,
                }),
            );
        }
    }
}

fn format_budget_hint(budget: &RuntimeRestartBudget) -> String {
    match budget.reset_at {
        Some(ref ts) => format!(
            "Restart budget: {}/{} remaining (resets at {})",
            budget.remaining,
            budget.max_restarts,
            ts.to_rfc3339()
        ),
        None => format!(
            "Restart budget: {}/{} remaining",
            budget.remaining, budget.max_restarts
        ),
    }
}

#[derive(Debug, Deserialize)]
struct RuntimeManifest {
    #[serde(default)]
    version: Option<u32>,
    #[serde(default)]
    runtimes: Vec<RuntimeManifestEntry>,
}

#[derive(Debug, Deserialize)]
struct RuntimeManifestEntry {
    id: String,
    adapter: String,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    profile: Option<String>,
    #[serde(default)]
    modalities: Vec<String>,
    #[serde(default)]
    accelerator: Option<String>,
    #[serde(default)]
    auto_start: Option<bool>,
    #[serde(default)]
    preset: Option<String>,
    #[serde(default)]
    tags: HashMap<String, String>,
    #[serde(default)]
    process: Option<ProcessConfig>,
}

#[derive(Debug, Deserialize)]
struct ProcessConfig {
    command: String,
    #[serde(default)]
    args: Vec<String>,
    #[serde(default)]
    env: HashMap<String, String>,
    #[serde(default)]
    workdir: Option<String>,
    #[serde(default)]
    health: Option<ProcessHealthConfig>,
}

#[derive(Debug, Deserialize)]
struct ProcessHealthConfig {
    url: String,
    #[serde(default = "ProcessHealthConfig::default_method")]
    method: String,
    #[serde(default = "ProcessHealthConfig::default_status")]
    expect_status: u16,
    #[serde(default)]
    expect_body: Option<String>,
    #[serde(default)]
    timeout_ms: Option<u64>,
}

impl ProcessHealthConfig {
    fn default_method() -> String {
        "GET".into()
    }

    fn default_status() -> u16 {
        200
    }
}

fn manifest_entry_to_definition(
    entry: RuntimeManifestEntry,
    source: Option<PathBuf>,
) -> Result<ManagedRuntimeDefinition, SupervisorError> {
    let mut descriptor = RuntimeDescriptor::new(entry.id.clone(), entry.adapter.clone());
    descriptor.name = entry.name;
    descriptor.profile = entry.profile;
    if !entry.modalities.is_empty() {
        descriptor.modalities = entry
            .modalities
            .iter()
            .filter_map(|value| match value.trim().to_ascii_lowercase().as_str() {
                "text" => Some(RuntimeModality::Text),
                "audio" => Some(RuntimeModality::Audio),
                "vision" => Some(RuntimeModality::Vision),
                other => {
                    warn!(
                        target: "arw::runtime",
                        modality = %other,
                        id = %entry.id,
                        "skipping unrecognised modality"
                    );
                    None
                }
            })
            .collect();
    }
    if let Some(accel) = entry.accelerator.as_ref() {
        descriptor.accelerator = match accel.trim().to_ascii_lowercase().as_str() {
            "cpu" => Some(arw_runtime::RuntimeAccelerator::Cpu),
            "gpu_cuda" | "cuda" => Some(arw_runtime::RuntimeAccelerator::GpuCuda),
            "gpu_rocm" | "rocm" => Some(arw_runtime::RuntimeAccelerator::GpuRocm),
            "gpu_metal" | "metal" => Some(arw_runtime::RuntimeAccelerator::GpuMetal),
            "gpu_vulkan" | "vulkan" => Some(arw_runtime::RuntimeAccelerator::GpuVulkan),
            "npu_coreml" | "coreml" => Some(arw_runtime::RuntimeAccelerator::NpuCoreml),
            "npu_directml" | "directml" => Some(arw_runtime::RuntimeAccelerator::NpuDirectml),
            "npu" => Some(arw_runtime::RuntimeAccelerator::NpuOther),
            _ => Some(arw_runtime::RuntimeAccelerator::Other),
        };
    }
    descriptor.tags = entry
        .tags
        .iter()
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();

    if let Some(process) = entry.process {
        descriptor
            .tags
            .insert("process.command".into(), process.command.clone());
        if !process.args.is_empty() {
            let encoded = serde_json::to_string(&process.args).map_err(|err| {
                SupervisorError::Manifest(format!("encode process args failed: {err}"))
            })?;
            descriptor.tags.insert("process.args".into(), encoded);
        }
        if !process.env.is_empty() {
            let encoded = serde_json::to_string(&process.env).map_err(|err| {
                SupervisorError::Manifest(format!("encode process env failed: {err}"))
            })?;
            descriptor.tags.insert("process.env".into(), encoded);
        }
        if let Some(dir) = process.workdir {
            descriptor.tags.insert("process.workdir".into(), dir);
        }
        if let Some(health) = process.health {
            descriptor
                .tags
                .insert("process.health.url".into(), health.url.clone());
            descriptor
                .tags
                .insert("process.health.method".into(), health.method.clone());
            descriptor.tags.insert(
                "process.health.expect_status".into(),
                health.expect_status.to_string(),
            );
            if let Some(body) = health.expect_body {
                descriptor
                    .tags
                    .insert("process.health.expect_body".into(), body);
            }
            if let Some(timeout) = health.timeout_ms {
                descriptor
                    .tags
                    .insert("process.health.timeout_ms".into(), timeout.to_string());
            }
        }
    }

    Ok(ManagedRuntimeDefinition::new(
        descriptor,
        entry.adapter,
        entry.auto_start.unwrap_or(false),
        entry.preset,
        source.map(|p| p.display().to_string()),
    ))
}

fn manifest_paths() -> Vec<PathBuf> {
    if let Ok(raw) = std::env::var("ARW_RUNTIME_MANIFEST") {
        let paths = raw
            .split(';')
            .map(|part| part.trim())
            .filter(|part| !part.is_empty())
            .map(PathBuf::from)
            .collect::<Vec<_>>();
        if !paths.is_empty() {
            return paths;
        }
    }
    if let Some(path) = arw_core::resolve_config_path("configs/runtime/runtimes.toml") {
        return vec![path];
    }
    Vec::new()
}

#[derive(Clone)]
struct ProcessLaunchSpec {
    command: String,
    args: Vec<String>,
    env: HashMap<String, String>,
    workdir: Option<String>,
    health: Option<ProcessHealthSpec>,
}

#[derive(Clone)]
struct ProcessHealthSpec {
    url: String,
    method: String,
    expect_status: u16,
    expect_body: Option<String>,
    timeout: Duration,
}

struct ProcessInstance {
    spec: ProcessLaunchSpec,
    child: tokio::sync::Mutex<tokio::process::Child>,
    started_at: Instant,
}

pub struct ProcessRuntimeAdapter {
    client: reqwest::Client,
    processes: RwLock<HashMap<String, Arc<ProcessInstance>>>,
    pending: RwLock<HashMap<String, ProcessLaunchSpec>>,
}

impl ProcessRuntimeAdapter {
    pub fn new() -> Result<Arc<Self>, AdapterError> {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(5))
            .build()
            .map_err(|err| AdapterError::Io(err.to_string()))?;
        Ok(Arc::new(Self {
            client,
            processes: RwLock::new(HashMap::new()),
            pending: RwLock::new(HashMap::new()),
        }))
    }

    fn parse_spec(
        &self,
        descriptor: &RuntimeDescriptor,
    ) -> Result<ProcessLaunchSpec, AdapterError> {
        let Some(command) = descriptor.tags.get("process.command") else {
            return Err(AdapterError::InvalidConfig(format!(
                "process.command tag missing for {}",
                descriptor.id
            )));
        };
        let args = descriptor
            .tags
            .get("process.args")
            .and_then(|raw| serde_json::from_str::<Vec<String>>(raw).ok())
            .unwrap_or_default();
        let env = descriptor
            .tags
            .get("process.env")
            .and_then(|raw| serde_json::from_str::<HashMap<String, String>>(raw).ok())
            .unwrap_or_default();
        let workdir = descriptor.tags.get("process.workdir").cloned();
        let health = descriptor.tags.get("process.health.url").map(|url| {
            let method = descriptor
                .tags
                .get("process.health.method")
                .cloned()
                .unwrap_or_else(|| "GET".into());
            let expect_status = descriptor
                .tags
                .get("process.health.expect_status")
                .and_then(|raw| raw.parse::<u16>().ok())
                .unwrap_or(200);
            let expect_body = descriptor.tags.get("process.health.expect_body").cloned();
            let timeout = descriptor
                .tags
                .get("process.health.timeout_ms")
                .and_then(|raw| raw.parse::<u64>().ok())
                .map(Duration::from_millis)
                .unwrap_or(Duration::from_secs(5));
            ProcessHealthSpec {
                url: url.clone(),
                method,
                expect_status,
                expect_body,
                timeout,
            }
        });
        Ok(ProcessLaunchSpec {
            command: command.clone(),
            args,
            env,
            workdir,
            health,
        })
    }
}

#[async_trait]
impl RuntimeAdapter for ProcessRuntimeAdapter {
    fn id(&self) -> &'static str {
        "process"
    }

    fn metadata(&self) -> arw_runtime::RuntimeAdapterMetadata {
        arw_runtime::RuntimeAdapterMetadata {
            modalities: vec![RuntimeModality::Text],
            tags: vec![
                ("adapter.kind".to_string(), "process".to_string()),
                (
                    "adapter.description".to_string(),
                    "Managed process runtime".to_string(),
                ),
            ]
            .into_iter()
            .collect(),
            ..Default::default()
        }
    }

    async fn prepare(
        &self,
        ctx: arw_runtime::PrepareContext<'_>,
    ) -> Result<arw_runtime::PreparedRuntime, AdapterError> {
        let spec = self.parse_spec(ctx.descriptor)?;
        self.pending
            .write()
            .await
            .insert(ctx.descriptor.id.clone(), spec.clone());
        Ok(arw_runtime::PreparedRuntime {
            command: spec.command,
            args: spec.args.clone(),
            runtime_id: Some(ctx.descriptor.id.clone()),
        })
    }

    async fn launch(
        &self,
        prepared: arw_runtime::PreparedRuntime,
    ) -> Result<arw_runtime::RuntimeHandle, AdapterError> {
        let spec = {
            let mut guard = self.pending.write().await;
            if let Some(id) = prepared.runtime_id.as_ref() {
                guard.remove(id)
            } else {
                guard.remove(&prepared.command)
            }
        }
        .unwrap_or(ProcessLaunchSpec {
            command: prepared.command.clone(),
            args: prepared.args.clone(),
            env: HashMap::new(),
            workdir: None,
            health: None,
        });

        let mut cmd = tokio::process::Command::new(&spec.command);
        for arg in &spec.args {
            cmd.arg(arg);
        }
        cmd.envs(&spec.env);
        if let Some(dir) = spec.workdir.as_ref() {
            cmd.current_dir(dir);
        }
        cmd.stdout(std::process::Stdio::null());
        cmd.stderr(std::process::Stdio::null());
        cmd.stdin(std::process::Stdio::null());

        let child = cmd
            .spawn()
            .map_err(|err| AdapterError::Launch(err.to_string()))?;
        let pid = child.id();
        let instance = Arc::new(ProcessInstance {
            spec,
            child: tokio::sync::Mutex::new(child),
            started_at: Instant::now(),
        });
        let runtime_id = prepared
            .runtime_id
            .unwrap_or_else(|| instance.spec.command.clone());
        self.processes
            .write()
            .await
            .insert(runtime_id.clone(), instance);
        Ok(arw_runtime::RuntimeHandle {
            id: runtime_id,
            pid,
        })
    }

    async fn shutdown(&self, handle: arw_runtime::RuntimeHandle) -> Result<(), AdapterError> {
        if let Some(instance) = self.processes.write().await.remove(&handle.id) {
            let mut guard = instance.child.lock().await;
            guard
                .start_kill()
                .map_err(|err| AdapterError::Io(err.to_string()))?;
            let _ = guard.wait().await;
        }
        Ok(())
    }

    async fn health(
        &self,
        handle: &arw_runtime::RuntimeHandle,
    ) -> Result<arw_runtime::RuntimeHealthReport, AdapterError> {
        let Some(instance) = self.processes.read().await.get(&handle.id).cloned() else {
            return Err(AdapterError::Unavailable(format!(
                "no process tracked for {}",
                handle.id
            )));
        };
        {
            let mut child = instance.child.lock().await;
            if let Some(status) = child
                .try_wait()
                .map_err(|err| AdapterError::Io(err.to_string()))?
            {
                return Err(AdapterError::Unavailable(format!(
                    "process exited with status {status}"
                )));
            }
        }
        if let Some(health) = instance.spec.health.as_ref() {
            let request = self
                .client
                .request(
                    health.method.parse().unwrap_or(reqwest::Method::GET),
                    &health.url,
                )
                .timeout(health.timeout);
            match request.send().await {
                Ok(resp) => {
                    let status_code = resp.status();
                    let body = resp.text().await.unwrap_or_else(|_| String::new());
                    if status_code.as_u16() == health.expect_status
                        && health
                            .expect_body
                            .as_ref()
                            .map(|needle| body.contains(needle))
                            .unwrap_or(true)
                    {
                        let mut status = RuntimeStatus::new(handle.id.clone(), RuntimeState::Ready)
                            .with_summary("Process healthy")
                            .touch();
                        status.set_severity(RuntimeSeverity::Info);
                        status
                            .detail
                            .push(format!("HTTP {} {}", status_code.as_u16(), health.url));
                        return Ok(arw_runtime::RuntimeHealthReport { status });
                    } else {
                        return Err(AdapterError::Unavailable(format!(
                            "health check failed: {} {} (body len {})",
                            status_code.as_u16(),
                            health.url,
                            body.len()
                        )));
                    }
                }
                Err(err) => {
                    return Err(AdapterError::Unavailable(format!(
                        "health request error: {err}"
                    )));
                }
            }
        }
        let uptime = instance.started_at.elapsed().as_secs();
        let mut status = RuntimeStatus::new(handle.id.clone(), RuntimeState::Ready)
            .with_summary("Process running")
            .touch();
        status.detail.push(format!("uptime {}s", uptime));
        status.set_severity(RuntimeSeverity::Info);
        Ok(arw_runtime::RuntimeHealthReport { status })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use arw_events::Bus;
    use std::collections::HashSet;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;
    use tokio::time::{timeout, Duration};

    #[derive(Default)]
    struct FakeAdapter {
        health_calls: AtomicUsize,
    }

    #[async_trait]
    impl RuntimeAdapter for FakeAdapter {
        fn id(&self) -> &'static str {
            "fake"
        }

        async fn prepare(
            &self,
            ctx: arw_runtime::PrepareContext<'_>,
        ) -> Result<arw_runtime::PreparedRuntime, AdapterError> {
            Ok(arw_runtime::PreparedRuntime {
                command: ctx.descriptor.id.clone(),
                args: Vec::new(),
                runtime_id: Some(ctx.descriptor.id.clone()),
            })
        }

        async fn launch(
            &self,
            prepared: arw_runtime::PreparedRuntime,
        ) -> Result<arw_runtime::RuntimeHandle, AdapterError> {
            Ok(arw_runtime::RuntimeHandle {
                id: prepared
                    .runtime_id
                    .unwrap_or_else(|| prepared.command.clone()),
                pid: Some(4242),
            })
        }

        async fn shutdown(&self, _handle: arw_runtime::RuntimeHandle) -> Result<(), AdapterError> {
            Ok(())
        }

        async fn health(
            &self,
            handle: &arw_runtime::RuntimeHandle,
        ) -> Result<arw_runtime::RuntimeHealthReport, AdapterError> {
            self.health_calls.fetch_add(1, Ordering::SeqCst);
            let mut status = RuntimeStatus::new(handle.id.clone(), RuntimeState::Ready)
                .with_summary("Runtime healthy")
                .touch();
            status.set_severity(RuntimeSeverity::Info);
            Ok(arw_runtime::RuntimeHealthReport { status })
        }
    }

    #[derive(Default)]
    struct FlakyAdapter {
        launch_calls: AtomicUsize,
        health_calls: AtomicUsize,
    }

    #[async_trait]
    impl RuntimeAdapter for FlakyAdapter {
        fn id(&self) -> &'static str {
            "flaky"
        }

        async fn prepare(
            &self,
            ctx: arw_runtime::PrepareContext<'_>,
        ) -> Result<arw_runtime::PreparedRuntime, AdapterError> {
            Ok(arw_runtime::PreparedRuntime {
                command: ctx.descriptor.id.clone(),
                args: Vec::new(),
                runtime_id: Some(ctx.descriptor.id.clone()),
            })
        }

        async fn launch(
            &self,
            prepared: arw_runtime::PreparedRuntime,
        ) -> Result<arw_runtime::RuntimeHandle, AdapterError> {
            self.launch_calls.fetch_add(1, Ordering::SeqCst);
            Ok(arw_runtime::RuntimeHandle {
                id: prepared
                    .runtime_id
                    .unwrap_or_else(|| prepared.command.clone()),
                pid: Some(5150),
            })
        }

        async fn shutdown(&self, _handle: arw_runtime::RuntimeHandle) -> Result<(), AdapterError> {
            Ok(())
        }

        async fn health(
            &self,
            handle: &arw_runtime::RuntimeHandle,
        ) -> Result<arw_runtime::RuntimeHealthReport, AdapterError> {
            let call = self.health_calls.fetch_add(1, Ordering::SeqCst);
            if call == 0 {
                return Err(AdapterError::Unavailable(
                    "simulated health failure".to_string(),
                ));
            }
            let mut status = RuntimeStatus::new(handle.id.clone(), RuntimeState::Ready)
                .with_summary("Runtime healthy after restart")
                .touch();
            status.set_severity(RuntimeSeverity::Info);
            Ok(arw_runtime::RuntimeHealthReport { status })
        }
    }

    #[derive(Default)]
    struct MetadataAdapter;

    #[async_trait]
    impl RuntimeAdapter for MetadataAdapter {
        fn id(&self) -> &'static str {
            "metadata"
        }

        fn metadata(&self) -> arw_runtime::RuntimeAdapterMetadata {
            arw_runtime::RuntimeAdapterMetadata {
                modalities: vec![RuntimeModality::Vision],
                default_accelerator: Some(RuntimeAccelerator::GpuCuda),
                default_profiles: vec!["default".to_string(), "burst".to_string()],
                tags: vec![
                    ("adapter.kind".to_string(), "metadata".to_string()),
                    (
                        "adapter.description".to_string(),
                        "vision runtime metadata defaults".to_string(),
                    ),
                ]
                .into_iter()
                .collect(),
            }
        }

        async fn prepare(
            &self,
            ctx: arw_runtime::PrepareContext<'_>,
        ) -> Result<arw_runtime::PreparedRuntime, AdapterError> {
            Ok(arw_runtime::PreparedRuntime {
                command: ctx.descriptor.id.clone(),
                args: Vec::new(),
                runtime_id: Some(ctx.descriptor.id.clone()),
            })
        }

        async fn launch(
            &self,
            prepared: arw_runtime::PreparedRuntime,
        ) -> Result<arw_runtime::RuntimeHandle, AdapterError> {
            Ok(arw_runtime::RuntimeHandle {
                id: prepared
                    .runtime_id
                    .unwrap_or_else(|| prepared.command.clone()),
                pid: Some(6001),
            })
        }

        async fn shutdown(&self, _handle: arw_runtime::RuntimeHandle) -> Result<(), AdapterError> {
            Ok(())
        }

        async fn health(
            &self,
            handle: &arw_runtime::RuntimeHandle,
        ) -> Result<arw_runtime::RuntimeHealthReport, AdapterError> {
            let mut status = RuntimeStatus::new(handle.id.clone(), RuntimeState::Ready)
                .with_summary("Runtime healthy")
                .touch();
            status.set_severity(RuntimeSeverity::Info);
            Ok(arw_runtime::RuntimeHealthReport { status })
        }
    }

    #[tokio::test]
    async fn supervisor_reports_health() {
        let bus = Bus::new(128);
        let registry = Arc::new(RuntimeRegistry::new(bus.clone()));
        let supervisor = RuntimeSupervisor::new_with_options(
            registry.clone(),
            bus.clone(),
            SupervisorOptions {
                health_interval: Duration::from_millis(50),
            },
        )
        .await;
        let adapter = Arc::new(FakeAdapter::default());
        supervisor.register_adapter(adapter.clone()).await;
        let descriptor = RuntimeDescriptor::new("fake-runtime", "fake");
        supervisor
            .install_definition(ManagedRuntimeDefinition::new(
                descriptor.clone(),
                "fake".into(),
                true,
                None,
                None,
            ))
            .await
            .expect("definition install");

        let mut rx = bus.subscribe();
        timeout(Duration::from_secs(2), async {
            loop {
                if let Ok(env) = rx.recv().await {
                    if env.kind == arw_topics::TOPIC_RUNTIME_RESTORE_COMPLETED {
                        break;
                    }
                }
            }
        })
        .await
        .expect("restore completion event");

        let snapshot = registry.snapshot().await;
        let record = snapshot
            .runtimes
            .iter()
            .find(|r| r.descriptor.id == "fake-runtime")
            .expect("runtime present");
        assert_eq!(record.status.state, RuntimeState::Ready);
        assert!(adapter.health_calls.load(Ordering::SeqCst) > 0);
    }

    #[tokio::test]
    async fn disabling_auto_start_stops_runtime() {
        let bus = Bus::new(128);
        let registry = Arc::new(RuntimeRegistry::new(bus.clone()));
        let supervisor = RuntimeSupervisor::new_with_options(
            registry.clone(),
            bus.clone(),
            SupervisorOptions {
                health_interval: Duration::from_millis(50),
            },
        )
        .await;
        let adapter = Arc::new(FakeAdapter::default());
        supervisor.register_adapter(adapter).await;

        let descriptor = RuntimeDescriptor::new("fake-runtime", "fake");
        supervisor
            .install_definition(ManagedRuntimeDefinition::new(
                descriptor.clone(),
                "fake".into(),
                true,
                None,
                None,
            ))
            .await
            .expect("definition install");

        let mut rx = bus.subscribe();
        timeout(Duration::from_secs(2), async {
            loop {
                if let Ok(env) = rx.recv().await {
                    if env.kind == arw_topics::TOPIC_RUNTIME_RESTORE_COMPLETED {
                        break;
                    }
                }
            }
        })
        .await
        .expect("restore completion event");

        supervisor
            .install_definition(ManagedRuntimeDefinition::new(
                descriptor.clone(),
                "fake".into(),
                false,
                None,
                None,
            ))
            .await
            .expect("definition update");

        let snapshot = registry.snapshot().await;
        let record = snapshot
            .runtimes
            .iter()
            .find(|r| r.descriptor.id == "fake-runtime")
            .expect("runtime present");
        assert_eq!(record.status.state, RuntimeState::Offline);
        assert_eq!(record.status.severity, RuntimeSeverity::Info);
    }

    #[tokio::test]
    async fn auto_restart_requeues_restore_on_health_failure() {
        let bus = Bus::new(128);
        let registry = Arc::new(RuntimeRegistry::new(bus.clone()));
        let supervisor = RuntimeSupervisor::new_with_options(
            registry.clone(),
            bus.clone(),
            SupervisorOptions {
                health_interval: Duration::from_millis(30),
            },
        )
        .await;
        let adapter = Arc::new(FlakyAdapter::default());
        supervisor.register_adapter(adapter.clone()).await;

        let descriptor = RuntimeDescriptor::new("flaky-runtime", "flaky");
        supervisor
            .install_definition(ManagedRuntimeDefinition::new(
                descriptor.clone(),
                "flaky".into(),
                true,
                None,
                None,
            ))
            .await
            .expect("definition install");

        let mut rx = bus.subscribe();
        timeout(Duration::from_secs(2), async {
            loop {
                if adapter.health_calls.load(Ordering::SeqCst) >= 1 {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("first health check executed");

        timeout(Duration::from_secs(3), async {
            loop {
                if adapter.launch_calls.load(Ordering::SeqCst) >= 2 {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        })
        .await
        .expect("auto-restart launched");

        timeout(Duration::from_secs(3), async {
            loop {
                if let Ok(env) = rx.recv().await {
                    if env.kind == arw_topics::TOPIC_RUNTIME_RESTORE_COMPLETED
                        && env.payload["runtime"] == "flaky-runtime"
                        && env.payload.get("ok").and_then(|value| value.as_bool()) == Some(true)
                    {
                        break;
                    }
                }
            }
        })
        .await
        .expect("saw successful restore completion");

        let snapshot = registry.snapshot().await;
        let record = snapshot
            .runtimes
            .iter()
            .find(|r| r.descriptor.id == "flaky-runtime")
            .expect("runtime present");
        assert_eq!(record.status.state, RuntimeState::Ready);
        assert!(
            adapter.launch_calls.load(Ordering::SeqCst) >= 2,
            "expected at least two launches"
        );
    }

    #[tokio::test]
    async fn removing_manifest_entry_removes_runtime() {
        let bus = Bus::new(128);
        let registry = Arc::new(RuntimeRegistry::new(bus.clone()));
        let supervisor = RuntimeSupervisor::new_with_options(
            registry.clone(),
            bus.clone(),
            SupervisorOptions {
                health_interval: Duration::from_millis(50),
            },
        )
        .await;
        let adapter = Arc::new(FakeAdapter::default());
        supervisor.register_adapter(adapter).await;

        let descriptor = RuntimeDescriptor::new("fake-runtime", "fake");
        supervisor
            .install_definition(ManagedRuntimeDefinition::new(
                descriptor.clone(),
                "fake".into(),
                true,
                None,
                Some("test-source".into()),
            ))
            .await
            .expect("definition install");

        let mut rx = bus.subscribe();
        timeout(Duration::from_secs(2), async {
            loop {
                if let Ok(env) = rx.recv().await {
                    if env.kind == arw_topics::TOPIC_RUNTIME_RESTORE_COMPLETED {
                        break;
                    }
                }
            }
        })
        .await
        .expect("restore completion event");

        supervisor
            .remove_definitions_from_source("test-source", &HashSet::new())
            .await;

        assert!(supervisor
            .definitions
            .read()
            .await
            .get("fake-runtime")
            .is_none());
        assert!(registry.descriptor("fake-runtime").await.is_none());
    }

    #[tokio::test]
    async fn adapter_metadata_fills_missing_descriptor_fields() {
        let bus = Bus::new(32);
        let registry = Arc::new(RuntimeRegistry::new(bus.clone()));
        let supervisor = RuntimeSupervisor::new(registry.clone(), bus.clone()).await;
        let adapter = Arc::new(MetadataAdapter);
        supervisor.register_adapter(adapter).await;

        let descriptor = RuntimeDescriptor::new("metadata-runtime", "metadata");
        supervisor
            .install_definition(ManagedRuntimeDefinition::new(
                descriptor,
                "metadata".into(),
                false,
                None,
                None,
            ))
            .await
            .expect("definition install");

        let snapshot = registry.snapshot().await;
        let record = snapshot
            .runtimes
            .iter()
            .find(|r| r.descriptor.id == "metadata-runtime")
            .expect("runtime present");
        assert_eq!(record.descriptor.modalities, vec![RuntimeModality::Vision]);
        assert_eq!(
            record.descriptor.accelerator,
            Some(RuntimeAccelerator::GpuCuda)
        );
        assert_eq!(record.descriptor.profile.as_deref(), Some("default"));
        assert_eq!(
            record
                .descriptor
                .tags
                .get("adapter.default_profiles")
                .map(String::as_str),
            Some("[\"default\",\"burst\"]")
        );
        assert_eq!(
            record
                .descriptor
                .tags
                .get("adapter.kind")
                .map(String::as_str),
            Some("metadata")
        );
        assert_eq!(
            record
                .descriptor
                .tags
                .get("adapter.description")
                .map(String::as_str),
            Some("vision runtime metadata defaults")
        );
    }
}
