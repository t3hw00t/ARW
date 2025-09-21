use chrono::SecondsFormat;
use once_cell::sync::OnceCell;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::future::Future;
use std::sync::Mutex;
use std::time::Duration;
use tokio::time;

use crate::{tasks::TaskHandle, training, AppState};
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
        "route_stats",
        Duration::from_millis(2000),
        |st| async move {
            let bus = st.bus().stats();
            let metrics = st.metrics().snapshot();
            Some(json!({
                "bus": {
                    "published": bus.published,
                    "delivered": bus.delivered,
                    "receivers": bus.receivers,
                    "lagged": bus.lagged,
                    "no_receivers": bus.no_receivers,
                },
                "events": metrics.events,
                "routes": metrics.routes,
                "tasks": metrics.tasks,
            }))
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

pub(crate) fn publish_read_model_patch(bus: &arw_events::Bus, id: &str, value: &Value) {
    static LAST: OnceCell<Mutex<HashMap<String, Value>>> = OnceCell::new();
    let map = LAST.get_or_init(|| Mutex::new(HashMap::new()));
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
