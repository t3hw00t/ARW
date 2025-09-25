use chrono::{DateTime, SecondsFormat, Utc};
use once_cell::sync::OnceCell;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashMap};
use std::future::Future;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime};
use tokio::{fs as afs, time};

const MAX_NOTES_LEN: usize = 64 * 1024;
const MAX_TREE_DEPTH: usize = 5;
const MAX_ENTRIES_PER_DIR: usize = 512;

use crate::{metrics, tasks::TaskHandle, training, AppState};
use arw_topics as topics;

pub(crate) fn start_read_models(state: AppState) -> Vec<TaskHandle> {
    let mut handles = Vec::new();
    handles.push(spawn_read_model(
        &state,
        "logic_units",
        Duration::from_millis(1500),
        |st| async move {
            if !st.kernel_enabled() {
                return None;
            }
            st.kernel()
                .list_logic_units_async(200)
                .await
                .ok()
                .map(|items| json!({ "items": items }))
        },
    ));

    handles.push(spawn_read_model(
        &state,
        "orchestrator_jobs",
        Duration::from_millis(2000),
        |st| async move {
            if !st.kernel_enabled() {
                return None;
            }
            st.kernel()
                .list_orchestrator_jobs_async(200)
                .await
                .ok()
                .map(|items| json!({ "items": items }))
        },
    ));

    handles.push(spawn_read_model(
        &state,
        "memory_recent",
        Duration::from_millis(2500),
        |st| async move {
            if !st.kernel_enabled() {
                return None;
            }
            st.kernel()
                .list_recent_memory_async(None, 200)
                .await
                .ok()
                .map(|items| json!({ "items": items }))
        },
    ));

    handles.push(spawn_read_model(
        &state,
        "episodes",
        Duration::from_millis(2500),
        |st| async move {
            if !st.kernel_enabled() {
                return None;
            }
            let items = crate::api::state::build_episode_rollups(&st, 1000).await;
            Some(json!({ "items": items }))
        },
    ));

    handles.push(spawn_projects_read_model(&state));

    handles.push(spawn_read_model(
        &state,
        "route_stats",
        Duration::from_millis(2000),
        |st| async move {
            let summary = st.metrics().snapshot();
            let bus = st.bus().stats();
            Some(metrics::route_stats_snapshot(&summary, &bus))
        },
    ));

    handles.push(spawn_read_model(
        &state,
        "background_tasks",
        Duration::from_millis(3000),
        |st| async move {
            let tasks = st.metrics().tasks_snapshot();
            Some(json!({ "tasks": tasks }))
        },
    ));

    handles.push(spawn_read_model(
        &state,
        "cluster_nodes",
        Duration::from_millis(5000),
        |st| async move {
            let nodes = st.cluster().snapshot().await;
            Some(json!({ "nodes": nodes }))
        },
    ));

    handles.push(spawn_read_model(
        &state,
        "research_watcher",
        Duration::from_millis(5000),
        |st| async move {
            if !st.kernel_enabled() {
                return None;
            }
            let pending = st
                .kernel()
                .list_research_watcher_items_async(Some("pending".to_string()), 100)
                .await
                .unwrap_or_default();
            let approved = st
                .kernel()
                .list_research_watcher_items_async(Some("approved".to_string()), 30)
                .await
                .unwrap_or_default();
            let archived = st
                .kernel()
                .list_research_watcher_items_async(Some("archived".to_string()), 30)
                .await
                .unwrap_or_default();
            let generated = chrono::Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true);
            Some(json!({
                "generated": generated,
                "pending": pending,
                "approved_recent": approved,
                "archived_recent": archived
            }))
        },
    ));

    handles.push(spawn_read_model(
        &state,
        "staging_actions",
        Duration::from_millis(4000),
        |st| async move {
            if !st.kernel_enabled() {
                return None;
            }
            let pending = st
                .kernel()
                .list_staging_actions_async(Some("pending".to_string()), 100)
                .await
                .unwrap_or_default();
            let decided = st
                .kernel()
                .list_staging_actions_async(None, 40)
                .await
                .unwrap_or_default();
            let generated = chrono::Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true);
            Some(json!({
                "generated": generated,
                "pending": pending,
                "recent": decided,
            }))
        },
    ));

    handles.push(spawn_read_model(
        &state,
        "training_metrics",
        Duration::from_millis(4000),
        |st| async move { Some(training::telemetry_snapshot(&st)) },
    ));

    handles.push(spawn_read_model(
        &state,
        "policy_leases",
        Duration::from_millis(4000),
        |st| async move {
            if !st.kernel_enabled() {
                return None;
            }
            Some(leases_snapshot(&st).await)
        },
    ));

    handles.push(spawn_snappy(&state));

    handles
}

fn spawn_read_model<F, Fut>(
    state: &AppState,
    id: &'static str,
    period: Duration,
    builder: F,
) -> TaskHandle
where
    F: Fn(AppState) -> Fut + Send + 'static,
    Fut: Future<Output = Option<Value>> + Send + 'static,
{
    let bus = state.bus();
    let state = state.clone();
    TaskHandle::new(
        format!("read_model::{id}"),
        tokio::spawn(async move {
            let mut tick = time::interval(period);
            loop {
                tick.tick().await;
                let state_clone = state.clone();
                if let Some(value) = builder(state_clone).await {
                    publish_read_model_patch(&bus, id, &value);
                }
            }
        }),
    )
}

static READ_MODEL_CACHE: OnceCell<Mutex<HashMap<String, Value>>> = OnceCell::new();

pub(crate) fn publish_read_model_patch(bus: &arw_events::Bus, id: &str, value: &Value) {
    let map = READ_MODEL_CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    let mut guard = map.lock().unwrap();
    let prev = guard.get(id).cloned().unwrap_or_else(|| json!({}));
    let patch = json_patch::diff(&prev, value);
    if patch.is_empty() {
        return;
    }
    let patch_val = serde_json::to_value(patch).unwrap_or_else(|_| json!([]));
    bus.publish(
        topics::TOPIC_READMODEL_PATCH,
        &json!({
            "id": id,
            "patch": patch_val
        }),
    );
    guard.insert(id.to_string(), value.clone());
}

pub(crate) fn cached_read_model(id: &str) -> Option<Value> {
    READ_MODEL_CACHE
        .get()
        .and_then(|map| map.lock().ok().and_then(|guard| guard.get(id).cloned()))
}

fn spawn_snappy(state: &AppState) -> TaskHandle {
    let state = state.clone();
    TaskHandle::new(
        "read_model::snappy",
        tokio::spawn(async move {
            let mut governor = SnappyGovernorState::new(SnappyConfig::from_env());
            let period = Duration::from_millis(governor.config.publish_ms.max(1));
            let mut tick = time::interval(period);
            loop {
                tick.tick().await;
                let summary = state.metrics().snapshot();
                let snapshot = SnappySnapshot::from_metrics(&governor.config, &summary);
                let bus = state.bus();
                if let Some(notice) = snapshot.notice_payload() {
                    bus.publish(topics::TOPIC_SNAPPY_NOTICE, &notice);
                }
                if snapshot.has_routes() && governor.should_emit_detail() {
                    if let Some(detail) = snapshot.detail_payload() {
                        bus.publish(topics::TOPIC_SNAPPY_DETAIL, &detail);
                    }
                }
                publish_read_model_patch(&bus, "snappy", &snapshot.to_json());
            }
        }),
    )
}

#[derive(Clone)]
struct SnappyConfig {
    budget_i2f_p95_ms: u64,
    budget_first_partial_p95_ms: u64,
    budget_full_result_p95_ms: u64,
    cadence_ms: u64,
    publish_ms: u64,
    protected_prefixes: Vec<String>,
    detail_every: Option<Duration>,
}

impl SnappyConfig {
    fn from_env() -> Self {
        Self {
            budget_i2f_p95_ms: env_u64("ARW_SNAPPY_I2F_P95_MS", 50),
            budget_first_partial_p95_ms: env_u64("ARW_SNAPPY_FIRST_PARTIAL_P95_MS", 150),
            budget_full_result_p95_ms: env_u64("ARW_SNAPPY_FULL_RESULT_P95_MS", 2000),
            cadence_ms: env_u64("ARW_SNAPPY_CADENCE_MS", 250),
            publish_ms: env_u64("ARW_SNAPPY_PUBLISH_MS", 2000),
            protected_prefixes: env_csv(
                "ARW_SNAPPY_PROTECTED_ENDPOINTS",
                "/admin/debug,/state/,/chat/,/events",
            ),
            detail_every: env_u64_opt("ARW_SNAPPY_DETAIL_EVERY").and_then(|secs| {
                if secs == 0 {
                    None
                } else {
                    Some(Duration::from_secs(secs))
                }
            }),
        }
    }

    fn matches(&self, path: &str) -> bool {
        if self.protected_prefixes.is_empty() {
            return false;
        }
        self.protected_prefixes.iter().any(|prefix| {
            if prefix.ends_with('*') {
                let trimmed = prefix.trim_end_matches('*');
                path.starts_with(trimmed)
            } else {
                path.starts_with(prefix)
            }
        })
    }

    fn budgets(&self) -> SnappyBudgets {
        SnappyBudgets {
            i2f_p95_ms: self.budget_i2f_p95_ms,
            first_partial_p95_ms: self.budget_first_partial_p95_ms,
            cadence_ms: self.cadence_ms,
            full_result_p95_ms: self.budget_full_result_p95_ms,
        }
    }
}

#[derive(Clone)]
struct SnappyBudgets {
    i2f_p95_ms: u64,
    first_partial_p95_ms: u64,
    cadence_ms: u64,
    full_result_p95_ms: u64,
}

struct SnappySnapshot {
    generated: String,
    routes: BTreeMap<String, metrics::RouteSummary>,
    max_path: Option<String>,
    max_p95_ms: u64,
    budgets: SnappyBudgets,
    protected_prefixes: Vec<String>,
}

impl SnappySnapshot {
    fn from_metrics(config: &SnappyConfig, summary: &metrics::MetricsSummary) -> Self {
        let mut routes = BTreeMap::new();
        for (path, stat) in summary.routes.by_path.iter() {
            if config.matches(path) {
                routes.insert(path.clone(), stat.clone());
            }
        }
        let (max_path, max_p95) = routes.iter().fold((None, 0u64), |acc, (path, stat)| {
            if stat.p95_ms > acc.1 {
                (Some(path.clone()), stat.p95_ms)
            } else {
                acc
            }
        });
        Self {
            generated: Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true),
            routes,
            max_path,
            max_p95_ms: max_p95,
            budgets: config.budgets(),
            protected_prefixes: config.protected_prefixes.clone(),
        }
    }

    fn has_routes(&self) -> bool {
        !self.routes.is_empty()
    }

    fn breach_full_result(&self) -> bool {
        self.max_path.is_some() && self.max_p95_ms > self.budgets.full_result_p95_ms
    }

    fn to_json(&self) -> Value {
        let mut routes = serde_json::Map::new();
        for (path, stat) in &self.routes {
            routes.insert(
                path.clone(),
                serde_json::to_value(stat).unwrap_or_else(|_| json!({})),
            );
        }
        json!({
            "generated": self.generated,
            "budgets": {
                "i2f_p95_ms": self.budgets.i2f_p95_ms,
                "first_partial_p95_ms": self.budgets.first_partial_p95_ms,
                "cadence_ms": self.budgets.cadence_ms,
                "full_result_p95_ms": self.budgets.full_result_p95_ms,
            },
            "protected_prefixes": self.protected_prefixes,
            "observed": {
                "max_path": self.max_path,
                "max_p95_ms": self.max_p95_ms,
                "routes": Value::Object(routes),
            },
            "breach": {
                "full_result": self.breach_full_result(),
            }
        })
    }

    fn notice_payload(&self) -> Option<Value> {
        if !self.breach_full_result() {
            return None;
        }
        let path = self.max_path.clone()?;
        Some(json!({
            "generated": self.generated,
            "path": path,
            "p95_max_ms": self.max_p95_ms,
            "budget_ms": self.budgets.full_result_p95_ms,
        }))
    }

    fn detail_payload(&self) -> Option<Value> {
        if self.routes.is_empty() {
            return None;
        }
        let mut map = serde_json::Map::new();
        for (path, stat) in &self.routes {
            map.insert(path.clone(), json!(stat.p95_ms));
        }
        Some(json!({
            "generated": self.generated,
            "p95_by_path": Value::Object(map),
        }))
    }
}

struct SnappyGovernorState {
    config: SnappyConfig,
    last_detail: Option<Instant>,
}

impl SnappyGovernorState {
    fn new(config: SnappyConfig) -> Self {
        Self {
            config,
            last_detail: None,
        }
    }

    fn should_emit_detail(&mut self) -> bool {
        let interval = match self.config.detail_every {
            Some(dur) if !dur.is_zero() => dur,
            _ => return false,
        };
        let now = Instant::now();
        match self.last_detail {
            Some(prev) if now.duration_since(prev) < interval => false,
            _ => {
                self.last_detail = Some(now);
                true
            }
        }
    }
}

fn env_u64(key: &str, default: u64) -> u64 {
    std::env::var(key)
        .ok()
        .and_then(|s| s.trim().parse::<u64>().ok())
        .unwrap_or(default)
}

fn env_u64_opt(key: &str) -> Option<u64> {
    std::env::var(key)
        .ok()
        .and_then(|s| s.trim().parse::<u64>().ok())
}

fn env_csv(key: &str, default: &str) -> Vec<String> {
    let raw = std::env::var(key).ok();
    let source = raw.as_deref().unwrap_or(default);
    source
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

fn spawn_projects_read_model(state: &AppState) -> TaskHandle {
    let bus = state.bus();
    TaskHandle::new(
        "read_model::projects",
        tokio::spawn(async move {
            let mut tick = time::interval(Duration::from_millis(4000));
            loop {
                tick.tick().await;
                let snapshot = projects_snapshot().await;
                publish_read_model_patch(&bus, "projects", &snapshot);
            }
        }),
    )
}

fn projects_root_dir() -> PathBuf {
    crate::util::state_dir().join("projects")
}

fn system_time_to_rfc3339(time: SystemTime) -> Option<String> {
    Some(DateTime::<Utc>::from(time).to_rfc3339_opts(SecondsFormat::Millis, true))
}

async fn notes_details(project_root: &Path) -> (Option<String>, Option<u64>, Option<String>) {
    let notes = project_root.join("NOTES.md");
    match afs::metadata(&notes).await {
        Ok(meta) => {
            let modified = meta.modified().ok().and_then(system_time_to_rfc3339);
            let size = Some(meta.len());
            let content = match afs::read(&notes).await {
                Ok(bytes) => {
                    let mut text = String::from_utf8_lossy(&bytes).to_string();
                    if text.len() > MAX_NOTES_LEN {
                        text.truncate(MAX_NOTES_LEN);
                    }
                    Some(text)
                }
                Err(_) => None,
            };
            (modified, size, content)
        }
        Err(_) => (None, None, None),
    }
}

async fn collect_tree(
    project_root: &Path,
    paths: &mut BTreeMap<String, Vec<Value>>,
    digest: &mut Sha256,
) -> std::io::Result<()> {
    let mut stack: Vec<(String, usize)> = vec![(String::new(), 0)];
    while let Some((rel, depth)) = stack.pop() {
        if depth > MAX_TREE_DEPTH {
            continue;
        }
        let abs = if rel.is_empty() {
            project_root.to_path_buf()
        } else {
            project_root.join(&rel)
        };
        let mut entries = Vec::new();
        let mut rd = match afs::read_dir(&abs).await {
            Ok(reader) => reader,
            Err(_) => {
                paths.insert(rel.clone(), Vec::new());
                continue;
            }
        };
        while let Some(ent) = rd.next_entry().await? {
            let name = ent.file_name().to_string_lossy().to_string();
            if name.starts_with('.') {
                continue;
            }
            let metadata = match ent.metadata().await {
                Ok(meta) => meta,
                Err(_) => continue,
            };
            let is_dir = metadata.is_dir();
            let modified = metadata.modified().ok().and_then(system_time_to_rfc3339);
            let rel_path = if rel.is_empty() {
                name.clone()
            } else {
                format!("{}/{}", rel, name)
            };
            let entry = json!({
                "name": name,
                "dir": is_dir,
                "rel": rel_path,
                "modified": modified,
                "size": if is_dir { Value::Null } else { Value::from(metadata.len()) },
            });
            let mut line = String::new();
            line.push_str(entry["rel"].as_str().unwrap_or(""));
            line.push('|');
            line.push(if is_dir { 'd' } else { 'f' });
            line.push('|');
            if let Some(ts) = entry["modified"].as_str() {
                line.push_str(ts);
            }
            line.push('|');
            if !is_dir {
                if let Some(sz) = entry["size"].as_u64() {
                    line.push_str(&sz.to_string());
                }
            }
            digest.update(line.as_bytes());
            entries.push(entry);
            if is_dir && depth < MAX_TREE_DEPTH {
                stack.push((rel_path, depth + 1));
            }
        }
        entries.sort_by(|a, b| {
            a["name"]
                .as_str()
                .unwrap_or("")
                .cmp(b["name"].as_str().unwrap_or(""))
        });
        if entries.len() > MAX_ENTRIES_PER_DIR {
            entries.truncate(MAX_ENTRIES_PER_DIR);
        }
        paths.insert(rel, entries);
    }
    Ok(())
}

pub(crate) async fn projects_snapshot() -> Value {
    let root_dir = projects_root_dir();
    projects_snapshot_at(&root_dir).await
}

pub(crate) async fn projects_snapshot_at(root_dir: &Path) -> Value {
    let generated = Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true);
    let mut items = Vec::new();
    if let Ok(mut rd) = afs::read_dir(root_dir).await {
        while let Ok(Some(ent)) = rd.next_entry().await {
            let file_type = match ent.file_type().await {
                Ok(ft) => ft,
                Err(_) => continue,
            };
            if !file_type.is_dir() {
                continue;
            }
            let name = ent.file_name().to_string_lossy().to_string();
            if name.starts_with('.') {
                continue;
            }
            let project_root = ent.path();
            let (notes_modified, notes_bytes, notes_content) = notes_details(&project_root).await;
            let mut tree_paths = BTreeMap::new();
            let mut digest = Sha256::new();
            let _ = collect_tree(&project_root, &mut tree_paths, &mut digest).await;
            let tree_value = tree_paths
                .into_iter()
                .map(|(k, v)| (k, Value::Array(v)))
                .collect::<serde_json::Map<String, Value>>();
            items.push(json!({
                "name": name,
                "notes": {
                    "modified": notes_modified,
                    "bytes": notes_bytes,
                    "content": notes_content,
                },
                "tree": {
                    "digest": format!("{:x}", digest.finalize()),
                    "paths": Value::Object(tree_value),
                }
            }));
        }
    }
    items.sort_by(|a, b| {
        a["name"]
            .as_str()
            .unwrap_or("")
            .cmp(b["name"].as_str().unwrap_or(""))
    });
    json!({
        "generated": generated,
        "items": items,
    })
}

pub(crate) async fn leases_snapshot(state: &AppState) -> Value {
    let items = state
        .kernel()
        .list_leases_async(200)
        .await
        .unwrap_or_default();
    let generated = chrono::Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true);
    json!({
        "generated": generated,
        "count": items.len(),
        "items": items,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use arw_events::Envelope;
    use arw_policy::PolicyEngine;
    use arw_topics as topics;
    use json_patch::Patch;
    use once_cell::sync::Lazy;
    use serde_json::Value;
    use std::sync::{Arc, Mutex as StdMutex, MutexGuard as StdMutexGuard};
    use tempfile::tempdir;
    use tokio::sync::Mutex;
    use tokio::time::timeout;

    static ENV_MUTEX: Lazy<StdMutex<()>> = Lazy::new(|| StdMutex::new(()));

    struct EnvGuard {
        _lock: StdMutexGuard<'static, ()>,
        prev: Vec<(&'static str, Option<String>)>,
    }

    impl EnvGuard {
        fn set(pairs: &[(&'static str, &str)]) -> Self {
            let lock = ENV_MUTEX.lock().expect("env mutex");
            let mut prev = Vec::new();
            for (key, value) in pairs {
                prev.push((*key, std::env::var(key).ok()));
                std::env::set_var(key, value);
            }
            Self { _lock: lock, prev }
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            for (key, maybe_val) in &self.prev {
                match maybe_val {
                    Some(val) => std::env::set_var(key, val),
                    None => std::env::remove_var(key),
                }
            }
        }
    }

    async fn build_state(path: &std::path::Path) -> AppState {
        std::env::set_var("ARW_DEBUG", "1");
        std::env::set_var("ARW_STATE_DIR", path.display().to_string());
        let bus = arw_events::Bus::new_with_replay(32, 32);
        let kernel = arw_kernel::Kernel::open(path).expect("init kernel");
        let policy = PolicyEngine::load_from_env();
        let policy_arc = Arc::new(Mutex::new(policy));
        let host: Arc<dyn arw_wasi::ToolHost> = Arc::new(arw_wasi::NoopHost);
        AppState::builder(bus, kernel, policy_arc, host, true)
            .with_sse_capacity(16)
            .build()
            .await
    }

    #[tokio::test]
    async fn snappy_publishes_patch_and_notice() {
        let temp = tempdir().expect("tempdir");
        let state = build_state(temp.path()).await;

        let _env_guard = EnvGuard::set(&[
            ("ARW_SNAPPY_PUBLISH_MS", "10"),
            ("ARW_SNAPPY_FULL_RESULT_P95_MS", "15"),
            ("ARW_SNAPPY_PROTECTED_ENDPOINTS", "/state/"),
            ("ARW_SNAPPY_DETAIL_EVERY", "0"),
        ]);

        state.metrics().record_route("/state/routes", 200, 30);

        let bus = state.bus();
        let mut patch_rx =
            bus.subscribe_filtered(vec![topics::TOPIC_READMODEL_PATCH.to_string()], Some(8));
        let mut notice_rx =
            bus.subscribe_filtered(vec![topics::TOPIC_SNAPPY_NOTICE.to_string()], Some(8));

        let snappy_handle = spawn_snappy(&state);

        let patch_env: Envelope = timeout(Duration::from_millis(200), patch_rx.recv())
            .await
            .expect("patch event timeout")
            .expect("patch event");
        assert_eq!(patch_env.kind, topics::TOPIC_READMODEL_PATCH);
        assert_eq!(patch_env.payload["id"].as_str(), Some("snappy"));

        let patch_val: Value = patch_env
            .payload
            .get("patch")
            .cloned()
            .expect("patch array");
        let patch: Patch = serde_json::from_value(patch_val).expect("patch decode");
        let mut doc = json!({});
        json_patch::patch(&mut doc, &patch).expect("apply patch");

        assert_eq!(doc["budgets"]["full_result_p95_ms"].as_u64(), Some(15));
        assert_eq!(doc["observed"]["max_p95_ms"].as_u64(), Some(30));
        let route = &doc["observed"]["routes"]["/state/routes"];
        assert_eq!(route["p95_ms"].as_u64(), Some(30));
        assert_eq!(route["hits"].as_u64(), Some(1));

        let notice_env: Envelope = timeout(Duration::from_millis(200), notice_rx.recv())
            .await
            .expect("notice event timeout")
            .expect("notice event");
        assert_eq!(notice_env.kind, topics::TOPIC_SNAPPY_NOTICE);
        assert_eq!(notice_env.payload["path"].as_str(), Some("/state/routes"));
        assert_eq!(notice_env.payload["p95_max_ms"].as_u64(), Some(30));
        assert_eq!(notice_env.payload["budget_ms"].as_u64(), Some(15));

        let (_name, _started, handle) = snappy_handle.into_inner();
        handle.abort();
        let _ = handle.await;
    }

    #[test]
    fn snappy_detail_emission_respects_interval() {
        let _env_guard = EnvGuard::set(&[]);
        let mut config = SnappyConfig::from_env();
        config.detail_every = Some(Duration::from_millis(50));
        let mut governor = SnappyGovernorState::new(config);

        assert!(governor.should_emit_detail(), "first emit should pass");
        assert!(
            !governor.should_emit_detail(),
            "subsequent emit inside interval should be throttled"
        );

        governor.last_detail = Some(Instant::now() - Duration::from_millis(75));
        assert!(
            governor.should_emit_detail(),
            "emission should resume after interval elapses"
        );
    }
}
