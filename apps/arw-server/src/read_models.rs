use chrono::{DateTime, SecondsFormat, Utc};
use once_cell::sync::{Lazy, OnceCell};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashMap};
use std::future::Future;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime};
use tokio::{fs as afs, time};
use walkdir::WalkDir;

const MAX_NOTES_LEN: usize = 64 * 1024;
const MAX_TREE_DEPTH: usize = 5;
const MAX_ENTRIES_PER_DIR: usize = 512;
const MAX_SCREENSHOTS_INDEX: usize = 120;
const MAX_SCREENSHOTS_TEXT_LEN: usize = 800;
const MAX_SCREENSHOTS_PREVIEW_LEN: usize = 160;
const MAX_SCREENSHOTS_LANGS_PER_SOURCE: usize = 6;
const SCREENSHOT_CAPTURE_EXTENSIONS: [&str; 5] = ["png", "jpg", "jpeg", "webp", "bmp"];

const METRIC_READ_MODEL_COALESCED_WAITERS: &str = "arw_read_model_coalesced_waiters";

static READ_MODEL_FLIGHTS: Lazy<Singleflight> = Lazy::new(Singleflight::default);

#[derive(Default)]
struct NotesSnapshot {
    modified: Option<String>,
    bytes: Option<u64>,
    content: Option<String>,
    sha256: Option<String>,
    truncated: bool,
}

use crate::singleflight::Singleflight;
use crate::{metrics, project_snapshots, tasks::TaskHandle, training, AppState};
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
        "screenshots_index",
        Duration::from_millis(5000),
        |_st| async move { Some(screenshots_snapshot().await) },
    ));

    handles.push(spawn_read_model(
        &state,
        "route_stats",
        Duration::from_millis(2000),
        |st| async move {
            let summary = st.metrics().snapshot();
            let bus = st.bus().stats();
            let cache = st.tool_cache().stats();
            Some(metrics::route_stats_snapshot(&summary, &bus, &cache))
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
            let now = chrono::Utc::now();
            let generated = now.to_rfc3339_opts(SecondsFormat::Millis, true);
            let generated_ms = now.timestamp_millis();
            let generated_ms = if generated_ms < 0 {
                0
            } else {
                generated_ms as u64
            };
            Some(json!({
                "nodes": nodes,
                "generated": generated,
                "generated_ms": generated_ms,
            }))
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
            let now = chrono::Utc::now();
            let generated = now.to_rfc3339_opts(SecondsFormat::Millis, true);
            let generated_ms = now.timestamp_millis();
            let generated_ms = if generated_ms < 0 {
                0
            } else {
                generated_ms as u64
            };
            Some(json!({
                "generated": generated,
                "generated_ms": generated_ms,
                "pending": pending,
                "recent": decided,
            }))
        },
    ));

    handles.push(spawn_read_model(
        &state,
        "training_metrics",
        Duration::from_millis(4000),
        |st| async move { Some(training::telemetry_snapshot(&st).await) },
    ));

    handles.push(spawn_read_model(
        &state,
        "context_metrics",
        Duration::from_millis(2500),
        |st| async move { Some(crate::context_metrics::snapshot(&st.bus())) },
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

    // Crash log snapshot (not kernel-dependent)
    handles.push(spawn_read_model(
        &state,
        "crashlog",
        Duration::from_millis(5_000),
        |_st| async move { Some(crashlog_snapshot().await) },
    ));

    handles.push(spawn_snappy(&state));

    // Service health aggregator from service.health events
    handles.push(spawn_service_health(&state));

    handles
}

fn spawn_read_model<F, Fut>(
    state: &AppState,
    id: &'static str,
    period: Duration,
    builder: F,
) -> TaskHandle
where
    F: Fn(AppState) -> Fut + Send + Clone + 'static,
    Fut: Future<Output = Option<Value>> + Send + 'static,
{
    let bus = state.bus();
    let bus_for_task = bus.clone();
    let bus_for_cb = bus; // move into callback
    let state = state.clone();
    let name = format!("read_model::{id}");
    crate::tasks::spawn_supervised_with(
        name.clone(),
        move || {
            let bus = bus_for_task.clone();
            let state = state.clone();
            let builder = builder.clone();
            async move {
                let mut tick = time::interval(period);
                loop {
                    tick.tick().await;
                    let state_clone = state.clone();
                    if let Some(value) = builder(state_clone).await {
                        publish_read_model_patch(&bus, id, &value);
                    }
                }
            }
        },
        Some({
            let bus = bus_for_cb.clone();
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
    )
}

pub(crate) async fn crashlog_snapshot() -> Value {
    with_read_model_singleflight("crashlog", || async { build_crashlog_snapshot().await }).await
}

async fn build_crashlog_snapshot() -> Value {
    use tokio::io::AsyncReadExt;
    let crash_root = crate::util::state_dir().join("crash");
    let mut items = Vec::new();
    if let Ok(mut rd) = afs::read_dir(&crash_root).await {
        while let Ok(Some(ent)) = rd.next_entry().await {
            let path = ent.path();
            if path.extension().and_then(|e| e.to_str()) != Some("json") {
                continue;
            }
            let mut f = match afs::File::open(&path).await {
                Ok(f) => f,
                Err(_) => continue,
            };
            let mut buf = Vec::new();
            if f.read_to_end(&mut buf).await.is_ok() {
                if let Ok(mut val) = serde_json::from_slice::<Value>(&buf) {
                    if let Some(obj) = val.as_object_mut() {
                        obj.insert(
                            "file".into(),
                            Value::from(
                                path.file_name()
                                    .unwrap_or_default()
                                    .to_string_lossy()
                                    .to_string(),
                            ),
                        );
                        obj.insert("archived".into(), Value::from(false));
                    }
                    items.push(val);
                }
            }
        }
    }
    let archive = crash_root.join("archive");
    if let Ok(mut rd) = afs::read_dir(&archive).await {
        while let Ok(Some(ent)) = rd.next_entry().await {
            let path = ent.path();
            if path.extension().and_then(|e| e.to_str()) != Some("json") {
                continue;
            }
            let mut f = match afs::File::open(&path).await {
                Ok(f) => f,
                Err(_) => continue,
            };
            let mut buf = Vec::new();
            if f.read_to_end(&mut buf).await.is_ok() {
                if let Ok(mut val) = serde_json::from_slice::<Value>(&buf) {
                    if let Some(obj) = val.as_object_mut() {
                        obj.insert(
                            "file".into(),
                            Value::from(
                                path.file_name()
                                    .unwrap_or_default()
                                    .to_string_lossy()
                                    .to_string(),
                            ),
                        );
                        obj.insert("archived".into(), Value::from(true));
                    }
                    items.push(val);
                }
            }
        }
    }
    items.sort_by(|a, b| b["ts_ms"].as_u64().cmp(&a["ts_ms"].as_u64()));
    serde_json::json!({"items": items, "count": items.len()})
}

static READ_MODEL_CACHE: OnceCell<Mutex<HashMap<String, Value>>> = OnceCell::new();

fn store_read_model_value(id: &str, value: &Value) {
    let map = READ_MODEL_CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    if let Ok(mut guard) = map.lock() {
        guard.insert(id.to_string(), value.clone());
    }
}

#[cfg(test)]
fn remove_cached_read_model_for_test(id: &str) {
    if let Some(map) = READ_MODEL_CACHE.get() {
        if let Ok(mut guard) = map.lock() {
            guard.remove(id);
        }
    }
}

async fn with_read_model_singleflight<F, Fut>(id: &'static str, builder: F) -> Value
where
    F: Fn() -> Fut,
    Fut: Future<Output = Value>,
{
    let mut guard = READ_MODEL_FLIGHTS.begin(id);
    if guard.is_leader() {
        let value = builder().await;
        store_read_model_value(id, &value);
        guard.notify_waiters();
        return value;
    }

    ::metrics::counter!(METRIC_READ_MODEL_COALESCED_WAITERS, 1);
    guard.wait().await;
    if let Some(value) = cached_read_model(id) {
        return value;
    }

    let value = builder().await;
    store_read_model_value(id, &value);
    guard.notify_waiters();
    value
}

pub(crate) fn publish_read_model_patch(bus: &arw_events::Bus, id: &str, value: &Value) {
    publish_read_model_patch_with_previous(bus, id, None, value);
}

pub(crate) fn publish_read_model_patch_with_previous(
    bus: &arw_events::Bus,
    id: &str,
    previous: Option<Value>,
    value: &Value,
) {
    let map = READ_MODEL_CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    let mut guard = map.lock().expect("read model cache lock");
    let prev = match previous {
        Some(prev) => prev,
        None => guard.get(id).cloned().unwrap_or_else(|| json!({})),
    };
    let patch = json_patch::diff(&prev, value);
    if patch.is_empty() {
        guard.insert(id.to_string(), value.clone());
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

fn spawn_service_health(state: &AppState) -> TaskHandle {
    let bus = state.bus();
    let bus_for_task = bus.clone();
    let bus_for_cb = bus;
    crate::tasks::spawn_supervised_with(
        "read_model::service_health",
        move || {
            let bus = bus_for_task.clone();
            async move {
                let mut rx = bus.subscribe_filtered(
                    vec![arw_topics::TOPIC_SERVICE_HEALTH.to_string()],
                    Some(64),
                );
                let mut history: std::collections::VecDeque<Value> =
                    std::collections::VecDeque::with_capacity(50);
                loop {
                    if let Ok(env) = rx.recv().await {
                        let mut item = env.payload;
                        if let Some(obj) = item.as_object_mut() {
                            obj.insert("time".into(), json!(env.time));
                        }
                        if history.len() >= 50 {
                            history.pop_front();
                        }
                        history.push_back(item.clone());
                        let value = json!({
                            "last": item,
                            "history": history,
                        });
                        publish_read_model_patch(&bus, "service_health", &value);
                    }
                }
            }
        },
        Some({
            let bus = bus_for_cb.clone();
            move |restarts| {
                if restarts >= 5 {
                    let payload = serde_json::json!({
                        "status": "degraded",
                        "component": "read_model::service_health",
                        "reason": "task_thrashing",
                        "restarts_window": restarts,
                        "window_secs": 30,
                    });
                    bus.publish(arw_topics::TOPIC_SERVICE_HEALTH, &payload);
                }
            }
        }),
    )
}

fn spawn_snappy(state: &AppState) -> TaskHandle {
    let state_for_task = state.clone();
    let bus_for_cb = state.bus();
    crate::tasks::spawn_supervised_with(
        "read_model::snappy",
        move || {
            let state = state_for_task.clone();
            async move {
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
            }
        },
        Some({
            let bus = bus_for_cb.clone();
            move |restarts| {
                if restarts >= 5 {
                    let payload = serde_json::json!({
                        "status": "degraded",
                        "component": "read_model::snappy",
                        "reason": "task_thrashing",
                        "restarts_window": restarts,
                        "window_secs": 30,
                    });
                    bus.publish(arw_topics::TOPIC_SERVICE_HEALTH, &payload);
                }
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

pub(crate) async fn screenshots_snapshot() -> Value {
    with_read_model_singleflight("screenshots_index", || async {
        build_screenshots_snapshot().await
    })
    .await
}

async fn build_screenshots_snapshot() -> Value {
    let state_dir = crate::util::state_dir();
    let base_dir = state_dir.join("screenshots");
    let generated = Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true);
    if !base_dir.exists() {
        return json!({
            "generated": generated,
            "total_sources": 0,
            "total_langs": 0,
            "items": [],
            "limit": MAX_SCREENSHOTS_INDEX,
            "more_sources": false,
        });
    }

    let max_items = env_u64("ARW_SCREENSHOTS_INDEX_LIMIT", MAX_SCREENSHOTS_INDEX as u64)
        .max(1)
        .min(512) as usize;
    let text_limit = env_u64(
        "ARW_SCREENSHOTS_TEXT_LIMIT",
        MAX_SCREENSHOTS_TEXT_LEN as u64,
    )
    .max(32)
    .min(4096) as usize;
    let preview_limit = env_u64(
        "ARW_SCREENSHOTS_PREVIEW_LIMIT",
        MAX_SCREENSHOTS_PREVIEW_LEN as u64,
    )
    .max(16)
    .min(text_limit as u64) as usize;
    let langs_limit = env_u64(
        "ARW_SCREENSHOTS_LANGS_LIMIT",
        MAX_SCREENSHOTS_LANGS_PER_SOURCE as u64,
    )
    .max(1)
    .min(16) as usize;

    let state_dir_clone = state_dir.clone();
    let base_clone = base_dir.clone();
    let result = tokio::task::spawn_blocking(move || {
        collect_screenshots_blocking(
            &state_dir_clone,
            &base_clone,
            max_items,
            text_limit,
            preview_limit,
            langs_limit,
        )
    })
    .await
    .unwrap_or_else(|_| ScreenshotsIndex::default());

    json!({
        "generated": generated,
        "total_sources": result.total_sources,
        "total_langs": result.total_langs,
        "more_sources": result.more_sources,
        "limit": max_items,
        "items": result.items,
    })
}

#[derive(Default)]
struct ScreenshotsIndex {
    total_sources: usize,
    total_langs: usize,
    more_sources: bool,
    items: Vec<Value>,
}

fn collect_screenshots_blocking(
    state_dir: &Path,
    base_dir: &Path,
    max_items: usize,
    text_limit: usize,
    preview_limit: usize,
    langs_limit: usize,
) -> ScreenshotsIndex {
    use std::collections::HashMap;

    let mut aggregates: HashMap<String, ScreenshotAggregate> = HashMap::new();
    let mut total_langs = 0usize;

    for entry in WalkDir::new(base_dir).follow_links(false).into_iter() {
        let entry = match entry {
            Ok(ent) => ent,
            Err(_) => continue,
        };
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let file_name = match path.file_name().and_then(|s| s.to_str()) {
            Some(name) => name,
            None => continue,
        };
        if !file_name.ends_with(".json") {
            continue;
        }
        let (base_stem, lang_from_name) = match parse_sidecar_filename(file_name) {
            Some(parts) => parts,
            None => continue,
        };

        let json_bytes = match std::fs::read(path) {
            Ok(bytes) => bytes,
            Err(_) => continue,
        };
        let sidecar: Value = match serde_json::from_slice(&json_bytes) {
            Ok(val) => val,
            Err(_) => continue,
        };

        let lang = sidecar
            .get("lang")
            .and_then(|v| v.as_str())
            .unwrap_or(lang_from_name)
            .trim()
            .to_string();

        let generated_at = sidecar
            .get("generated_at")
            .and_then(|v| v.as_str())
            .map(|s| s.trim().to_string())
            .or_else(|| {
                std::fs::metadata(path)
                    .ok()
                    .and_then(|m| m.modified().ok())
                    .and_then(system_time_to_rfc3339)
            });

        let source_path_string = sidecar
            .get("source_path")
            .and_then(|v| v.as_str())
            .map(|s| s.trim().to_string())
            .or_else(|| guess_source_path(path, base_stem).map(|p| normalized_path_string(&p)));

        let source_path_display = source_path_string.clone().unwrap_or_else(|| {
            normalized_path_string(&path.parent().unwrap_or(base_dir).join(base_stem))
        });

        let source_rel = source_path_string
            .as_deref()
            .and_then(|s| relative_path_string(state_dir, Path::new(s)))
            .or_else(|| {
                relative_path_string(base_dir, &path.parent().unwrap_or(base_dir).join(base_stem))
            });

        let ocr_rel = relative_path_string(state_dir, path);
        let ocr_path = normalized_path_string(path);

        let text_raw = sidecar
            .get("text")
            .and_then(|v| v.as_str())
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string());

        let (text, text_truncated) = match text_raw {
            Some(ref text) => truncate_to_chars(text, text_limit),
            None => (String::new(), false),
        };
        let text_opt = if text.is_empty() {
            None
        } else {
            Some(text.clone())
        };
        let (preview, _) = if text.is_empty() {
            (String::new(), false)
        } else {
            truncate_to_chars(&text, preview_limit)
        };
        let preview_opt = if preview.is_empty() {
            None
        } else {
            Some(preview)
        };

        let lang_entry = ScreenshotLangEntry {
            lang,
            ocr_path,
            ocr_rel,
            generated_at,
            generated_sort: None,
            text: text_opt,
            text_truncated,
            text_preview: preview_opt,
        };

        let key = source_path_display.clone();
        let aggregate = aggregates.entry(key.clone()).or_insert_with(|| {
            ScreenshotAggregate::new(source_path_display.clone(), source_rel.clone())
        });
        aggregate.push_lang(lang_entry);
        total_langs += 1;
    }

    if aggregates.is_empty() {
        return ScreenshotsIndex::default();
    }

    let total_sources_all = aggregates.len();
    let mut items: Vec<_> = aggregates.into_iter().map(|(_, agg)| agg).collect();
    items.sort_by(|a, b| {
        compare_optional_datetime(&b.latest, &a.latest)
            .then_with(|| a.source_path.cmp(&b.source_path))
    });

    let more_sources = items.len() > max_items;
    if items.len() > max_items {
        items.truncate(max_items);
    }

    let items_json: Vec<Value> = items
        .into_iter()
        .map(|agg| agg.into_value(langs_limit))
        .collect();

    ScreenshotsIndex {
        total_sources: total_sources_all,
        total_langs,
        more_sources,
        items: items_json,
    }
}

fn compare_optional_datetime(
    a: &Option<DateTime<Utc>>,
    b: &Option<DateTime<Utc>>,
) -> std::cmp::Ordering {
    match (a, b) {
        (Some(a), Some(b)) => a.cmp(b),
        (Some(_), None) => std::cmp::Ordering::Greater,
        (None, Some(_)) => std::cmp::Ordering::Less,
        (None, None) => std::cmp::Ordering::Equal,
    }
}

struct ScreenshotAggregate {
    source_path: String,
    source_rel: Option<String>,
    langs: Vec<ScreenshotLangEntry>,
    latest: Option<DateTime<Utc>>,
}

impl ScreenshotAggregate {
    fn new(source_path: String, source_rel: Option<String>) -> Self {
        Self {
            source_path,
            source_rel,
            langs: Vec::new(),
            latest: None,
        }
    }

    fn push_lang(&mut self, mut entry: ScreenshotLangEntry) {
        if entry.text_truncated {
            // already truncated in caller
        }
        if entry.generated_at.is_none() && entry.ocr_rel.is_none() {
            // nothing special
        }
        if let Some(ref ts) = entry.generated_at {
            if let Ok(dt) = DateTime::parse_from_rfc3339(ts) {
                let utc = dt.with_timezone(&Utc);
                if self.latest.map_or(true, |cur| utc > cur) {
                    self.latest = Some(utc);
                }
                entry.generated_sort = Some(utc);
            }
        }
        self.langs.push(entry);
    }

    fn into_value(mut self, langs_limit: usize) -> Value {
        self.langs.sort_by(|a, b| {
            compare_optional_datetime(&b.generated_sort, &a.generated_sort)
                .then_with(|| a.lang.cmp(&b.lang))
        });
        let more_langs = self.langs.len() > langs_limit;
        if self.langs.len() > langs_limit {
            self.langs.truncate(langs_limit);
        }
        let langs_json: Vec<Value> = self
            .langs
            .into_iter()
            .map(|lang| lang.into_value())
            .collect();

        json!({
            "source_path": self.source_path,
            "source_rel": self.source_rel,
            "latest_generated_at": self.latest.map(|dt| dt.to_rfc3339_opts(SecondsFormat::Millis, true)),
            "lang_count": langs_json.len(),
            "more_langs": more_langs,
            "langs": langs_json,
        })
    }
}

struct ScreenshotLangEntry {
    lang: String,
    ocr_path: String,
    ocr_rel: Option<String>,
    generated_at: Option<String>,
    generated_sort: Option<DateTime<Utc>>,
    text: Option<String>,
    text_truncated: bool,
    text_preview: Option<String>,
}

impl ScreenshotLangEntry {
    fn into_value(self) -> Value {
        let mut obj = serde_json::Map::new();
        obj.insert("lang".into(), json!(self.lang));
        obj.insert("ocr_path".into(), json!(self.ocr_path));
        if let Some(rel) = self.ocr_rel {
            obj.insert("ocr_rel".into(), json!(rel));
        }
        if let Some(ts) = self.generated_at {
            obj.insert("generated_at".into(), json!(ts));
        }
        if let Some(text) = self.text {
            obj.insert("text".into(), json!(text));
            obj.insert("text_truncated".into(), json!(self.text_truncated));
        }
        if let Some(preview) = self.text_preview {
            obj.insert("text_preview".into(), json!(preview));
        }
        Value::Object(obj)
    }
}

fn parse_sidecar_filename(name: &str) -> Option<(&str, &str)> {
    let idx = name.rfind(".ocr.")?;
    let base = &name[..idx];
    let rest = &name[(idx + 5)..];
    let lang = rest.strip_suffix(".json")?;
    if base.is_empty() || lang.is_empty() {
        return None;
    }
    Some((base, lang))
}

fn guess_source_path(sidecar: &Path, base_stem: &str) -> Option<PathBuf> {
    let parent = sidecar.parent()?;
    for ext in SCREENSHOT_CAPTURE_EXTENSIONS {
        let candidate = parent.join(format!("{base_stem}.{ext}"));
        if candidate.exists() {
            return Some(candidate);
        }
    }
    None
}

fn relative_path_string(base: &Path, path: &Path) -> Option<String> {
    path.strip_prefix(base)
        .ok()
        .map(|rel| normalized_path_string(rel))
}

fn normalized_path_string(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

fn truncate_to_chars(input: &str, max_chars: usize) -> (String, bool) {
    let mut buf = String::new();
    let mut truncated = false;
    for (idx, ch) in input.chars().enumerate() {
        if idx >= max_chars {
            truncated = true;
            break;
        }
        buf.push(ch);
    }
    (buf, truncated)
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

async fn notes_details(project_root: &Path) -> NotesSnapshot {
    let mut snapshot = NotesSnapshot::default();
    let notes = project_root.join("NOTES.md");

    match afs::metadata(&notes).await {
        Ok(meta) => {
            snapshot.modified = meta.modified().ok().and_then(system_time_to_rfc3339);
            snapshot.bytes = Some(meta.len());
        }
        Err(_) => return snapshot,
    }

    match afs::read(&notes).await {
        Ok(bytes) => {
            let mut hasher = Sha256::new();
            hasher.update(&bytes);
            snapshot.sha256 = Some(format!("{:x}", hasher.finalize()));

            let mut text = String::from_utf8_lossy(&bytes).to_string();
            if text.len() > MAX_NOTES_LEN {
                snapshot.truncated = true;
                text.truncate(MAX_NOTES_LEN);
            }
            snapshot.content = Some(text);
        }
        Err(_) => {
            // Keep defaults when notes cannot be read; metadata already captured when available.
        }
    }

    snapshot
}

async fn collect_tree(
    project_root: &Path,
    paths: &mut BTreeMap<String, Vec<Value>>,
    truncated: &mut BTreeMap<String, usize>,
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
            let overflow = entries.len() - MAX_ENTRIES_PER_DIR;
            entries.truncate(MAX_ENTRIES_PER_DIR);
            truncated.insert(rel.clone(), overflow);
        }
        paths.insert(rel, entries);
    }
    Ok(())
}

pub(crate) async fn projects_snapshot() -> Value {
    with_read_model_singleflight("projects", || async {
        let root_dir = projects_root_dir();
        projects_snapshot_at(&root_dir).await
    })
    .await
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
            let notes = notes_details(&project_root).await;
            let mut tree_paths = BTreeMap::new();
            let mut truncated_dirs = BTreeMap::new();
            let mut digest = Sha256::new();
            let _ = collect_tree(
                &project_root,
                &mut tree_paths,
                &mut truncated_dirs,
                &mut digest,
            )
            .await;
            let tree_value = tree_paths
                .into_iter()
                .map(|(k, v)| (k, Value::Array(v)))
                .collect::<serde_json::Map<String, Value>>();
            let truncated_value = if truncated_dirs.is_empty() {
                Value::Null
            } else {
                Value::Object(
                    truncated_dirs
                        .into_iter()
                        .map(|(k, v)| (k, Value::from(v as u64)))
                        .collect(),
                )
            };
            let digest_hex = format!("{:x}", digest.finalize());
            let snapshots = match project_snapshots::list_snapshots(&project_root, &name, 5).await {
                Ok(list) => list,
                Err(_) => Vec::new(),
            };
            let snapshots_count = snapshots.len() as u64;
            let snapshots_items =
                serde_json::to_value(&snapshots).unwrap_or_else(|_| Value::Array(Vec::new()));
            let snapshots_latest = snapshots
                .first()
                .cloned()
                .map(|s| serde_json::to_value(s).unwrap_or(Value::Null))
                .unwrap_or(Value::Null);
            items.push(json!({
                "name": name,
                "notes": {
                    "modified": notes.modified,
                    "bytes": notes.bytes,
                    "sha256": notes.sha256,
                    "content": notes.content,
                    "truncated": notes.truncated,
                },
                "tree": {
                    "digest": digest_hex,
                    "paths": Value::Object(tree_value),
                    "truncated": truncated_value,
                },
                "snapshots": {
                    "count": snapshots_count,
                    "latest": snapshots_latest,
                    "items": snapshots_items,
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
    let state = state.clone();
    with_read_model_singleflight("policy_leases", move || {
        let state = state.clone();
        async move {
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
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::env;
    use arw_events::Envelope;
    use arw_policy::PolicyEngine;
    use arw_topics as topics;
    use json_patch::Patch;
    use serde_json::Value;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;
    use tempfile::tempdir;
    use tokio::sync::Notify;
    use tokio::time::timeout;

    #[tokio::test]
    async fn notes_snapshot_includes_sha_and_content() {
        let dir = tempdir().unwrap();
        let project = dir.path();
        tokio::fs::create_dir_all(project).await.unwrap();
        let notes_path = project.join("NOTES.md");
        let body = "hello notes";
        tokio::fs::write(&notes_path, body).await.unwrap();

        let snapshot = notes_details(project).await;
        assert_eq!(snapshot.content.as_deref(), Some(body));
        assert_eq!(snapshot.bytes, Some(body.len() as u64));
        assert!(!snapshot.truncated);

        let mut hasher = Sha256::new();
        hasher.update(body.as_bytes());
        let expected = format!("{:x}", hasher.finalize());
        assert_eq!(snapshot.sha256, Some(expected));
    }

    #[tokio::test]
    async fn notes_snapshot_marks_truncation() {
        let dir = tempdir().unwrap();
        let project = dir.path();
        tokio::fs::create_dir_all(project).await.unwrap();
        let notes_path = project.join("NOTES.md");
        let long_body = "x".repeat(MAX_NOTES_LEN + 10);
        tokio::fs::write(&notes_path, &long_body).await.unwrap();

        let snapshot = notes_details(project).await;
        assert!(snapshot.truncated);
        assert_eq!(
            snapshot.content.as_ref().map(|s| s.len()),
            Some(MAX_NOTES_LEN)
        );

        let mut hasher = Sha256::new();
        hasher.update(long_body.as_bytes());
        let expected = format!("{:x}", hasher.finalize());
        assert_eq!(snapshot.sha256, Some(expected));
    }

    #[tokio::test]
    async fn tree_snapshot_tracks_overflow_counts() {
        let dir = tempdir().unwrap();
        let project_root = dir.path().join("proj");
        tokio::fs::create_dir_all(&project_root).await.unwrap();
        for idx in 0..(MAX_ENTRIES_PER_DIR + 3) {
            let file = project_root.join(format!("file_{idx:03}.txt"));
            tokio::fs::write(file, b"ok").await.unwrap();
        }

        let snapshot = projects_snapshot_at(dir.path()).await;
        let items = snapshot["items"].as_array().expect("items array");
        let tree = &items[0]["tree"];
        let truncated = tree["truncated"].as_object().expect("truncated object");
        assert_eq!(truncated.get("").and_then(|v| v.as_u64()), Some(3));

        let root_entries = tree["paths"]
            .as_object()
            .unwrap()
            .get("")
            .unwrap()
            .as_array()
            .unwrap();
        assert_eq!(root_entries.len(), MAX_ENTRIES_PER_DIR);
    }

    async fn build_state(path: &std::path::Path, env_guard: &mut env::EnvGuard) -> AppState {
        env_guard.set("ARW_DEBUG", "1");
        crate::util::reset_state_dir_for_tests();
        env_guard.set("ARW_STATE_DIR", path.display().to_string());
        let bus = arw_events::Bus::new_with_replay(32, 32);
        let kernel = arw_kernel::Kernel::open(path).expect("init kernel");
        let policy = PolicyEngine::load_from_env();
        let policy_handle = crate::policy::PolicyHandle::new(policy, bus.clone());
        let host: Arc<dyn arw_wasi::ToolHost> = Arc::new(arw_wasi::NoopHost);
        AppState::builder(bus, kernel, policy_handle, host, true)
            .with_sse_capacity(16)
            .build()
            .await
    }

    #[tokio::test]
    async fn snappy_publishes_patch_and_notice() {
        let temp = tempdir().expect("tempdir");
        let mut ctx = crate::test_support::begin_state_env(temp.path());
        let state = build_state(temp.path(), &mut ctx.env).await;

        ctx.env.apply([
            ("ARW_SNAPPY_PUBLISH_MS", Some("10")),
            ("ARW_SNAPPY_FULL_RESULT_P95_MS", Some("15")),
            ("ARW_SNAPPY_PROTECTED_ENDPOINTS", Some("/state/")),
            ("ARW_SNAPPY_DETAIL_EVERY", Some("0")),
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

    #[tokio::test]
    async fn publish_patch_after_cached_snapshot() {
        remove_cached_read_model_for_test("test-patch-after-cache");
        let bus = arw_events::Bus::new_with_replay(8, 8);
        let mut rx =
            bus.subscribe_filtered(vec![topics::TOPIC_READMODEL_PATCH.to_string()], Some(4));

        publish_read_model_patch_with_previous(
            &bus,
            "test-patch-after-cache",
            None,
            &json!({"count": 0}),
        );

        // Drain initial emission so we can assert on the scenario under test.
        let _baseline: Envelope = timeout(Duration::from_millis(200), rx.recv())
            .await
            .expect("baseline timeout")
            .expect("baseline patch");

        let prev = super::cached_read_model("test-patch-after-cache");
        let next = json!({"count": 1});
        super::store_read_model_value("test-patch-after-cache", &next);
        publish_read_model_patch_with_previous(&bus, "test-patch-after-cache", prev, &next);

        let env: Envelope = timeout(Duration::from_millis(200), rx.recv())
            .await
            .expect("patch timeout")
            .expect("patch event");
        assert_eq!(env.kind, topics::TOPIC_READMODEL_PATCH);
        assert_eq!(env.payload["id"].as_str(), Some("test-patch-after-cache"));
        let patch_items = env.payload["patch"].as_array().expect("patch array");
        assert!(!patch_items.is_empty(), "expected non-empty patch diff");
    }

    #[tokio::test]
    async fn read_model_singleflight_coalesces_builders() {
        remove_cached_read_model_for_test("test-singleflight");
        let counter = Arc::new(AtomicUsize::new(0));
        let started = Arc::new(AtomicUsize::new(0));
        let ready = Arc::new(Notify::new());

        async fn call(
            counter: Arc<AtomicUsize>,
            started: Arc<AtomicUsize>,
            ready: Arc<Notify>,
        ) -> Value {
            let order = started.fetch_add(1, Ordering::SeqCst) + 1;
            if order == 3 {
                ready.notify_waiters();
            }
            super::with_read_model_singleflight("test-singleflight", || {
                let counter = counter.clone();
                let ready = ready.clone();
                async move {
                    ready.notified().await;
                    counter.fetch_add(1, Ordering::SeqCst);
                    serde_json::json!({"ok": true})
                }
            })
            .await
        }

        let (v1, v2, v3) = tokio::join!(
            call(counter.clone(), started.clone(), ready.clone()),
            call(counter.clone(), started.clone(), ready.clone()),
            call(counter.clone(), started.clone(), ready.clone()),
        );

        assert_eq!(counter.load(Ordering::SeqCst), 1);
        assert_eq!(v1, serde_json::json!({"ok": true}));
        assert_eq!(v1, v2);
        assert_eq!(v1, v3);

        remove_cached_read_model_for_test("test-singleflight");
    }

    #[tokio::test]
    async fn screenshots_snapshot_collects_sidecars() {
        use std::collections::HashSet;

        remove_cached_read_model_for_test("screenshots_index");
        let temp = tempdir().expect("tempdir");
        let mut ctx = crate::test_support::begin_state_env(temp.path());
        ctx.env
            .set("ARW_STATE_DIR", temp.path().display().to_string());

        let shots_dir = temp.path().join("screenshots/2025/09/30");
        std::fs::create_dir_all(&shots_dir).unwrap();
        let shot_path = shots_dir.join("example.png");
        std::fs::write(&shot_path, b"fake").unwrap();

        let sidecar_eng = shots_dir.join("example.ocr.eng.json");
        std::fs::write(
            &sidecar_eng,
            serde_json::to_vec(&json!({
                "source_path": shot_path.to_string_lossy(),
                "lang": "eng",
                "generated_at": "2025-09-29T12:00:00Z",
                "text": "English summary of the screenshot content."
            }))
            .unwrap(),
        )
        .unwrap();

        let sidecar_de = shots_dir.join("example.ocr.de.json");
        std::fs::write(
            &sidecar_de,
            serde_json::to_vec(&json!({
                "source_path": shot_path.to_string_lossy(),
                "lang": "de",
                "generated_at": "2025-09-30T08:30:00Z",
                "text": "Deutsche Zusammenfassung des Screenshots."
            }))
            .unwrap(),
        )
        .unwrap();

        let snapshot = build_screenshots_snapshot().await;
        assert_eq!(snapshot["total_sources"].as_u64(), Some(1));
        assert_eq!(snapshot["total_langs"].as_u64(), Some(2));
        let items = snapshot["items"].as_array().expect("items array");
        assert_eq!(items.len(), 1);
        let langs = items[0]["langs"].as_array().expect("langs array");
        assert_eq!(langs.len(), 2);
        let langs_set: HashSet<_> = langs.iter().filter_map(|l| l["lang"].as_str()).collect();
        assert!(langs_set.contains("eng"));
        assert!(langs_set.contains("de"));
        assert_eq!(items[0]["more_langs"].as_bool(), Some(false));
    }

    #[test]
    fn snappy_detail_emission_respects_interval() {
        let _env_guard = env::guard();
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
