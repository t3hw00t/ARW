use anyhow::Result as AnyResult;
use arw_core::recipes;
use chrono::{DateTime, SecondsFormat, Utc};
use once_cell::sync::{Lazy, OnceCell};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashMap};
use std::future::Future;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime};
use tokio::{fs as afs, task::spawn_blocking, time};
use tracing::warn;
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
use crate::{
    memory_service, metrics, project_snapshots, state_observer, tasks::TaskHandle, training, util,
    AppState,
};
use arw_kernel::ActionListOptions;
use arw_kernel::KernelSession;
use arw_topics as topics;

#[derive(Clone)]
pub struct MemoryRecentBundle {
    pub snapshot: Value,
    pub modular: Value,
    pub generated: String,
    pub generated_ms: u64,
    pub lane_snapshots: BTreeMap<String, Value>,
}

pub(crate) const MEMORY_LANE_IDS: [(&str, &str); 6] = [
    ("memory_lane_short_term", "short_term"),
    ("memory_lane_ephemeral", "ephemeral"),
    ("memory_lane_episodic", "episodic"),
    ("memory_lane_episodic_summary", "episodic_summary"),
    ("memory_lane_semantic", "semantic"),
    ("memory_lane_profile", "profile"),
];

pub(crate) fn summarize_memory_recent_items(items: &[Value]) -> Value {
    let mut lane_counts: BTreeMap<String, usize> = BTreeMap::new();
    let mut modular_recent: Vec<Value> = Vec::new();
    let mut modular_pending = 0usize;
    let mut modular_blocked = 0usize;

    for item in items {
        if let Some(lane) = item.get("lane").and_then(|v| v.as_str()) {
            *lane_counts.entry(lane.to_string()).or_insert(0) += 1;
        }

        let Some(value_obj) = item.get("value").and_then(|v| v.as_object()) else {
            continue;
        };
        let Some(payload_kind) = value_obj.get("payload_kind").and_then(|v| v.as_str()) else {
            continue;
        };

        let lifecycle_stage = value_obj
            .get("lifecycle")
            .and_then(|v| v.get("stage"))
            .and_then(|v| v.as_str())
            .unwrap_or("accepted");
        let validation_gate = value_obj
            .get("lifecycle")
            .and_then(|v| v.get("validation_gate"))
            .and_then(|v| v.as_str())
            .unwrap_or("skipped");

        match lifecycle_stage {
            "pending_human_review" => modular_pending += 1,
            "blocked" => modular_blocked += 1,
            _ => {}
        }

        let mut recent_entry = serde_json::Map::new();
        recent_entry.insert("id".into(), item.get("id").cloned().unwrap_or(Value::Null));
        recent_entry.insert(
            "lane".into(),
            item.get("lane").cloned().unwrap_or(Value::Null),
        );
        recent_entry.insert(
            "turn_id".into(),
            value_obj.get("turn_id").cloned().unwrap_or(Value::Null),
        );
        recent_entry.insert(
            "agent_id".into(),
            value_obj.get("agent_id").cloned().unwrap_or(Value::Null),
        );
        recent_entry.insert(
            "intent".into(),
            value_obj.get("intent").cloned().unwrap_or(Value::Null),
        );
        recent_entry.insert(
            "payload_kind".into(),
            Value::String(payload_kind.to_string()),
        );
        recent_entry.insert(
            "lifecycle_stage".into(),
            Value::String(lifecycle_stage.to_string()),
        );
        recent_entry.insert(
            "validation_gate".into(),
            Value::String(validation_gate.to_string()),
        );
        recent_entry.insert(
            "confidence".into(),
            value_obj.get("confidence").cloned().unwrap_or(Value::Null),
        );
        recent_entry.insert(
            "created_ms".into(),
            value_obj.get("created_ms").cloned().unwrap_or(Value::Null),
        );
        recent_entry.insert(
            "payload_summary".into(),
            value_obj
                .get("payload_summary")
                .cloned()
                .unwrap_or(Value::Null),
        );
        if payload_kind == "tool_invocation" {
            recent_entry.insert(
                "invocation_id".into(),
                value_obj
                    .get("invocation_id")
                    .cloned()
                    .unwrap_or(Value::Null),
            );
            recent_entry.insert(
                "tool_id".into(),
                value_obj.get("tool_id").cloned().unwrap_or(Value::Null),
            );
            recent_entry.insert(
                "requested_by".into(),
                value_obj
                    .get("requested_by")
                    .cloned()
                    .unwrap_or(Value::Null),
            );
            recent_entry.insert(
                "result_status".into(),
                value_obj
                    .get("result_status")
                    .cloned()
                    .unwrap_or(Value::Null),
            );
        }
        if let Some(summary_excerpt) = item
            .get("extra")
            .and_then(|v| v.get("summary_excerpt"))
            .cloned()
        {
            recent_entry.insert("summary_excerpt".into(), summary_excerpt);
        }

        modular_recent.push(Value::Object(recent_entry));
    }

    if modular_recent.len() > 50 {
        modular_recent.truncate(50);
    }

    json!({
        "lanes": lane_counts,
        "modular": {
            "recent": modular_recent,
            "pending_human_review": modular_pending,
            "blocked": modular_blocked,
        }
    })
}

pub(crate) fn build_memory_recent_bundle(mut items: Vec<Value>) -> MemoryRecentBundle {
    memory_service::attach_memory_ptrs(&mut items);
    let summary = summarize_memory_recent_items(&items);
    let now = Utc::now();
    let generated = now.to_rfc3339_opts(SecondsFormat::Millis, true);
    let generated_ms = now.timestamp_millis().max(0) as u64;
    let snapshot = json!({
        "items": items,
        "summary": summary,
        "generated": generated.clone(),
        "generated_ms": generated_ms,
    });
    let modular = snapshot
        .get("summary")
        .and_then(|v| v.get("modular"))
        .cloned()
        .unwrap_or_else(|| {
            json!({
                "recent": [],
                "pending_human_review": 0,
                "blocked": 0,
            })
        });
    let modular_snapshot = json!({
        "generated": generated.clone(),
        "generated_ms": generated_ms,
        "pending_human_review": modular
            .get("pending_human_review")
            .and_then(|v| v.as_u64())
            .unwrap_or(0),
        "blocked": modular
            .get("blocked")
            .and_then(|v| v.as_u64())
            .unwrap_or(0),
        "recent": modular.get("recent").cloned().unwrap_or_else(|| json!([])),
    });
    let mut lane_snapshots = BTreeMap::new();
    for (_, lane) in MEMORY_LANE_IDS.iter() {
        let filtered: Vec<Value> = snapshot
            .get("items")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter(|item| {
                        item.get("lane")
                            .and_then(|v| v.as_str())
                            .map(|l| l == *lane)
                            .unwrap_or(false)
                    })
                    .cloned()
                    .collect()
            })
            .unwrap_or_default();
        lane_snapshots.insert(
            (*lane).to_string(),
            json!({
                "lane": lane,
                "items": filtered,
                "generated": generated.clone(),
                "generated_ms": generated_ms,
            }),
        );
    }
    MemoryRecentBundle {
        snapshot,
        modular: modular_snapshot,
        generated,
        generated_ms,
        lane_snapshots,
    }
}

pub(crate) fn publish_memory_bundle(bus: &arw_events::Bus, bundle: &MemoryRecentBundle) {
    publish_read_model_patch(bus, "memory_recent", &bundle.snapshot);
    publish_read_model_patch(bus, "memory_modular_review", &bundle.modular);
    for (id, lane) in MEMORY_LANE_IDS.iter() {
        if let Some(snapshot) = bundle.lane_snapshots.get(*lane) {
            publish_read_model_patch(bus, id, snapshot);
        } else {
            let empty = json!({
                "lane": lane,
                "items": [],
                "generated": bundle.generated,
                "generated_ms": bundle.generated_ms,
            });
            publish_read_model_patch(bus, id, &empty);
        }
    }
}

pub(crate) fn start_read_models(state: AppState) -> Vec<TaskHandle> {
    if util::smoke_profile_enabled() {
        return start_read_models_smoke(state);
    }

    let mut handles = Vec::new();
    handles.push(spawn_read_model(
        &state,
        "logic_units",
        Duration::from_millis(1500),
        |st| async move {
            if !st.kernel_enabled() {
                return None;
            }
            let kernel = st.kernel().clone();
            kernel_session_exec(kernel, |session| session.list_logic_units(200))
                .await
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
            let kernel = st.kernel().clone();
            kernel_session_exec(kernel, |session| session.list_orchestrator_jobs(200))
                .await
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
            let kernel = st.kernel().clone();
            kernel_session_exec(kernel, |session| {
                let items = session.list_recent_memory(None, 200)?;
                Ok(build_memory_recent_bundle(items).snapshot)
            })
            .await
        },
    ));

    handles.push(spawn_read_model(
        &state,
        "kernel_pool_wait",
        Duration::from_millis(2000),
        |st| async move {
            if !st.kernel_enabled() {
                return None;
            }
            let (count, total_ms) = st.kernel().pool_wait_stats();
            let avg_ms = if count > 0 {
                total_ms / count as f64
            } else {
                0.0
            };
            Some(json!({
                "wait_count": count,
                "wait_total_ms": total_ms,
                "wait_avg_ms": avg_ms,
            }))
        },
    ));

    for (id, lane) in MEMORY_LANE_IDS.iter() {
        let lane_name = (*lane).to_string();
        handles.push(spawn_read_model(
            &state,
            id,
            Duration::from_millis(2500),
            move |st| {
                let lane_clone = lane_name.clone();
                async move {
                    if !st.kernel_enabled() {
                        return None;
                    }
                    let kernel = st.kernel().clone();
                    kernel_session_exec(kernel, move |session| {
                        let items = session.list_recent_memory(Some(&lane_clone), 200)?;
                        let bundle = build_memory_recent_bundle(items);
                        let snapshot = bundle
                            .lane_snapshots
                            .get(&lane_clone)
                            .cloned()
                            .unwrap_or_else(|| {
                                json!({
                                    "lane": lane_clone,
                                    "items": [],
                                    "generated": bundle.generated,
                                    "generated_ms": bundle.generated_ms,
                                })
                            });
                        Ok(snapshot)
                    })
                    .await
                }
            },
        ));
    }

    handles.push(spawn_read_model(
        &state,
        "memory_modular_review",
        Duration::from_millis(2500),
        |st| async move {
            if !st.kernel_enabled() {
                return None;
            }
            st.kernel()
                .list_recent_memory_async(None, 200)
                .await
                .ok()
                .map(|items| build_memory_recent_bundle(items).modular)
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
            let (items, version) = crate::api::state::build_episode_rollups(&st, 1000).await;
            let items: Vec<Value> = items.into_iter().map(|ep| ep.into_value()).collect();
            Some(json!({ "version": version, "items": items }))
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
        "recipes_gallery",
        Duration::from_millis(5000),
        |_st| async move { Some(recipes_snapshot().await) },
    ));

    handles.push(spawn_read_model(
        &state,
        "route_stats",
        Duration::from_millis(4000),
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
        Duration::from_millis(5000),
        |st| async move {
            let (version, tasks) = st.metrics().tasks_snapshot_with_version();
            Some(json!({ "version": version, "tasks": tasks }))
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
                "ttl_seconds": crate::cluster::SNAPSHOT_TTL_SECONDS,
            }))
        },
    ));

    handles.push(spawn_read_model(
        &state,
        "runtime_bundles",
        Duration::from_millis(8000),
        |st| async move { Some(st.runtime_bundles().snapshot().await) },
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

    let observations_last = Arc::new(AtomicU64::new(u64::MAX));
    handles.push(spawn_read_model(
        &state,
        "observations",
        Duration::from_millis(1500),
        {
            let last = observations_last.clone();
            move |_st| {
                let last = last.clone();
                async move {
                    let current = state_observer::observations_version_value();
                    if last.load(Ordering::Relaxed) == current {
                        return None;
                    }
                    let (version, items) =
                        state_observer::observations_snapshot(None, None, None).await;
                    last.store(version, Ordering::Relaxed);
                    Some(json!({
                        "version": version,
                        "items": items,
                    }))
                }
            }
        },
    ));

    let beliefs_last = Arc::new(AtomicU64::new(u64::MAX));
    handles.push(spawn_read_model(
        &state,
        "beliefs",
        Duration::from_millis(1500),
        {
            let last = beliefs_last.clone();
            move |_st| {
                let last = last.clone();
                async move {
                    let current = state_observer::beliefs_version_value();
                    if last.load(Ordering::Relaxed) == current {
                        return None;
                    }
                    let (version, items) = state_observer::beliefs_snapshot().await;
                    last.store(version, Ordering::Relaxed);
                    Some(json!({
                        "version": version,
                        "items": items,
                    }))
                }
            }
        },
    ));

    let intents_last = Arc::new(AtomicU64::new(u64::MAX));
    handles.push(spawn_read_model(
        &state,
        "intents",
        Duration::from_millis(1500),
        {
            let last = intents_last.clone();
            move |_st| {
                let last = last.clone();
                async move {
                    let current = state_observer::intents_version_value();
                    if last.load(Ordering::Relaxed) == current {
                        return None;
                    }
                    let (version, items) = state_observer::intents_snapshot().await;
                    last.store(version, Ordering::Relaxed);
                    Some(json!({
                        "version": version,
                        "items": items,
                    }))
                }
            }
        },
    ));

    let actions_last = Arc::new(AtomicU64::new(u64::MAX));
    handles.push(spawn_read_model(
        &state,
        "actions",
        Duration::from_millis(2000),
        {
            let last = actions_last.clone();
            move |st| {
                let last = last.clone();
                async move {
                    if !st.kernel_enabled() {
                        return None;
                    }
                    let current = state_observer::actions_version_value();
                    if last.load(Ordering::Relaxed) == current {
                        return None;
                    }
                    let mut options = ActionListOptions::new(200);
                    options.limit = options.clamped_limit();
                    let items = st
                        .kernel()
                        .list_actions_async(options)
                        .await
                        .unwrap_or_default();
                    let items: Vec<Value> = items
                        .into_iter()
                        .map(crate::api::actions::sanitize_action_record)
                        .collect();
                    last.store(current, Ordering::Relaxed);
                    Some(json!({
                        "version": current,
                        "items": items,
                    }))
                }
            }
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

fn start_read_models_smoke(state: AppState) -> Vec<TaskHandle> {
    let mut handles = Vec::new();
    handles.push(spawn_read_model(
        &state,
        "route_stats",
        Duration::from_millis(4_000),
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
        Duration::from_millis(5_000),
        |st| async move {
            let (version, tasks) = st.metrics().tasks_snapshot_with_version();
            Some(json!({ "version": version, "tasks": tasks }))
        },
    ));

    handles.push(spawn_read_model(
        &state,
        "runtime_bundles",
        Duration::from_millis(8_000),
        |st| async move { Some(st.runtime_bundles().snapshot().await) },
    ));

    handles.push(spawn_read_model(
        &state,
        "recipes_gallery",
        Duration::from_millis(8_000),
        |_st| async move { Some(recipes_snapshot().await) },
    ));

    handles.push(spawn_service_health(&state));
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

#[derive(Default)]
struct ReadModelCache {
    entries: HashMap<String, CacheEntry>,
}

struct CacheEntry {
    version: u64,
    value: Value,
}

impl ReadModelCache {
    fn load_with(&self, id: &str, override_prev: Option<Value>) -> (Value, u64) {
        if let Some(prev) = override_prev {
            let version = self.entries.get(id).map(|entry| entry.version).unwrap_or(0);
            return (prev, version);
        }
        match self.entries.get(id) {
            Some(entry) => (entry.value.clone(), entry.version),
            None => (json!({}), 0),
        }
    }

    fn set(&mut self, id: &str, value: Value) {
        self.entries
            .entry(id.to_string())
            .and_modify(|entry| entry.value = value.clone())
            .or_insert(CacheEntry { version: 0, value });
    }

    fn set_if_version(&mut self, id: &str, expected: u64, value: Value) -> bool {
        match self.entries.get_mut(id) {
            Some(entry) if entry.version == expected => {
                entry.value = value;
                true
            }
            None if expected == 0 => {
                self.entries
                    .insert(id.to_string(), CacheEntry { version: 0, value });
                true
            }
            _ => false,
        }
    }

    fn replace_if_version(&mut self, id: &str, expected: u64, value: Value) -> Result<u64, u64> {
        let entry = self.entries.entry(id.to_string()).or_insert(CacheEntry {
            version: 0,
            value: Value::Null,
        });
        if entry.version != expected {
            return Err(entry.version);
        }
        entry.version = entry.version.saturating_add(1);
        entry.value = value;
        Ok(entry.version)
    }

    fn get(&self, id: &str) -> Option<Value> {
        self.entries.get(id).map(|entry| entry.value.clone())
    }

    #[cfg(test)]
    fn remove(&mut self, id: &str) {
        self.entries.remove(id);
    }
}

static READ_MODEL_CACHE: OnceCell<Mutex<ReadModelCache>> = OnceCell::new();

fn store_read_model_value(id: &str, value: &Value) {
    let map = READ_MODEL_CACHE.get_or_init(|| Mutex::new(ReadModelCache::default()));
    if let Ok(mut guard) = map.lock() {
        guard.set(id, value.clone());
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

    ::metrics::counter!(METRIC_READ_MODEL_COALESCED_WAITERS).increment(1);
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
    let cache = READ_MODEL_CACHE.get_or_init(|| Mutex::new(ReadModelCache::default()));
    let mut prev_override = previous;

    loop {
        let override_value = prev_override.take();
        let (prev, version) = {
            let guard = cache.lock().expect("read model cache lock poisoned");
            guard.load_with(id, override_value)
        };

        if prev == *value {
            if let Ok(mut guard) = cache.lock() {
                if guard.set_if_version(id, version, value.clone()) {
                    return;
                }
            }
            prev_override = None;
            continue;
        }

        let patch = json_patch::diff(&prev, value);
        if patch.is_empty() {
            if let Ok(mut guard) = cache.lock() {
                if guard.set_if_version(id, version, value.clone()) {
                    return;
                }
            }
            prev_override = None;
            continue;
        }

        let patch_val = serde_json::to_value(&patch).unwrap_or_else(|_| json!([]));
        let mut guard = cache.lock().expect("read model cache lock poisoned");
        match guard.replace_if_version(id, version, value.clone()) {
            Ok(_) => {
                drop(guard);
                bus.publish(
                    topics::TOPIC_READMODEL_PATCH,
                    &json!({
                        "id": id,
                        "patch": patch_val
                    }),
                );
                break;
            }
            Err(_) => {
                prev_override = None;
                continue;
            }
        }
    }
}

pub(crate) fn cached_read_model(id: &str) -> Option<Value> {
    READ_MODEL_CACHE
        .get()
        .and_then(|map| map.lock().ok().and_then(|guard| guard.get(id)))
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
                // Larger buffer to reduce lag on busy buses
                let mut rx = bus.subscribe_filtered(
                    vec![arw_topics::TOPIC_SERVICE_HEALTH.to_string()],
                    Some(256),
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

async fn kernel_session_exec<T, F>(kernel: arw_kernel::Kernel, op: F) -> Option<T>
where
    T: Send + 'static,
    F: FnOnce(KernelSession) -> AnyResult<T> + Send + 'static,
{
    match spawn_blocking(move || -> AnyResult<T> {
        let session = kernel.session()?;
        op(session)
    })
    .await
    {
        Ok(Ok(value)) => Some(value),
        Ok(Err(err)) => {
            warn!(
                target = "arw::read_model",
                error = %err,
                "kernel session operation failed"
            );
            None
        }
        Err(err) => {
            warn!(
                target = "arw::read_model",
                error = %err,
                "kernel session join failed"
            );
            None
        }
    }
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
                if governor.config.warmup_ms > 0 {
                    let jitter_ms: u64 = (rand::random::<u32>() % 50) as u64;
                    let total = governor.config.warmup_ms.saturating_add(jitter_ms);
                    time::sleep(Duration::from_millis(total)).await;
                }
                let mut tick = time::interval(period);
                loop {
                    tick.tick().await;
                    if governor.config.skip_if_lagged_over > 0 {
                        let lagged = state.bus().stats().lagged;
                        if lagged > governor.config.skip_if_lagged_over {
                            continue;
                        }
                    }
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
    warmup_ms: u64,
    skip_if_lagged_over: u64,
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
            warmup_ms: env_u64("ARW_SNAPPY_WARMUP_MS", 250),
            skip_if_lagged_over: env_u64("ARW_SNAPPY_SKIP_IF_LAGGED", 512),
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
    total_hits: u64,
    total_errors: u64,
    worker_queue_depth: u64,
    worker_busy: u64,
    worker_configured: u64,
    budgets: SnappyBudgets,
    protected_prefixes: Vec<String>,
}

impl SnappySnapshot {
    fn from_metrics(config: &SnappyConfig, summary: &metrics::MetricsSummary) -> Self {
        let mut routes = BTreeMap::new();
        let mut total_hits = 0u64;
        let mut total_errors = 0u64;
        for (path, stat) in summary.routes.by_path.iter() {
            if config.matches(path) {
                total_hits = total_hits.saturating_add(stat.hits);
                total_errors = total_errors.saturating_add(stat.errors);
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
            total_hits,
            total_errors,
            worker_queue_depth: summary.worker.queue_depth,
            worker_busy: summary.worker.busy,
            worker_configured: summary.worker.configured,
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
                "totals": {
                    "hits": self.total_hits,
                    "errors": self.total_errors,
                    "error_rate": if self.total_hits > 0 {
                        self.total_errors as f64 / self.total_hits as f64
                    } else {
                        0.0
                    },
                },
                "workers": {
                    "queue_depth": self.worker_queue_depth,
                    "busy": self.worker_busy,
                    "configured": self.worker_configured,
                }
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
        let mut p95_only = serde_json::Map::new();
        for (path, stat) in &self.routes {
            map.insert(
                path.clone(),
                json!({
                    "p95_ms": stat.p95_ms,
                    "ewma_ms": stat.ewma_ms,
                    "max_ms": stat.max_ms,
                    "hits": stat.hits,
                    "errors": stat.errors,
                    "error_rate": if stat.hits > 0 {
                        stat.errors as f64 / stat.hits as f64
                    } else {
                        0.0
                    }
                }),
            );
            p95_only.insert(path.clone(), json!(stat.p95_ms));
        }
        Some(json!({
            "generated": self.generated,
            "routes": Value::Object(map),
            "p95_by_path": Value::Object(p95_only),
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

pub(crate) async fn recipes_snapshot() -> Value {
    with_read_model_singleflight("recipes_gallery", || async {
        build_recipes_snapshot().await
    })
    .await
}

async fn build_recipes_snapshot() -> Value {
    let state_dir = crate::util::state_dir();
    let recipes_dir = state_dir.join("recipes");
    let generated = Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true);

    if !recipes_dir.exists() {
        return json!({
            "generated": generated,
            "items": [],
            "invalid": [],
            "total": 0,
            "invalid_count": 0,
        });
    }

    let state_dir_clone = state_dir.clone();
    let recipes_dir_clone = recipes_dir.clone();
    let index = tokio::task::spawn_blocking(move || {
        collect_recipes_blocking(&state_dir_clone, &recipes_dir_clone)
    })
    .await
    .unwrap_or_default();

    json!({
        "generated": generated,
        "items": index.items,
        "invalid": index.invalid,
        "total": index.items.len(),
        "invalid_count": index.invalid.len(),
    })
}

#[derive(Default)]
struct RecipesIndex {
    items: Vec<Value>,
    invalid: Vec<Value>,
}

fn collect_recipes_blocking(state_dir: &Path, recipes_dir: &Path) -> RecipesIndex {
    let mut index = RecipesIndex::default();
    let entries = match std::fs::read_dir(recipes_dir) {
        Ok(entries) => entries,
        Err(err) => {
            index.invalid.push(json!({
                "path": normalized_path_string(recipes_dir),
                "error": format!("read_dir_failed: {}", err),
            }));
            return index;
        }
    };

    for entry in entries {
        let entry = match entry {
            Ok(entry) => entry,
            Err(err) => {
                index.invalid.push(json!({
                    "path": normalized_path_string(recipes_dir),
                    "error": format!("entry_error: {}", err),
                }));
                continue;
            }
        };
        let path = entry.path();
        let meta = match entry.metadata() {
            Ok(meta) => meta,
            Err(err) => {
                index.invalid.push(json!({
                    "path": normalized_path_string(&path),
                    "error": format!("metadata_error: {}", err),
                }));
                continue;
            }
        };

        if meta.is_dir() || (meta.is_file() && recipes::looks_like_manifest_candidate(&path)) {
            match recipes::Recipe::load(&path) {
                Ok(recipe) => {
                    index.items.push(build_recipe_entry(&recipe, state_dir));
                }
                Err(err) => {
                    index.invalid.push(json!({
                        "path": normalized_path_string(&path),
                        "error": err.to_string(),
                    }));
                }
            }
        }
    }

    index.items.sort_by(|a, b| {
        let a_id = a
            .get("summary")
            .and_then(|v| v.get("id"))
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let b_id = b
            .get("summary")
            .and_then(|v| v.get("id"))
            .and_then(|v| v.as_str())
            .unwrap_or("");
        a_id.cmp(b_id)
    });

    index.invalid.sort_by(|a, b| {
        let a_path = a.get("path").and_then(|v| v.as_str()).unwrap_or("");
        let b_path = b.get("path").and_then(|v| v.as_str()).unwrap_or("");
        a_path.cmp(b_path)
    });

    index
}

fn build_recipe_entry(recipe: &recipes::Recipe, state_dir: &Path) -> Value {
    let summary = recipe.summary().clone();
    let summary_value = serde_json::to_value(&summary).unwrap_or_else(|_| json!({}));
    let manifest_json = recipe.manifest().clone();
    let manifest_bytes = serde_json::to_vec(&manifest_json).unwrap_or_default();
    let manifest_sha = format!("{:x}", Sha256::digest(&manifest_bytes));
    let manifest_meta = std::fs::metadata(recipe.manifest_path()).ok();
    let manifest_modified = manifest_meta
        .as_ref()
        .and_then(|meta| meta.modified().ok())
        .and_then(system_time_to_rfc3339);
    let manifest_size = manifest_meta.as_ref().map(|meta| meta.len());
    let manifest_path = normalized_path_string(recipe.manifest_path());
    let manifest_rel = relative_path_string(state_dir, recipe.manifest_path());

    let source_root = normalized_path_string(recipe.source_root());
    let source_rel = relative_path_string(state_dir, recipe.source_root());

    json!({
        "summary": summary_value,
        "manifest": manifest_json,
        "manifest_sha256": manifest_sha,
        "manifest_path": manifest_path,
        "manifest_rel": manifest_rel,
        "manifest_modified": manifest_modified,
        "manifest_size": manifest_size,
        "source_root": source_root,
        "source_rel": source_rel,
        "source_kind": recipe.kind(),
        "tool_ids": recipe.tool_ids(),
        "workflow_steps": recipe.workflow_steps(),
        "permission_modes": recipe.permission_modes(),
    })
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

    let max_items =
        env_u64("ARW_SCREENSHOTS_INDEX_LIMIT", MAX_SCREENSHOTS_INDEX as u64).clamp(1, 512) as usize;
    let text_limit = env_u64(
        "ARW_SCREENSHOTS_TEXT_LIMIT",
        MAX_SCREENSHOTS_TEXT_LEN as u64,
    )
    .clamp(32, 4096) as usize;
    let preview_limit = env_u64(
        "ARW_SCREENSHOTS_PREVIEW_LIMIT",
        MAX_SCREENSHOTS_PREVIEW_LEN as u64,
    )
    .clamp(16, text_limit as u64) as usize;
    let langs_limit = env_u64(
        "ARW_SCREENSHOTS_LANGS_LIMIT",
        MAX_SCREENSHOTS_LANGS_PER_SOURCE as u64,
    )
    .clamp(1, 16) as usize;

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
    let mut items: Vec<_> = aggregates.into_values().collect();
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
                if self.latest.is_none_or(|cur| utc > cur) {
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
    path.strip_prefix(base).ok().map(normalized_path_string)
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
            let snapshots = project_snapshots::list_snapshots(&project_root, &name, 5)
                .await
                .unwrap_or_default();
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
            let kernel = state.kernel().clone();
            let items = kernel_session_exec(kernel, |session| session.list_leases(200))
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
    use chrono::{SecondsFormat, Utc};
    use json_patch::Patch;
    use serde_json::Value;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;
    use tempfile::tempdir;
    use tokio::sync::Notify;
    use tokio::time::timeout;

    fn sample_recipe_manifest() -> &'static str {
        r#"id: sample-recipe
name: Sample Recipe
version: "1.0.0"
model:
  preferred: "local:llama"
permissions:
  file.read: allow
prompts:
  system: "Do sample things"
tools:
  - id: sample_tool
    params: {}
workflows:
  - step: "do"
    tool: sample_tool
"#
    }

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
    async fn observations_read_model_publishes_patches() {
        let temp = tempdir().expect("tempdir");
        let mut ctx = crate::test_support::begin_state_env(temp.path());
        remove_cached_read_model_for_test("observations");
        crate::state_observer::reset_for_tests().await;
        let state = build_state(temp.path(), &mut ctx.env).await;

        let bus = state.bus();
        let mut rx =
            bus.subscribe_filtered(vec![topics::TOPIC_READMODEL_PATCH.to_string()], Some(8));

        let handle = spawn_read_model(
            &state,
            "observations",
            Duration::from_millis(50),
            |_st| async move {
                let (version, items) =
                    state_observer::observations_snapshot(None, None, None).await;
                Some(json!({
                    "version": version,
                    "items": items,
                }))
            },
        );

        let mut doc = json!({});

        let initial_env: Envelope = timeout(Duration::from_millis(200), rx.recv())
            .await
            .expect("initial patch timeout")
            .expect("initial patch env");
        assert_eq!(initial_env.kind, topics::TOPIC_READMODEL_PATCH);
        assert_eq!(initial_env.payload["id"].as_str(), Some("observations"));
        let initial_patch: Patch = serde_json::from_value(
            initial_env
                .payload
                .get("patch")
                .cloned()
                .expect("initial patch array"),
        )
        .expect("initial patch decode");
        json_patch::patch(&mut doc, &initial_patch).expect("apply initial patch");

        let env = Envelope {
            time: Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true),
            kind: "observations.test".to_string(),
            payload: json!({"msg": "hello"}),
            policy: None,
            ce: None,
        };
        crate::state_observer::ingest_for_tests(&env).await;

        let update_env: Envelope = timeout(Duration::from_millis(400), rx.recv())
            .await
            .expect("update patch timeout")
            .expect("update patch env");
        assert_eq!(update_env.kind, topics::TOPIC_READMODEL_PATCH);
        assert_eq!(update_env.payload["id"].as_str(), Some("observations"));
        let update_patch: Patch = serde_json::from_value(
            update_env
                .payload
                .get("patch")
                .cloned()
                .expect("update patch array"),
        )
        .expect("update patch decode");
        json_patch::patch(&mut doc, &update_patch).expect("apply update patch");

        let items = doc["items"].as_array().expect("items array");
        assert_eq!(items.len(), 1);
        let first = &items[0];
        assert_eq!(first["kind"].as_str(), Some("observations.test"));
        assert_eq!(first["payload"]["msg"].as_str(), Some("hello"));
        assert!(doc["version"].as_u64().unwrap_or_default() >= 1);

        let (_name, _started, task) = handle.into_inner();
        task.abort();
        let _ = task.await;
    }

    #[tokio::test]
    async fn beliefs_read_model_publishes_patches() {
        let temp = tempdir().expect("tempdir");
        let mut ctx = crate::test_support::begin_state_env(temp.path());
        remove_cached_read_model_for_test("beliefs");
        crate::state_observer::reset_for_tests().await;
        let state = build_state(temp.path(), &mut ctx.env).await;

        let bus = state.bus();
        let mut rx =
            bus.subscribe_filtered(vec![topics::TOPIC_READMODEL_PATCH.to_string()], Some(8));

        let handle = spawn_read_model(
            &state,
            "beliefs",
            Duration::from_millis(50),
            |_st| async move {
                let (version, items) = state_observer::beliefs_snapshot().await;
                Some(json!({
                    "version": version,
                    "items": items,
                }))
            },
        );

        let mut doc = json!({});

        let initial_env: Envelope = timeout(Duration::from_millis(200), rx.recv())
            .await
            .expect("initial patch timeout")
            .expect("initial patch env");
        assert_eq!(initial_env.payload["id"].as_str(), Some("beliefs"));
        let initial_patch: Patch = serde_json::from_value(
            initial_env
                .payload
                .get("patch")
                .cloned()
                .expect("initial patch array"),
        )
        .expect("initial patch decode");
        json_patch::patch(&mut doc, &initial_patch).expect("apply initial patch");

        let env = Envelope {
            time: Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true),
            kind: "feedback.suggested".to_string(),
            payload: json!({
                "suggestions": [
                    json!({
                        "title": "Focus log hygiene",
                        "body": "Rotate debug logs before exporting to support.",
                    })
                ]
            }),
            policy: None,
            ce: None,
        };
        crate::state_observer::ingest_for_tests(&env).await;

        let update_env: Envelope = timeout(Duration::from_millis(400), rx.recv())
            .await
            .expect("update patch timeout")
            .expect("update patch env");
        assert_eq!(update_env.payload["id"].as_str(), Some("beliefs"));
        let update_patch: Patch = serde_json::from_value(
            update_env
                .payload
                .get("patch")
                .cloned()
                .expect("update patch array"),
        )
        .expect("update patch decode");
        json_patch::patch(&mut doc, &update_patch).expect("apply update patch");

        let items = doc["items"].as_array().expect("items array");
        assert_eq!(items.len(), 1);
        assert_eq!(items[0]["title"].as_str(), Some("Focus log hygiene"));

        let (_name, _started, task) = handle.into_inner();
        task.abort();
        let _ = task.await;
    }

    #[tokio::test]
    async fn intents_read_model_publishes_patches() {
        let temp = tempdir().expect("tempdir");
        let mut ctx = crate::test_support::begin_state_env(temp.path());
        remove_cached_read_model_for_test("intents");
        crate::state_observer::reset_for_tests().await;
        let state = build_state(temp.path(), &mut ctx.env).await;

        let bus = state.bus();
        let mut rx =
            bus.subscribe_filtered(vec![topics::TOPIC_READMODEL_PATCH.to_string()], Some(8));

        let handle = spawn_read_model(
            &state,
            "intents",
            Duration::from_millis(50),
            |_st| async move {
                let (version, items) = state_observer::intents_snapshot().await;
                Some(json!({
                    "version": version,
                    "items": items,
                }))
            },
        );

        let mut doc = json!({});

        let initial_env: Envelope = timeout(Duration::from_millis(200), rx.recv())
            .await
            .expect("initial patch timeout")
            .expect("initial patch env");
        assert_eq!(initial_env.payload["id"].as_str(), Some("intents"));
        let initial_patch: Patch = serde_json::from_value(
            initial_env
                .payload
                .get("patch")
                .cloned()
                .expect("initial patch array"),
        )
        .expect("initial patch decode");
        json_patch::patch(&mut doc, &initial_patch).expect("apply initial patch");

        let env = Envelope {
            time: Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true),
            kind: "intents.proposed".to_string(),
            payload: json!({
                "corr_id": "intent-123",
                "goal": "Summarize recent crash logs",
            }),
            policy: None,
            ce: None,
        };
        crate::state_observer::ingest_for_tests(&env).await;

        let update_env: Envelope = timeout(Duration::from_millis(400), rx.recv())
            .await
            .expect("update patch timeout")
            .expect("update patch env");
        assert_eq!(update_env.payload["id"].as_str(), Some("intents"));
        let update_patch: Patch = serde_json::from_value(
            update_env
                .payload
                .get("patch")
                .cloned()
                .expect("update patch array"),
        )
        .expect("update patch decode");
        json_patch::patch(&mut doc, &update_patch).expect("apply update patch");

        let items = doc["items"].as_array().expect("items array");
        assert_eq!(items.len(), 1);
        assert_eq!(items[0]["payload"]["corr_id"].as_str(), Some("intent-123"));

        let (_name, _started, task) = handle.into_inner();
        task.abort();
        let _ = task.await;
    }

    #[tokio::test]
    async fn actions_read_model_publishes_patches() {
        let temp = tempdir().expect("tempdir");
        let mut ctx = crate::test_support::begin_state_env(temp.path());
        remove_cached_read_model_for_test("actions");
        crate::state_observer::reset_for_tests().await;
        let state = build_state(temp.path(), &mut ctx.env).await;

        let bus = state.bus();
        let mut rx =
            bus.subscribe_filtered(vec![topics::TOPIC_READMODEL_PATCH.to_string()], Some(8));

        let handle = spawn_read_model(
            &state,
            "actions",
            Duration::from_millis(50),
            |st| async move {
                if !st.kernel_enabled() {
                    return None;
                }
                let version = state_observer::actions_version_value();
                let mut options = ActionListOptions::new(200);
                options.limit = options.clamped_limit();
                let items = st
                    .kernel()
                    .list_actions_async(options)
                    .await
                    .unwrap_or_default();
                let items: Vec<Value> = items
                    .into_iter()
                    .map(crate::api::actions::sanitize_action_record)
                    .collect();
                Some(json!({
                    "version": version,
                    "items": items,
                }))
            },
        );

        let action_id = uuid::Uuid::new_v4().to_string();
        state
            .kernel()
            .insert_action_async(
                &action_id,
                "net.http.get",
                &json!({"url": "https://example.com"}),
                None,
                None,
                "completed",
            )
            .await
            .expect("insert action");

        let env = Envelope {
            time: Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true),
            kind: "actions.completed".to_string(),
            payload: json!({"id": action_id}),
            policy: None,
            ce: None,
        };
        crate::state_observer::ingest_for_tests(&env).await;

        let mut doc = json!({});

        let patch_env: Envelope = timeout(Duration::from_millis(400), rx.recv())
            .await
            .expect("patch timeout")
            .expect("patch env");
        assert_eq!(patch_env.payload["id"].as_str(), Some("actions"));
        let patch: Patch = serde_json::from_value(
            patch_env
                .payload
                .get("patch")
                .cloned()
                .expect("patch array"),
        )
        .expect("patch decode");
        json_patch::patch(&mut doc, &patch).expect("apply patch");

        let items = doc["items"].as_array().expect("items array");
        assert_eq!(items.len(), 1);
        assert_eq!(items[0]["id"].as_str(), Some(action_id.as_str()));
        assert_eq!(items[0]["kind"].as_str(), Some("net.http.get"));
        assert!(items[0].get("output").is_some());

        let (_name, _started, task) = handle.into_inner();
        task.abort();
        let _ = task.await;
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
            ("ARW_SNAPPY_WARMUP_MS", Some("0")),
            ("ARW_SNAPPY_SKIP_IF_LAGGED", Some("0")),
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
        assert_eq!(doc["observed"]["max_p95_ms"].as_u64(), Some(50));
        let route = &doc["observed"]["routes"]["/state/routes"];
        assert_eq!(route["p95_ms"].as_u64(), Some(50));
        assert_eq!(route["hits"].as_u64(), Some(1));

        let notice_env: Envelope = timeout(Duration::from_millis(200), notice_rx.recv())
            .await
            .expect("notice event timeout")
            .expect("notice event");
        assert_eq!(notice_env.kind, topics::TOPIC_SNAPPY_NOTICE);
        assert_eq!(notice_env.payload["path"].as_str(), Some("/state/routes"));
        assert_eq!(notice_env.payload["p95_max_ms"].as_u64(), Some(50));
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

    #[tokio::test]
    async fn recipes_snapshot_collects_installed_manifests() {
        remove_cached_read_model_for_test("recipes_gallery");
        let temp = tempdir().expect("tempdir");
        let mut ctx = crate::test_support::begin_state_env(temp.path());
        ctx.env
            .set("ARW_STATE_DIR", temp.path().display().to_string());

        let recipe_dir = temp.path().join("recipes/sample");
        std::fs::create_dir_all(&recipe_dir).unwrap();
        std::fs::write(recipe_dir.join("manifest.yaml"), sample_recipe_manifest()).unwrap();

        let snapshot = build_recipes_snapshot().await;
        assert_eq!(snapshot["total"].as_u64(), Some(1));
        let items = snapshot["items"].as_array().expect("items array");
        assert_eq!(items.len(), 1);
        let first = &items[0];
        assert_eq!(first["summary"]["id"].as_str(), Some("sample-recipe"));
        assert_eq!(first["tool_ids"].as_array().map(|arr| arr.len()), Some(1));
        assert_eq!(
            first["permission_modes"].as_array().map(|arr| arr.len()),
            Some(1)
        );

        remove_cached_read_model_for_test("recipes_gallery");
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
