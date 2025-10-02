use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use arw_events::Bus;
use arw_runtime::{
    RegistrySnapshot, RuntimeDescriptor, RuntimeModality, RuntimeRecord, RuntimeRestartBudget,
    RuntimeSeverity, RuntimeState, RuntimeStatus,
};
use arw_topics::{
    TOPIC_RUNTIME_RESTORE_COMPLETED, TOPIC_RUNTIME_RESTORE_REQUESTED, TOPIC_RUNTIME_STATE_CHANGED,
};
use chrono::{DateTime, Duration as ChronoDuration, SecondsFormat, Utc};
use serde_json::{json, Value};
use tokio::fs as afs;
use tokio::sync::{Mutex as TokioMutex, RwLock};
use tracing::warn;

use crate::read_models;
use crate::tasks::TaskHandle;
use crate::AppState;

const READ_MODEL_KEY: &str = "runtime_registry";

#[derive(Default)]
struct RuntimeStore {
    desired: HashMap<String, RuntimeDescriptor>,
    statuses: HashMap<String, RuntimeStatus>,
    restart_attempts: HashMap<String, RestartHistory>,
    updated_at: DateTime<Utc>,
}

impl RuntimeStore {
    fn new() -> Self {
        Self {
            desired: HashMap::new(),
            statuses: HashMap::new(),
            restart_attempts: HashMap::new(),
            updated_at: Utc::now(),
        }
    }
}

#[derive(Default)]
struct RestartHistory {
    attempts: VecDeque<DateTime<Utc>>,
}

impl RestartHistory {
    fn prune(&mut self, now: DateTime<Utc>, window: ChronoDuration) -> bool {
        let mut changed = false;
        while let Some(front) = self.attempts.front().copied() {
            if now.signed_duration_since(front) > window {
                self.attempts.pop_front();
                changed = true;
            } else {
                break;
            }
        }
        changed
    }

    fn push_attempt(&mut self, ts: DateTime<Utc>, max: usize) -> bool {
        if self.attempts.len() >= max {
            return false;
        }
        self.attempts.push_back(ts);
        true
    }

    fn snapshot(&self, config: &RestartBudgetConfig) -> RuntimeRestartBudget {
        let max_allowed = config.max_restarts();
        let used_count = self.attempts.len().min(max_allowed);
        let used = used_count as u32;
        let remaining = max_allowed.saturating_sub(used_count) as u32;
        let reset_at = self.attempts.front().map(|ts| *ts + config.window());
        let window_seconds = std::cmp::max(config.window().num_seconds(), 0) as u64;
        RuntimeRestartBudget {
            window_seconds,
            max_restarts: config.max_restarts() as u32,
            used,
            remaining,
            reset_at,
        }
    }
}

#[derive(Clone)]
struct RestartBudgetConfig {
    max_restarts: usize,
    window: ChronoDuration,
}

impl RestartBudgetConfig {
    fn from_env() -> Self {
        let max_restarts = std::env::var("ARW_RUNTIME_RESTART_MAX")
            .ok()
            .and_then(|raw| raw.trim().parse::<usize>().ok())
            .filter(|value| *value > 0)
            .unwrap_or(3);
        let window_secs = std::env::var("ARW_RUNTIME_RESTART_WINDOW_SEC")
            .ok()
            .and_then(|raw| raw.trim().parse::<i64>().ok())
            .filter(|value| *value > 0)
            .unwrap_or(600);
        Self {
            max_restarts,
            window: ChronoDuration::seconds(window_secs),
        }
    }

    fn max_restarts(&self) -> usize {
        self.max_restarts
    }

    fn window(&self) -> ChronoDuration {
        self.window
    }
}

#[derive(Clone, Debug)]
pub struct RestartDenied {
    pub budget: RuntimeRestartBudget,
}

#[derive(Clone)]
pub(crate) struct RuntimeRegistry {
    state: Arc<RwLock<RuntimeStore>>,
    bus: Bus,
    storage: Option<Arc<RuntimeStorage>>,
    restart_config: Arc<RestartBudgetConfig>,
}

impl RuntimeRegistry {
    fn new_internal(
        bus: Bus,
        storage: Option<Arc<RuntimeStorage>>,
        config: RestartBudgetConfig,
    ) -> Self {
        Self {
            state: Arc::new(RwLock::new(RuntimeStore::new())),
            bus,
            storage,
            restart_config: Arc::new(config),
        }
    }

    #[cfg(test)]
    pub fn new(bus: Bus) -> Self {
        Self::new_internal(bus, None, RestartBudgetConfig::from_env())
    }

    pub async fn with_storage(bus: Bus, path: PathBuf) -> Self {
        let storage = Arc::new(RuntimeStorage::new(path));
        let registry =
            Self::new_internal(bus, Some(storage.clone()), RestartBudgetConfig::from_env());
        if let Err(err) = registry.restore_from_storage().await {
            warn!(
                target: "arw::runtime",
                error = %err,
                "failed to restore runtime registry snapshot"
            );
        }
        registry
    }

    #[cfg(test)]
    fn with_budget_config(bus: Bus, config: RestartBudgetConfig) -> Self {
        Self::new_internal(bus, None, config)
    }

    #[allow(dead_code)]
    pub async fn register_descriptor(&self, descriptor: RuntimeDescriptor) {
        let mut guard = self.state.write().await;
        let id = descriptor.id.clone();
        guard.desired.insert(id.clone(), descriptor);
        guard.updated_at = Utc::now();
        drop(guard);
        self.publish_snapshot().await;
    }

    #[allow(dead_code)]
    pub async fn remove_descriptor(&self, id: &str) {
        let mut guard = self.state.write().await;
        guard.desired.remove(id);
        guard.statuses.remove(id);
        guard.updated_at = Utc::now();
        drop(guard);
        self.publish_snapshot().await;
    }

    pub async fn apply_status(&self, mut status: RuntimeStatus) {
        if status.summary.is_empty() {
            status.summary = format!("state set to {:?}", status.state);
        }
        let now = Utc::now();
        status.updated_at = now;
        let mut guard = self.state.write().await;
        let history = guard.restart_attempts.entry(status.id.clone()).or_default();
        let _pruned = history.prune(now, self.restart_config.window());
        let budget_snapshot = history.snapshot(&self.restart_config);
        status.restart_budget = Some(budget_snapshot.clone());
        guard.statuses.insert(status.id.clone(), status.clone());
        guard.updated_at = now;
        drop(guard);
        let mut payload = serde_json::json!({
            "id": status.id,
            "state": status.state,
            "severity": status.severity,
            "summary": status.summary,
            "detail": status.detail,
            "updated": status.updated_at.to_rfc3339(),
        });
        if let Value::Object(ref mut map) = payload {
            if let Ok(value) = serde_json::to_value(&budget_snapshot) {
                map.insert("restart_budget".to_string(), value);
            }
        }
        self.bus.publish(TOPIC_RUNTIME_STATE_CHANGED, &payload);
        self.publish_snapshot().await;
    }

    #[allow(dead_code)]
    pub async fn set_offline(&self, id: &str, reason: impl Into<String>) {
        let mut status = RuntimeStatus::new(id.to_string(), RuntimeState::Offline);
        status.severity = RuntimeSeverity::Warn;
        status.summary = "Runtime marked offline".to_string();
        status.detail.push(reason.into());
        self.apply_status(status).await;
    }

    pub async fn snapshot(&self) -> RegistrySnapshot {
        let mut guard = self.state.write().await;
        let now = Utc::now();
        let mut mutated = false;
        let mut runtimes: Vec<RuntimeRecord> = Vec::new();
        let desired_entries: Vec<(String, RuntimeDescriptor)> = guard
            .desired
            .iter()
            .map(|(id, descriptor)| (id.clone(), descriptor.clone()))
            .collect();
        for (id, descriptor) in desired_entries {
            let mut status = guard
                .statuses
                .get(&id)
                .cloned()
                .unwrap_or_else(|| RuntimeStatus::new(id.clone(), RuntimeState::Unknown));
            let history = guard.restart_attempts.entry(id.clone()).or_default();
            if history.prune(now, self.restart_config.window()) {
                mutated = true;
            }
            if status.restart_budget.is_none() {
                status.restart_budget = Some(history.snapshot(&self.restart_config));
            }
            runtimes.push(RuntimeRecord {
                descriptor: descriptor.clone(),
                status,
            });
        }
        let status_only: Vec<(String, RuntimeStatus)> = guard
            .statuses
            .iter()
            .filter(|(id, _)| !guard.desired.contains_key(*id))
            .map(|(id, status)| (id.clone(), status.clone()))
            .collect();
        for (id, mut status) in status_only {
            let descriptor = RuntimeDescriptor::new(id.clone(), "external");
            let history = guard.restart_attempts.entry(id.clone()).or_default();
            if history.prune(now, self.restart_config.window()) {
                mutated = true;
            }
            if status.restart_budget.is_none() {
                status.restart_budget = Some(history.snapshot(&self.restart_config));
            }
            runtimes.push(RuntimeRecord { descriptor, status });
        }
        runtimes.sort_by(|a, b| a.descriptor.id.cmp(&b.descriptor.id));
        if mutated {
            guard.updated_at = now;
        }
        RegistrySnapshot {
            updated_at: guard.updated_at,
            runtimes,
        }
    }

    async fn publish_snapshot(&self) {
        let snapshot = self.snapshot().await;
        if let Ok(value) = serde_json::to_value(&snapshot) {
            read_models::publish_read_model_patch(&self.bus, READ_MODEL_KEY, &value);
        }
        if let Some(storage) = &self.storage {
            if let Err(err) = storage.persist(&snapshot).await {
                let path = storage.path.clone();
                warn!(
                    target: "arw::runtime",
                    error = %err,
                    path = %path.display(),
                    "failed to persist runtime registry snapshot"
                );
            }
        }
    }

    async fn ensure_descriptor(&self, id: &str) {
        let mut guard = self.state.write().await;
        guard.desired.entry(id.to_string()).or_insert_with(|| {
            let mut descriptor = RuntimeDescriptor::new(id.to_string(), "health-probe");
            descriptor.modalities.push(RuntimeModality::Text);
            descriptor
        });
        guard.updated_at = Utc::now();
    }

    async fn record_restart_attempt(&self, id: &str) -> (bool, RuntimeRestartBudget) {
        let now = Utc::now();
        let mut guard = self.state.write().await;
        let history = guard.restart_attempts.entry(id.to_string()).or_default();
        let mut touched = history.prune(now, self.restart_config.window());
        let allowed = history.push_attempt(now, self.restart_config.max_restarts());
        if allowed {
            touched = true;
        }
        let snapshot = history.snapshot(&self.restart_config);
        if touched {
            guard.updated_at = now;
        }
        drop(guard);
        (allowed, snapshot)
    }

    async fn current_budget(&self, id: &str) -> RuntimeRestartBudget {
        let now = Utc::now();
        let mut guard = self.state.write().await;
        let history = guard.restart_attempts.entry(id.to_string()).or_default();
        let touched = history.prune(now, self.restart_config.window());
        let snapshot = history.snapshot(&self.restart_config);
        if touched {
            guard.updated_at = now;
        }
        drop(guard);
        snapshot
    }

    pub async fn request_restore(
        &self,
        id: &str,
        restart: bool,
        preset: Option<String>,
    ) -> Result<RuntimeRestartBudget, RestartDenied> {
        self.ensure_descriptor(id).await;
        let mut detail_entries = Vec::new();
        if let Some(ref preset_name) = preset {
            if !preset_name.trim().is_empty() {
                detail_entries.push(format!("Preset: {}", preset_name));
            }
        }
        if !restart {
            detail_entries.push("Restart flag disabled".to_string());
        }

        let budget_snapshot = if restart {
            let (allowed, budget) = self.record_restart_attempt(id).await;
            if !allowed {
                let mut denied_details = detail_entries.clone();
                denied_details
                    .push("Automatic restart denied: restart budget exhausted.".to_string());
                denied_details.push(format_budget_hint(&budget));
                denied_details.push(
                    "Adjust ARW_RUNTIME_RESTART_MAX or ARW_RUNTIME_RESTART_WINDOW_SEC to change the budget."
                        .to_string(),
                );
                let mut denied = RuntimeStatus::new(id.to_string(), RuntimeState::Error)
                    .with_summary("Restart budget exhausted")
                    .touch();
                denied.severity = RuntimeSeverity::Error;
                denied.detail = denied_details;
                self.apply_status(denied).await;
                return Err(RestartDenied { budget });
            }
            budget
        } else {
            self.current_budget(id).await
        };

        detail_entries.push(format_budget_hint(&budget_snapshot));

        let mut status = RuntimeStatus::new(id.to_string(), RuntimeState::Starting)
            .with_summary("Restore requested")
            .touch();
        status.detail.extend(detail_entries.clone());
        self.apply_status(status).await;

        self.bus.publish(
            TOPIC_RUNTIME_RESTORE_REQUESTED,
            &json!({
                "runtime": id,
                "restart": restart,
                "preset": preset,
            }),
        );

        let registry = self.clone();
        let bus = self.bus.clone();
        let runtime_id = id.to_string();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_secs(1)).await;
            let mut ready = RuntimeStatus::new(runtime_id.clone(), RuntimeState::Ready)
                .with_summary("Runtime ready after restore")
                .touch();
            ready.detail.push("Runtime restore completed".to_string());
            let budget_hint = registry.current_budget(&runtime_id).await;
            ready.detail.push(format_budget_hint(&budget_hint));
            registry.apply_status(ready).await;
            bus.publish(
                TOPIC_RUNTIME_RESTORE_COMPLETED,
                &json!({"runtime": runtime_id}),
            );
        });

        Ok(budget_snapshot)
    }

    pub async fn handle_health_event(&self, payload: Value) {
        let Some(target) = payload
            .get("target")
            .and_then(|v| v.as_str())
            .map(str::to_owned)
        else {
            return;
        };
        self.ensure_descriptor(&target).await;
        if let Some(status) = RuntimeStatus::from_health_payload(&target, &payload) {
            self.apply_status(status).await;
        }
    }
}

impl RuntimeRegistry {
    async fn restore_from_storage(&self) -> Result<(), std::io::Error> {
        let Some(storage) = &self.storage else {
            return Ok(());
        };
        let maybe_snapshot = storage.load().await?;
        let Some(snapshot) = maybe_snapshot else {
            return Ok(());
        };
        let mut guard = self.state.write().await;
        guard.desired.clear();
        guard.statuses.clear();
        for record in snapshot.runtimes {
            guard
                .desired
                .insert(record.descriptor.id.clone(), record.descriptor);
            guard
                .statuses
                .insert(record.status.id.clone(), record.status);
        }
        guard.updated_at = snapshot.updated_at;
        Ok(())
    }
}

struct RuntimeStorage {
    path: PathBuf,
    lock: TokioMutex<()>,
}

impl RuntimeStorage {
    fn new(path: PathBuf) -> Self {
        Self {
            path,
            lock: TokioMutex::new(()),
        }
    }

    async fn load(&self) -> Result<Option<RegistrySnapshot>, std::io::Error> {
        match afs::read(&self.path).await {
            Ok(bytes) => {
                if bytes.is_empty() {
                    return Ok(None);
                }
                match serde_json::from_slice::<RegistrySnapshot>(&bytes) {
                    Ok(snapshot) => Ok(Some(snapshot)),
                    Err(err) => Err(std::io::Error::new(std::io::ErrorKind::InvalidData, err)),
                }
            }
            Err(err) => {
                if err.kind() == std::io::ErrorKind::NotFound {
                    Ok(None)
                } else {
                    Err(err)
                }
            }
        }
    }

    async fn persist(&self, snapshot: &RegistrySnapshot) -> Result<(), std::io::Error> {
        let _guard = self.lock.lock().await;
        if let Some(parent) = self.path.parent() {
            afs::create_dir_all(parent).await?;
        }
        let mut json_bytes = serde_json::to_vec_pretty(snapshot)
            .map_err(|err| std::io::Error::new(std::io::ErrorKind::InvalidData, err))?;
        json_bytes.push(b'\n');
        write_atomic(self.path.as_path(), &json_bytes).await
    }
}

async fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), std::io::Error> {
    let tmp = path.with_extension("tmp");
    afs::write(&tmp, bytes).await?;
    match afs::rename(&tmp, path).await {
        Ok(_) => Ok(()),
        Err(_) => {
            let _ = afs::remove_file(path).await;
            let result = afs::rename(&tmp, path).await;
            if result.is_err() {
                let _ = afs::remove_file(&tmp).await;
            }
            result
        }
    }
}

fn format_budget_hint(budget: &RuntimeRestartBudget) -> String {
    let base = format!(
        "Restart budget: {} used of {} within {}s window.",
        budget.used, budget.max_restarts, budget.window_seconds
    );
    if let Some(reset_at) = budget.reset_at {
        format!(
            "{} Window resets at {}.",
            base,
            reset_at.to_rfc3339_opts(SecondsFormat::Secs, true)
        )
    } else {
        base
    }
}

pub(crate) fn start(state: AppState) -> Vec<TaskHandle> {
    let registry = state.runtime();
    let mut tasks = Vec::new();

    // Seed the read-model on startup so listeners have an immediate view.
    tasks.push(TaskHandle::new(
        "runtime.registry.seed",
        tokio::spawn(async move {
            registry.publish_snapshot().await;
        }),
    ));

    let registry = state.runtime();
    let mut rx = state.bus().subscribe();
    tasks.push(TaskHandle::new(
        "runtime.registry.health_listener",
        tokio::spawn(async move {
            while let Ok(env) = rx.recv().await {
                if env.kind.as_str() == "runtime.health" {
                    registry.handle_health_event(env.payload.clone()).await;
                }
            }
        }),
    ));

    tasks
}

#[cfg(test)]
mod tests {
    use super::*;
    use arw_events::Bus;
    use tempfile::tempdir;
    use tokio::time::timeout;

    #[tokio::test]
    async fn request_restore_marks_runtime_ready() {
        let bus = Bus::new(64);
        let registry = RuntimeRegistry::new(bus.clone());
        let mut rx = bus.subscribe();

        registry
            .request_restore("runtime-a", true, Some("standard".into()))
            .await
            .expect("restart budget available");

        // Drain events until the restore request surfaces.
        timeout(Duration::from_secs(2), async {
            loop {
                if let Ok(env) = rx.recv().await {
                    if env.kind == TOPIC_RUNTIME_RESTORE_REQUESTED {
                        return env;
                    }
                }
            }
        })
        .await
        .expect("restore request timeout");

        // Wait for completion event
        let completed = timeout(Duration::from_secs(3), async {
            loop {
                if let Ok(env) = rx.recv().await {
                    if env.kind == TOPIC_RUNTIME_RESTORE_COMPLETED {
                        return env;
                    }
                }
            }
        })
        .await
        .expect("completion timeout");
        assert_eq!(completed.payload["runtime"], "runtime-a");

        let snapshot = registry.snapshot().await;
        let record = snapshot
            .runtimes
            .iter()
            .find(|r| r.descriptor.id == "runtime-a")
            .expect("runtime present");
        assert_eq!(record.status.state, RuntimeState::Ready);
    }

    #[tokio::test]
    async fn persists_and_restores_runtime_registry_snapshot() {
        let dir = tempdir().expect("tempdir");
        let path = dir.path().join("registry.json");

        let bus = Bus::new(64);
        let registry = RuntimeRegistry::with_storage(bus.clone(), path.clone()).await;

        let mut descriptor = RuntimeDescriptor::new("runtime-a", "llama.cpp");
        descriptor.name = Some("LLaMA CPU".into());
        descriptor.modalities.push(RuntimeModality::Text);
        registry.register_descriptor(descriptor.clone()).await;

        let mut status =
            RuntimeStatus::new("runtime-a", RuntimeState::Ready).with_summary("Runtime ready");
        status.detail.push("warm cache".into());
        let expected_status = status.clone();
        registry.apply_status(status).await;

        let on_disk = tokio::fs::read(&path)
            .await
            .expect("registry snapshot persisted");
        assert!(!on_disk.is_empty(), "snapshot should not be empty");

        drop(registry);

        let bus2 = Bus::new(64);
        let restored = RuntimeRegistry::with_storage(bus2.clone(), path.clone()).await;
        let snapshot = restored.snapshot().await;
        assert_eq!(snapshot.runtimes.len(), 1);
        let record = &snapshot.runtimes[0];
        assert_eq!(record.descriptor.id, descriptor.id);
        assert_eq!(record.descriptor.adapter, descriptor.adapter);
        assert_eq!(record.status.state, RuntimeState::Ready);
        assert_eq!(record.status.summary, expected_status.summary);
        assert!(record.status.detail.contains(&"warm cache".to_string()));
    }

    #[tokio::test]
    async fn enforces_restart_budget_limit() {
        let config = RestartBudgetConfig {
            max_restarts: 2,
            window: ChronoDuration::seconds(3_600),
        };
        let bus = Bus::new(64);
        let registry = RuntimeRegistry::with_budget_config(bus.clone(), config);

        registry
            .request_restore("runtime-budget", true, None)
            .await
            .expect("first restart allowed");
        tokio::time::sleep(Duration::from_millis(1_100)).await;

        registry
            .request_restore("runtime-budget", true, None)
            .await
            .expect("second restart allowed");
        tokio::time::sleep(Duration::from_millis(1_100)).await;

        let mut rx = bus.subscribe();
        let denied = registry
            .request_restore("runtime-budget", true, None)
            .await
            .expect_err("restart budget should be exhausted");
        assert_eq!(denied.budget.remaining, 0);

        let mut saw_restore_request = false;
        while let Ok(Ok(env)) = tokio::time::timeout(Duration::from_millis(200), rx.recv()).await {
            if env.kind == TOPIC_RUNTIME_RESTORE_REQUESTED
                && env.payload["runtime"] == "runtime-budget"
            {
                saw_restore_request = true;
                break;
            }
        }
        assert!(
            !saw_restore_request,
            "restart budget should block new restore requests"
        );

        let snapshot = registry.snapshot().await;
        let record = snapshot
            .runtimes
            .iter()
            .find(|r| r.descriptor.id == "runtime-budget")
            .expect("runtime present");
        assert_eq!(record.status.state, RuntimeState::Error);
        assert_eq!(record.status.severity, RuntimeSeverity::Error);
        assert_eq!(record.status.summary, "Restart budget exhausted");
        assert!(record
            .status
            .detail
            .iter()
            .any(|entry| entry.contains("Restart budget:")));
        let budget = record
            .status
            .restart_budget
            .as_ref()
            .expect("restart budget present");
        assert_eq!(budget.remaining, 0);
        assert_eq!(budget.used, budget.max_restarts);
        assert_eq!(budget.window_seconds, 3_600);
    }
}
